use embassy_futures::{
    join::join,
    select::{select, Either},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_usb::class::msc::device::{
    BlockDevice, Config as MscConfig, MscClass, State, StopReason,
};
use esp_hal::{
    peripherals::{GPIO19, GPIO20, USB_FS},
    usb::otg::{
        embassy_usb_device::{Config, Driver},
        Usb,
    },
};

mod disk;

static OPEN: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static CANCEL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub struct Resources {
    pub usb: USB_FS<'static>,
    pub dp: GPIO20<'static>,
    pub dm: GPIO19<'static>,
}

pub fn request() {
    OPEN.signal(());
}
pub async fn requested() {
    OPEN.wait().await;
}
pub fn cancel() {
    CANCEL.signal(());
}

impl BlockDevice for disk::Disk {
    type Error = &'static str;
    fn block_size(&self) -> u32 {
        disk::SECTOR as u32
    }
    fn block_count(&self) -> u32 {
        disk::SECTORS as u32
    }
    fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.read(lba, buf)
    }
    fn write_blocks(&mut self, lba: u32, data: &[u8]) -> Result<(), Self::Error> {
        self.write(lba, data)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        // The host still owns the FAT view here; commit only after safe eject.
        Ok(())
    }
}

pub async fn run(resources: Resources, board: &mut crate::panel::Board<'_>) {
    CANCEL.reset();
    crate::link::set_up(false);
    crate::screen::nano_ui_set_ble_connected(false);
    crate::midi::cancel_learn();
    let json = match crate::midi::prepare_storage() {
        Ok(json) => json,
        Err(err) => {
            crate::ui::usb_storage_status(err, true);
            crate::ble::touch_only(board).await;
            return;
        }
    };
    let performance = match crate::performance::export_json() {
        Ok(json) => json,
        Err(error) => {
            crate::performance::report(error);
            crate::ui::usb_storage_status(error, true);
            crate::ble::touch_only(board).await;
            return;
        }
    };
    let mut help = crate::midi::storage_help();
    help.push_str("\r\nperformance.json contains controller banks. Maximum size: 512 KiB.\r\n\
        Edit both files, then safely eject. Both are validated before saving.\r\n");
    let mut disk = match disk::Disk::with_performance(json.as_bytes(), performance.as_bytes(), help.as_bytes()) {
        Ok(disk) => disk,
        Err(err) => {
            crate::ui::usb_storage_status(err, true);
            crate::ble::touch_only(board).await;
            return;
        }
    };
    crate::ui::usb_storage_status(
        "Open NANO MIDI on your computer. Edit midi.json or performance.json, then safely eject the drive.\n\
         MIDI editing and Bluetooth control are paused. Cancel discards computer edits.", false,
    );
    let usb = Usb::new_fs(resources.usb, resources.dp, resources.dm);
    let mut ep_out = [0; 256];
    let driver = Driver::new(usb, &mut ep_out, Config::default());
    // Espressif's development VID/PID; this is an unofficial development device.
    let mut config = embassy_usb::Config::new(0x303a, 0x4002);
    config.manufacturer = Some("Nano community");
    config.product = Some("Nano MIDI configuration");
    config.serial_number = Some("NANOMIDI43B");
    config.max_power = 100;
    config.device_class = 0;
    config.device_sub_class = 0;
    config.device_protocol = 0;
    config.composite_with_iads = false;
    let mut descriptors = [0; 256];
    let mut bos = [0; 64];
    let mut control = [0; 64];
    let mut msos = [];
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut descriptors,
        &mut bos,
        &mut msos,
        &mut control,
    );
    let mut msc = MscClass::new(
        &mut builder,
        &mut state,
        MscConfig::new(64)
            .vendor_id("NANO")
            .product_id("MIDI CONFIG")
            .serial_number("NANOMIDI43B"),
    );
    let mut device = builder.build();
    let mut blocks = [0; 1024];
    let session = async {
        let result = select(
            device.run(),
            select(msc.run(&mut disk, &mut blocks), CANCEL.wait()),
        )
        .await;
        device.disable().await;
        let status = match result {
            Either::Second(Either::First(StopReason::Ejected)) => disk
                .json()
                .and_then(|data| {
                    let json = core::str::from_utf8(&data).map_err(|_| "midi.json is not UTF-8")?;
                    let performance = disk.performance()?;
                    let performance = core::str::from_utf8(&performance).map_err(|_| "performance.json is not UTF-8")?;
                    crate::midi::keys_from_json(json)?;
                    crate::performance::validate_json(performance)?;
                    crate::performance::import_json(performance)?;
                    if let Err(error) = crate::midi::import_json(json) {
                        crate::midi::report(error);
                        return Err("Performance saved, but MIDI save failed; previous MIDI retained");
                    }
                    Ok(())
                })
                .map(|()| {
                    "Both configuration files saved and verified. Restart to resume control."
                })
                .unwrap_or_else(|err| {
                    crate::midi::report(err);
                    err
                }),
            Either::Second(Either::Second(())) => {
                "USB edits discarded. Previous bindings retained. Restart to resume."
            }
            Either::First(_) | Either::Second(Either::First(_)) => {
                crate::midi::report("USB device stopped unexpectedly");
                "USB device stopped unexpectedly. Previous bindings retained."
            }
        };
        crate::ui::usb_storage_status(status, true);
        core::future::pending::<()>().await;
    };
    join(session, crate::ble::touch_only(board)).await;
}
