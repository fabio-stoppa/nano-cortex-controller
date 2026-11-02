//! Persists the screen layout in the first sector of the `nvs` partition.
//!
//! Current MIDI and performance configuration live in separate partitions.
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

fn record(mode: u8) -> [u8; 8] {
    let mut r = [0u8; 8];
    r[..4].copy_from_slice(&MAGIC.to_le_bytes());
    r[4] = mode;
    r[5] = !mode;
    r
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
        let mut r = [0u8; 8];
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
    let saved = with_region(|region| {
        region
            .erase(0, SECTOR)
            .map_err(|e| log::warn!("erase {e:?}"))
            .ok()?;
        region
            .write(0, &record(mode))
            .map_err(|e| log::warn!("write {e:?}"))
            .ok()
    });
    if saved.is_none() {
        log::warn!("could not save layout");
    }
}

mod midi_record;
mod json_record;
mod reset;

pub fn initialize() -> Result<bool, &'static str> {
    let flash = flash().ok_or("Flash unavailable for controller factory reset")?;
    let mut buf = vec![0; PARTITION_TABLE_MAX_LEN];
    let table = read_partition_table(&mut *flash, &mut buf)
        .map_err(|_| "Cannot read partition table for controller factory reset")?;
    let find = |label: &str, minimum: u32| {
        let entry = table.iter().find(|entry| entry.label_as_str() == label)
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
    let performance = find("performance", PERFORMANCE_SLOT * 2)?;
    let nvs_size = nvs.len();
    let midi_size = midi.len();
    let performance_size = performance.len();
    let ranges = [
        (nvs.offset(), nvs_size),
        (midi.offset(), midi_size),
        (performance.offset(), performance_size),
    ];
    for (index, &(offset, size)) in ranges.iter().enumerate() {
        let end = offset.checked_add(size).ok_or("Controller reset partition range overflow")?;
        if offset % SECTOR != 0 || end > flash.capacity() as u32
            || ranges[..index].iter().any(|&(old_offset, old_size)| {
                offset < old_offset + old_size && old_offset < end
            })
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
            self.entry.as_flash_region(&mut **self.flash.borrow_mut()).read(offset, bytes)
                .map_err(|_| "Controller reset flash read failed")
        }
        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), &'static str> {
            self.entry.as_flash_region(&mut **self.flash.borrow_mut()).write(offset, bytes)
                .map_err(|_| "Controller reset flash write failed")
        }
        fn erase(&mut self, start: u32, end: u32) -> Result<(), &'static str> {
            self.entry.as_flash_region(&mut **self.flash.borrow_mut()).erase(start, end)
                .map_err(|_| "Controller reset flash erase failed")
        }
    }
    let flash = core::cell::RefCell::new(flash);
    reset::initialize(
        &mut Regions { flash: &flash, entry: nvs }, nvs_size,
        &mut Regions { flash: &flash, entry: midi }, midi_size,
        &mut Regions { flash: &flash, entry: performance }, performance_size,
    )
}

pub fn save_midi(keys: &[u16]) -> bool {
    let result = crate::midi::json_for_keys(keys).and_then(|json| save_midi_json(&json));
    if let Err(err) = result { log::warn!("Could not save MIDI bindings: {err}"); }
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

fn with_json<T>(label: &str, capacity: u32, f: impl FnOnce(&mut JsonStorage<'_, '_>) -> Result<T, &'static str>) -> Result<T, &'static str> {
    let flash = flash().ok_or("Flash unavailable")?;
    let mut buf = vec![0; PARTITION_TABLE_MAX_LEN];
    let table = read_partition_table(&mut *flash, &mut buf).map_err(|_| "Cannot read partition table")?;
    let entry = table.iter().find(|entry| entry.label_as_str() == label)
        .ok_or("Configuration partition missing; flash using flash.sh")?;
    if entry.len() < capacity {
        return Err("Configuration partition too small");
    }
    f(&mut JsonStorage(entry.as_flash_region(flash))).inspect_err(|error| {
        log::warn!("Configuration partition {label}: {error}");
    })
}

pub fn load_midi_json() -> Result<Option<String>, &'static str> {
    with_json("midi", json_record::CAPACITY, |storage| json_record::load(storage))
}

pub fn save_midi_json(json: &str) -> Result<(), &'static str> {
    with_json("midi", json_record::CAPACITY, |storage| json_record::save(storage, json))
}

const PERFORMANCE_SLOT: u32 = 1024 * 1024;

pub fn load_performance_json() -> Result<Option<String>, &'static str> {
    with_json("performance", PERFORMANCE_SLOT * 2, |storage| {
        json_record::load_sized(storage, PERFORMANCE_SLOT, crate::performance::data::MAX_JSON)
    })
}

pub fn save_performance_json(json: &str) -> Result<(), &'static str> {
    with_json("performance", PERFORMANCE_SLOT * 2, |storage| {
        json_record::save_sized(storage, json, PERFORMANCE_SLOT, crate::performance::data::MAX_JSON)
    })
}
