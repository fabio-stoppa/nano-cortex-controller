use super::midi_record::checksum;
use alloc::string::String;
use alloc::vec;

pub const SLOT: u32 = 64 * 1024;
pub const CAPACITY: u32 = SLOT * 2;
pub const MAX_JSON: usize = 48 * 1024;
const HEADER: usize = 32;
const MAGIC: &[u8; 8] = b"MIDIJS01";

pub trait Storage {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), &'static str>;
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), &'static str>;
    fn erase(&mut self, start: u32, end: u32) -> Result<(), &'static str>;
}

enum SlotError {
    Read(&'static str),
    Invalid(&'static str),
}

impl SlotError {
    fn message(&self) -> &'static str {
        match self {
            Self::Read(message) | Self::Invalid(message) => message,
        }
    }
}

fn slot(storage: &mut impl Storage, index: u32) -> Result<Option<(u64, String)>, SlotError> {
    let mut header = [0u8; HEADER];
    storage
        .read(index * SLOT, &mut header)
        .map_err(SlotError::Read)?;
    if header.iter().all(|byte| *byte == 0xff) {
        return Ok(None);
    }
    if &header[..8] != MAGIC
        || checksum(&header[..28]) != u32::from_le_bytes(header[28..32].try_into().unwrap())
    {
        return Err(SlotError::Invalid("Invalid MIDI JSON record header"));
    }
    let generation = u64::from_le_bytes(header[8..16].try_into().unwrap());
    let length = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    if length == 0 || length > MAX_JSON {
        return Err(SlotError::Invalid("Invalid MIDI JSON record length"));
    }
    let mut data = vec![0; length];
    storage
        .read(index * SLOT + HEADER as u32, &mut data)
        .map_err(SlotError::Read)?;
    if checksum(&data) != u32::from_le_bytes(header[20..24].try_into().unwrap()) {
        return Err(SlotError::Invalid("MIDI JSON record checksum failed"));
    }
    let text =
        String::from_utf8(data).map_err(|_| SlotError::Invalid("MIDI JSON record is not UTF-8"))?;
    Ok(Some((generation, text)))
}

enum Current {
    Empty,
    Valid(u32, u64, String),
    Damaged,
}

fn current(storage: &mut impl Storage) -> Result<Current, &'static str> {
    let a = slot(storage, 0);
    let b = slot(storage, 1);
    let mut invalid = false;
    for result in [&a, &b] {
        if let Err(err) = result {
            log::warn!("MIDI JSON storage: {}", err.message());
            if let SlotError::Read(message) = err {
                return Err(message);
            }
            invalid = true;
        }
    }
    Ok(match (a.ok().flatten(), b.ok().flatten()) {
        (Some((ga, _)), Some((gb, b))) if gb > ga => Current::Valid(1, gb, b),
        (Some((ga, a)), _) => Current::Valid(0, ga, a),
        (_, Some((gb, b))) => Current::Valid(1, gb, b),
        _ if invalid => Current::Damaged,
        _ => Current::Empty,
    })
}

pub fn load(storage: &mut impl Storage) -> Result<Option<String>, &'static str> {
    match current(storage)? {
        Current::Valid(_, _, json) => Ok(Some(json)),
        Current::Empty => Ok(None),
        Current::Damaged => Err("No valid MIDI JSON record; storage recovery required"),
    }
}

pub fn save(storage: &mut impl Storage, json: &str) -> Result<(), &'static str> {
    if json.is_empty() || json.len() > MAX_JSON {
        return Err("midi.json exceeds storage limits");
    }
    let (index, generation) = match current(storage)? {
        Current::Valid(_, _, old) if old == json => return Ok(()),
        Current::Valid(index, generation, _) => (
            1 - index,
            generation
                .checked_add(1)
                .ok_or("MIDI storage generation overflow")?,
        ),
        Current::Empty => (0, 1),
        Current::Damaged => {
            log::warn!("Rebuilding damaged MIDI JSON storage from supplied configuration");
            (0, 1)
        }
    };
    let offset = index * SLOT;
    storage.erase(offset, offset + SLOT)?;
    storage.write(offset + HEADER as u32, json.as_bytes())?;
    let mut check = vec![0; json.len()];
    storage.read(offset + HEADER as u32, &mut check)?;
    if check != json.as_bytes() {
        return Err("MIDI JSON write verification failed");
    }
    let mut header = [0; HEADER];
    header[..8].copy_from_slice(MAGIC);
    header[8..16].copy_from_slice(&generation.to_le_bytes());
    header[16..20].copy_from_slice(&(json.len() as u32).to_le_bytes());
    header[20..24].copy_from_slice(&checksum(json.as_bytes()).to_le_bytes());
    let crc = checksum(&header[..28]);
    header[28..32].copy_from_slice(&crc.to_le_bytes());
    storage.write(offset, &header)?;
    let verified = slot(storage, index)
        .map_err(|err| err.message())?
        .ok_or("MIDI JSON commit missing")?;
    if verified.0 != generation || verified.1 != json {
        return Err("MIDI JSON commit verification failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Flash {
        bytes: alloc::vec::Vec<u8>,
        budget: Option<usize>,
        erases: usize,
        read_failure: Option<u32>,
    }

    impl Storage for Flash {
        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), &'static str> {
            if self.read_failure == Some(offset) {
                return Err("Flash read failed");
            }
            bytes.copy_from_slice(&self.bytes[offset as usize..offset as usize + bytes.len()]);
            Ok(())
        }
        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), &'static str> {
            for (i, &byte) in bytes.iter().enumerate() {
                if let Some(budget) = &mut self.budget {
                    if *budget == 0 {
                        return Err("Power lost during write");
                    }
                    *budget -= 1;
                }
                self.bytes[offset as usize + i] &= byte;
            }
            Ok(())
        }
        fn erase(&mut self, start: u32, end: u32) -> Result<(), &'static str> {
            self.bytes[start as usize..end as usize].fill(0xff);
            self.erases += 1;
            Ok(())
        }
    }

    fn flash() -> Flash {
        Flash {
            bytes: vec![0xff; CAPACITY as usize],
            budget: None,
            erases: 0,
            read_failure: None,
        }
    }

    #[test]
    fn generations_round_trip_and_unchanged_file_does_not_wear_flash() {
        let mut flash = flash();
        assert_eq!(load(&mut flash).unwrap(), None);
        save(&mut flash, "{}").unwrap();
        assert_eq!(load(&mut flash).unwrap().as_deref(), Some("{}"));
        save(&mut flash, "{}").unwrap();
        assert_eq!(flash.erases, 1);
        save(&mut flash, "{\"bindings\":[]}").unwrap();
        assert_eq!(
            load(&mut flash).unwrap().as_deref(),
            Some("{\"bindings\":[]}")
        );
        save(&mut flash, "{}").unwrap();
        assert_eq!(load(&mut flash).unwrap().as_deref(), Some("{}"));
    }

    #[test]
    fn every_interrupted_data_or_header_write_preserves_previous_file() {
        let mut original = flash();
        save(&mut original, "{\"previous\":true}").unwrap();
        let changed = "{\"new\":true}";
        for limit in 0..changed.len() + HEADER {
            let mut flash = original.clone();
            flash.budget = Some(limit);
            assert!(save(&mut flash, changed).is_err(), "limit {limit}");
            assert_eq!(
                load(&mut flash).unwrap().as_deref(),
                Some("{\"previous\":true}"),
                "limit {limit}"
            );
        }
    }

    #[test]
    fn interrupted_initial_migration_can_be_retried() {
        let original = flash();
        let migrated = "{\"legacy\":true}";
        for limit in 0..migrated.len() + HEADER {
            let mut flash = original.clone();
            flash.budget = Some(limit);
            assert!(save(&mut flash, migrated).is_err());
            flash.budget = None;
            save(&mut flash, migrated).unwrap();
            assert_eq!(load(&mut flash).unwrap().as_deref(), Some(migrated));
        }
    }

    #[test]
    fn read_failure_never_rebuilds_or_overwrites_unknown_records() {
        let mut original = flash();
        save(&mut original, "{\"previous\":true}").unwrap();
        for offset in [0, SLOT, HEADER as u32] {
            let mut flash = original.clone();
            flash.read_failure = Some(offset);
            assert!(load(&mut flash).is_err());
            assert!(save(&mut flash, "{\"new\":true}").is_err());
            assert_eq!(flash.bytes, original.bytes);
            assert_eq!(flash.erases, original.erases);
            flash.read_failure = None;
            assert_eq!(
                load(&mut flash).unwrap().as_deref(),
                Some("{\"previous\":true}")
            );
        }
    }

    #[test]
    fn corrupt_latest_record_recovers_checked_previous_generation() {
        let mut flash = flash();
        save(&mut flash, "{\"previous\":true}").unwrap();
        save(&mut flash, "{\"new\":true}").unwrap();
        flash.bytes[SLOT as usize + HEADER] ^= 1;
        assert_eq!(
            load(&mut flash).unwrap().as_deref(),
            Some("{\"previous\":true}")
        );
        flash.bytes[HEADER] ^= 1;
        assert!(load(&mut flash).is_err());
    }

    #[test]
    fn maximum_file_size_and_oversized_rejection() {
        let mut flash = flash();
        let maximum = " ".repeat(MAX_JSON);
        save(&mut flash, &maximum).unwrap();
        assert_eq!(load(&mut flash).unwrap().unwrap(), maximum);
        assert!(save(&mut flash, &" ".repeat(MAX_JSON + 1)).is_err());
        assert!(save(&mut flash, "").is_err());
        assert_eq!(load(&mut flash).unwrap().unwrap(), maximum);
    }
}
