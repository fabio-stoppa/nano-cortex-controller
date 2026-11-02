use alloc::{vec, vec::Vec};

pub const SECTOR: usize = 512;
pub const SECTORS: usize = 6144;
pub const MAX_MIDI: usize = 48 * 1024;
pub const MAX_SCENES: usize = crate::scenes::data::MAX_JSON;
pub const MAX_PREFERENCES: usize = crate::settings::preferences::MAX_JSON;
const FAT_SECTORS: usize = 9;
const ROOT: usize = 1 + 2 * FAT_SECTORS;
const DATA: usize = ROOT + 16;
const CLUSTER: usize = 2 * SECTOR;
const CLUSTERS: usize = (SECTORS - DATA) / 2;
const LFN_OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];

/// Disposable FAT12 image; flash is never exposed as writable USB blocks.
pub struct Disk {
    bytes: Vec<u8>,
}

impl Disk {
    pub fn new(
        midi: &[u8],
        scenes: &[u8],
        preferences: &[u8],
        help: &[u8],
    ) -> Result<Self, &'static str> {
        if midi.is_empty() || midi.len() > MAX_MIDI {
            return Err("midi.json must be 1 byte to 48 KiB");
        }
        if scenes.is_empty() || scenes.len() > MAX_SCENES {
            return Err("scenes.json exceeds the 1 MiB journal slot or is empty");
        }
        if preferences.is_empty() || preferences.len() > MAX_PREFERENCES {
            return Err("preferences.json must be 1 byte to 4 KiB");
        }
        let mut disk = Self {
            bytes: vec![0; SECTORS * SECTOR],
        };
        let boot = &mut disk.bytes[..SECTOR];
        boot[..11].copy_from_slice(b"\xeb\x3c\x90NANOCFG ");
        boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        boot[13] = 2;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 2;
        boot[17..19].copy_from_slice(&256u16.to_le_bytes());
        boot[19..21].copy_from_slice(&(SECTORS as u16).to_le_bytes());
        boot[21] = 0xf8;
        boot[22..24].copy_from_slice(&(FAT_SECTORS as u16).to_le_bytes());
        boot[24..26].copy_from_slice(&32u16.to_le_bytes());
        boot[26..28].copy_from_slice(&64u16.to_le_bytes());
        boot[38] = 0x29;
        boot[39..43].copy_from_slice(&0x4e414e4fu32.to_le_bytes());
        boot[43..54].copy_from_slice(b"NANO CONFIG");
        boot[54..62].copy_from_slice(b"FAT12   ");
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);
        disk.set_fat(0, 0xff8);
        disk.set_fat(1, 0xfff);
        let root = ROOT * SECTOR;
        disk.bytes[root..root + 11].copy_from_slice(b"NANO CONFIG");
        disk.bytes[root + 11] = 0x08;
        disk.long_name(1, b"MIDI~1  JSN", b"midi.json");
        let next = disk.add_file(2, b"MIDI~1  JSN", 2, midi)?;
        disk.long_name(3, b"SCENES~1JSN", b"scenes.json");
        let next = disk.add_file(4, b"SCENES~1JSN", next, scenes)?;
        let next = disk.add_file(5, b"README  TXT", next, help)?;
        disk.long_name(6, b"PREFER~1JSN", b"preferences.json");
        disk.add_file(8, b"PREFER~1JSN", next, preferences)?;
        Ok(disk)
    }

    fn checksum(name: &[u8]) -> u8 {
        name.iter()
            .fold(0u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte))
    }

    fn long_name(&mut self, entry: usize, alias: &[u8; 11], name: &[u8]) {
        let entries = name.len().div_ceil(13);
        for index in 0..entries {
            let ordinal = entries - index;
            let offset = ROOT * SECTOR + (entry + index) * 32;
            let lfn = &mut self.bytes[offset..offset + 32];
            lfn.fill(0xff);
            lfn[0] = ordinal as u8 | if index == 0 { 0x40 } else { 0 };
            lfn[11] = 0x0f;
            lfn[12] = 0;
            lfn[13] = Self::checksum(alias);
            lfn[26..28].fill(0);
            for (i, offset) in LFN_OFFSETS.iter().enumerate() {
                let position = (ordinal - 1) * 13 + i;
                let character = if position < name.len() {
                    u16::from(name[position])
                } else if position == name.len() {
                    0
                } else {
                    0xffff
                };
                lfn[*offset..*offset + 2].copy_from_slice(&character.to_le_bytes());
            }
        }
    }

    fn set_fat(&mut self, cluster: usize, value: u16) {
        let index = cluster + cluster / 2;
        for fat in 0..2 {
            let offset = (1 + fat * FAT_SECTORS) * SECTOR + index;
            let word = u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]]);
            let word = if cluster % 2 == 0 {
                (word & 0xf000) | (value & 0xfff)
            } else {
                (word & 0x000f) | (value & 0xfff) << 4
            };
            self.bytes[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
        }
    }

    fn add_file(
        &mut self,
        entry: usize,
        name: &[u8; 11],
        start: usize,
        content: &[u8],
    ) -> Result<usize, &'static str> {
        let blocks = content.len().div_ceil(CLUSTER);
        if start + blocks > CLUSTERS + 2 {
            return Err("USB configuration disk full");
        }
        let offset = ROOT * SECTOR + entry * 32;
        self.bytes[offset..offset + 11].copy_from_slice(name);
        self.bytes[offset + 11] = 0x20;
        self.bytes[offset + 24..offset + 26].copy_from_slice(&0x5c21u16.to_le_bytes());
        self.bytes[offset + 26..offset + 28].copy_from_slice(&(start as u16).to_le_bytes());
        self.bytes[offset + 28..offset + 32].copy_from_slice(&(content.len() as u32).to_le_bytes());
        for (i, chunk) in content.chunks(CLUSTER).enumerate() {
            let cluster = start + i;
            self.set_fat(
                cluster,
                if i + 1 == blocks {
                    0xfff
                } else {
                    (cluster + 1) as u16
                },
            );
            let offset = DATA * SECTOR + (cluster - 2) * CLUSTER;
            self.bytes[offset..offset + chunk.len()].copy_from_slice(chunk);
        }
        Ok(start + blocks)
    }

    pub fn read(&self, lba: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        buf.copy_from_slice(&self.bytes[Self::range(lba, buf.len())?]);
        Ok(())
    }

    pub fn write(&mut self, lba: u32, data: &[u8]) -> Result<(), &'static str> {
        self.bytes[Self::range(lba, data.len())?].copy_from_slice(data);
        Ok(())
    }

    fn range(lba: u32, length: usize) -> Result<core::ops::Range<usize>, &'static str> {
        let start = (lba as usize)
            .checked_mul(SECTOR)
            .ok_or("Invalid USB disk address")?;
        let end = start.checked_add(length).ok_or("Invalid USB disk length")?;
        if length == 0 || length % SECTOR != 0 || end > SECTORS * SECTOR {
            return Err("USB disk access out of range");
        }
        Ok(start..end)
    }

    pub fn midi(&self) -> Result<Vec<u8>, &'static str> {
        self.file(b"midi.json", MAX_MIDI)
    }

    pub fn scenes(&self) -> Result<Vec<u8>, &'static str> {
        self.file(b"scenes.json", MAX_SCENES)
    }

    pub fn preferences(&self) -> Result<Vec<u8>, &'static str> {
        self.file(b"preferences.json", MAX_PREFERENCES)
    }

    fn file(&self, name: &[u8], maximum: usize) -> Result<Vec<u8>, &'static str> {
        let boot = &self.bytes[..SECTOR];
        if boot[11..24] != [0, 2, 2, 1, 0, 2, 0, 1, 0, 24, 0xf8, 9, 0]
            || boot[510..512] != [0x55, 0xaa]
            || boot[32..36] != [0; 4]
        {
            return Err("USB disk was reformatted; changes rejected");
        }
        let fat = &self.bytes[SECTOR..(1 + FAT_SECTORS) * SECTOR];
        if fat != &self.bytes[(1 + FAT_SECTORS) * SECTOR..ROOT * SECTOR] {
            return Err("FAT copies disagree; changes rejected");
        }
        let mut found = None;
        let mut long_name = None;
        let mut characters = [0xffffu16; 26];
        let mut expected = 0;
        for entry in self.bytes[ROOT * SECTOR..DATA * SECTOR].chunks_exact(32) {
            if entry[0] == 0 {
                break;
            }
            if entry[0] == 0xe5 {
                long_name = None;
                continue;
            }
            if entry[11] == 0x0f {
                let ordinal = usize::from(entry[0] & 0x3f);
                if entry[0] & 0x40 != 0 {
                    characters.fill(0xffff);
                    expected = ordinal;
                    long_name = Some(entry[13]);
                }
                if !(1..=2).contains(&ordinal)
                    || entry[0] & 0x80 != 0
                    || ordinal != expected
                    || entry[12] != 0
                    || entry[26..28] != [0, 0]
                    || long_name != Some(entry[13])
                {
                    long_name = None;
                    expected = 0;
                    continue;
                }
                for (i, &offset) in LFN_OFFSETS.iter().enumerate() {
                    characters[(ordinal - 1) * 13 + i] =
                        u16::from_le_bytes([entry[offset], entry[offset + 1]]);
                }
                expected -= 1;
                continue;
            }
            let checksum_matches = long_name.take() == Some(Self::checksum(&entry[..11]));
            let target = expected == 0
                && checksum_matches
                && characters.iter().enumerate().all(|(i, &character)| {
                    if i < name.len() {
                        character <= 127 && (character as u8).eq_ignore_ascii_case(&name[i])
                    } else if i == name.len() {
                        character == 0
                    } else {
                        character == 0xffff || character == 0
                    }
                });
            if !target || entry[11] & 0x18 != 0 {
                continue;
            }
            if found.is_some() {
                return Err("Duplicate configuration files");
            }
            found = Some(entry);
        }
        let entry = found.ok_or("Configuration file missing; all three files are required")?;
        let length = u32::from_le_bytes(entry[28..32].try_into().unwrap()) as usize;
        if length == 0 || length > maximum || entry[20..22] != [0, 0] {
            return Err("Invalid configuration file size or cluster");
        }
        let mut cluster = usize::from(u16::from_le_bytes(entry[26..28].try_into().unwrap()));
        let mut seen = vec![false; CLUSTERS + 2];
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            if !(2..CLUSTERS + 2).contains(&cluster) {
                return Err("Configuration cluster out of range");
            }
            if seen[cluster] {
                return Err("Cyclic configuration cluster chain");
            }
            seen[cluster] = true;
            let offset = DATA * SECTOR + (cluster - 2) * CLUSTER;
            let count = core::cmp::min(CLUSTER, length - result.len());
            result.extend_from_slice(&self.bytes[offset..offset + count]);
            let index = cluster + cluster / 2;
            let word = u16::from_le_bytes([fat[index], fat[index + 1]]);
            let next = if cluster % 2 == 0 {
                word & 0xfff
            } else {
                word >> 4
            };
            if result.len() == length {
                if next < 0xff8 {
                    return Err("Configuration chain does not end at file size");
                }
            } else if next >= 0xff8 {
                return Err("Truncated configuration cluster chain");
            }
            cluster = usize::from(next);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    #[test]
    fn computer_reads_and_replaces_all_json_files() {
        let mut disk = Disk::new(b"midi", b"scenes", b"preferences", b"help").unwrap();
        {
            let fs = fatfs::FileSystem::new(Cursor::new(&mut disk.bytes), fatfs::FsOptions::new())
                .unwrap();
            assert_eq!(fs.fat_type(), fatfs::FatType::Fat12);
            let root = fs.root_dir();
            for (name, original, length) in [
                ("midi.json", "midi", 2000),
                ("scenes.json", "scenes", 10000),
                ("preferences.json", "preferences", 3000),
            ] {
                let mut read = Vec::new();
                root.open_file(name)
                    .unwrap()
                    .read_to_end(&mut read)
                    .unwrap();
                assert_eq!(read, original.as_bytes());
                {
                    let mut temp = root.create_file("replacement.json").unwrap();
                    temp.write_all(&vec![b' '; length]).unwrap();
                }
                root.remove(name).unwrap();
                root.rename("replacement.json", &root, name).unwrap();
            }
            drop(root);
            fs.unmount().unwrap();
        }
        assert_eq!(disk.midi().unwrap(), vec![b' '; 2000]);
        assert_eq!(disk.scenes().unwrap(), vec![b' '; 10000]);
        assert_eq!(disk.preferences().unwrap(), vec![b' '; 3000]);
    }

    #[test]
    fn maximum_files_fit_with_room_for_editor_temporary_copy() {
        let midi = vec![b'm'; MAX_MIDI];
        let scenes = vec![b's'; MAX_SCENES];
        let preferences = vec![b'p'; MAX_PREFERENCES];
        let mut disk = Disk::new(&midi, &scenes, &preferences, b"help").unwrap();
        {
            let fs = fatfs::FileSystem::new(Cursor::new(&mut disk.bytes), fatfs::FsOptions::new())
                .unwrap();
            let root = fs.root_dir();
            {
                let mut replacement = root.create_file("temporary.json").unwrap();
                replacement.write_all(&scenes).unwrap();
            }
            root.remove("scenes.json").unwrap();
            root.rename("temporary.json", &root, "scenes.json").unwrap();
            drop(root);
            fs.unmount().unwrap();
        }
        assert_eq!(disk.midi().unwrap(), midi);
        assert_eq!(disk.scenes().unwrap(), scenes);
        assert_eq!(disk.preferences().unwrap(), preferences);
    }

    #[test]
    fn fragmented_files_follow_fat_order() {
        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        let data: Vec<u8> = (0..8192).map(|i| (i % 251) as u8).collect();
        {
            let fs = fatfs::FileSystem::new(Cursor::new(&mut disk.bytes), fatfs::FsOptions::new())
                .unwrap();
            let root = fs.root_dir();
            let mut file = root.open_file("scenes.json").unwrap();
            file.truncate().unwrap();
            for (i, chunk) in data.chunks(CLUSTER).enumerate() {
                file.write_all(chunk).unwrap();
                root.create_file(&alloc::format!("gap{i}.txt"))
                    .unwrap()
                    .write_all(&vec![1; CLUSTER])
                    .unwrap();
            }
            drop(file);
            drop(root);
            fs.unmount().unwrap();
        }
        assert_eq!(disk.scenes().unwrap(), data);
    }

    #[test]
    fn corrupt_or_missing_files_and_reformatting_are_rejected() {
        let mut disk = Disk::new(&vec![1; 2000], b"{}", b"{}", b"help").unwrap();
        disk.set_fat(2, 2);
        assert!(disk.midi().unwrap_err().contains("Cyclic"));
        disk.set_fat(2, 0xfff);
        assert!(disk.midi().unwrap_err().contains("Truncated"));
        disk.set_fat(2, 0xff7);
        assert!(disk.midi().unwrap_err().contains("out of range"));
        disk.bytes[SECTOR + 3] ^= 1;
        assert!(disk.midi().unwrap_err().contains("disagree"));
        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        disk.bytes[ROOT * SECTOR + 64] = 0xe5;
        assert!(disk.midi().unwrap_err().contains("missing"));
        disk.bytes[13] = 1;
        assert!(disk.scenes().unwrap_err().contains("reformatted"));
    }

    #[test]
    fn bounds_and_size_limits_are_enforced() {
        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        let mut block = [0; SECTOR];
        assert!(disk.read(SECTORS as u32, &mut block).is_err());
        assert!(disk.write(u32::MAX, &block).is_err());
        assert!(disk.read(0, &mut block[..10]).is_err());
        assert!(disk.write(0, &[]).is_err());
        assert!(Disk::new(&vec![0; MAX_MIDI + 1], b"{}", b"{}", b"help").is_err());
        assert!(Disk::new(b"{}", &vec![0; MAX_SCENES + 1], b"{}", b"help").is_err());
        assert!(Disk::new(b"{}", b"{}", &vec![0; MAX_PREFERENCES + 1], b"help").is_err());
        assert!(Disk::new(b"", b"{}", b"{}", b"help").is_err());
        assert!(Disk::new(b"{}", b"{}", b"", b"help").is_err());
    }

    #[test]
    fn duplicate_names_invalid_lfn_and_oversized_directory_entries_are_rejected() {
        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        let root = ROOT * SECTOR;
        disk.bytes.copy_within(root + 32..root + 96, root + 9 * 32);
        assert!(disk.midi().unwrap_err().contains("Duplicate"));

        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        disk.bytes[root + 32 + 13] ^= 1;
        assert!(disk.midi().unwrap_err().contains("missing"));

        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        disk.bytes[root + 64 + 28..root + 64 + 32]
            .copy_from_slice(&((MAX_MIDI + 1) as u32).to_le_bytes());
        assert!(disk.midi().unwrap_err().contains("size"));
    }

    #[test]
    fn missing_duplicate_and_incomplete_preferences_names_are_rejected() {
        let root = ROOT * SECTOR;
        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        disk.bytes[root + 8 * 32] = 0xe5;
        assert!(disk.preferences().unwrap_err().contains("missing"));

        let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
        disk.bytes
            .copy_within(root + 6 * 32..root + 9 * 32, root + 9 * 32);
        assert!(disk.preferences().unwrap_err().contains("Duplicate"));

        for (entry, field) in [(6, 0), (7, 0), (7, 13)] {
            let mut disk = Disk::new(b"{}", b"{}", b"{}", b"help").unwrap();
            disk.bytes[root + entry * 32 + field] ^= 1;
            assert!(disk.preferences().unwrap_err().contains("missing"));
        }
    }
}
