//! Protobuf-style varint encode/decode.

/// Protobuf `int32`: negatives are sign-extended to a 10-byte varint (`-1` is
/// `FF FF FF FF FF FF FF FF FF 01`).
pub fn encode_int32(value: i32, out: &mut [u8]) -> Option<usize> {
    if value >= 0 {
        return encode_varint(value as u32, out);
    }
    let mut x = value as u64;
    let mut n = 0;
    loop {
        if n >= out.len() || n >= 10 {
            return None;
        }
        let mut byte = (x & 0x7F) as u8;
        x >>= 7;
        if x == 0 {
            out[n] = byte;
            return Some(n + 1);
        }
        byte |= 0x80;
        out[n] = byte;
        n += 1;
    }
}

pub fn encode_varint(mut value: u32, out: &mut [u8]) -> Option<usize> {
    let mut n = 0;
    while value >= 0x80 {
        if n + 1 >= out.len() {
            return None;
        }
        out[n] = (value as u8 & 0x7F) | 0x80;
        n += 1;
        value >>= 7;
    }
    if n >= out.len() {
        return None;
    }
    out[n] = value as u8;
    Some(n + 1)
}

pub fn read_varint(data: &[u8], offset: usize) -> Option<(u32, usize)> {
    if offset >= data.len() {
        return None;
    }
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    let end = (offset + 10).min(data.len());
    for i in offset..end {
        let b = data[i];
        result |= u32::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 32 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_small() {
        let mut buf = [0u8; 5];
        let n = encode_varint(127, &mut buf).unwrap();
        assert_eq!(&buf[..n], &[127]);
        assert_eq!(read_varint(&buf[..n], 0), Some((127, 1)));
    }

    #[test]
    fn roundtrip_multi() {
        let mut buf = [0u8; 5];
        let n = encode_varint(300, &mut buf).unwrap();
        assert_eq!(&buf[..n], &[0xAC, 0x02]);
        assert_eq!(read_varint(&buf[..n], 0), Some((300, 2)));
    }
}
