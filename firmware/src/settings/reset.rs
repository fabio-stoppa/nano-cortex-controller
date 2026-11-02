use super::json_record::Storage;

const SECTOR: u32 = 4096;
const RESET_MARKER: &[u8; 16] = b"NCCRESET00000003";

fn erase_verified(storage: &mut impl Storage, size: u32) -> Result<(), &'static str> {
    storage.erase(0, size)?;
    let mut bytes = [0; 256];
    for offset in (0..size).step_by(bytes.len()) {
        storage.read(offset, &mut bytes)?;
        if bytes.iter().any(|byte| *byte != 0xff) {
            return Err("Controller factory reset erase verification failed");
        }
    }
    Ok(())
}

pub fn initialize(
    nvs: &mut impl Storage,
    nvs_size: u32,
    midi: &mut impl Storage,
    midi_size: u32,
    performance: &mut impl Storage,
    performance_size: u32,
) -> Result<bool, &'static str> {
    if nvs_size < 3 * SECTOR
        || [nvs_size, midi_size, performance_size]
            .iter()
            .any(|size| *size == 0 || size % SECTOR != 0)
    {
        return Err("Controller factory reset partition geometry invalid");
    }
    let marker_offset = nvs_size - SECTOR;
    let mut marker = [0; 16];
    nvs.read(marker_offset, &mut marker)?;
    if &marker == RESET_MARKER {
        return Ok(false);
    }
    erase_verified(midi, midi_size)?;
    erase_verified(performance, performance_size)?;
    erase_verified(nvs, nvs_size)?;
    nvs.write(marker_offset, RESET_MARKER)?;
    nvs.read(marker_offset, &mut marker)?;
    if &marker != RESET_MARKER {
        return Err("Controller factory reset marker verification failed");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Clone)]
    struct Flash {
        bytes: Vec<u8>,
        fail: Option<&'static str>,
        operations: usize,
    }

    impl Flash {
        fn dirty(size: u32) -> Self {
            Self {
                bytes: alloc::vec![0; size as usize],
                fail: None,
                operations: 0,
            }
        }
    }

    impl Storage for Flash {
        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), &'static str> {
            if self.fail == Some("read") {
                return Err("Mock reset read failed");
            }
            bytes.copy_from_slice(&self.bytes[offset as usize..offset as usize + bytes.len()]);
            Ok(())
        }
        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), &'static str> {
            self.operations += 1;
            if self.fail == Some("write") {
                self.bytes[offset as usize] = bytes[0];
                return Err("Mock reset marker write interrupted");
            }
            if self.fail != Some("bad-write") {
                self.bytes[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
            }
            Ok(())
        }
        fn erase(&mut self, start: u32, end: u32) -> Result<(), &'static str> {
            self.operations += 1;
            if self.fail == Some("erase") {
                return Err("Mock reset erase failed");
            }
            if self.fail != Some("bad-erase") {
                self.bytes[start as usize..end as usize].fill(0xff);
            }
            Ok(())
        }
    }

    #[test]
    fn erases_all_data_and_both_journal_generations_only_once() {
        let mut nvs = Flash::dirty(6 * SECTOR);
        let mut midi = Flash::dirty(32 * SECTOR);
        let mut performance = Flash::dirty(512 * SECTOR);
        assert!(initialize(
            &mut nvs,
            6 * SECTOR,
            &mut midi,
            32 * SECTOR,
            &mut performance,
            512 * SECTOR
        )
        .unwrap());
        assert!(midi.bytes.iter().all(|byte| *byte == 0xff));
        assert!(performance.bytes.iter().all(|byte| *byte == 0xff));
        assert!(nvs.bytes[..5 * SECTOR as usize]
            .iter()
            .all(|byte| *byte == 0xff));
        let operations = (nvs.operations, midi.operations, performance.operations);
        nvs.bytes[0] = 42;
        midi.bytes[0] = 43;
        performance.bytes[0] = 44;
        assert!(!initialize(
            &mut nvs,
            6 * SECTOR,
            &mut midi,
            32 * SECTOR,
            &mut performance,
            512 * SECTOR
        )
        .unwrap());
        assert_eq!(
            operations,
            (nvs.operations, midi.operations, performance.operations)
        );
        assert_eq!(
            (nvs.bytes[0], midi.bytes[0], performance.bytes[0]),
            (42, 43, 44)
        );
    }

    #[test]
    fn failures_never_mark_reset_complete_and_retry_erases_everything() {
        for target in 0..3 {
            for failure in ["read", "erase", "bad-erase", "write", "bad-write"] {
                if target != 0 && matches!(failure, "write" | "bad-write") {
                    continue;
                }
                let mut nvs = Flash::dirty(3 * SECTOR);
                let mut midi = Flash::dirty(SECTOR);
                let mut performance = Flash::dirty(SECTOR);
                match target {
                    0 => nvs.fail = Some(failure),
                    1 => midi.fail = Some(failure),
                    _ => performance.fail = Some(failure),
                }
                assert!(initialize(
                    &mut nvs,
                    3 * SECTOR,
                    &mut midi,
                    SECTOR,
                    &mut performance,
                    SECTOR
                )
                .is_err());
                assert_ne!(
                    &nvs.bytes[2 * SECTOR as usize..2 * SECTOR as usize + 16],
                    RESET_MARKER
                );
                nvs.fail = None;
                midi.fail = None;
                performance.fail = None;
                assert!(initialize(
                    &mut nvs,
                    3 * SECTOR,
                    &mut midi,
                    SECTOR,
                    &mut performance,
                    SECTOR
                )
                .unwrap());
                assert!(midi
                    .bytes
                    .iter()
                    .chain(&performance.bytes)
                    .all(|byte| *byte == 0xff));
            }
        }
    }

    #[test]
    fn invalid_geometry_never_erases_any_partition() {
        let mut nvs = Flash::dirty(3 * SECTOR);
        let mut midi = Flash::dirty(SECTOR);
        let mut performance = Flash::dirty(SECTOR);
        assert!(initialize(
            &mut nvs,
            2 * SECTOR,
            &mut midi,
            SECTOR,
            &mut performance,
            SECTOR
        )
        .is_err());
        assert_eq!(
            (nvs.operations, midi.operations, performance.operations),
            (0, 0, 0)
        );
    }
}
