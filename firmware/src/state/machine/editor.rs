//! Sends the editor frames from `nano_wire::editor`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use nano_wire::editor::{
    assign_footswitch_payload, cab_float_payload, cab_mic_payload, delete_preset_payload,
    library_request_payload, load_cab_mic_payload, load_cab_payload, load_capture_payload,
    read_expression_payload, read_settings_payload, rename_preset_payload, save_preset_payload,
    select_preset_payload, state_lists_payload, tuner_mode_payload, write_expression_payload,
    write_led_usb_payload, write_mute_payload, CabFloat, ExpBypass, ExpBypassTarget, ExpRange,
    ExpRangeTarget, Footswitch, LibraryKind, MuteTarget, StateLists,
};

use super::ctx::{ble_write_c304, c_label, st};
use super::rx::request_current_state_now;

fn send(label: &str, buf: &[u8], n: usize) -> bool {
    if n == 0 || !crate::link::connected() {
        return false;
    }
    ble_write_c304(&c_label(label), &buf[..n])
}

fn on_foot(index0: u8) -> u8 {
    let slots = st().foot_slots;
    for (i, slot) in slots.iter().enumerate() {
        if *slot == i16::from(index0) {
            return i as u8;
        }
    }
    4
}

/// Load preset `slot` (1..=64). Footswitch assignments are left unchanged.
pub fn nano_state_select_preset(slot: u8) -> bool {
    if !(1..=64).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 96];
    let index = slot - 1;
    let n = select_preset_payload(index, on_foot(index), &mut buf);
    if !send(&format!("Preset {slot}"), &buf, n) {
        return false;
    }
    let s = st();
    s.preset_slot = slot;
    s.preset_dirty = false;
    crate::screen::nano_ui_set_preset_dirty(false);
    true
}

/// Store the loaded sound into the current preset under its current name.
pub fn nano_state_save_preset() -> bool {
    let s = st();
    let slot = s.preset_slot;
    let name = s.preset_name.clone();
    if !(1..=64).contains(&slot) || name.len() < 4 {
        return false;
    }
    let mut buf = [0u8; 128];
    let n = save_preset_payload(slot - 1, &name, &mut buf);
    if !send(&format!("Save preset {slot}"), &buf, n) {
        return false;
    }
    request_current_state_now("Save");
    true
}

/// Copy what is loaded now into another slot. The pedal requires a unique name
/// of at least 4 characters.
pub fn nano_state_copy_preset(slot: u8, name: &str) -> bool {
    if !(1..=64).contains(&slot) || name.len() < 4 {
        return false;
    }
    let mut buf = [0u8; 128];
    let n = save_preset_payload(slot - 1, name, &mut buf);
    send(&format!("Copy preset {slot}"), &buf, n)
}

pub fn nano_state_rename_preset(slot: u8, name: &str) -> bool {
    if !(1..=64).contains(&slot) || name.len() < 4 {
        return false;
    }
    let mut buf = [0u8; 128];
    let n = rename_preset_payload(slot - 1, name, &mut buf);
    send(&format!("Rename preset {slot}"), &buf, n)
}

pub fn nano_state_delete_preset(slot: u8) -> bool {
    if !(1..=64).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 32];
    let n = delete_preset_payload(slot - 1, &mut buf);
    send(&format!("Delete preset {slot}"), &buf, n)
}

/// Assign `slot` (1..=64) to one Nano footswitch. This drops unsaved edits.
pub fn nano_state_assign_footswitch(which: Footswitch, slot: u8) -> bool {
    if !(1..=64).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 96];
    let n = assign_footswitch_payload(which, slot - 1, &mut buf);
    send("Footswitch assign", &buf, n)
}

pub fn nano_state_request_library() -> bool {
    let mut buf = [0u8; 24];
    let n = library_request_payload(&mut buf);
    send("Library", &buf, n)
}

pub fn nano_state_load_library_capture(
    slot: u8,
    kind: LibraryKind,
    library_index: u32,
    name: &str,
) -> bool {
    if !(1..=25).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 160];
    let n = load_capture_payload(slot - 1, kind, library_index, name, &mut buf);
    send(&format!("Load capture {slot}"), &buf, n)
}

pub fn nano_state_load_library_cab(
    slot: u8,
    kind: LibraryKind,
    library_index: u32,
    name: &str,
) -> bool {
    if !(1..=5).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 160];
    let n = load_cab_payload(slot - 1, kind, library_index, name, &mut buf);
    send(&format!("Load cab {slot}"), &buf, n)
}

pub fn nano_state_write_cab_float(which: CabFloat, normalized: f32) -> bool {
    let mut buf = [0u8; 24];
    let n = cab_float_payload(which, normalized, &mut buf);
    send("Cab param", &buf, n)
}

pub fn nano_state_write_cab_mic(
    slot: u8,
    library_index: u32,
    cab_name: &str,
    position: u8,
    mic_name: &str,
) -> bool {
    if !(1..=5).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 160];
    let n = load_cab_mic_payload(slot - 1, library_index, cab_name, position, mic_name, &mut buf);
    if n == 0 {
        let n = cab_mic_payload(library_index, cab_name, position, mic_name, &mut buf);
        return send("Cab mic", &buf, n);
    }
    send("Cab mic slot", &buf, n)
}

pub fn nano_state_read_expression(slot: u8) -> bool {
    if !(1..=64).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 24];
    let n = read_expression_payload(slot - 1, &mut buf);
    send("Expression read", &buf, n)
}

pub fn nano_state_write_expression(
    slot: u8,
    ranges: &[(ExpRangeTarget, ExpRange)],
    bypasses: &[(ExpBypassTarget, ExpBypass)],
) -> bool {
    if !(1..=64).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 256];
    let n = write_expression_payload(slot - 1, ranges, bypasses, &mut buf);
    send("Expression write", &buf, n)
}

pub fn nano_state_read_settings() -> bool {
    let mut buf = [0u8; 16];
    let n = read_settings_payload(&mut buf);
    send("Settings read", &buf, n)
}

pub fn nano_state_write_led_usb(brightness: u8, usb_db: f32) -> bool {
    let mut buf = [0u8; 32];
    let n = write_led_usb_payload(brightness, usb_db, &mut buf);
    send("LED and USB volume", &buf, n)
}

pub fn nano_state_write_mute(which: MuteTarget, muted: bool) -> bool {
    let mut buf = [0u8; 16];
    let n = write_mute_payload(which, muted, &mut buf);
    send(if muted { "Mute on" } else { "Mute off" }, &buf, n)
}

/// Reference pitch 400..=480 Hz, and whether the tuner output is muted.
pub fn nano_state_set_tuner_reference(on: bool, hz: f32, live: bool, muted: bool) -> bool {
    let mut buf = [0u8; 32];
    let n = tuner_mode_payload(on, hz, live, muted, &mut buf);
    send("Tuner mode", &buf, n)
}

pub fn nano_state_request_lists(which: StateLists) -> bool {
    let mut buf = [0u8; 24];
    let n = state_lists_payload(which, &mut buf);
    send("State lists", &buf, n)
}

fn step_index(current: u8, delta: i8, first: u8, last: u8) -> u8 {
    let span = u16::from(last - first) + 1;
    let pos = u16::from(current.saturating_sub(first)) % span;
    let pos = if delta < 0 {
        (pos + span - 1) % span
    } else {
        (pos + 1) % span
    };
    first + pos as u8
}

fn unique_copy_name(current: &str, names: &[String]) -> String {
    let mut stem = String::new();
    for ch in current.chars() {
        let mut next = stem.clone();
        next.push(ch);
        if next.len() > 40 {
            break;
        }
        stem = next;
    }
    let stem = if stem.len() >= 4 {
        stem
    } else {
        String::from("Copy")
    };
    for n in 2..100 {
        let candidate = format!("{stem} {n}");
        if candidate.len() > 64 {
            continue;
        }
        if names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&candidate))
        {
            continue;
        }
        return candidate;
    }
    String::from("Copy 99")
}

pub fn nano_state_preset_slot() -> u8 {
    st().preset_slot
}

pub fn nano_state_preset_name() -> String {
    st().preset_name.clone()
}

/// Load the previous or next catalog preset. `delta` is negative or positive.
pub fn nano_state_step_preset(delta: i8) -> bool {
    if delta == 0 {
        return false;
    }
    let names = nano_state_preset_names();
    let count = if names.is_empty() {
        64
    } else {
        names.len().min(64) as u8
    };
    if count == 0 {
        return false;
    }
    let current = nano_state_preset_slot();
    let current = if (1..=count).contains(&current) {
        current
    } else {
        1
    };
    let next = step_index(current, delta, 1, count);
    if !nano_state_select_preset(next) {
        return false;
    }
    if let Some(name) = names.get((next - 1) as usize) {
        if !name.is_empty() {
            crate::screen::nano_ui_set_preset_name(name);
        }
    }
    true
}

/// Store the loaded sound in the next slot under a name the catalog does not already use.
pub fn nano_state_copy_current() -> bool {
    let slot = nano_state_preset_slot();
    if !(1..=64).contains(&slot) {
        return false;
    }
    let live = nano_state_preset_name();
    let names = nano_state_preset_names();
    let current = if live.len() >= 4 {
        live
    } else if let Some(name) = names.get((slot - 1) as usize) {
        if name.len() >= 4 {
            name.clone()
        } else {
            String::from("Copy")
        }
    } else {
        String::from("Copy")
    };
    let next = if slot == 64 { 1 } else { slot + 1 };
    nano_state_copy_preset(next, &unique_copy_name(&current, &names))
}

/// Step the factory capture slot (1..=25). The pedal already knows these slots.
pub fn nano_state_step_capture(delta: i8) -> bool {
    if delta == 0 {
        return false;
    }
    let slot = st().amp.capture_slot;
    let current = if (1..=25).contains(&slot) { slot } else { 1 };
    let next = step_index(current, delta, 1, 25);
    super::api::nano_state_write_capture_slot(next)
}

pub fn nano_state_preset_dirty() -> bool {
    st().preset_dirty
}

/// Catalog names in slot order. Empty entries stay empty strings.
pub fn nano_state_preset_names() -> Vec<String> {
    let s = st();
    if !s.preset_catalog.valid {
        return Vec::new();
    }
    s.preset_catalog
        .names
        .iter()
        .take(s.preset_catalog.count as usize)
        .map(|name| super::ctx::c_bytes_to_string(name))
        .collect()
}

pub(super) fn note_preset_dirty(dirty: bool) {
    let s = st();
    if s.preset_dirty == dirty {
        return;
    }
    s.preset_dirty = dirty;
    crate::screen::nano_ui_set_preset_dirty(dirty);
}

pub(super) fn note_loaded_preset(name: &str, slot: u8, foot: [i16; 4]) {
    let s = st();
    if slot >= 1 && slot <= 64 {
        s.preset_slot = slot;
    }
    if !name.is_empty() && name != "-" {
        s.preset_name = name.to_string();
    }
    s.foot_slots = foot;
}
