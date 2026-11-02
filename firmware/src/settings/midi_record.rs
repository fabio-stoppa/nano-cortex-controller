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
mod tests {
    use super::checksum;

    #[test]
    fn crc_matches_standard_check_value() {
        assert_eq!(checksum(b"123456789"), 0xcbf4_3926);
        assert_eq!(checksum(b""), 0);
    }
}
