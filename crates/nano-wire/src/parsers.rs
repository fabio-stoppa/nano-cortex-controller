//! C305 / protobuf parsers (ports of `nano_proto.c`).

use crate::types::{
    Amp, PresetCatalog, TunerRoute, AMP_CAPTURE_NAME_MAX, BPM_MAX, BPM_MIN, PRESET_COUNT,
    PRESET_NAME_MAX,
};
use crate::varint::read_varint;

fn read_f32_le(p: &[u8]) -> f32 {
    f32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

fn clamp_u8(val: u32) -> u8 {
    val.min(255) as u8
}

fn proto_string_is_printable(data: &[u8]) -> bool {
    !data.is_empty() && data.iter().all(|&b| (0x20..=0x7E).contains(&b))
}

pub fn first_string(data: &[u8], field_num: u32) -> Option<String> {
    let mut i = 0;
    while i < data.len() {
        let (tag, next) = read_varint(data, i)?;
        i = next;
        let field = tag >> 3;
        let wire = tag & 0x07;
        match wire {
            0 => {
                let (_, next) = read_varint(data, i)?;
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let (flen, next) = read_varint(data, i)?;
                i = next;
                let flen = flen as usize;
                if i + flen > data.len() {
                    break;
                }
                if field == field_num && flen > 0 && proto_string_is_printable(&data[i..i + flen]) {
                    return Some(String::from_utf8_lossy(&data[i..i + flen]).into_owned());
                }
                i += flen;
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                i += 4;
            }
            _ => break,
        }
    }
    None
}

pub fn field_varint(data: &[u8], field_num: u32) -> Option<u32> {
    let mut i = 0;
    while i < data.len() {
        let (tag, next) = read_varint(data, i)?;
        i = next;
        let field = tag >> 3;
        let wire = tag & 0x07;
        match wire {
            0 => {
                let (val, next) = read_varint(data, i)?;
                if field == field_num {
                    return Some(val);
                }
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let (flen, next) = read_varint(data, i)?;
                i = next + flen as usize;
                if i > data.len() {
                    break;
                }
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                i += 4;
            }
            _ => break,
        }
    }
    None
}

pub fn parse_amp(data: &[u8]) -> Option<Amp> {
    let mut out = Amp::default();
    let mut have = [false; 5];
    let mut i = 0;

    while i < data.len() {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;
        let field = tag >> 3;
        let wire = tag & 0x07;

        match wire {
            0 => {
                let Some((val, next)) = read_varint(data, i) else {
                    break;
                };
                let raw = clamp_u8(val);
                match field {
                    3 if !have[0] => {
                        out.gain = raw;
                        have[0] = true;
                    }
                    4 if !have[1] => {
                        out.level = raw;
                        have[1] = true;
                    }
                    5 if !have[2] => {
                        out.bass = raw;
                        have[2] = true;
                    }
                    6 if !have[3] => {
                        out.mid = raw;
                        have[3] = true;
                    }
                    7 if !have[4] => {
                        out.treble = raw;
                        have[4] = true;
                    }
                    11 if !out.have_capture_slot => {
                        out.capture_slot = raw;
                        out.have_capture_slot = true;
                    }
                    44 if !out.have_capture_volume => {
                        out.capture_volume = raw;
                        out.have_capture_volume = true;
                    }
                    _ => {}
                }
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let Some((flen, next)) = read_varint(data, i) else {
                    break;
                };
                i = next;
                let flen = flen as usize;
                if i + flen > data.len() {
                    break;
                }
                if field == 32 && !out.have_capture_name {
                    if let Some(name) = first_string(&data[i..i + flen], 2) {
                        if !name.is_empty() {
                            out.capture_name = name.chars().take(AMP_CAPTURE_NAME_MAX - 1).collect();
                            out.have_capture_name = true;
                        }
                    }
                }
                i += flen;
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                i += 4;
            }
            _ => break,
        }
    }

    if !out.have_capture_volume {
        // Tag for field 44 wire0 = (44<<3)|0 = 352 = E0 02
        let mut off = 0;
        while off + 3 <= data.len() {
            if data[off] == 0xE0 && data[off + 1] == 0x02 {
                if let Some((val, _)) = read_varint(data, off + 2) {
                    if val <= 255 {
                        out.capture_volume = val as u8;
                        out.have_capture_volume = true;
                        break;
                    }
                }
            }
            off += 1;
        }
    }

    out.valid = have.iter().any(|&h| h) || out.have_capture_volume;
    out.valid.then_some(out)
}

pub fn parse_bpm(data: &[u8]) -> Option<u16> {
    if data.is_empty() {
        return None;
    }
    let field_want = 56u32;
    let mut i = 0;

    while i < data.len() {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;
        let field = tag >> 3;
        let wire = tag & 0x07;

        match wire {
            0 => {
                let Some((_, next)) = read_varint(data, i) else {
                    break;
                };
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let Some((flen, next)) = read_varint(data, i) else {
                    break;
                };
                i = next;
                let flen = flen as usize;
                if i + flen > data.len() {
                    break;
                }
                if flen > 0 {
                    if let Some(bpm) = parse_bpm(&data[i..i + flen]) {
                        return Some(bpm);
                    }
                }
                i += flen;
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                if field == field_want {
                    let f = read_f32_le(&data[i..]);
                    if f.is_finite() && (BPM_MIN as f32..=BPM_MAX as f32).contains(&f) {
                        return Some(f.round() as u16);
                    }
                }
                i += 4;
            }
            _ => break,
        }
    }

    // Raw tag scan: (56<<3)|5 = 453 = C5 03
    let mut off = 0;
    while off + 6 <= data.len() {
        if data[off] == 0xC5 && data[off + 1] == 0x03 {
            let f = read_f32_le(&data[off + 2..]);
            if f.is_finite() && (BPM_MIN as f32..=BPM_MAX as f32).contains(&f) {
                return Some(f.round() as u16);
            }
        }
        off += 1;
    }
    None
}

pub fn parse_fe_preset_catalog(data: &[u8]) -> Option<PresetCatalog> {
    if data.is_empty() {
        return None;
    }
    let mut names = Vec::new();
    let mut i = 0;

    while i < data.len() && names.len() < PRESET_COUNT {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;
        let field = tag >> 3;
        let wire = tag & 0x07;

        match wire {
            0 => {
                let Some((_, next)) = read_varint(data, i) else {
                    break;
                };
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let Some((flen, next)) = read_varint(data, i) else {
                    break;
                };
                i = next;
                let flen = flen as usize;
                if i + flen > data.len() {
                    break;
                }
                if field == 18 {
                    let name = first_string(&data[i..i + flen], 1)
                        .filter(|n| !n.is_empty())
                        .map(|n| n.chars().take(PRESET_NAME_MAX - 1).collect())
                        .unwrap_or_default();
                    names.push(name);
                }
                i += flen;
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                i += 4;
            }
            _ => break,
        }
    }

    let valid = !names.is_empty();
    valid.then_some(PresetCatalog { names, valid })
}

pub fn parse_tuner_state(data: &[u8]) -> Option<(bool, TunerRoute)> {
    let mut f45_seen = false;
    let mut f45_val = 0u32;
    let mut f51_seen = false;
    let mut i = 0;

    while i < data.len() {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;
        let field = tag >> 3;
        let wire = tag & 0x07;

        match wire {
            0 => {
                let Some((val, next)) = read_varint(data, i) else {
                    break;
                };
                if field == 45 {
                    f45_seen = true;
                    f45_val = val;
                } else if field == 51 {
                    f51_seen = true;
                }
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let Some((flen, next)) = read_varint(data, i) else {
                    break;
                };
                i = next + flen as usize;
                if i > data.len() {
                    break;
                }
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                i += 4;
            }
            _ => break,
        }
    }

    if f45_seen && f45_val >= 1 {
        return Some((true, TunerRoute::Unknown));
    }
    if f51_seen && !f45_seen {
        return Some((false, TunerRoute::Unknown));
    }
    None
}

pub fn parse_tuner_active(data: &[u8]) -> Option<bool> {
    parse_tuner_state(data).map(|(active, _)| active)
}

fn map_pedal_knob_id(hw_id: u8) -> Option<u8> {
    (hw_id <= 4).then_some(hw_id)
}

fn try_parse_amp_gain_2d_notify(data: &[u8]) -> Option<u8> {
    if data.len() < 12 {
        return None;
    }
    let mut i = 0;
    while i + 11 <= data.len() {
        if data[i] != 0x2D {
            i += 1;
            continue;
        }
        let value = read_f32_le(&data[i + 1..]);
        if (value - 440.0).abs() <= 1.0 {
            i += 1;
            continue;
        }
        if i + 6 > data.len() || data[i + 5] != 0x63 {
            i += 1;
            continue;
        }

        let back_start = i.saturating_sub(8);
        let mut gain_prefix = false;
        let mut back = back_start;
        while back + 5 <= i {
            if data[back] == 0xC0
                && data[back + 1] == 0x08
                && data[back + 2] == 0x01
                && data[back + 3] == 0x20
            {
                if let Some((_, next)) = read_varint(data, back + 4) {
                    if next == i {
                        gain_prefix = true;
                        break;
                    }
                }
            }
            back += 1;
        }
        if !gain_prefix {
            i += 1;
            continue;
        }

        let value = value.clamp(0.0, 255.0);
        return Some((value + 0.5) as u8);
    }
    None
}

/// True when the byte after a knob-style value marks a non-amp frame.
///
/// Amp knob writes continue with field 5 (`0x28`). FX model-select uses
/// trailer `0x88`; float FX/tuner params use `0x2D`. Matching those as amp
/// knobs paints gain (id 0) / EQ at a few % while presets reload.
fn amp_knob_style_false_trailer(b: u8) -> bool {
    matches!(b, 0x88 | 0x2D)
}

/// Trailer after `C0 08 01 18 <id> 20 <v>` that is not a hardware amp knob.
fn amp_hw_knob_false_trailer(b: u8) -> bool {
    matches!(b, 0x2D | 0x1F | 0x88 | 0x91)
}

pub fn try_parse_amp_knob_notify(data: &[u8]) -> Option<(u8, u8)> {
    if data.len() < 5 {
        return None;
    }
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i] == 0xC0 && data[i + 1] == 0x18 && data[i + 2] <= 4 && data[i + 3] == 0x20 {
            if let Some((val, next)) = read_varint(data, i + 4) {
                if next < data.len() && amp_knob_style_false_trailer(data[next]) {
                    i += 1;
                    continue;
                }
                return Some((data[i + 2], clamp_u8(val)));
            }
        }
        i += 1;
    }
    None
}

pub fn try_parse_capture_volume_notify(data: &[u8]) -> Option<u8> {
    if data.len() < 5 {
        return None;
    }
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i] == 0xC0 && data[i + 1] == 0x18 && data[i + 2] == 0x0A && data[i + 3] == 0x20 {
            if let Some((val, _)) = read_varint(data, i + 4) {
                return Some(clamp_u8(val));
            }
        }
        i += 1;
    }

    i = 0;
    while i + 7 < data.len() {
        if data[i] == 0xC0
            && data[i + 1] == 0x08
            && data[i + 2] == 0x01
            && data[i + 3] == 0x18
            && data[i + 4] == 0x0A
            && data[i + 5] == 0x20
        {
            if let Some((val, next)) = read_varint(data, i + 6) {
                if next < data.len() && data[next] == 0x2D {
                    i += 1;
                    continue;
                }
                return Some(clamp_u8(val));
            }
        }
        i += 1;
    }
    None
}

pub fn try_parse_amp_hw_notify(data: &[u8]) -> Option<(u8, u8)> {
    if data.len() < 7 {
        return None;
    }
    // Float gain: C0 08 01 20 <…> 2D <f32> 63 (not the bare integer shortcut).
    if let Some(raw) = try_parse_amp_gain_2d_notify(data) {
        return Some((0, raw));
    }

    let mut i = 0;
    while i + 6 < data.len() {
        if data[i] != 0xC0 || data[i + 1] != 0x08 || data[i + 2] != 0x01 {
            i += 1;
            continue;
        }
        let p = i + 3;
        // Explicit knob id: C0 08 01 18 <id<=4> 20 <val>
        // Do NOT treat bare C0 08 01 20 <val> as gain — that prefix is shared
        // with tuner/BPM/route frames and spuriously zeros gain on preset change.
        if p < data.len() && data[p] == 0x18 {
            if p + 3 >= data.len() || data[p + 2] != 0x20 {
                i += 1;
                continue;
            }
            let Some(mapped) = map_pedal_knob_id(data[p + 1]) else {
                i += 1;
                continue;
            };
            if let Some((val, next)) = read_varint(data, p + 3) {
                if next < data.len() && amp_hw_knob_false_trailer(data[next]) {
                    i += 1;
                    continue;
                }
                return Some((mapped, clamp_u8(val)));
            }
        }
        i += 1;
    }
    None
}

pub fn try_parse_bpm_notify_ex(data: &[u8]) -> Option<(u16, bool)> {
    if data.len() < 12 {
        return None;
    }
    let mut i = 0;
    while i + 6 <= data.len() {
        if data[i] != 0x2D || data[i + 5] != 0x91 {
            i += 1;
            continue;
        }
        let value = read_f32_le(&data[i + 1..]);
        if !value.is_finite() || !(BPM_MIN as f32..=BPM_MAX as f32).contains(&value) {
            i += 1;
            continue;
        }

        let back_start = i.saturating_sub(12);
        let mut prefix = false;
        let mut back = back_start;
        while back + 3 <= i {
            if data[back] == 0xC0 && data[back + 1] == 0x08 && data[back + 2] == 0x01 {
                prefix = true;
                break;
            }
            back += 1;
        }
        if !prefix {
            i += 1;
            continue;
        }

        let tap = i >= 4
            && data[i - 4] == 0x18
            && data[i - 3] == 0x01
            && data[i - 2] == 0x20
            && data[i - 1] == 0x01;
        if !tap && !(i >= 2 && data[i - 2] == 0x20 && data[i - 1] == 0x01) {
            i += 1;
            continue;
        }

        return Some((value.round() as u16, tap));
    }
    None
}

pub fn try_parse_bpm_notify(data: &[u8]) -> Option<u16> {
    try_parse_bpm_notify_ex(data).map(|(bpm, _)| bpm)
}

pub fn log_bytes(tag: &str, label: &str, data: &[u8]) {
    let mut hex = String::new();
    for b in data.iter().take(24) {
        hex.push_str(&format!("{b:02X} "));
    }
    log::info!(target: tag, "{label}: {hex}");
}

pub fn log_bytes_full(tag: &str, label: &str, data: &[u8]) {
    if data.is_empty() {
        log::info!(target: tag, "{label}: (empty)");
        return;
    }
    let mut line = format!("{label}: ");
    for b in data {
        if line.len() > 100 {
            log::info!(target: tag, "{line}");
            line = "  ... ".into();
        }
        line.push_str(&format!("{b:02X} "));
    }
    if !line.is_empty() {
        log::info!(target: tag, "{line}");
    }
}

pub fn log_amp(tag: &str, amp: &Amp) {
    if !amp.valid {
        log::info!(target: tag, "Amp: (not parsed yet)");
        return;
    }
    let pct = |raw: u8| (u32::from(raw) * 100 + 127) / 255;
    log::info!(
        target: tag,
        "Amp — Gain: {}% Level: {}% CapVol: {}% Bass: {}% Mid: {}% Treble: {}%  (raw {}/{}/{}/{}/{}/{})",
        pct(amp.gain),
        pct(amp.level),
        if amp.have_capture_volume { pct(amp.capture_volume) } else { 0 },
        pct(amp.bass),
        pct(amp.mid),
        pct(amp.treble),
        amp.gain,
        amp.level,
        if amp.have_capture_volume { amp.capture_volume } else { 0 },
        amp.bass,
        amp.mid,
        amp.treble
    );
}

pub fn log_bpm_hunt(tag: &str, data: &[u8]) {
    log::info!(target: tag, "BPM hunt: scan {}-byte state blob (rust)", data.len());
    if let Some(bpm) = parse_bpm(data) {
        log::info!(target: tag, "BPM hunt: parsed BPM={bpm}");
    }
}

pub fn log_bpm_raw_scan(tag: &str, label: &str, data: &[u8]) {
    log::info!(target: tag, "BPM raw scan [{label}]: {} bytes (rust)", data.len());
}

pub fn log_proto_fields(tag: &str, label: &str, data: &[u8]) {
    log::info!(
        target: tag,
        "STATE {label} — protobuf scan ({} bytes, rust)",
        data.len()
    );
    // Keep scan logging lighter than the C version; amp/BPM parsers are the source of truth.
    let mut i = 0usize;
    let mut entries = 0u32;
    while i < data.len() && entries < 128 {
        let Some((tag_val, next)) = read_varint(data, i) else {
            break;
        };
        i = next;
        let field = tag_val >> 3;
        let wire = tag_val & 0x07;
        entries += 1;
        match wire {
            0 => {
                let Some((val, next)) = read_varint(data, i) else {
                    break;
                };
                log::info!(target: tag, "STATE {label}   f{field} w0 varint {val}");
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let Some((flen, next)) = read_varint(data, i) else {
                    break;
                };
                i = next + flen as usize;
                if i > data.len() {
                    break;
                }
                log::info!(target: tag, "STATE {label}   f{field} w2 bytes len={flen}");
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                let f = f32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
                log::info!(target: tag, "STATE {label}   f{field} w5 float={f}");
                i += 4;
            }
            _ => break,
        }
    }
    log::info!(target: tag, "STATE {label} — scan done ({entries} entries)");
}

pub fn log_tuner_state_fields(tag: &str, data: &[u8]) {
    let f45 = field_varint(data, 45);
    let f48 = field_varint(data, 48);
    let f49 = field_varint(data, 49);
    let f50 = field_varint(data, 50);
    let f51 = field_varint(data, 51);
    log::info!(
        target: tag,
        "Tuner preset: f45={} f48={} f49={} f50={} f51={}",
        f45.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f48.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f49.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f50.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f51.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_varint_u32(mut v: u32) -> Vec<u8> {
        let mut out = Vec::new();
        while v >= 0x80 {
            out.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
        out
    }

    fn field_varint_bytes(field: u32, value: u32) -> Vec<u8> {
        let mut out = encode_varint_u32((field << 3) | 0);
        out.extend(encode_varint_u32(value));
        out
    }

    fn field_fixed32(field: u32, f: f32) -> Vec<u8> {
        let mut out = encode_varint_u32((field << 3) | 5);
        out.extend_from_slice(&f.to_le_bytes());
        out
    }

    #[test]
    fn parse_amp_fields_3_to_7() {
        let mut data = Vec::new();
        data.extend(field_varint_bytes(3, 127));
        data.extend(field_varint_bytes(4, 128));
        data.extend(field_varint_bytes(5, 100));
        data.extend(field_varint_bytes(6, 171));
        data.extend(field_varint_bytes(7, 132));
        data.extend(field_varint_bytes(44, 200));
        let amp = parse_amp(&data).unwrap();
        assert_eq!(amp.gain, 127);
        assert_eq!(amp.level, 128);
        assert_eq!(amp.bass, 100);
        assert_eq!(amp.mid, 171);
        assert_eq!(amp.treble, 132);
        assert_eq!(amp.capture_volume, 200);
        assert!(amp.have_capture_volume);
    }

    #[test]
    fn parse_bpm_fixed32() {
        let data = field_fixed32(56, 122.0);
        assert_eq!(parse_bpm(&data), Some(122));
    }

    #[test]
    fn parse_bpm_raw_tag() {
        let mut data = vec![0x00, 0xC5, 0x03];
        data.extend_from_slice(&120.0f32.to_le_bytes());
        assert_eq!(parse_bpm(&data), Some(120));
    }

    #[test]
    fn amp_knob_notify() {
        // C0 18 00 20 C8 01
        let data = [0xC0, 0x18, 0x00, 0x20, 0xC8, 0x01];
        assert_eq!(try_parse_amp_knob_notify(&data), Some((0, 200)));
    }

    #[test]
    fn amp_knob_notify_ignores_fx_model_select() {
        // Preset reload: C0 18 <slot<=4> 20 <id…> 88 — must not become gain/EQ
        let gain_slot = [0xC0, 0x18, 0x00, 0x20, 0x05, 0x88];
        assert_eq!(try_parse_amp_knob_notify(&gain_slot), None);
        let mid_slot = [0xC0, 0x18, 0x03, 0x20, 0x05, 0x88];
        assert_eq!(try_parse_amp_knob_notify(&mid_slot), None);
        let with_trailer = [0xC0, 0x18, 0x00, 0x20, 0xC8, 0x01, 0x28, 0x00, 0x1A];
        assert_eq!(try_parse_amp_knob_notify(&with_trailer), Some((0, 200)));
    }

    #[test]
    fn amp_hw_notify_ignores_global_toggle() {
        let data = [0x0A, 0xC0, 0x08, 0x01, 0x18, 0x03, 0x20, 0x01, 0x1F, 0x00, 0x00, 0x00];
        assert_eq!(try_parse_amp_hw_notify(&data), None);
    }

    #[test]
    fn amp_hw_notify_ignores_bare_c0_08_01_20() {
        // Shared with tuner/BPM prefixes — must not paint gain at a few %.
        let bare = [0x0A, 0xC0, 0x08, 0x01, 0x20, 0x05, 0x38, 0x01, 0x7F, 0x00, 0x00, 0x00];
        assert_eq!(try_parse_amp_hw_notify(&bare), None);
        // Explicit gain id still works.
        let explicit = [0x0C, 0xC0, 0x08, 0x01, 0x18, 0x00, 0x20, 0xC8, 0x01, 0x30, 0x01];
        assert_eq!(try_parse_amp_hw_notify(&explicit), Some((0, 200)));
    }

    #[test]
    fn bpm_notify_tap() {
        let mut data = vec![0x0F, 0xC0, 0x08, 0x01, 0x18, 0x01, 0x20, 0x01, 0x2D];
        data.extend_from_slice(&122.0f32.to_le_bytes());
        data.push(0x91);
        assert_eq!(try_parse_bpm_notify_ex(&data), Some((122, true)));
    }

    #[test]
    fn tuner_on_from_f45() {
        let data = field_varint_bytes(45, 1);
        assert_eq!(parse_tuner_active(&data), Some(true));
    }

    #[test]
    fn tuner_off_from_f51() {
        let data = field_varint_bytes(51, 6010);
        assert_eq!(parse_tuner_active(&data), Some(false));
    }
}
