//! Preset, library, cab, expression, and device-settings frames.
//!
//! Layouts match the community editors (DrD85 / AJs-Creations) and the
//! 2026-10-09 phone-app trace on NanOS 2.2.1. Single-packet only: the length
//! byte is one octet, so a frame that would exceed 255 bytes is rejected.

use crate::varint::{encode_int32, encode_varint, read_varint};

pub const MSG_SAVE_PRESET: u32 = 3;
pub const MSG_PRESET_SLOTS: u32 = 29;
pub const MSG_DELETE_PRESET: u32 = 50;
pub const MSG_READ_SETTINGS: u32 = 65;
pub const MSG_WRITE_SETTINGS: u32 = 67;
pub const MSG_LIBRARY: u32 = 76;
pub const MSG_LOAD_CAB: u32 = 78;
pub const MSG_LOAD_CAPTURE: u32 = 80;
pub const MSG_CAB_PARAM: u32 = 94;
pub const MSG_READ_EXPRESSION: u32 = 60;
pub const MSG_WRITE_EXPRESSION: u32 = 62;
pub const MSG_RENAME_PRESET: u32 = 111;
pub const MSG_DIRTY: u32 = 115;
pub const MSG_TUNER_MODE: u32 = 127;

/// Footswitch slot in 4-Preset mode. Wire field on a type 29 assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Footswitch {
    Ia = 5,
    Ib = 6,
    Iia = 7,
    Iib = 8,
}

/// Which list a type 1 state read should include.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateLists {
    /// No name lists. Same shape as the live preset refresh.
    Current,
    Presets,
    Captures,
    Irs,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryKind {
    Factory,
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CabFloat {
    /// Tag `0x2D`, normalized 0..1.
    Level,
    /// Tag `0x35`.
    HighPass,
    /// Tag `0x3D`.
    LowPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpRangeTarget {
    Gain = 4,
    Bass = 5,
    Mid = 6,
    Treble = 7,
    Pre1Amount = 8,
    Pre2Amount = 9,
    Post1Amount = 10,
    Post2Amount = 11,
    Post3Amount = 12,
    CaptureVolume = 13,
    Gate = 21,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpBypassTarget {
    Capture = 14,
    Cab = 15,
    Pre1 = 16,
    Pre2 = 17,
    Post1 = 18,
    Post2 = 19,
    Post3 = 20,
    Gate = 22,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpRange {
    pub inverted: bool,
    pub min: u8,
    pub max: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpBypass {
    HeelToe { inverted: bool, delay_ms: u16 },
    Switch { inverted: bool, latch: bool },
    Stop { delay_ms: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuteTarget {
    /// Type 67 field 12. Tag `0x60`.
    Input,
    /// Type 67 field 13. Tag `0x68`.
    Output,
}

struct Pay {
    buf: [u8; 240],
    len: usize,
}

impl Pay {
    fn new() -> Self {
        Self {
            buf: [0; 240],
            len: 0,
        }
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.len >= self.buf.len() {
            return false;
        }
        self.buf[self.len] = byte;
        self.len += 1;
        true
    }

    fn push_slice(&mut self, bytes: &[u8]) -> bool {
        if self.len + bytes.len() > self.buf.len() {
            return false;
        }
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        true
    }

    fn varint(&mut self, value: u32) -> bool {
        let mut tmp = [0u8; 5];
        let Some(n) = encode_varint(value, &mut tmp) else {
            return false;
        };
        self.push_slice(&tmp[..n])
    }

    fn int32(&mut self, value: i32) -> bool {
        let mut tmp = [0u8; 10];
        let Some(n) = encode_int32(value, &mut tmp) else {
            return false;
        };
        self.push_slice(&tmp[..n])
    }

    fn f32(&mut self, value: f32) -> bool {
        self.push_slice(&value.to_le_bytes())
    }

    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

fn finish(payload: &[u8], msg: u32, buf: &mut [u8]) -> usize {
    let body = payload.len() + 4;
    let total = body + 2;
    if body > 255 || total > buf.len() {
        return 0;
    }
    buf[0] = body as u8;
    buf[1] = 0xC0;
    buf[2..2 + payload.len()].copy_from_slice(payload);
    let at = 2 + payload.len();
    buf[at] = (msg & 0xFF) as u8;
    buf[at + 1] = ((msg >> 8) & 0xFF) as u8;
    buf[at + 2] = 0;
    buf[at + 3] = 0;
    total
}

fn preset_index_ok(index: u8) -> bool {
    index < 64
}

/// Trailing message type of a single-packet `C0` frame.
pub fn message_type(data: &[u8]) -> Option<u32> {
    if data.len() < 6 || data[1] != 0xC0 {
        return None;
    }
    let n = data.len();
    Some(
        u32::from(data[n - 4])
            | (u32::from(data[n - 3]) << 8)
            | (u32::from(data[n - 2]) << 16)
            | (u32::from(data[n - 1]) << 24),
    )
}

fn payload_of(data: &[u8]) -> Option<&[u8]> {
    message_type(data)?;
    Some(&data[2..data.len() - 4])
}

/// First varint of `field`, if present.
pub fn payload_varint(data: &[u8], field: u32) -> Option<u32> {
    let payload = payload_of(data)?;
    let mut i = 0;
    while i < payload.len() {
        let (tag, next) = read_varint(payload, i)?;
        i = next;
        let num = tag >> 3;
        let wire = tag & 7;
        match wire {
            0 => {
                let (val, next) = read_varint(payload, i)?;
                if num == field {
                    return Some(val);
                }
                i = next;
            }
            1 => {
                if i + 8 > payload.len() {
                    return None;
                }
                i += 8;
            }
            2 => {
                let (len, next) = read_varint(payload, i)?;
                i = next + len as usize;
                if i > payload.len() {
                    return None;
                }
            }
            5 => {
                if i + 4 > payload.len() {
                    return None;
                }
                i += 4;
            }
            _ => return None,
        }
    }
    None
}

/// Type 115. Field 3 = 1 means unsaved edits. No field 3 means clean.
pub fn preset_dirty(data: &[u8]) -> Option<bool> {
    if message_type(data)? != MSG_DIRTY {
        return None;
    }
    Some(payload_varint(data, 3) == Some(1))
}

/// Load preset `index` (0-based). Footswitch slots are left unchanged (`-1`).
/// `on_foot` is which of IA/IB/IIA/IIB already holds it, or 4 when none does.
pub fn select_preset_payload(index: u8, on_foot: u8, buf: &mut [u8]) -> usize {
    if !preset_index_ok(index) || on_foot > 4 {
        return 0;
    }
    let mut p = Pay::new();
    // field 3 context = PlayMode
    if !p.push(0x18) || !p.push(0x00) || !p.push(0x20) || !p.varint(u32::from(index)) {
        return 0;
    }
    for tag in [0x28u8, 0x30, 0x38, 0x40] {
        if !p.push(tag) || !p.int32(-1) {
            return 0;
        }
    }
    if !p.push(0x48) || !p.varint(u32::from(on_foot)) {
        return 0;
    }
    finish(p.bytes(), MSG_PRESET_SLOTS, buf)
}

/// Put `index` on one footswitch and leave the other three alone.
/// Field 4 is `-1`, so the loaded preset does not change.
/// An assignment discards unsaved edits on the pedal.
pub fn assign_footswitch_payload(which: Footswitch, index: u8, buf: &mut [u8]) -> usize {
    if !preset_index_ok(index) {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x18) || !p.push(0x00) || !p.push(0x20) || !p.int32(-1) {
        return 0;
    }
    for slot in [Footswitch::Ia, Footswitch::Ib, Footswitch::Iia, Footswitch::Iib] {
        let tag = (slot as u8) << 3;
        let value = if slot == which {
            i32::from(index)
        } else {
            -1
        };
        if !p.push(tag) || !p.int32(value) {
            return 0;
        }
    }
    if !p.push(0x48) || !p.push(0x00) {
        return 0;
    }
    finish(p.bytes(), MSG_PRESET_SLOTS, buf)
}

fn name_bytes(name: &str) -> Option<&[u8]> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || bytes.contains(&0) {
        return None;
    }
    Some(bytes)
}

/// Type 3. Stores whatever is loaded now into `index` under `name`.
pub fn save_preset_payload(index: u8, name: &str, buf: &mut [u8]) -> usize {
    let Some(name) = name_bytes(name) else {
        return 0;
    };
    if !preset_index_ok(index) {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x08)
        || !p.push(0x01)
        || !p.push(0x18)
        || !p.varint(u32::from(index))
        || !p.push(0x2A)
        || !p.varint(name.len() as u32)
        || !p.push_slice(name)
    {
        return 0;
    }
    finish(p.bytes(), MSG_SAVE_PRESET, buf)
}

/// Type 111. The pedal rejects a name another preset already uses.
pub fn rename_preset_payload(index: u8, name: &str, buf: &mut [u8]) -> usize {
    let Some(name) = name_bytes(name) else {
        return 0;
    };
    if !preset_index_ok(index) {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x08)
        || !p.push(0x01)
        || !p.push(0x18)
        || !p.varint(u32::from(index))
        || !p.push(0x22)
        || !p.varint(name.len() as u32)
        || !p.push_slice(name)
    {
        return 0;
    }
    finish(p.bytes(), MSG_RENAME_PRESET, buf)
}

/// Type 50 `{1: 2, 3: index}`.
pub fn delete_preset_payload(index: u8, buf: &mut [u8]) -> usize {
    if !preset_index_ok(index) {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x08) || !p.push(0x02) || !p.push(0x18) || !p.varint(u32::from(index)) {
        return 0;
    }
    finish(p.bytes(), MSG_DELETE_PRESET, buf)
}

/// Type 76. Factory and user captures and IRs.
pub fn library_request_payload(buf: &mut [u8]) -> usize {
    finish(&[0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x30, 0x01], MSG_LIBRARY, buf)
}

fn library_slot_payload(
    msg: u32,
    slot_index: u8,
    kind: LibraryKind,
    library_index: u32,
    name: &str,
    mic: Option<(u8, &str)>,
    buf: &mut [u8],
) -> usize {
    let Some(name) = name_bytes(name) else {
        return 0;
    };
    let mut id = Pay::new();
    if !id.push(0x08) || !id.varint(library_index) || !id.push(0x12) || !id.varint(name.len() as u32) || !id.push_slice(name)
    {
        return 0;
    }
    if let Some((position, mic_name)) = mic {
        let Some(mic_name) = name_bytes(mic_name) else {
            return 0;
        };
        if !(1..=6).contains(&position) {
            return 0;
        }
        if !id.push(0x18)
            || !id.varint(u32::from(position - 1))
            || !id.push(0x22)
            || !id.varint(mic_name.len() as u32)
            || !id.push_slice(mic_name)
        {
            return 0;
        }
    }
    let tag = match kind {
        LibraryKind::Factory => 0x22,
        LibraryKind::User => 0x2A,
    };
    let mut p = Pay::new();
    if !p.push(0x18)
        || !p.varint(u32::from(slot_index))
        || !p.push(tag)
        || !p.varint(id.len as u32)
        || !p.push_slice(id.bytes())
    {
        return 0;
    }
    finish(p.bytes(), msg, buf)
}

/// Type 80. `slot` and the library item are 0-based.
pub fn load_capture_payload(
    slot: u8,
    kind: LibraryKind,
    library_index: u32,
    name: &str,
    buf: &mut [u8],
) -> usize {
    if slot >= 25 {
        return 0;
    }
    library_slot_payload(MSG_LOAD_CAPTURE, slot, kind, library_index, name, None, buf)
}

/// Type 78. `slot` is 0-based, 0..=4.
pub fn load_cab_payload(
    slot: u8,
    kind: LibraryKind,
    library_index: u32,
    name: &str,
    buf: &mut [u8],
) -> usize {
    if slot >= 5 {
        return 0;
    }
    library_slot_payload(MSG_LOAD_CAB, slot, kind, library_index, name, None, buf)
}

/// Type 78 with mic and position inside the factory IR record.
/// `position` is 1..=6. `library_index` must be the cab's real library index;
/// `0` is ignored by the pedal.
pub fn load_cab_mic_payload(
    slot: u8,
    library_index: u32,
    cab_name: &str,
    position: u8,
    mic_name: &str,
    buf: &mut [u8],
) -> usize {
    if slot >= 5 || library_index == 0 {
        return 0;
    }
    library_slot_payload(
        MSG_LOAD_CAB,
        slot,
        LibraryKind::Factory,
        library_index,
        cab_name,
        Some((position, mic_name)),
        buf,
    )
}

/// Type 94 float. `normalized` is clamped to 0..=1.
pub fn cab_float_payload(which: CabFloat, normalized: f32, buf: &mut [u8]) -> usize {
    if !normalized.is_finite() {
        return 0;
    }
    let tag = match which {
        CabFloat::Level => 0x2D,
        CabFloat::HighPass => 0x35,
        CabFloat::LowPass => 0x3D,
    };
    let mut p = Pay::new();
    if !p.push(tag) || !p.f32(normalized.clamp(0.0, 1.0)) {
        return 0;
    }
    finish(p.bytes(), MSG_CAB_PARAM, buf)
}

/// Type 94 mic record. `library_index` is field 1 of the type 96 cab record.
pub fn cab_mic_payload(
    library_index: u32,
    cab_name: &str,
    position: u8,
    mic_name: &str,
    buf: &mut [u8],
) -> usize {
    let Some(cab_name) = name_bytes(cab_name) else {
        return 0;
    };
    let Some(mic_name) = name_bytes(mic_name) else {
        return 0;
    };
    if library_index == 0 || !(1..=6).contains(&position) {
        return 0;
    }
    let mut inner = Pay::new();
    if !inner.push(0x08)
        || !inner.varint(library_index)
        || !inner.push(0x12)
        || !inner.varint(cab_name.len() as u32)
        || !inner.push_slice(cab_name)
        || !inner.push(0x18)
        || !inner.varint(u32::from(position - 1))
        || !inner.push(0x22)
        || !inner.varint(mic_name.len() as u32)
        || !inner.push_slice(mic_name)
    {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x1A) || !p.varint(inner.len as u32) || !p.push_slice(inner.bytes()) {
        return 0;
    }
    finish(p.bytes(), MSG_CAB_PARAM, buf)
}

/// Type 60 `{1: 3, 3: presetIndex}`.
pub fn read_expression_payload(index: u8, buf: &mut [u8]) -> usize {
    if !preset_index_ok(index) {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x08) || !p.push(0x03) || !p.push(0x18) || !p.varint(u32::from(index)) {
        return 0;
    }
    finish(p.bytes(), MSG_READ_EXPRESSION, buf)
}

fn push_record(p: &mut Pay, field: u8, inner: &[u8]) -> bool {
    let tag = (u32::from(field) << 3) | 2;
    p.varint(tag) && p.varint(inner.len() as u32) && p.push_slice(inner)
}

/// Type 62. Replaces the whole expression set. Empty slices clear it.
/// Records are written in ascending field order.
pub fn write_expression_payload(
    index: u8,
    ranges: &[(ExpRangeTarget, ExpRange)],
    bypasses: &[(ExpBypassTarget, ExpBypass)],
    buf: &mut [u8],
) -> usize {
    if !preset_index_ok(index) {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x18) || !p.varint(u32::from(index)) {
        return 0;
    }
    let mut fields: Vec<u8> = ranges
        .iter()
        .map(|(t, _)| *t as u8)
        .chain(bypasses.iter().map(|(t, _)| *t as u8))
        .collect();
    fields.sort_unstable();
    fields.dedup();
    for field in fields {
        if let Some((_, range)) = ranges.iter().find(|(t, _)| *t as u8 == field) {
            let mut inner = Pay::new();
            if !inner.push(0x08)
                || !inner.push(u8::from(range.inverted))
                || !inner.push(0x10)
                || !inner.varint(u32::from(range.min))
                || !inner.push(0x18)
                || !inner.varint(u32::from(range.max))
                || !push_record(&mut p, field, inner.bytes())
            {
                return 0;
            }
            continue;
        }
        let Some((_, bypass)) = bypasses.iter().find(|(t, _)| *t as u8 == field) else {
            continue;
        };
        let mut inner = Pay::new();
        let oneof = match *bypass {
            ExpBypass::HeelToe { inverted, delay_ms } => {
                let delay = u32::from(delay_ms.min(2000));
                let mut body = Pay::new();
                if !body.push(0x08)
                    || !body.push(u8::from(inverted))
                    || !body.push(0x10)
                    || !body.varint(delay)
                {
                    return 0;
                }
                (1u8, body)
            }
            ExpBypass::Switch { inverted, latch } => {
                let mut body = Pay::new();
                if !body.push(0x08) || !body.push(u8::from(inverted)) || !body.push(0x10) || !body.push(u8::from(latch))
                {
                    return 0;
                }
                (2u8, body)
            }
            ExpBypass::Stop { delay_ms } => {
                let delay = u32::from(delay_ms.min(2000));
                let mut body = Pay::new();
                if !body.push(0x08) || !body.varint(delay) {
                    return 0;
                }
                (3u8, body)
            }
        };
        if !push_record(&mut inner, oneof.0, oneof.1.bytes()) || !push_record(&mut p, field, inner.bytes()) {
            return 0;
        }
    }
    finish(p.bytes(), MSG_WRITE_EXPRESSION, buf)
}

/// Type 65 `{1: 3}`.
pub fn read_settings_payload(buf: &mut [u8]) -> usize {
    finish(&[0x08, 0x03], MSG_READ_SETTINGS, buf)
}

/// Type 67 fields 4 and 14 together, the way the phone app writes them.
/// `brightness` is 0..=255. `usb_db` is the USB playback volume.
pub fn write_led_usb_payload(brightness: u8, usb_db: f32, buf: &mut [u8]) -> usize {
    if !usb_db.is_finite() {
        return 0;
    }
    let mut p = Pay::new();
    if !p.push(0x08) || !p.push(0x01) || !p.push(0x20) || !p.varint(u32::from(brightness)) || !p.push(0x75) || !p.f32(usb_db)
    {
        return 0;
    }
    finish(p.bytes(), MSG_WRITE_SETTINGS, buf)
}

/// Type 67 mute. A zero is written out; it is the value that means unmuted.
pub fn write_mute_payload(which: MuteTarget, muted: bool, buf: &mut [u8]) -> usize {
    let tag = match which {
        MuteTarget::Input => 0x60,
        MuteTarget::Output => 0x68,
    };
    finish(&[tag, u8::from(muted)], MSG_WRITE_SETTINGS, buf)
}

/// Type 127. `hz` is clamped to 400..=480. `live` is true when this client
/// opened the tuner, false when the pedal already had it open.
pub fn tuner_mode_payload(on: bool, hz: f32, live: bool, muted: bool, buf: &mut [u8]) -> usize {
    if !on {
        return finish(&[0x20, 0x00], MSG_TUNER_MODE, buf);
    }
    if !hz.is_finite() {
        return 0;
    }
    let hz = hz.clamp(400.0, 480.0);
    let mut p = Pay::new();
    if !p.push(0x20)
        || !p.push(0x01)
        || !p.push(0x2D)
        || !p.f32(hz)
        || !p.push(0x30)
        || !p.push(u8::from(live))
        || !p.push(0x38)
        || !p.push(u8::from(muted))
    {
        return 0;
    }
    finish(p.bytes(), MSG_TUNER_MODE, buf)
}

/// Type 1. Field 3 omits presets, field 4 omits captures, field 5 omits IRs
/// when the value is 1.
pub fn state_lists_payload(which: StateLists, buf: &mut [u8]) -> usize {
    let body: &[u8] = match which {
        StateLists::All => &[0x08, 0x03],
        StateLists::Current => &[0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01],
        StateLists::Presets => &[0x08, 0x03, 0x18, 0x00, 0x20, 0x01, 0x28, 0x01],
        StateLists::Captures => &[0x08, 0x03, 0x18, 0x01, 0x20, 0x00, 0x28, 0x01],
        StateLists::Irs => &[0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x00],
    };
    finish(body, 1, buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_select_index_13_not_on_a_footswitch() {
        let mut buf = [0u8; 80];
        let n = select_preset_payload(13, 4, &mut buf);
        assert!(n > 0);
        assert_eq!(message_type(&buf[..n]), Some(MSG_PRESET_SLOTS));
        assert_eq!(&buf[2..6], &[0x18, 0x00, 0x20, 0x0D]);
        assert_eq!(buf[n - 6], 0x48);
        assert_eq!(buf[n - 5], 0x04);
        assert_eq!(&buf[n - 4..n], &[0x1D, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn footswitch_ib_gets_index_0() {
        let mut buf = [0u8; 96];
        let n = assign_footswitch_payload(Footswitch::Ib, 0, &mut buf);
        assert!(buf[..n].windows(2).any(|w| w == [0x30, 0x00]));
        assert_eq!(&buf[n - 6..n - 4], &[0x48, 0x00]);
        assert_eq!(message_type(&buf[..n]), Some(29));
    }

    #[test]
    fn save_and_delete_match_the_trace() {
        let mut buf = [0u8; 64];
        let n = save_preset_payload(62, "A", &mut buf);
        assert_eq!(&buf[..n], &[0x0B, 0xC0, 0x08, 0x01, 0x18, 0x3E, 0x2A, 0x01, b'A', 0x03, 0x00, 0x00, 0x00]);
        let n = delete_preset_payload(23, &mut buf);
        assert_eq!(&buf[..n], &[0x08, 0xC0, 0x08, 0x02, 0x18, 0x17, 0x32, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn expression_gain_and_clear_match_the_trace() {
        let mut buf = [0u8; 64];
        let n = write_expression_payload(
            61,
            &[(
                ExpRangeTarget::Gain,
                ExpRange {
                    inverted: false,
                    min: 0,
                    max: 255,
                },
            )],
            &[],
            &mut buf,
        );
        assert_eq!(
            &buf[..n],
            &[
                0x0F, 0xC0, 0x18, 0x3D, 0x22, 0x07, 0x08, 0x00, 0x10, 0x00, 0x18, 0xFF, 0x01, 0x3E, 0x00, 0x00, 0x00
            ]
        );
        let n = write_expression_payload(61, &[], &[], &mut buf);
        assert_eq!(&buf[..n], &[0x06, 0xC0, 0x18, 0x3D, 0x3E, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn led_and_usb_match_the_trace() {
        let mut buf = [0u8; 32];
        let n = write_led_usb_payload(153, -6.0, &mut buf);
        assert_eq!(
            &buf[..n],
            &[0x0E, 0xC0, 0x08, 0x01, 0x20, 0x99, 0x01, 0x75, 0x00, 0x00, 0xC0, 0xC0, 0x43, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn mute_input_and_tuner_and_lists() {
        let mut buf = [0u8; 32];
        let n = write_mute_payload(MuteTarget::Input, true, &mut buf);
        assert_eq!(&buf[..n], &[0x06, 0xC0, 0x60, 0x01, 0x43, 0x00, 0x00, 0x00]);
        let n = tuner_mode_payload(true, 440.0, true, true, &mut buf);
        assert_eq!(message_type(&buf[..n]), Some(127));
        assert_eq!(&buf[2..8], &[0x20, 0x01, 0x2D, 0x00, 0x00, 0xDC]);
        assert_eq!(buf[8], 0x43);
        let n = tuner_mode_payload(false, 440.0, false, false, &mut buf);
        assert_eq!(&buf[..n], &[0x06, 0xC0, 0x20, 0x00, 0x7F, 0x00, 0x00, 0x00]);
        let n = state_lists_payload(StateLists::Presets, &mut buf);
        assert_eq!(
            &buf[..n],
            &[0x0C, 0xC0, 0x08, 0x03, 0x18, 0x00, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn cab_mic_keeps_the_library_index() {
        let mut buf = [0u8; 64];
        let n = cab_mic_payload(5, "Cab", 3, "Dyn", &mut buf);
        assert!(n > 0);
        assert_eq!(message_type(&buf[..n]), Some(94));
        assert!(buf[..n].windows(2).any(|w| w == [0x08, 0x05]));
        assert_eq!(cab_mic_payload(0, "Cab", 3, "Dyn", &mut buf), 0);
    }

    #[test]
    fn dirty_flag() {
        let dirty = [0x06, 0xC0, 0x08, 0x01, 0x18, 0x01, 0x73, 0x00, 0x00, 0x00];
        let clean = [0x04, 0xC0, 0x08, 0x01, 0x73, 0x00, 0x00, 0x00];
        assert_eq!(preset_dirty(&dirty), Some(true));
        assert_eq!(preset_dirty(&clean), Some(false));
    }

    #[test]
    fn library_capture_names_the_slot() {
        let mut buf = [0u8; 64];
        let n = load_capture_payload(8, LibraryKind::Factory, 3, "Plexi", &mut buf);
        assert!(n > 0);
        assert_eq!(message_type(&buf[..n]), Some(80));
        assert_eq!(&buf[2..5], &[0x18, 0x08, 0x22]);
    }
}
