//! Public C ABI: `nano_state_*` / `nano_rx_*` and the C305 feed entry points.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::vec::Vec;

use log::{debug, error, info, warn};

use nano_wire::builders;
use nano_wire::parsers;
use nano_wire::types::{
    DeviceSettings, Knob, LibraryCatalog, MeteringKind, NanoAmpC, TunerSettings, BPM_MAX, BPM_MIN,
};

use super::amp_live::{reset_amp_sync, try_inline_amp_update};
use super::classify::{is_fe_stream, is_state_stream_chunk, sniff_unknown_c305};
use super::ctx::{
    alloc_buf, amp_copy, amp_knob_set, amp_publish, ble_write_c304, c_label, is_tuner_active,
    now_us, preset_catalog_zero, push_amp_ui, set_label, st, AMP_SEQ, KNOB_NAMES, LIBRARY_BUF_MAX,
    NANO_KNOB_LEVEL, NANO_KNOB_TREBLE, NANO_RX_CURRENT, NANO_RX_IDLE, NANO_RX_METADATA, TAG,
    TAP_HISTORY_MAX,
};
use super::rx::{
    append_fe_payload, append_state_payload, check_state_rx_timeout, request_current_state_now,
    schedule_state_parse,
};
use super::tempo::{on_tempo_notify, tempo_bpm_f, write_tempo_tap_bpm};
use super::timers::{
    cancel_tempo_auto_exit, ensure_timers, schedule_refresh, schedule_tempo_auto_exit,
};

const STATE_REQUEST_DELAY_US: u64 = 2200 * 1000;
const TUNER_FAST_SYNC_US: u64 = 400 * 1000;
const TUNER_WATCH_POLL_US: u64 = 5000 * 1000;
const TAP_TIMEOUT_US: i64 = 2000 * 1000;
const TEMPO_EXIT_ECHO_IGNORE_US: i64 = 800 * 1000;

pub fn nano_state_reset() {
    reset_amp_sync();
    crate::tuner::nano_tuner_reset();
    crate::fx::nano_fx_reset();
    cancel_tempo_auto_exit();
    let s = st();
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = 0;
    s.tap_count = 0;
    s.library_pending = false;
    s.library_request_us = 0;
    s.library_rx_active = false;
    s.library_rx_dropping = false;
    s.library_rx.clear();
    s.c305_rx.reset();
    s.tuner_settings = TunerSettings::default();
    s.device_settings = nano_wire::types::DeviceSettings::default();
    s.cab_settings = nano_wire::types::CabSettings::default();
    s.cab_settings_slot = 0;
    s.cab_settings_key = (u8::MAX, u8::MAX);
    s.expression_assignments = nano_wire::editor::ExpressionAssignments::default();
    s.expression_slot = 0;
    s.meters = [None; 3];
    s.calibration_active = false;
    s.calibration_min = u8::MAX;
    s.calibration_max = 0;
    s.calibration_sampled = false;
    s.protocol_status.clear();
    crate::screen::nano_ui_set_bpm(0, false);
    crate::screen::nano_ui_set_tempo_mode(false);
    crate::screen::nano_ui_set_tuner_settings(440.0, false);
    crate::screen::nano_ui_set_device_settings(&s.device_settings);
    crate::screen::nano_ui_set_cab_settings(0, &s.cab_settings);
    crate::screen::nano_ui_set_expression_assignments(0, &s.expression_assignments);
    crate::screen::nano_ui_clear_tool_meters();
    crate::screen::nano_ui_set_calibration(false, None, None);
    crate::screen::nano_ui_set_protocol_status("");
}

pub fn nano_state_library_snapshot() -> LibraryCatalog {
    st().library_catalog.clone()
}

pub fn nano_state_tuner_settings() -> TunerSettings {
    st().tuner_settings
}

pub fn nano_state_device_settings() -> DeviceSettings {
    st().device_settings
}

pub fn nano_state_metering() -> [Option<u32>; 3] {
    st().meters
}

pub fn nano_state_cab_settings_snapshot() -> (u8, nano_wire::types::CabSettings) {
    let s = st();
    (s.cab_settings_slot, s.cab_settings)
}

pub fn nano_state_expression_assignments_snapshot() -> (u8, nano_wire::editor::ExpressionAssignments)
{
    let s = st();
    (s.expression_slot, s.expression_assignments.clone())
}

pub fn nano_state_calibration_snapshot() -> (bool, Option<u8>, Option<u8>) {
    let s = st();
    (
        s.calibration_active,
        s.calibration_sampled.then_some(s.calibration_min),
        s.calibration_sampled.then_some(s.calibration_max),
    )
}

pub fn nano_rx_mode_set(mode: u32) {
    let s = st();
    s.mode = mode;
    if mode == NANO_RX_CURRENT {
        s.state_len = 0;
    }
}

pub fn nano_rx_mode_get() -> u32 {
    st().mode
}

/// Feed one C305 notification (handles FE stream reassembly + amp parse).
pub fn feed_c305_bytes(data: &[u8]) {
    if data.is_empty() {
        return;
    }
    if feed_library_chunk(data) {
        return;
    }
    let mode = st().mode;
    if mode == NANO_RX_METADATA || mode == NANO_RX_CURRENT {
        match st().c305_rx.push(data) {
            Ok(nano_wire::c305::FragmentStatus::Incomplete) => return,
            Ok(nano_wire::c305::FragmentStatus::Complete) => {
                let message = st().c305_rx.take_message();
                if message.len() < 4 {
                    warn!(target: TAG, "Fragmented C305 message missing its message type");
                    return;
                }
                let type_at = message.len() - 4;
                let message_type = u32::from_le_bytes([
                    message[type_at],
                    message[type_at + 1],
                    message[type_at + 2],
                    message[type_at + 3],
                ]);
                if message_type == 2 {
                    info!(
                        target: TAG,
                        "C305 reassembled state response ({} bytes)",
                        type_at
                    );
                    super::rx::apply_full_state_response(&message[..type_at]);
                } else {
                    debug!(
                        target: TAG,
                        "C305 reassembled message type {message_type} ({} bytes) ignored",
                        type_at
                    );
                }
                return;
            }
            Ok(nano_wire::c305::FragmentStatus::NotFragment) => {}
            Err(error) => {
                warn!(target: TAG, "C305 fragment reassembly failed: {error:?}");
                return;
            }
        }
    }
    if handle_editor_message(data) {
        return;
    }
    feed_c305(data);
}

fn handle_editor_message(data: &[u8]) -> bool {
    let Some(message_type) = nano_wire::editor::message_type(data) else {
        return false;
    };
    let Some(payload) = nano_wire::editor::message_payload(data, message_type) else {
        return false;
    };
    let s = st();
    match message_type {
        nano_wire::editor::MSG_EXPRESSION_RESPONSE => {
            match parsers::parse_expression_assignments(payload) {
                Some(assignments) => {
                    info!(
                        target: TAG,
                        "Expression assignments loaded for preset {} ({} ranges, {} switches)",
                        s.expression_slot,
                        assignments.ranges.len(),
                        assignments.bypasses.len()
                    );
                    s.expression_assignments = assignments;
                    crate::screen::nano_ui_set_expression_assignments(
                        s.expression_slot,
                        &s.expression_assignments,
                    );
                }
                None => warn!(target: TAG, "Malformed expression-assignment response"),
            }
            return true;
        }
        nano_wire::editor::MSG_METERING => {
            if let Some(meter) = parsers::parse_metering(payload) {
                let index = meter.kind as usize;
                s.meters[index] = Some(meter.value);
                if meter.kind == MeteringKind::ExpressionPedal && s.calibration_active {
                    let value = meter.value.min(u32::from(u8::MAX)) as u8;
                    s.calibration_min = s.calibration_min.min(value);
                    s.calibration_max = s.calibration_max.max(value);
                    s.calibration_sampled = true;
                    crate::screen::nano_ui_set_calibration(
                        true,
                        Some(s.calibration_min),
                        Some(s.calibration_max),
                    );
                }
                crate::screen::nano_ui_set_metering(meter);
            } else {
                warn!(target: TAG, "Malformed Nano metering message");
            }
            return true;
        }
        nano_wire::editor::MSG_SETTINGS_RESPONSE => {
            match parsers::parse_device_settings(payload) {
                Some(settings) => {
                    s.device_settings = settings;
                    crate::screen::nano_ui_set_device_settings(&settings);
                }
                None => warn!(target: TAG, "Settings response had no supported fields"),
            }
            return true;
        }
        nano_wire::editor::MSG_CAB_SETTINGS_RESPONSE => {
            match parsers::parse_cab_settings(payload) {
                Some(settings) => {
                    s.cab_settings = settings;
                    crate::screen::nano_ui_set_cab_settings(s.cab_settings_slot, &settings);
                }
                None => warn!(
                    target: TAG,
                    "Cab settings reply without the values ({} bytes): {:02x?}",
                    payload.len(),
                    &payload[..payload.len().min(64)]
                ),
            }
            return true;
        }
        nano_wire::editor::MSG_PRESET_SLOTS => {
            let mut ack = [0u8; 16];
            let n = builders::preset_change_ack(&mut ack);
            if n > 0 && ble_write_c304(&c_label("Preset change acknowledgment"), &ack[..n]) {
                info!(target: TAG, "Acknowledged Nano preset change");
            } else {
                warn!(target: TAG, "Could not acknowledge Nano preset change");
            }
            // Field 4 is the 0-based index of the preset the pedal just loaded.
            if let Some(index) = nano_wire::editor::payload_varint(data, 4).filter(|i| *i < 64) {
                super::editor::note_preset_selected(index as u8 + 1);
            }
            set_label(&mut s.state_probe_label, "PRESET");
            if s.mode == NANO_RX_IDLE {
                request_current_state_now("Pedal preset change");
            }
            return true;
        }
        _ => {}
    }

    if let Some((response, status)) = nano_wire::editor::response_ack(data) {
        let label = match response {
            nano_wire::editor::MSG_PRESET_SLOTS_RESPONSE => "Preset change",
            nano_wire::editor::MSG_SETTINGS_WRITE_RESPONSE => "Settings write",
            nano_wire::editor::MSG_CAB_LOAD_RESPONSE => "Cab load",
            nano_wire::editor::MSG_CAPTURE_LOAD_RESPONSE => "Capture load",
            nano_wire::editor::MSG_RENAME_PRESET_RESPONSE => "Preset rename",
            _ => "Bluetooth request",
        };
        let text = match status {
            Some(1) => alloc::format!("{label}: OK"),
            Some(code) => alloc::format!("{label}: rejected ({code})"),
            None => alloc::format!("{label}: acknowledged"),
        };
        info!(target: TAG, "{text}");
        s.protocol_status = text.clone();
        crate::screen::nano_ui_set_protocol_status(&text);
        return true;
    }
    false
}

fn feed_library_chunk(data: &[u8]) -> bool {
    let s = st();
    if s.library_pending && now_us() - s.library_request_us > 30_000_000 {
        warn!(target: TAG, "Nano library response timed out");
        s.library_pending = false;
        s.library_request_us = 0;
        s.library_rx_dropping = s.library_rx_active;
        s.library_rx_active = false;
        s.library_rx.clear();
    }
    if !s.library_pending && !s.library_rx_active && !s.library_rx_dropping {
        return false;
    }
    if data.len() < 2 {
        return false;
    }
    // Flag bits: 0x40 starts a stream, 0x80 ends it. Other bits can be set too.
    if s.library_rx_dropping {
        if data[1] & 0x80 != 0 {
            s.library_rx_dropping = false;
        }
        return true;
    }

    if data[1] == 0xC0 {
        if nano_wire::editor::message_type(data) != Some(nano_wire::editor::MSG_LIBRARY_CONTENT) {
            return false;
        }
        s.library_pending = false;
        s.library_request_us = 0;
        s.library_rx.clear();
        s.library_rx_active = false;
        s.library_rx_dropping = false;
        match parsers::parse_library(&data[2..data.len() - 4]) {
            Some(catalog) => {
                info!(
                    target: TAG,
                    "Nano library loaded ({} captures, {} IRs)",
                    catalog.captures.len(),
                    catalog.cabs.len()
                );
                s.library_catalog = catalog;
                crate::screen::nano_ui_library_updated();
                if s.amp.valid {
                    push_amp_ui();
                }
            }
            None => error!(target: TAG, "Malformed Nano library response"),
        }
        return true;
    }

    if data[1] & 0x40 != 0 {
        s.library_rx.clear();
        s.library_rx_active = true;
        s.library_rx_dropping = false;
    } else if !s.library_rx_active {
        return false;
    }
    let frame_size = data.len().saturating_add(2);
    if data.len() > u16::MAX as usize
        || s.library_rx.len().saturating_add(frame_size) > LIBRARY_BUF_MAX
        || s.library_rx.try_reserve(frame_size).is_err()
    {
        error!(
            target: TAG,
            "Nano library response exceeds {} byte receive limit",
            LIBRARY_BUF_MAX
        );
        s.library_rx.clear();
        s.library_rx_active = false;
        s.library_pending = false;
        s.library_request_us = 0;
        s.library_rx_dropping = data[1] & 0x80 == 0;
        return true;
    }
    s.library_rx
        .extend_from_slice(&(data.len() as u16).to_le_bytes());
    s.library_rx.extend_from_slice(data);
    if data[1] & 0x80 == 0 {
        return true;
    }

    s.library_rx_active = false;
    s.library_pending = false;
    s.library_request_us = 0;
    let frames = core::mem::take(&mut s.library_rx);
    let mut payload = Vec::new();
    let mut offset = 0;
    while offset < frames.len() {
        if offset + 2 > frames.len() {
            error!(target: TAG, "Malformed buffered C305 frame length");
            return true;
        }
        let frame_len = u16::from_le_bytes([frames[offset], frames[offset + 1]]) as usize;
        let start = offset + 2;
        let end = start.saturating_add(frame_len);
        if end > frames.len() || frame_len < 2 {
            error!(target: TAG, "Malformed buffered C305 frame");
            return true;
        }
        if payload.try_reserve(frame_len - 2).is_err() {
            error!(target: TAG, "Unable to allocate Nano library response buffer");
            return true;
        }
        payload.extend_from_slice(&frames[start + 2..end]);
        offset = end;
    }
    if payload.len() < 4 {
        error!(target: TAG, "Nano library response is missing its message type");
        return true;
    }
    let type_at = payload.len() - 4;
    let message_type = u32::from_le_bytes([
        payload[type_at],
        payload[type_at + 1],
        payload[type_at + 2],
        payload[type_at + 3],
    ]);
    if message_type != nano_wire::editor::MSG_LIBRARY_CONTENT {
        error!(target: TAG, "Unexpected fragmented C305 message type {message_type}");
        let mut offset = 0;
        while offset < frames.len() {
            let frame_len = u16::from_le_bytes([frames[offset], frames[offset + 1]]) as usize;
            let start = offset + 2;
            let end = start + frame_len;
            feed_c305(&frames[start..end]);
            offset = end;
        }
        return true;
    }
    match parsers::parse_library(&payload[..type_at]) {
        Some(catalog) => {
            info!(
                target: TAG,
                "Nano library loaded ({} captures, {} IRs)",
                catalog.captures.len(),
                catalog.cabs.len()
            );
            s.library_catalog = catalog;
            crate::screen::nano_ui_library_updated();
            if s.amp.valid {
                push_amp_ui();
            }
        }
        None => error!(target: TAG, "Malformed Nano library response"),
    }
    true
}

fn feed_c305(data: &[u8]) {
    let s = st();
    let len = data.len();

    check_state_rx_timeout();

    let tuner_live = is_tuner_active();
    if tuner_live {
        crate::tuner::nano_tuner_note_c305_rx();
    }

    /*
     * Pitch / tuner frames are length-prefixed: <len> C0 08 01 22 …
     * Natural mid-range 22/01 often uses len=0x10 — do NOT treat that as preset ACK.
     */
    if len >= 5 && data[1] == 0xC0 && data[2] == 0x08 && data[3] == 0x01 && data[4] == 0x22 {
        if crate::tuner::feed_c305_bytes(data) {
            return;
        }
    }

    if let Some(on) = nano_wire::editor::tuner_mode_notify(data) {
        if on {
            crate::tuner::nano_tuner_opened_on_nano();
        } else {
            crate::tuner::nano_tuner_closed_on_nano();
        }
        return;
    }

    if let Some(dirty) = nano_wire::editor::preset_dirty(data) {
        super::editor::note_preset_dirty(dirty);
        return;
    }

    if len >= 2 && data[0] == 0x10 && data[1] == 0xC0 {
        if tuner_live {
            debug!(target: TAG, "C305 diverted (preset ACK) len={len}");
        } else {
            info!(target: TAG, "Preset change ACK — requesting state");
            set_label(&mut s.state_probe_label, "PRESET");
            crate::fx::nano_fx_clear_model_trust();
            crate::fx::nano_fx_note_preset_change();
            if s.mode == NANO_RX_IDLE {
                request_current_state_now("Preset change");
            }
        }
        return;
    }

    // While tuner is live, ignore preset/amp/FX traffic — pitch must stay snappy.
    if is_state_stream_chunk(data) {
        if tuner_live {
            debug!(
                target: TAG,
                "C305 dropped (state stream FE 41) while tuner live len={len}"
            );
            return;
        }
        if s.mode == NANO_RX_METADATA {
            debug!(target: TAG, "State stream chunk before request ({len} bytes)");
            return;
        }
        if s.mode == NANO_RX_IDLE {
            s.state_len = 0;
            s.mode = NANO_RX_CURRENT;
            s.state_rx_since_us = now_us();
            s.incomplete_defer_count = 0;
            s.incomplete_probe_label.clear();
        }
        if data[1] == 0xC1 {
            info!(target: TAG, "BPM hunt: C1 state dump chunk len={len}");
        }
        append_state_payload(data);
        schedule_state_parse();
        return;
    }

    /*
     * Segmented current-state continuations (desktop decoder):
     *  - byte1 high-bit continuation frames
     *  - additional FE/FD/CE/D0 segments after the first chunk
     *
     * Live notifies use lead byte C0 (high bit set) — never treat those as
     * dump continuations or knob turns pollute the state buffer.
     */
    if s.mode == NANO_RX_CURRENT
        && len >= 3
        && data[1] != 0xC0
        && ((data[1] & 0x80) != 0
            || data[0] == 0xFE
            || data[0] == 0xFD
            || data[0] == 0xCE
            || data[0] == 0xD0)
    {
        if tuner_live {
            debug!(
                target: TAG,
                "C305 dropped (state continuation) while tuner live len={len}"
            );
            return;
        }
        append_state_payload(data);
        schedule_state_parse();
        return;
    }

    if is_fe_stream(data, s.mode) {
        if tuner_live {
            debug!(target: TAG, "C305 dropped (FE stream) while tuner live len={len}");
            return;
        }
        if s.mode == NANO_RX_METADATA {
            append_fe_payload(data);
        }
        return;
    }

    if tuner_live {
        // Amp/FX updates wait until tuner exits — do not spend cycles on them.
        debug!(
            target: TAG,
            "C305 unhandled while tuner live len={len} lead=0x{:02X}", data[0]
        );
        return;
    }

    if let Some((bpm, tap)) = parsers::try_parse_bpm_notify_ex(&data[..len]) {
        on_tempo_notify(bpm, tap);
        return;
    }

    if try_inline_amp_update(data) {
        return;
    }

    if crate::tuner::feed_c305_bytes(data) {
        return;
    }

    if crate::fx::nano_fx_feed_c305(data) {
        return;
    }

    /* Late dump tails (byte1 high-bit, often 0x80) carry gate f53/f54 after a
     * mid-f34 gap. When RX already closed, salvage gate directly from the payload. */
    if len > 2 {
        let tail = &data[2..];
        if crate::fx::nano_fx_try_apply_gate_bytes(tail) {
            return;
        }
    }
    if crate::fx::nano_fx_try_apply_gate_bytes(data) {
        return;
    }

    if s.mode == NANO_RX_IDLE {
        sniff_unknown_c305(data);
        schedule_refresh();
    } else {
        debug!(
            target: TAG,
            "other C305 while mode={} len={len} lead={:02X}",
            s.mode,
            data[0]
        );
    }
}

/// Apply live amp / capture-volume notifies from the GATT callback (no FIFO).
pub fn try_amp_notify_fast_bytes(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    try_inline_amp_update(data)
}

pub fn nano_state_amp_ready() -> bool {
    st().amp.valid
}

/// Consistent copy for the UI task (seqlock). Returns false when amp is not valid.
pub fn nano_state_snapshot_amp(out: &mut NanoAmpC) -> bool {
    AMP_SEQ.read(|| {
        let s = st();
        if s.amp.valid {
            *out = amp_copy(&s.amp);
            true
        } else {
            false
        }
    })
}

/// Optimistically update amp state and send C304 knob write (touch UI).
pub fn nano_state_write_amp_knob(knob: u32, raw: u8) -> bool {
    let s = st();
    if knob > NANO_KNOB_TREBLE as u32 || !crate::link::connected() {
        return false;
    }
    let knob_id = knob as u8;

    amp_knob_set(&mut s.amp, knob_id, raw);
    s.amp.valid = true;
    if s.mode == NANO_RX_CURRENT {
        s.amp_live_dirty |= 1u8 << knob_id;
    }
    if knob_id == NANO_KNOB_LEVEL {
        s.master_write_us = now_us();
        s.master_write_raw = raw as i32;
    }
    amp_publish();

    let mut buf = [0u8; 32];
    let Some(k) = Knob::from_u8(knob_id) else {
        return false;
    };
    let n = builders::amp_knob_payload(k, raw, &mut buf);
    if n == 0 {
        return false;
    }

    let name = KNOB_NAMES[knob_id as usize];
    debug!(target: TAG, "Amp {name} -> raw {raw} (touch)");
    ble_write_c304(&c_label(&format!("Amp {name} = {raw}")), &buf[..n])
}

/// Capture volume (field 44 / write id 0x0A), raw 0..255.
pub fn nano_state_write_capture_volume(raw: u8) -> bool {
    let s = st();
    if !crate::link::connected() {
        return false;
    }

    s.amp.capture_volume = raw;
    s.amp.have_capture_volume = true;
    s.amp.valid = true;
    s.cap_vol_kept = raw;
    s.cap_vol_kept_valid = true;
    if s.mode == NANO_RX_CURRENT {
        s.cap_vol_live_dirty = true;
    }

    let mut buf = [0u8; 32];
    let n = builders::capture_volume_payload(raw, &mut buf);
    if n == 0 {
        return false;
    }

    debug!(target: TAG, "Capture volume -> raw {raw} (touch)");
    push_amp_ui();
    ble_write_c304(c"Capture volume", &buf[..n])
}

/// Select factory/user capture slot 1..25 and refresh preset state.
pub fn nano_state_write_capture_slot(slot: u8) -> bool {
    let s = st();
    if !(1..=25).contains(&slot) || !crate::link::connected() {
        return false;
    }

    s.amp.capture_slot = slot;
    s.amp.have_capture_slot = true;
    if let Some(name) = crate::amp::capture_name(slot).map(|name| name.to_owned()) {
        let dst = &mut s.amp.capture_name;
        dst.fill(0);
        let bytes = name.as_bytes();
        let n = bytes.len().min(dst.len() - 1);
        dst[..n].copy_from_slice(&bytes[..n]);
        s.amp.have_capture_name = true;
    }
    s.amp.valid = true;
    push_amp_ui();

    let mut buf = [0u8; 16];
    let n = builders::capture_select_payload(slot, &mut buf);
    if n == 0 {
        return false;
    }

    let label = format!("Capture slot {slot}");
    info!(target: TAG, "{label}");
    if !ble_write_c304(&c_label(&label), &buf[..n]) {
        return false;
    }

    request_preset_refresh("Capture select");
    true
}

/// Call after C305 notify enabled: sends metadata, then state request ~2.2s later.
pub fn nano_state_begin_sync() {
    let mut buf = [0u8; 32];

    // Amp/metadata sync only — tuner mode is learned from live C305 traffic.
    reset_amp_sync();
    let s = st();
    if s.state_buf.is_null() {
        s.state_buf = alloc_buf();
        if s.state_buf.is_null() {
            error!(target: TAG, "Failed to allocate state buffer");
            return;
        }
    }
    if s.fe_buf.is_null() {
        s.fe_buf = alloc_buf();
        if s.fe_buf.is_null() {
            error!(target: TAG, "Failed to allocate FE metadata buffer");
            return;
        }
    }
    s.fe_len = 0;
    s.preset_catalog = preset_catalog_zero();

    ensure_timers();

    nano_rx_mode_set(NANO_RX_METADATA);

    let n = builders::fe_metadata_request(&mut buf);
    if n > 0 {
        ble_write_c304(c"FE metadata request", &buf[..n]);
    }

    s.sync_timer.start_once(STATE_REQUEST_DELAY_US);
    set_label(&mut s.state_probe_label, "INITIAL");
    info!(target: TAG, "Sync started — preset state request in 2.2s");
}

/// Faster preset polling while tuner appears off (catches footswitch without C305).
pub fn nano_state_request_tuner_sync() {
    ensure_timers();
    let s = st();

    s.tuner_fast_sync_timer.start_once(TUNER_FAST_SYNC_US);
}

/// Faster preset polling while tuner appears off (catches footswitch without C305).
pub fn nano_state_tuner_watch_update(tuner_active: bool) {
    ensure_timers();
    let s = st();

    if tuner_active {
        // Tuner live: route/mode come from C305 only — do not poll preset state.
        if s.tuner_watch_running {
            s.tuner_watch_timer.stop();
            s.tuner_watch_running = false;
        }
        s.tuner_fast_sync_timer.stop();
        s.refresh_timer.stop();
        // Abort an in-flight FE/state capture so leftover chunks can't block pitch.
        if s.mode == NANO_RX_CURRENT {
            info!(target: TAG, "Aborting preset state RX — tuner live");
            s.mode = NANO_RX_IDLE;
            s.state_len = 0;
            s.state_rx_since_us = 0;
            s.state_probe_label.clear();
        }
        return;
    }

    if !s.amp.valid {
        return;
    }

    if !s.tuner_watch_running {
        s.tuner_watch_timer.start_periodic(TUNER_WATCH_POLL_US);
        s.tuner_watch_running = true;
        if s.mode == NANO_RX_IDLE {
            request_current_state_now("Tuner watch");
        } else {
            s.pending_tuner_sync = true;
        }
    }
}

fn request_preset_refresh(label: &str) {
    ensure_timers();
    let s = st();
    if s.mode == NANO_RX_IDLE && s.amp.valid {
        request_current_state_now(label);
    }
}

/// Re-read preset state (e.g. after FX block toggle).
pub fn nano_state_request_preset_refresh(label: &str) {
    let label = if label.is_empty() {
        "Preset refresh"
    } else {
        label
    };
    request_preset_refresh(label);
}

/// Apply discovered BPM and stop boot probes.
pub fn nano_state_on_bpm_found(bpm: u16) {
    let s = st();
    if bpm < BPM_MIN || bpm > BPM_MAX {
        return;
    }
    s.bpm_known = true;
    s.last_bpm = bpm;
    crate::screen::nano_ui_set_bpm(bpm, true);
}

/// Last known BPM (0 if unknown).
pub fn nano_state_get_bpm() -> u16 {
    let s = st();
    if s.bpm_known {
        s.last_bpm
    } else {
        0
    }
}

/// True while Nano Tempo Mode is considered active (UI or pedal).
pub fn nano_state_tempo_active() -> bool {
    st().tempo_mode_active
}

/// Open Tempo Mode screen: enter Nano Tempo Mode via C304 tap frame.
pub fn nano_state_tempo_enter() -> bool {
    let s = st();
    if !crate::link::connected() {
        return false;
    }
    s.tap_count = 0;
    let bpm_f = tempo_bpm_f();
    info!(target: TAG, "Tempo Mode enter @ {bpm_f:.0} BPM");
    let ok = write_tempo_tap_bpm(bpm_f, c"Tempo Mode enter");
    if ok {
        let bpm = crate::float::round(bpm_f) as u16;
        s.bpm_known = true;
        s.last_bpm = bpm;
        s.tempo_mode_active = true;
        // Number only — blinker phase-locks on the pedal echo/notify.
        crate::screen::nano_ui_set_bpm(bpm, true);
    }
    ok
}

/// Close Tempo Mode screen: best-effort exit-shaped C304 write.
pub fn nano_state_tempo_exit() -> bool {
    cancel_tempo_auto_exit();
    let s = st();
    s.tap_count = 0;
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = now_us() + TEMPO_EXIT_ECHO_IGNORE_US;
    if !crate::link::connected() {
        return false;
    }
    let bpm_f = tempo_bpm_f();
    let mut buf = [0u8; 24];
    let n = builders::bpm_exit_payload(bpm_f, &mut buf);
    if n == 0 {
        return false;
    }
    info!(target: TAG, "Tempo Mode exit @ {bpm_f:.0} BPM");
    ble_write_c304(c"Tempo Mode exit", &buf[..n])
}

/// TAP button: send C304 tap frame. From the 2nd interval onward average BPM
/// and update UI/blinker.
pub fn nano_state_tempo_tap() -> bool {
    let s = st();
    if !crate::link::connected() {
        return false;
    }

    let now = now_us();
    if s.tap_count > 0 && (now - s.tap_times[s.tap_count - 1]) > TAP_TIMEOUT_US {
        s.tap_count = 0;
    }

    if s.tap_count >= TAP_HISTORY_MAX {
        s.tap_times.copy_within(1.., 0);
        s.tap_count = TAP_HISTORY_MAX - 1;
    }
    s.tap_times[s.tap_count] = now;
    s.tap_count += 1;

    let mut bpm_f = tempo_bpm_f();
    if s.tap_count >= 2 {
        let mut sum_us = 0.0f64;
        for i in 1..s.tap_count {
            sum_us += (s.tap_times[i] - s.tap_times[i - 1]) as f64;
        }
        let avg_us = sum_us / (s.tap_count - 1) as f64;
        if avg_us >= 1.0 {
            bpm_f = (60_000_000.0 / avg_us) as f32;
            if bpm_f < BPM_MIN as f32 {
                bpm_f = BPM_MIN as f32;
            } else if bpm_f > BPM_MAX as f32 {
                bpm_f = BPM_MAX as f32;
            }
        }
    }

    let bpm = crate::float::round(bpm_f) as u16;
    info!(target: TAG, "Tempo Mode tap #{} -> {bpm} BPM", s.tap_count);
    if !write_tempo_tap_bpm(bpm_f, c"Tempo Mode tap") {
        return false;
    }

    s.bpm_known = true;
    s.last_bpm = bpm;
    s.tempo_mode_active = true;
    // Update readout now; phase-lock on C305 echo so we match the Nano LED.
    crate::screen::nano_ui_set_bpm(bpm, true);
    // Footswitch / TAP: leave Tempo Mode a few seconds after the last tap.
    schedule_tempo_auto_exit();
    true
}
