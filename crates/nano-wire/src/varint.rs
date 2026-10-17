//! Protobuf-style varint encode/decode.

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
