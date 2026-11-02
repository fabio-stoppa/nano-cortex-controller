const ADDRESS_START: usize = 8;
const ADDRESS_END: usize = ADDRESS_START + 7;

pub fn write(record: &mut [u8; 16], address: Option<[u8; 7]>) {
    let Some(address) = address else {
        record[ADDRESS_START..].fill(0xff);
        return;
    };
    record[ADDRESS_START..ADDRESS_END].copy_from_slice(&address);
    record[ADDRESS_END] = checksum(&address);
}

pub fn read(record: &[u8; 16]) -> Option<[u8; 7]> {
    let address: [u8; 7] = record[ADDRESS_START..ADDRESS_END].try_into().ok()?;
    (address[0] <= 3 && record[ADDRESS_END] == checksum(&address)).then_some(address)
}

fn checksum(address: &[u8; 7]) -> u8 {
    address.iter().fold(0xa5, |sum, byte| sum ^ byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_device_record_round_trips_and_detects_corruption() {
        let address = [1, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut record = [0xff; 16];
        write(&mut record, Some(address));
        assert_eq!(read(&record), Some(address));

        record[12] ^= 1;
        assert_eq!(read(&record), None);
    }

    #[test]
    fn absent_and_invalid_device_records_are_ignored() {
        let mut record = [0xff; 16];
        assert_eq!(read(&record), None);
        write(&mut record, Some([4, 1, 2, 3, 4, 5, 6]));
        assert_eq!(read(&record), None);
    }
}
