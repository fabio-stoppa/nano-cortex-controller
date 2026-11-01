pub const MAX: usize = 256;
pub const SIZE: usize = 8 + MAX * 2 + 4;
const MAGIC: u32 = 0x4d49_4432;
const LEGACY_MAGIC: u32 = 0x4d49_4431;
const LEGACY_MAX: usize = 64;

pub(super) fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
pub fn encode(keys: &[u16]) -> Option<[u8; SIZE]> {
    if keys.len() > MAX {
        return None;
    }
    let mut record = [0u8; SIZE];
    record[..4].copy_from_slice(&MAGIC.to_le_bytes());
    record[4..6].copy_from_slice(&(keys.len() as u16).to_le_bytes());
    for (i, key) in keys.iter().enumerate() {
        record[8 + i * 2..10 + i * 2].copy_from_slice(&key.to_le_bytes());
    }
    let crc = checksum(&record[..SIZE - 4]);
    record[SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
    Some(record)
}

pub fn decode(record: &[u8], count: usize) -> Option<[u16; MAX]> {
    if count > MAX {
        return None;
    }
    let magic = u32::from_le_bytes(record.get(..4)?.try_into().ok()?);
    let stored = if magic == LEGACY_MAGIC {
        let old = record.get(..8 + LEGACY_MAX * 2)?;
        let sum = old[8..].iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
        let stored = usize::from(old[4]);
        if stored > LEGACY_MAX || old[5] != !sum {
            return None;
        }
        stored
    } else if magic == MAGIC {
        let record = record.get(..SIZE)?;
        let stored = usize::from(u16::from_le_bytes(record[4..6].try_into().ok()?));
        let crc = u32::from_le_bytes(record[SIZE - 4..].try_into().ok()?);
        if stored > MAX || crc != checksum(&record[..SIZE - 4]) {
            return None;
        }
        stored
    } else {
        return None;
    };
    let mut keys = [0u16; MAX];
    for (i, key) in keys.iter_mut().enumerate().take(stored.min(count)) {
        *key = u16::from_le_bytes(record[8 + i * 2..10 + i * 2].try_into().ok()?);
    }
    Some(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_bindings_round_trip_including_shared_keys() {
        let mut keys = [0x9107; MAX];
        keys[0] = 0;
        keys[MAX - 1] = 0xa003;
        assert_eq!(decode(&encode(&keys).unwrap(), MAX), Some(keys));
        assert!(encode(&[0; MAX + 1]).is_none());
    }

    #[test]
    fn legacy_indices_are_preserved_and_new_actions_unassigned() {
        let mut old = [0u8; 8 + LEGACY_MAX * 2];
        old[..4].copy_from_slice(&LEGACY_MAGIC.to_le_bytes());
        old[4] = 42;
        old[8..10].copy_from_slice(&0x9107u16.to_le_bytes());
        old[8 + 41 * 2..10 + 41 * 2].copy_from_slice(&0x9234u16.to_le_bytes());
        old[5] = !old[8..].iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
        let keys = decode(&old, MAX).unwrap();
        assert_eq!(keys[0], 0x9107);
        assert_eq!(keys[41], 0x9234);
        assert!(keys[42..].iter().all(|key| *key == 0));
        assert_eq!(decode(&encode(&keys).unwrap(), MAX), Some(keys));
        old[8] ^= 1;
        assert!(decode(&old, MAX).is_none());
    }

    #[test]
    fn corrupt_and_truncated_records_are_rejected() {
        let record = encode(&[0x9001; MAX]).unwrap();
        for index in [0, 4, 8, SIZE - 1] {
            let mut corrupt = record;
            corrupt[index] ^= 1;
            assert!(decode(&corrupt, MAX).is_none());
        }
        assert!(decode(&record[..SIZE - 1], MAX).is_none());
        assert!(decode(&record, MAX + 1).is_none());
        assert_eq!(decode(&record, 1).unwrap()[1], 0);
    }
}
