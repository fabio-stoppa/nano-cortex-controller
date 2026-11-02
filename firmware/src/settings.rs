//! Persists screen layout and autosave preferences in the first NVS sector.
//!
//! Current MIDI and scene configuration live in separate partitions.
//! The last NVS sector marks completion of the controller-wide storage reset.

use alloc::{string::String, vec};
use esp_bootloader_esp_idf::partitions::{
    read_partition_table, DataPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};
use esp_storage::FlashStorage;

const MAGIC: u32 = 0x4c41_5931;
const SECTOR: u32 = 4096;

// The flash size is probed once at construction, and that probe returns
// nothing once the radio and scan are running, so the handle is kept.
// Only the first core's UI task touches it.
static mut FLASH: Option<FlashStorage<'static>> = None;

fn flash() -> Option<&'static mut FlashStorage<'static>> {
    let slot = unsafe { &mut *core::ptr::addr_of_mut!(FLASH) };
    if slot.as_ref().is_none_or(|f| f.capacity() == 0) {
        // Core 1 runs the scan interrupts from PSRAM, so it is parked during a write.
        let f = FlashStorage::new(unsafe { esp_hal::peripherals::FLASH::steal() })
            .multicore_auto_park();
        *slot = Some(f);
    }
    slot.as_mut().filter(|f| f.capacity() != 0)
}

fn record(mode: u8, autosave: bool, selected_nano: Option<[u8; 7]>) -> [u8; 16] {
    let mut r = [0u8; 16];
    r[..4].copy_from_slice(&MAGIC.to_le_bytes());
    r[4] = mode;
    r[5] = !mode;
    r[6] = u8::from(autosave);
    r[7] = !r[6];
    selected_device::write(&mut r, selected_nano);
    r
}

fn selected_nano_from_record(record: &[u8; 16]) -> Option<[u8; 7]> {
    let magic = u32::from_le_bytes(record[..4].try_into().ok()?);
    if magic != MAGIC
        || record[5] != !record[4]
        || record[4] > 3
        || record[6] > 1
        || record[7] != !record[6]
    {
        return None;
    }
    selected_device::read(record)
}

fn with_region<T>(
    f: impl FnOnce(&mut esp_bootloader_esp_idf::partitions::FlashRegion<'_, '_>) -> Option<T>,
) -> Option<T> {
    let flash = flash()?;
    let mut buf = vec![0u8; PARTITION_TABLE_MAX_LEN];
    let table = read_partition_table(&mut *flash, &mut buf)
        .map_err(|e| log::warn!("ptable {e:?}"))
        .ok()?;
    let entry = table
        .find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
        .map_err(|e| log::warn!("find {e:?}"))
        .ok()??;
    let mut region = entry.as_flash_region(flash);
    if (region.partition_size() as u32) < SECTOR {
        return None;
    }
    f(&mut region)
}

/// Saved layout mode, if a valid record exists.
pub fn load_layout() -> Option<i32> {
    with_region(|region| {
        let mut r = [0u8; 16];
        region.read(0, &mut r).ok()?;
        let magic = u32::from_le_bytes(r[..4].try_into().ok()?);
        (magic == MAGIC && r[5] == !r[4] && r[4] <= 3).then_some(r[4] as i32)
    })
}

pub fn save_layout(mode: i32) {
    let mode = mode.clamp(0, 3) as u8;
    if load_layout() == Some(mode as i32) {
        return;
    }
    if let Err(error) = save_preferences(mode, load_autosave()) {
        log::warn!("could not save layout: {error}");
    }
}

pub fn load_autosave() -> bool {
    with_region(|region| {
        let mut r = [0u8; 16];
        region
            .read(0, &mut r)
            .map_err(|error| log::warn!("read autosave {error:?}"))
            .ok()?;
        Some(
            u32::from_le_bytes(r[..4].try_into().ok()?) == MAGIC
                && r[5] == !r[4]
                && r[4] <= 3
                && r[6] == 1
                && r[7] == !r[6],
        )
    })
    .unwrap_or(false)
}

pub fn save_autosave(enabled: bool) -> Result<(), &'static str> {
    save_preferences(load_layout().unwrap_or(0) as u8, enabled)
}

fn save_preferences(mode: u8, autosave: bool) -> Result<(), &'static str> {
    let selected_nano = with_region(|region| {
        let mut current = [0u8; 16];
        region
            .read(0, &mut current)
            .map_err(|e| log::warn!("read selected Nano {e:?}"))
            .ok()?;
        Some(selected_nano_from_record(&current))
    })
    .flatten();
    save_preferences_with_selection(mode, autosave, selected_nano)
}

fn save_preferences_with_selection(
    mode: u8,
    autosave: bool,
    selected_nano: Option<[u8; 7]>,
) -> Result<(), &'static str> {
    let _pause = crate::bounce::FlashPause::begin()?;
    let expected = record(mode, autosave, selected_nano);
    let saved = with_region(|region| {
        let mut current = [0u8; 16];
        region
            .read(0, &mut current)
            .map_err(|e| log::warn!("read preferences {e:?}"))
            .ok()?;
        if current == expected {
            return Some(());
        }
        region
            .erase(0, SECTOR)
            .map_err(|e| log::warn!("erase {e:?}"))
            .ok()?;
        region
            .write(0, &expected)
            .map_err(|e| log::warn!("write {e:?}"))
            .ok()?;
        let mut readback = [0u8; 16];
        region
            .read(0, &mut readback)
            .map_err(|e| log::warn!("readback {e:?}"))
            .ok()?;
        (readback == expected).then_some(())
    });
    saved.ok_or("Could not save controller preferences")
}

pub fn load_selected_nano() -> Option<[u8; 7]> {
    with_region(|region| {
        let mut record = [0u8; 16];
        region.read(0, &mut record).ok()?;
        Some(selected_nano_from_record(&record))
    })
    .flatten()
}

pub fn save_selected_nano(address: Option<[u8; 7]>) -> Result<(), &'static str> {
    save_preferences_with_selection(load_layout().unwrap_or(0) as u8, load_autosave(), address)
}

mod json_record;
mod midi_record;
pub mod preferences;
mod reset;
pub mod selected_device;

pub fn import_preferences(preferences: &preferences::Preferences) -> Result<(), &'static str> {
    save_preferences(preferences.orientation.layout(), preferences.autosave)
}

pub fn reset_preferences() -> Result<(), &'static str> {
    import_preferences(&preferences::Preferences::new(0, false)?)
}

pub fn initialize() -> Result<bool, &'static str> {
    let flash = flash().ok_or("Flash unavailable for controller factory reset")?;
    let mut buf = vec![0; PARTITION_TABLE_MAX_LEN];
    let table = read_partition_table(&mut *flash, &mut buf)
        .map_err(|_| "Cannot read partition table for controller factory reset")?;
    let find = |label: &str, minimum: u32| {
        let entry = table
            .iter()
            .find(|entry| entry.label_as_str() == label)
            .ok_or("Controller factory reset partition missing; flash using flash.sh")?;
        if entry.partition_type() != PartitionType::Data(DataPartitionSubType::Nvs)
            || entry.len() < minimum
        {
            return Err("Controller factory reset partition type or size invalid");
        }
        Ok(entry)
    };
    let nvs = find("nvs", 3 * SECTOR)?;
    let midi = find("midi", json_record::CAPACITY)?;
    let scenes = find("scenes", SCENES_SLOT * 2)?;
    let nvs_size = nvs.len();
    let midi_size = midi.len();
    let scenes_size = scenes.len();
    let ranges = [
        (nvs.offset(), nvs_size),
        (midi.offset(), midi_size),
        (scenes.offset(), scenes_size),
    ];
    for (index, &(offset, size)) in ranges.iter().enumerate() {
        let end = offset
            .checked_add(size)
            .ok_or("Controller reset partition range overflow")?;
        if offset % SECTOR != 0
            || end > flash.capacity() as u32
            || ranges[..index]
                .iter()
                .any(|&(old_offset, old_size)| offset < old_offset + old_size && old_offset < end)
        {
            return Err("Controller reset partition ranges invalid or overlapping");
        }
    }
    struct Regions<'a, 'b, 'd> {
        flash: &'a core::cell::RefCell<&'b mut FlashStorage<'d>>,
        entry: esp_bootloader_esp_idf::partitions::PartitionEntry,
    }
    impl json_record::Storage for Regions<'_, '_, '_> {
        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), &'static str> {
            self.entry
                .as_flash_region(&mut **self.flash.borrow_mut())
                .read(offset, bytes)
                .map_err(|_| "Controller reset flash read failed")
        }
        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), &'static str> {
            self.entry
                .as_flash_region(&mut **self.flash.borrow_mut())
                .write(offset, bytes)
                .map_err(|_| "Controller reset flash write failed")
        }
        fn erase(&mut self, start: u32, end: u32) -> Result<(), &'static str> {
            self.entry
                .as_flash_region(&mut **self.flash.borrow_mut())
                .erase(start, end)
                .map_err(|_| "Controller reset flash erase failed")
        }
    }
    let flash = core::cell::RefCell::new(flash);
    reset::initialize(
        &mut Regions {
            flash: &flash,
            entry: nvs,
        },
        nvs_size,
        &mut Regions {
            flash: &flash,
            entry: midi,
        },
        midi_size,
        &mut Regions {
            flash: &flash,
            entry: scenes,
        },
        scenes_size,
    )
}

pub fn save_midi(keys: &[u16], zero_is_press: &[bool]) -> bool {
    let result =
        crate::midi::json_for_bindings(keys, zero_is_press).and_then(|json| save_midi_json(&json));
    if let Err(err) = result {
        log::warn!("Could not save MIDI bindings: {err}");
    }
    result.is_ok()
}

struct JsonStorage<'a, 'd>(esp_bootloader_esp_idf::partitions::FlashRegion<'a, 'd>);

impl json_record::Storage for JsonStorage<'_, '_> {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), &'static str> {
        self.0.read(offset, bytes).map_err(|err| {
            log::warn!("MIDI JSON read {err:?}");
            "MIDI JSON flash read failed"
        })
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), &'static str> {
        self.0.write(offset, bytes).map_err(|err| {
            log::warn!("MIDI JSON write {err:?}");
            "MIDI JSON flash write failed"
        })
    }
    fn erase(&mut self, start: u32, end: u32) -> Result<(), &'static str> {
        self.0.erase(start, end).map_err(|err| {
            log::warn!("MIDI JSON erase {err:?}");
            "MIDI JSON flash erase failed"
        })
    }
}

fn with_json<T>(
    label: &str,
    capacity: u32,
    f: impl FnOnce(&mut JsonStorage<'_, '_>) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let flash = flash().ok_or("Flash unavailable")?;
    let mut buf = vec![0; PARTITION_TABLE_MAX_LEN];
    let table =
        read_partition_table(&mut *flash, &mut buf).map_err(|_| "Cannot read partition table")?;
    let entry = table
        .iter()
        .find(|entry| entry.label_as_str() == label)
        .ok_or("Configuration partition missing; flash using flash.sh")?;
    if entry.len() < capacity {
        return Err("Configuration partition too small");
    }
    f(&mut JsonStorage(entry.as_flash_region(flash))).inspect_err(|error| {
        log::warn!("Configuration partition {label}: {error}");
    })
}

pub fn load_midi_json() -> Result<Option<String>, &'static str> {
    with_json("midi", json_record::CAPACITY, |storage| {
        json_record::load(storage)
    })
}

pub fn save_midi_json(json: &str) -> Result<(), &'static str> {
    let _pause = crate::bounce::FlashPause::begin()?;
    with_json("midi", json_record::CAPACITY, |storage| {
        json_record::save(storage, json)
    })
}

const SCENES_SLOT: u32 = 1024 * 1024;

pub fn load_scenes_json() -> Result<Option<String>, &'static str> {
    with_json("scenes", SCENES_SLOT * 2, |storage| {
        json_record::load_sized(storage, SCENES_SLOT, crate::scenes::data::MAX_JSON)
    })
}

pub fn save_scenes_json(json: &str) -> Result<(), &'static str> {
    let _pause = crate::bounce::FlashPause::begin()?;
    with_json("scenes", SCENES_SLOT * 2, |storage| {
        json_record::save_sized(storage, json, SCENES_SLOT, crate::scenes::data::MAX_JSON)
    })
}
