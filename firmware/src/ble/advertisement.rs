const NANO_SERVICES: [u16; 2] = [0xa002, 0xa003];

fn nano_uuid(uuid: &[u8]) -> bool {
    let short = match uuid.len() {
        2 => &uuid[..2],
        4 if uuid[2..] == [0, 0] => &uuid[..2],
        16 if uuid[..12] == [0xfb, 0x34, 0x9b, 0x5f, 0x80, 0, 0, 0x80, 0, 0x10, 0, 0]
            && uuid[14..] == [0, 0] =>
        {
            &uuid[12..14]
        }
        _ => return false,
    };
    NANO_SERVICES.contains(&u16::from_le_bytes([short[0], short[1]]))
}

/// UUIDs in BLE advertising data are little-endian, including Bluetooth-base UUIDs.
pub fn has_nano_service(mut data: &[u8]) -> bool {
    while let Some((&length, rest)) = data.split_first() {
        let length = usize::from(length);
        if length == 0 || length > rest.len() {
            return false;
        }
        let (field, remaining) = rest.split_at(length);
        let value = &field[1..];
        let width = match field[0] {
            0x02 | 0x03 | 0x16 => 2,
            0x04 | 0x05 | 0x20 => 4,
            0x06 | 0x07 | 0x21 => 16,
            _ => {
                data = remaining;
                continue;
            }
        };
        let service_data = matches!(field[0], 0x16 | 0x20 | 0x21);
        if service_data {
            if value.len() >= width && nano_uuid(&value[..width]) {
                return true;
            }
        } else if value.len() % width == 0 && value.chunks_exact(width).any(nano_uuid) {
            return true;
        }
        data = remaining;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn field(kind: u8, value: &[u8]) -> Vec<u8> {
        let mut data = alloc::vec![value.len() as u8 + 1, kind];
        data.extend_from_slice(value);
        data
    }

    #[test]
    fn renamed_and_unnamed_nanos_are_detected_by_service() {
        for uuid in NANO_SERVICES {
            for kind in [0x02, 0x03, 0x16] {
                let service = field(kind, &uuid.to_le_bytes());
                assert!(has_nano_service(&service));
                let mut renamed = field(0x09, b"Nanonilda");
                renamed.extend_from_slice(&service);
                assert!(has_nano_service(&renamed));
            }
        }
        assert!(has_nano_service(&field(0x03, &[0x0f, 0x18, 0x02, 0xa0])));
    }

    #[test]
    fn bluetooth_base_uuids_and_service_data_are_supported() {
        for short in NANO_SERVICES {
            let mut uuid = alloc::vec![0xfb, 0x34, 0x9b, 0x5f, 0x80, 0, 0, 0x80, 0, 0x10, 0, 0];
            uuid.extend_from_slice(&short.to_le_bytes());
            uuid.extend_from_slice(&[0, 0]);
            for kind in [0x06, 0x07, 0x21] {
                assert!(has_nano_service(&field(kind, &uuid)));
            }
            uuid[0] ^= 1;
            assert!(!has_nano_service(&field(0x07, &uuid)));
            let uuid32 = [short as u8, (short >> 8) as u8, 0, 0];
            for kind in [0x04, 0x05, 0x20] {
                assert!(has_nano_service(&field(kind, &uuid32)));
            }
        }
        assert!(has_nano_service(&field(0x16, &[0x02, 0xa0, 1, 2, 3])));
    }

    #[test]
    fn unrelated_names_manufacturer_data_and_malformed_fields_do_not_match() {
        for data in [
            field(0x09, b"Nanonilda"),
            field(0xff, &[0x02, 0xa0]),
            field(0x03, &[0x0f, 0x18]),
            field(0x03, &[0x02, 0xa0, 0]),
            field(0x16, &[0x02]),
            alloc::vec![0, 3, 0x03, 0x02, 0xa0],
            alloc::vec![5, 0x03, 0x02, 0xa0],
            alloc::vec![],
        ] {
            assert!(!has_nano_service(&data), "{data:?}");
        }
    }
}
