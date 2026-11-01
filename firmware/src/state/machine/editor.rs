//! Sends the editor frames from `nano_wire::editor`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use nano_wire::editor::{
    assign_footswitch_payload, cab_float_payload, cab_mic_payload, delete_preset_payload,
    library_request_payload, load_cab_mic_payload, load_cab_payload, load_capture_payload,
    read_cab_settings_payload, read_expression_payload, read_settings_payload,
    rename_preset_payload, reset_expression_calibration_payload,
    save_expression_calibration_payload, save_preset_payload, select_preset_payload,
    state_lists_payload, tuner_mode_payload, write_expression_payload, write_jack_mode_payload,
    write_led_usb_payload, write_mute_payload, write_usb_playback_payload, CabFloat, ExpBypass,
    ExpBypassTarget, ExpRange, ExpRangeTarget, Footswitch, LibraryKind, MuteTarget, StateLists,
};

use nano_wire::types::JackMode;

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
    note_preset_selected(slot);
    true
}

/// Record `slot` (1..=64) as loaded and show its catalog name.
pub(super) fn note_preset_selected(slot: u8) {
    if !(1..=64).contains(&slot) {
        return;
    }
    let s = st();
    s.preset_slot = slot;
    let name = if s.preset_catalog.valid {
        super::ctx::c_bytes_to_string(&s.preset_catalog.names[usize::from(slot - 1)])
    } else {
        String::new()
    };
    if !name.is_empty() {
        s.preset_name = name.clone();
    }
    if s.preset_dirty {
        s.preset_dirty = false;
        crate::screen::nano_ui_set_preset_dirty(false);
    }
    if !name.is_empty() {
        crate::screen::nano_ui_set_preset_name(&name);
    }
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

/// True when another preset already uses `name` (the pedal compares case-insensitively).
pub fn nano_state_preset_name_taken(slot: u8, name: &str) -> bool {
    nano_state_preset_names()
        .iter()
        .enumerate()
        .any(|(i, other)| i + 1 != usize::from(slot) && other.eq_ignore_ascii_case(name))
}

fn send_rename(slot: u8, name: &str) -> bool {
    let mut buf = [0u8; 128];
    let n = rename_preset_payload(slot - 1, name, &mut buf);
    send(&format!("Rename preset {slot}"), &buf, n)
}

fn send_delete(slot: u8) -> bool {
    let mut buf = [0u8; 32];
    let n = delete_preset_payload(slot - 1, &mut buf);
    send(&format!("Delete preset {slot}"), &buf, n)
}

/// Re-read the catalog so the list shows what the pedal actually kept.
fn refresh_catalog() {
    super::api::nano_state_begin_sync();
}

pub fn nano_state_rename_preset(slot: u8, name: &str) -> bool {
    if !(1..=64).contains(&slot)
        || !nano_wire::editor::preset_name_valid(name)
        || nano_state_preset_name_taken(slot, name)
    {
        return false;
    }
    if !send_rename(slot, name) {
        return false;
    }
    refresh_catalog();
    true
}

pub fn nano_state_delete_preset(slot: u8) -> bool {
    if !(1..=64).contains(&slot) || !send_delete(slot) {
        return false;
    }
    refresh_catalog();
    true
}

/// Copy or move preset `src` to `dst` (both 1..=64). An occupied `dst` is
/// deleted first. The pedal wants unique names, so a copy gets a "name 2"
/// style name and a move parks the sound under a temporary name until the
/// source is gone, then renames it back.
pub fn nano_state_transfer_preset(src: u8, dst: u8, remove_src: bool) -> bool {
    if !(1..=64).contains(&src) || !(1..=64).contains(&dst) || src == dst {
        return false;
    }
    let names = nano_state_preset_names();
    let src_name = match names.get(usize::from(src - 1)) {
        Some(name) if name.len() >= 4 => name.clone(),
        _ => return false,
    };
    let dst_used = names
        .get(usize::from(dst - 1))
        .is_some_and(|name| !name.is_empty());
    let parked = unique_copy_name(&src_name, &names);
    if !nano_state_select_preset(src) {
        return false;
    }
    if dst_used && !send_delete(dst) {
        return false;
    }
    if !nano_state_copy_preset(dst, &parked) {
        return false;
    }
    if remove_src {
        let ok = send_delete(src) && send_rename(dst, &src_name) && nano_state_select_preset(dst);
        if !ok {
            return false;
        }
    }
    refresh_catalog();
    true
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
    let s = st();
    s.library_rx.clear();
    s.library_rx_active = false;
    s.library_rx_dropping = false;
    s.library_pending = true;
    s.library_request_us = super::ctx::now_us();
    if send("Library", &buf, n) {
        true
    } else {
        s.library_pending = false;
        s.library_request_us = 0;
        false
    }
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
    let n = load_cab_mic_payload(
        slot - 1,
        library_index,
        cab_name,
        position,
        mic_name,
        &mut buf,
    );
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
    if !send("Expression read", &buf, n) {
        return false;
    }
    st().expression_slot = slot;
    true
}

pub fn nano_state_read_cab_settings(slot: u8) -> bool {
    if !(1..=5).contains(&slot) {
        return false;
    }
    let mut buf = [0u8; 24];
    let n = read_cab_settings_payload(slot - 1, &mut buf);
    if !send("Cab settings read", &buf, n) {
        return false;
    }
    let s = st();
    s.cab_settings_slot = slot;
    s.cab_settings = nano_wire::types::CabSettings::default();
    crate::screen::nano_ui_set_cab_settings(slot, &s.cab_settings);
    true
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

pub fn nano_state_write_jack_mode(mode: JackMode) -> bool {
    let mut buf = [0u8; 16];
    let n = write_jack_mode_payload(mode, &mut buf);
    if !send("EXP MIDI jack mode", &buf, n) {
        return false;
    }
    nano_state_read_settings()
}

pub fn nano_state_write_usb_playback(db: f32) -> bool {
    let mut buf = [0u8; 16];
    let n = write_usb_playback_payload(db, &mut buf);
    if !send("USB playback level", &buf, n) {
        return false;
    }
    nano_state_read_settings()
}

/// Reads the cab output / filters whenever the preset or its cab slot changes.
pub(super) fn sync_cab_settings(complete_dump: bool) {
    let s = st();
    // The Nano omits zero fields, so a complete dump without field 12 means no cab.
    let cab_slot = match s.amp.active_cab_slot() {
        Some(slot) => slot,
        None if complete_dump => 0,
        None => return,
    };
    let key = (s.preset_slot, cab_slot);
    if key == s.cab_settings_key {
        return;
    }
    if (1..=5).contains(&key.1) {
        if nano_state_read_cab_settings(key.1) {
            st().cab_settings_key = key;
        }
    } else {
        s.cab_settings_key = key;
        s.cab_settings = nano_wire::types::CabSettings::default();
        crate::screen::nano_ui_set_cab_settings(0, &s.cab_settings);
    }
}

pub fn nano_state_tools_open() -> bool {
    let mut success = nano_state_read_settings();
    let s = st();
    let preset = s.preset_slot;
    let cab_slot = s.amp.active_cab_slot().unwrap_or(0);
    if preset > 0 {
        success &= nano_state_read_expression(preset);
    }
    if (1..=5).contains(&cab_slot) {
        success &= nano_state_read_cab_settings(cab_slot);
    } else {
        crate::screen::nano_ui_set_cab_settings(0, &nano_wire::types::CabSettings::default());
    }
    success
}

pub fn nano_state_expression_calibration_start() -> bool {
    let mut buf = [0u8; 16];
    let n = reset_expression_calibration_payload(&mut buf);
    if !send("Expression calibration reset", &buf, n) {
        return false;
    }
    let s = st();
    s.calibration_active = true;
    s.calibration_min = u8::MAX;
    s.calibration_max = 0;
    s.calibration_sampled = false;
    crate::screen::nano_ui_set_calibration(true, None, None);
    true
}

pub fn nano_state_expression_calibration_save() -> bool {
    let s = st();
    if !s.calibration_active || !s.calibration_sampled || s.calibration_min >= s.calibration_max {
        return false;
    }
    let min = s.calibration_min;
    let max = s.calibration_max;
    let mut buf = [0u8; 16];
    let n = save_expression_calibration_payload(min, max, &mut buf);
    if !send("Expression calibration save", &buf, n) {
        return false;
    }
    s.calibration_active = false;
    crate::screen::nano_ui_set_calibration(false, Some(min), Some(max));
    true
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
    if !send("Tuner mode", &buf, n) {
        return false;
    }
    if (400.0..=480.0).contains(&hz) {
        st().tuner_settings.reference_hz = Some(hz);
    }
    st().tuner_settings.muted = Some(muted);
    true
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
        if next.len() > nano_wire::editor::PRESET_NAME_LIMIT - 3 {
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
        if candidate.len() > nano_wire::editor::PRESET_NAME_LIMIT {
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
