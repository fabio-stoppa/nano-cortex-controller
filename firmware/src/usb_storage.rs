use alloc::{boxed::Box, string::String};
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

mod config;
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
        // Host flushes are not ejects; keep all writes in the disposable image.
        Ok(())
    }
}

fn prepare() -> Result<disk::Disk, &'static str> {
    // Refuse to export empty defaults over an unreadable saved configuration.
    if let Some(json) = crate::settings::load_midi_json()? {
        crate::midi::keys_from_json(&json)?;
    }
    let midi = crate::midi::export_json()?;
    let scenes = crate::scenes::export_json()?;
    let preferences = crate::ui::export_preferences_json()?;
    // Preserve touch/MIDI edits made before USB, even when computer edits cancel.
    crate::midi::import_json(&midi)?;
    let mut help = String::from(
        "Nano controller configuration\r\n\r\n\
         Back up midi.json, scenes.json and preferences.json before editing or replacing them.\r\n\
         Safely eject the drive to validate and save all three files. Check the controller screen.\r\n\
         Cancel discards USB edits. Restart the controller to resume Bluetooth.\r\n\
         All three files are required. Invalid files are rejected before any are saved.\r\n\
         Saves are verified individually, not atomic across files; check any error.\r\n\
         Do not format the drive. Other files are temporary and are not imported.\r\n\
         midi.json limit: 48 KiB. scenes.json limit: 1048544 bytes (format version 4, six scenes per preset).\r\n\
         preferences.json limit: 4 KiB; format nano-controller-preferences, version 1.\r\n\
         orientation: landscape, portrait, portrait_flipped, landscape_flipped.\r\n\
         autosave: true or false. Preferences take effect after restart, not on the Nano.\r\n\
         MIDI channels: 1-16; numbers: 0-127; kinds: cc, note, program.\r\n\
         Omitted MIDI actions are cleared; a message may target several actions.\r\n\r\n\
         Active MIDI action IDs:\r\n",
    );
    for action in 0..crate::midi::ACTION_COUNT {
        if crate::midi::active_action(action) {
            use core::fmt::Write;
            writeln!(
                help,
                "{action}: {}{}\r",
                crate::midi::action_name(action),
                if crate::midi::is_continuous(action) {
                    " (CC only)"
                } else {
                    ""
                }
            )
            .unwrap();
        }
    }
    disk::Disk::new(
        midi.as_bytes(),
        scenes.as_bytes(),
        preferences.as_bytes(),
        help.as_bytes(),
    )
}

fn finish(disk: disk::Disk) -> Result<(), &'static str> {
    let midi = disk.midi()?;
    let scenes = disk.scenes()?;
    let preferences = disk.preferences()?;
    // Release the large FAT image before parsing scene snapshots.
    drop(disk);
    let midi = String::from_utf8(midi).map_err(|_| "midi.json is not UTF-8")?;
    let scenes = String::from_utf8(scenes).map_err(|_| "scenes.json is not UTF-8")?;
    let preferences =
        String::from_utf8(preferences).map_err(|_| "preferences.json is not UTF-8")?;
    config::import(&midi, &scenes, &preferences)
}

pub async fn run(resources: Resources, board: &mut crate::panel::Board<'_>) {
    CANCEL.reset();
    crate::link::set_up(false);
    crate::link::clear_midi();
    crate::screen::nano_ui_set_ble_connected(false);
    crate::midi::cancel_learn();
    crate::scenes::cancel("Scene recall cancelled for USB configuration");
    let mut disk = match prepare() {
        Ok(disk) => disk,
        Err(error) => {
            log::error!("USB configuration: {error}");
            crate::ui::usb_storage_status(error, true);
            crate::ble::touch_only(board).await;
            return;
        }
    };
    crate::ui::usb_storage_status(
        "Open NANO CONFIG on your computer. Back up or replace midi.json, scenes.json and preferences.json, then safely eject.\n\
         Bluetooth is paused. Cancel discards USB edits. Restart to resume control.", false,
    );
    let usb = Usb::new_fs(resources.usb, resources.dp, resources.dm);
    let mut ep_out = [0; 256];
    let driver = Driver::new(usb, &mut ep_out, Config::default());
    // Espressif's development VID/PID, not an official product identity.
    let mut usb_config = embassy_usb::Config::new(0x303a, 0x4002);
    usb_config.manufacturer = Some("Nano community");
    usb_config.product = Some("Nano controller configuration");
    usb_config.serial_number = Some("NANOCONFIG43B");
    usb_config.max_power = 100;
    usb_config.device_class = 0;
    usb_config.device_sub_class = 0;
    usb_config.device_protocol = 0;
    usb_config.composite_with_iads = false;
    let mut descriptors = [0; 256];
    let mut bos = [0; 64];
    let mut control = [0; 64];
    let mut msos = [];
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        driver,
        usb_config,
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
            .product_id("CONFIG")
            .serial_number("NANOCONFIG43B"),
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
            Either::Second(Either::First(StopReason::Ejected)) => match finish(disk) {
                Ok(()) => "MIDI, scenes and preferences saved and verified. Restart to apply preferences and resume control.",
                Err(error) => {
                    log::error!("USB configuration import: {error}");
                    error
                }
            },
            Either::Second(Either::Second(())) => {
                drop(disk);
                "USB edits discarded. Restart to resume control."
            }
            _ => {
                log::error!("USB configuration device stopped unexpectedly");
                "USB stopped unexpectedly. USB edits were not saved. Restart to resume."
            }
        };
        crate::ui::usb_storage_status(status, true);
        core::future::pending::<()>().await;
    };
    join(Box::pin(session), crate::ble::touch_only(board)).await;
}
