use alloc::vec;
use alloc::vec::Vec;

pub const SECTOR: usize = 512;
pub const SECTORS: usize = 2048;
pub const MAX_JSON: usize = 48 * 1024;
const FAT_SECTORS: usize = 6;
const ROOT_SECTORS: usize = 16;
const ROOT: usize = 1 + 2 * FAT_SECTORS;
const DATA: usize = ROOT + ROOT_SECTORS;
const CLUSTERS: usize = SECTORS - DATA;

/// A disposable FAT12 view. Only midi.json is committed after the host ejects.
pub struct Disk {
    bytes: Vec<u8>,
}

impl Disk {
    pub fn new(json: &[u8], help: &[u8]) -> Result<Self, &'static str> {
        if json.len() > MAX_JSON {
            return Err("midi.json exceeds 48 KiB");
        }
        let mut disk = Self {
            bytes: vec![0; SECTORS * SECTOR],
        };
        let boot = &mut disk.bytes[..SECTOR];
        boot[..11].copy_from_slice(b"\xeb\x3c\x90NANOMIDI");
        boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        boot[13] = 1;
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
        boot[43..54].copy_from_slice(b"NANO MIDI  ");
        boot[54..62].copy_from_slice(b"FAT12   ");
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);
        disk.set_fat(0, 0xff8);
        disk.set_fat(1, 0xfff);
        let root = ROOT * SECTOR;
        disk.bytes[root..root + 11].copy_from_slice(b"NANO MIDI  ");
        disk.bytes[root + 11] = 0x08;
        let alias = b"MIDI~1  JSN";
        let lfn = &mut disk.bytes[root + 32..root + 64];
        lfn.fill(0xff);
        lfn[0] = 0x41;
        lfn[11] = 0x0f;
        lfn[12] = 0;
        lfn[13] = Self::name_checksum(alias);
        lfn[26..28].fill(0);
        for (i, offset) in Self::LFN_OFFSETS.iter().enumerate() {
            let character = if i < 9 {
                u16::from(b"midi.json"[i])
            } else if i == 9 {
                0
            } else {
                0xffff
            };
            lfn[*offset..*offset + 2].copy_from_slice(&character.to_le_bytes());
        }
        let next = disk.add_file(2, alias, 2, json)?;
        disk.add_file(3, b"README  TXT", next, help)?;
        Ok(disk)
    }

    const LFN_OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];

    fn name_checksum(name: &[u8]) -> u8 {
        name.iter()
            .fold(0u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte))
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
        let blocks = content.len().div_ceil(SECTOR);
        if start + blocks > CLUSTERS + 2 {
            return Err("USB configuration disk full");
        }
        let offset = ROOT * SECTOR + entry * 32;
        self.bytes[offset..offset + 11].copy_from_slice(name);
        self.bytes[offset + 11] = 0x20;
        self.bytes[offset + 24..offset + 26].copy_from_slice(&0x5c21u16.to_le_bytes());
        self.bytes[offset + 26..offset + 28].copy_from_slice(&(start as u16).to_le_bytes());
        self.bytes[offset + 28..offset + 32].copy_from_slice(&(content.len() as u32).to_le_bytes());
        for (i, chunk) in content.chunks(SECTOR).enumerate() {
            let cluster = start + i;
            self.set_fat(
                cluster,
                if i + 1 == blocks {
                    0xfff
                } else {
                    (cluster + 1) as u16
                },
            );
            let offset = (DATA + cluster - 2) * SECTOR;
            self.bytes[offset..offset + chunk.len()].copy_from_slice(chunk);
        }
        Ok(start + blocks)
    }

    pub fn read(&self, lba: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        let range = Self::range(lba, buf.len())?;
        buf.copy_from_slice(&self.bytes[range]);
        Ok(())
    }

    pub fn write(&mut self, lba: u32, data: &[u8]) -> Result<(), &'static str> {
        let range = Self::range(lba, data.len())?;
        self.bytes[range].copy_from_slice(data);
        Ok(())
    }

    fn range(lba: u32, length: usize) -> Result<core::ops::Range<usize>, &'static str> {
        let start = (lba as usize)
            .checked_mul(SECTOR)
            .ok_or("Invalid disk address")?;
        let end = start.checked_add(length).ok_or("Invalid disk length")?;
        if length == 0 || length % SECTOR != 0 || end > SECTORS * SECTOR {
            return Err("USB disk access out of range");
        }
        Ok(start..end)
    }

    pub fn json(&self) -> Result<Vec<u8>, &'static str> {
        let boot = &self.bytes[..SECTOR];
        if boot[11..24] != [0, 2, 1, 1, 0, 2, 0, 1, 0, 8, 0xf8, 6, 0]
            || boot[510..512] != [0x55, 0xaa]
        {
            return Err("USB disk was reformatted; changes rejected");
        }
        let fat = &self.bytes[SECTOR..(1 + FAT_SECTORS) * SECTOR];
        if fat != &self.bytes[(1 + FAT_SECTORS) * SECTOR..ROOT * SECTOR] {
            return Err("FAT copies disagree; safely eject before finishing");
        }
        let mut found = None;
        let mut long_name = None;
        for entry in self.bytes[ROOT * SECTOR..DATA * SECTOR].chunks_exact(32) {
            if entry[0] == 0 {
                break;
            }
            if entry[0] == 0xe5 {
                long_name = None;
                continue;
            }
            if entry[11] == 0x0f {
                let target = entry[0] == 0x41
                    && entry[12] == 0
                    && entry[26..28] == [0, 0]
                    && Self::LFN_OFFSETS.iter().enumerate().all(|(i, &offset)| {
                        let character = u16::from_le_bytes([entry[offset], entry[offset + 1]]);
                        if i < 9 {
                            character <= 127
                                && (character as u8).eq_ignore_ascii_case(&b"midi.json"[i])
                        } else if i == 9 {
                            character == 0
                        } else {
                            character == 0xffff || character == 0
                        }
                    });
                long_name = target.then_some(entry[13]);
                continue;
            }
            let target = long_name
                .take()
                .is_some_and(|check| check == Self::name_checksum(&entry[..11]));
            if entry[11] & 0x18 != 0 {
                continue;
            }
            if target {
                if found.is_some() {
                    return Err("Duplicate midi.json files");
                }
                found = Some(entry);
            }
        }
        let entry = found.ok_or("midi.json is missing; previous bindings retained")?;
        let length =
            u32::from_le_bytes(entry[28..32].try_into().map_err(|_| "Invalid directory")?) as usize;
        if length == 0 || length > MAX_JSON {
            return Err("midi.json must be between 1 byte and 48 KiB");
        }
        let mut cluster = usize::from(u16::from_le_bytes([entry[26], entry[27]]));
        let mut visited = vec![false; CLUSTERS + 2];
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            if !(2..CLUSTERS + 2).contains(&cluster) || visited[cluster] {
                return Err("Invalid or cyclic midi.json cluster chain");
            }
            visited[cluster] = true;
            let offset = (DATA + cluster - 2) * SECTOR;
            let take = (length - result.len()).min(SECTOR);
            result.extend_from_slice(&self.bytes[offset..offset + take]);
            let index = cluster + cluster / 2;
            let word = u16::from_le_bytes([fat[index], fat[index + 1]]);
            let next = if cluster % 2 == 0 {
                word & 0xfff
            } else {
                word >> 4
            };
            if result.len() == length {
                if next < 0xff8 {
                    return Err("midi.json cluster chain does not end at file size");
                }
            } else if next >= 0xff8 {
                return Err("Truncated midi.json cluster chain");
            }
            cluster = usize::from(next);
        }
        Ok(result)
    }

    #[cfg(test)]
    pub fn image(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    #[test]
    fn computer_filesystem_reads_json_and_replaces_it_via_rename() {
        let original = b"{\"format\":\"nano-midi\",\"version\":1,\"bindings\":[]}";
        let mut disk = Disk::new(original, b"Instructions").unwrap();
        {
            let fs =
                fatfs::FileSystem::new(Cursor::new(disk.image()), fatfs::FsOptions::new()).unwrap();
            assert_eq!(fs.fat_type(), fatfs::FatType::Fat12);
            let mut read = Vec::new();
            fs.root_dir()
                .open_file("midi.json")
                .unwrap()
                .read_to_end(&mut read)
                .unwrap();
            assert_eq!(read, original);
            let root = fs.root_dir();
            let replacement = vec![b' '; 2000];
            {
                let mut temp = root.create_file("temporary.json").unwrap();
                temp.write_all(&replacement).unwrap();
            }
            root.remove("midi.json").unwrap();
            root.rename("temporary.json", &root, "midi.json").unwrap();
            drop(root);
            fs.unmount().unwrap();
        }
        assert_eq!(disk.json().unwrap(), vec![b' '; 2000]);
    }

    #[test]
    fn fragmented_multi_cluster_files_are_read_in_fat_order() {
        let mut disk = Disk::new(b"{}", b"help").unwrap();
        let data: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        {
            let fs =
                fatfs::FileSystem::new(Cursor::new(disk.image()), fatfs::FsOptions::new()).unwrap();
            let root = fs.root_dir();
            let mut file = root.open_file("midi.json").unwrap();
            file.truncate().unwrap();
            for (i, chunk) in data.chunks(SECTOR).enumerate() {
                file.write_all(chunk).unwrap();
                let mut spacer = root.create_file(&alloc::format!("gap{i}.txt")).unwrap();
                spacer.write_all(&[1; SECTOR]).unwrap();
            }
            drop(file);
            drop(root);
            fs.unmount().unwrap();
        }
        assert_eq!(disk.json().unwrap(), data);
    }

    #[test]
    fn deletion_reformat_and_bad_cluster_chains_are_rejected() {
        let mut disk = Disk::new(&vec![b' '; 1000], b"help").unwrap();
        disk.set_fat(2, 2);
        assert!(disk.json().unwrap_err().contains("cyclic"));
        disk.set_fat(2, 0xfff);
        assert!(disk.json().unwrap_err().contains("Truncated"));
        disk.set_fat(2, 3);
        disk.bytes[SECTOR + 3] ^= 1;
        assert!(disk.json().unwrap_err().contains("disagree"));
        let mut disk = Disk::new(b"{}", b"help").unwrap();
        disk.bytes[ROOT * SECTOR + 64] = 0xe5;
        assert!(disk.json().unwrap_err().contains("missing"));
        disk.bytes[11] = 1;
        assert!(disk.json().unwrap_err().contains("reformatted"));
    }

    #[test]
    fn block_bounds_and_file_size_limits_are_enforced() {
        let mut disk = Disk::new(b"{}", b"help").unwrap();
        let mut block = [0; SECTOR];
        assert!(disk.read(SECTORS as u32, &mut block).is_err());
        assert!(disk.write(u32::MAX, &block).is_err());
        assert!(disk.read(0, &mut block[..10]).is_err());
        assert!(disk.write(0, &[]).is_err());
        assert!(Disk::new(&vec![0; MAX_JSON + 1], b"help").is_err());
        let large = vec![b' '; MAX_JSON];
        assert_eq!(Disk::new(&large, b"help").unwrap().json().unwrap(), large);
    }
}
