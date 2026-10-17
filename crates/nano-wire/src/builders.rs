//! C304 packet builders (ports of `nano_protocol.c`).

use crate::types::{Knob, BPM_MAX, BPM_MIN};
use crate::varint::encode_varint;

fn copy_fixed(src: &[u8], buf: &mut [u8]) -> usize {
    if src.len() > buf.len() {
        return 0;
    }
    buf[..src.len()].copy_from_slice(src);
    src.len()
}

fn append_f32_le(value: f32, out: &mut [u8]) -> Option<usize> {
    if out.len() < 4 {
        return None;
    }
    out[..4].copy_from_slice(&value.to_le_bytes());
    Some(4)
}

pub fn fe_metadata_request(buf: &mut [u8]) -> usize {
    copy_fixed(&[0x06, 0xC0, 0x08, 0x03, 0x01, 0x00, 0x00, 0x00], buf)
}

pub fn request_current_state(buf: &mut [u8]) -> usize {
    copy_fixed(
        &[
            0x0C, 0xC0, 0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00,
        ],
        buf,
    )
}

pub fn preset_change_ack(buf: &mut [u8]) -> usize {
    copy_fixed(&[0x06, 0xC0, 0x20, 0x01, 0x1E, 0x00, 0x00, 0x00], buf)
}

pub fn post_save_refresh(buf: &mut [u8]) -> usize {
    copy_fixed(
        &[
            0x0C, 0xC0, 0x08, 0x03, 0x18, 0x00, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00,
        ],
        buf,
    )
}

/// Intentionally empty — trailer 0x91 enters tap-tempo and can rewrite BPM.
pub fn bpm_probe_payload(_step: u32, _buf: &mut [u8]) -> (usize, bool) {
    (0, false)
}

fn knob_style_payload(id: u8, raw_value: u8, buf: &mut [u8]) -> usize {
    if buf.len() < 12 {
        return 0;
    }
    let mut value_bytes = [0u8; 5];
    let Some(vb_len) = encode_varint(u32::from(raw_value), &mut value_bytes) else {
        return 0;
    };
    let total = 1 + 3 + vb_len + 6;
    if total > buf.len() {
        return 0;
    }
    let mut i = 0;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0xC0;
    i += 1;
    buf[i] = 0x18;
    i += 1;
    buf[i] = id;
    i += 1;
    buf[i] = 0x20;
    i += 1;
    buf[i..i + vb_len].copy_from_slice(&value_bytes[..vb_len]);
    i += vb_len;
    buf[i] = 0x28;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x1A;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[0] = (i - 2) as u8;
    i
}

pub fn amp_knob_payload(knob: Knob, raw_value: u8, buf: &mut [u8]) -> usize {
    knob_style_payload(knob as u8, raw_value, buf)
}

pub fn capture_volume_payload(raw_value: u8, buf: &mut [u8]) -> usize {
    knob_style_payload(0x0A, raw_value, buf)
}

pub fn capture_select_payload(slot: u8, buf: &mut [u8]) -> usize {
    if !(1..=25).contains(&slot) || buf.len() < 10 {
        return 0;
    }
    let mut pkt = [0x08u8, 0xC0, 0x18, 0x04, 0x20, 0x00, 0x1C, 0x00, 0x00, 0x00];
    pkt[5] = slot - 1;
    copy_fixed(&pkt, buf)
}

pub fn global_toggle_payload(id: u8, value: u8, buf: &mut [u8]) -> usize {
    let mut pkt = [
        0x0Au8, 0xC0, 0x08, 0x01, 0x18, 0x00, 0x20, 0x00, 0x1F, 0x00, 0x00, 0x00,
    ];
    if buf.len() < pkt.len() {
        return 0;
    }
    pkt[5] = id;
    pkt[7] = value;
    copy_fixed(&pkt, buf)
}

/// Select the Nano FX-button focus slot (mirrors the C305 press notify).
///
/// `0A C0 08 01 18 02 20 <model_slot 0..=4> 1C 00 00 00`
pub fn fx_slot_focus_payload(model_slot: u8, buf: &mut [u8]) -> usize {
    if model_slot > 4 {
        return 0;
    }
    let mut pkt = [
        0x0Au8, 0xC0, 0x08, 0x01, 0x18, 0x02, 0x20, 0x00, 0x1C, 0x00, 0x00, 0x00,
    ];
    if buf.len() < pkt.len() {
        return 0;
    }
    pkt[7] = model_slot;
    copy_fixed(&pkt, buf)
}

pub fn gate_reduction_payload(mut pct: u8, buf: &mut [u8]) -> usize {
    if buf.len() < 16 {
        return 0;
    }
    if pct > 100 {
        pct = 100;
    }
    let encoded = (u32::from(pct) * 255 + 50) / 100;
    let mut value_bytes = [0u8; 5];
    let Some(vb_len) = encode_varint(encoded, &mut value_bytes) else {
        return 0;
    };
    let total = 1 + 3 + vb_len + 6;
    if total > buf.len() {
        return 0;
    }
    let mut i = 0;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0xC0;
    i += 1;
    buf[i] = 0x18;
    i += 1;
    buf[i] = 0x0B;
    i += 1;
    buf[i] = 0x20;
    i += 1;
    buf[i..i + vb_len].copy_from_slice(&value_bytes[..vb_len]);
    i += vb_len;
    buf[i] = 0x28;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x1A;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[0] = (i - 2) as u8;
    i
}

pub fn fx_param_refresh_payload(refresh_slot: u8, buf: &mut [u8]) -> usize {
    let mut pkt = [0x08u8, 0xC0, 0x08, 0x03, 0x18, 0x00, 0x89, 0x00, 0x00, 0x00];
    if buf.len() < pkt.len() {
        return 0;
    }
    pkt[5] = refresh_slot;
    copy_fixed(&pkt, buf)
}

pub fn fx_model_select_payload(model_slot: u8, id: &[u8], buf: &mut [u8]) -> usize {
    if id.is_empty() || id.len() > 3 || buf.len() < 9 + id.len() {
        return 0;
    }
    let mut i = 0;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0xC0;
    i += 1;
    buf[i] = 0x18;
    i += 1;
    buf[i] = model_slot;
    i += 1;
    buf[i] = 0x20;
    i += 1;
    buf[i..i + id.len()].copy_from_slice(id);
    i += id.len();
    buf[i] = 0x88;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[0] = (i - 2) as u8;
    i
}

pub fn fx_param_payload(model_slot: u8, param_index: u8, normalized: f32, buf: &mut [u8]) -> usize {
    if buf.len() < 20 {
        return 0;
    }
    let mut i = 0;
    buf[i] = 0x0F;
    i += 1;
    buf[i] = 0xC0;
    i += 1;
    buf[i] = 0x08;
    i += 1;
    buf[i] = 0x01;
    i += 1;
    buf[i] = 0x18;
    i += 1;
    buf[i] = model_slot;
    i += 1;
    buf[i] = 0x20;
    i += 1;
    buf[i] = param_index;
    i += 1;
    buf[i] = 0x2D;
    i += 1;
    if append_f32_le(normalized, &mut buf[i..]).is_none() {
        return 0;
    }
    i += 4;
    buf[i] = 0x63;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    i
}

fn bpm_float_frame(bpm: f32, include_tap_marker: bool, buf: &mut [u8]) -> usize {
    let need = if include_tap_marker { 17 } else { 15 };
    if buf.len() < need || !(BPM_MIN as f32..=BPM_MAX as f32).contains(&bpm) {
        return 0;
    }
    let mut i = 0;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0xC0;
    i += 1;
    buf[i] = 0x08;
    i += 1;
    buf[i] = 0x01;
    i += 1;
    if include_tap_marker {
        buf[i] = 0x18;
        i += 1;
        buf[i] = 0x01;
        i += 1;
    }
    buf[i] = 0x20;
    i += 1;
    buf[i] = 0x01;
    i += 1;
    buf[i] = 0x2D;
    i += 1;
    if append_f32_le(bpm, &mut buf[i..]).is_none() {
        return 0;
    }
    i += 4;
    buf[i] = 0x91;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[i] = 0x00;
    i += 1;
    buf[0] = (i - 2) as u8;
    i
}

pub fn bpm_tap_payload(bpm: f32, buf: &mut [u8]) -> usize {
    bpm_float_frame(bpm, true, buf)
}

pub fn bpm_exit_payload(bpm: f32, buf: &mut [u8]) -> usize {
    bpm_float_frame(bpm, false, buf)
}

pub fn tuner_enable_payload(buf: &mut [u8]) -> usize {
    copy_fixed(
        &[
            0x0D, 0xC0, 0x08, 0x01, 0x2D, 0x00, 0x00, 0xDC, 0x43, 0x38, 0x01, 0x7F, 0x00, 0x00,
            0x00,
        ],
        buf,
    )
}

pub fn tuner_enable_footswitch_payload(buf: &mut [u8]) -> usize {
    copy_fixed(
        &[
            0x12, 0xC0, 0x08, 0x01, 0x22, 0x01, 0x44, 0x2D, 0x56, 0xC5, 0xD7, 0x3F, 0x30, 0x01,
            0x38, 0x01, 0x80, 0x00,
        ],
        buf,
    )
}

pub fn tuner_disable_payload(buf: &mut [u8]) -> usize {
    copy_fixed(
        &[
            0x0B, 0xC0, 0x08, 0x01, 0x2D, 0x00, 0x00, 0xDC, 0x43, 0x7F, 0x00, 0x00, 0x00,
        ],
        buf,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_state_fixed() {
        let mut buf = [0u8; 32];
        let n = fe_metadata_request(&mut buf);
        assert_eq!(&buf[..n], &[0x06, 0xC0, 0x08, 0x03, 0x01, 0x00, 0x00, 0x00]);
        let n = request_current_state(&mut buf);
        assert_eq!(
            &buf[..n],
            &[
                0x0C, 0xC0, 0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00
            ]
        );
    }

    #[test]
    fn amp_gain_200() {
        let mut buf = [0u8; 32];
        let n = amp_knob_payload(Knob::Gain, 200, &mut buf);
        // length C0 18 00 20 C8 01 28 00 1A 00 00 00
        assert_eq!(
            &buf[..n],
            &[0x0B, 0xC0, 0x18, 0x00, 0x20, 0xC8, 0x01, 0x28, 0x00, 0x1A, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn bpm_tap_frame() {
        let mut buf = [0u8; 32];
        let n = bpm_tap_payload(120.0, &mut buf);
        assert_eq!(n, 17);
        assert_eq!(buf[0], 0x0F);
        assert_eq!(&buf[1..5], &[0xC0, 0x08, 0x01, 0x18]);
        assert_eq!(buf[13], 0x91); // after 2D + f32
    }

    #[test]
    fn bpm_probe_empty() {
        let mut buf = [0u8; 16];
        assert_eq!(bpm_probe_payload(0, &mut buf), (0, false));
    }
}
