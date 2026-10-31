//! Persists the screen layout in the first sector of the `nvs` partition.
//!
//! Nothing else in this firmware uses that partition, so one raw sector holds
//! a small checked record. Writes are skipped when the value has not changed.

use alloc::vec;
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
    let table = read_partition_table(&mut *flash, &mut buf).map_err(|e| log::warn!("ptable {e:?}")).ok()?;
    let entry = table
        .find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
        .map_err(|e| log::warn!("find {e:?}")).ok()??;
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
        region.erase(0, SECTOR).map_err(|e| log::warn!("erase {e:?}")).ok()?;
        region.write(0, &record(mode)).map_err(|e| log::warn!("write {e:?}")).ok()
    });
    if saved.is_none() {
        log::warn!("could not save layout");
    }
}
