//! FE metadata and preset "current state" C305 reassembly, parsing and refresh scheduling.

use core::ptr;
use core::slice;

use esp_idf_svc::sys::*;
use log::{info, warn};

use nano_wire::builders;
use nano_wire::parsers;
use nano_wire::types::{NanoAmpC, NanoPresetCatalogC, PRESET_COUNT, PRESET_NAME_MAX};
use nano_wire::varint::read_varint;

use super::amp_live::resolve_pending_level_from_dump;
use super::api::{
    nano_rx_mode_set, nano_state_on_bpm_found, nano_state_request_tuner_sync,
    nano_state_tuner_watch_update,
};
use super::ctx::{
    amp_copy, amp_knob_get, amp_knob_set, ble_write_c304, c_bytes_to_string, c_label,
    is_tuner_active, now_us, push_amp_ui, set_label, st, NANO_RX_CURRENT, NANO_RX_IDLE,
    STATE_BUF_MAX, TAG,
};
use super::timers::schedule_parse_in;

const REFRESH_MIN_INTERVAL_US: i64 = 150 * 1000;
const LIVE_POLL_INTERVAL_US: u64 = 15000 * 1000;
const STATE_RX_TIMEOUT_US: i64 = 5000 * 1000;
/// Max times we defer finish while waiting for a gate tail after mid-f34 truncation.
const STATE_INCOMPLETE_DEFER_MAX: u32 = 2;
/// Re-request current state when INITIAL/ROUTINE dump still lacks gate fields.
const STATE_GATE_RETRY_MAX: u32 = 2;
/// Quiet window after a chunk when the protobuf still looks truncated.
const PARSE_TAIL_US: u64 = 80 * 1000;
/// Quiet window when the buffer walks clean — catch a last MTU fragment only.
const PARSE_COMPLETE_US: u64 = 20 * 1000;

pub(super) fn append_fe_payload(data: &[u8]) {
    let s = st();
    if data.len() <= 2 || s.fe_buf.is_null() {
        return;
    }
    let payload = &data[2..];
    let plen = payload.len();
    if s.fe_len + plen > STATE_BUF_MAX {
        warn!(target: TAG, "FE metadata buffer full, dropping {plen} bytes");
        return;
    }
    unsafe { ptr::copy_nonoverlapping(payload.as_ptr(), s.fe_buf.add(s.fe_len), plen) };
    s.fe_len += plen;
}

fn fe_slice() -> &'static [u8] {
    let s = st();
    if s.fe_buf.is_null() || s.fe_len == 0 {
        return &[];
    }
    unsafe { slice::from_raw_parts(s.fe_buf, s.fe_len) }
}

fn state_slice(len: usize) -> &'static [u8] {
    let s = st();
    if s.state_buf.is_null() || len == 0 {
        return &[];
    }
    unsafe { slice::from_raw_parts(s.state_buf, len) }
}

pub(super) fn finish_fe_capture() {
    let s = st();
    if s.fe_len > 0 {
        let fe = fe_slice();
        if let Some(cat) = parsers::parse_fe_preset_catalog(fe) {
            s.preset_catalog = NanoPresetCatalogC::from(&cat);
            info!(
                target: TAG,
                "Preset catalog loaded ({} names)", s.preset_catalog.count
            );
        } else {
            warn!(
                target: TAG,
                "FE metadata parse found no preset names ({} bytes)", fe.len()
            );
        }

        if let Some(fe_bpm) = parsers::parse_bpm(fe) {
            info!(target: TAG, "BPM {fe_bpm} (field 56 in FE metadata)");
            nano_state_on_bpm_found(fe_bpm);
        } else {
            parsers::log_bpm_raw_scan(TAG, "FE metadata", fe);
        }
    }
    s.fe_len = 0;
}

fn update_preset_label_locked(state: &[u8]) {
    let s = st();
    let mut label = String::from("-");

    if let Some(direct) = parsers::first_string(state, 2).filter(|n| !n.is_empty()) {
        label = direct.chars().take(PRESET_NAME_MAX - 1).collect();
    } else if let Some(slot) = parsers::field_varint(state, 1) {
        let mut idx: u32 = 0;
        if slot >= 1 && slot <= PRESET_COUNT as u32 {
            idx = slot;
        } else if slot < PRESET_COUNT as u32 {
            idx = slot + 1;
        }
        if idx > 0 {
            let name = &s.preset_catalog.names[(idx - 1) as usize];
            if s.preset_catalog.valid && name[0] != 0 {
                label = c_bytes_to_string(name);
            } else {
                label = format!("Preset {idx}");
            }
        }
    }

    crate::ui::screen::nano_ui_set_preset_name(&label);
}

pub(super) fn append_state_payload(data: &[u8]) {
    let s = st();
    if data.len() <= 2 || s.state_buf.is_null() {
        return;
    }
    let payload = &data[2..];
    let plen = payload.len();
    if s.state_len + plen > STATE_BUF_MAX {
        warn!(target: TAG, "State buffer full, dropping {plen} bytes");
        return;
    }
    unsafe { ptr::copy_nonoverlapping(payload.as_ptr(), s.state_buf.add(s.state_len), plen) };
    s.state_len += plen;
    s.state_rx_since_us = now_us();
    info!(target: TAG, "State chunk +{plen} → {} bytes", s.state_len);
}

/// Pick a short quiet-window: complete dumps finish fast; truncated ones wait for tails.
pub(super) fn schedule_state_parse() {
    let s = st();
    let delay = if s.state_len == 0 {
        PARSE_COMPLETE_US
    } else {
        let buf = state_slice(s.state_len);
        if state_dump_truncated(buf) {
            PARSE_TAIL_US
        } else {
            PARSE_COMPLETE_US
        }
    };
    schedule_parse_in(delay);
}

fn finish_state_capture() {
    let s = st();
    s.state_len = 0;
    s.mode = NANO_RX_IDLE;
    s.state_rx_since_us = 0;
    s.incomplete_defer_count = 0;
    s.incomplete_probe_label.clear();
}

/// True when a length-delimited field claims more bytes than remain — typical
/// mid-f34 truncation that leaves gate f53/f54 in a later MTU tail.
fn state_dump_truncated(data: &[u8]) -> bool {
    if data.len() < 3 {
        return false;
    }

    let len = data.len();
    let mut i: usize = 0;
    while i < len {
        let Some((tag, next)) = read_varint(data, i) else {
            return i > 0;
        };
        i = next;
        let wire = tag & 0x07;
        if wire == 0 {
            let Some((_, next)) = read_varint(data, i) else {
                return true;
            };
            i = next;
        } else if wire == 1 {
            if i + 8 > len {
                return true;
            }
            i += 8;
        } else if wire == 2 {
            let Some((flen, next)) = read_varint(data, i) else {
                return true;
            };
            i = next;
            if i + flen as usize > len {
                return true;
            }
            i += flen as usize;
        } else if wire == 5 {
            if i + 4 > len {
                return true;
            }
            i += 4;
        } else {
            return true;
        }
    }
    false
}

fn abort_stale_state_rx(why: &str) {
    let s = st();
    if s.mode != NANO_RX_CURRENT {
        return;
    }
    warn!(
        target: TAG,
        "State RX aborted ({why}, had {} bytes)", s.state_len
    );
    // One last parse so a delayed gate tail still applies before we drop RX.
    if s.state_len > 0 {
        if !s.incomplete_probe_label.is_empty() {
            let label = s.incomplete_probe_label.clone();
            set_label(&mut s.state_probe_label, &label);
        }
        s.incomplete_defer_count = STATE_INCOMPLETE_DEFER_MAX;
        parse_state_buffer();
        return;
    }
    finish_state_capture();
}

pub(super) fn check_state_rx_timeout() {
    let s = st();
    if s.mode != NANO_RX_CURRENT || s.state_rx_since_us == 0 {
        return;
    }
    if now_us() - s.state_rx_since_us > STATE_RX_TIMEOUT_US {
        abort_stale_state_rx("timeout");
    }
}

fn maybe_retry_tuner_sync() {
    let s = st();
    if !s.pending_tuner_sync {
        return;
    }
    s.pending_tuner_sync = false;
    nano_state_request_tuner_sync();
}

fn start_live_poll() {
    let s = st();
    if s.poll_timer.is_null() || s.live_poll_active {
        return;
    }
    s.live_poll_active = true;
    unsafe { esp_timer_start_periodic(s.poll_timer, LIVE_POLL_INTERVAL_US) };
    info!(
        target: TAG,
        "Live amp sync: polling state every {}s",
        LIVE_POLL_INTERVAL_US / 1_000_000
    );
}

pub(super) fn send_state_request(label: &str, from_initial_sync: bool) {
    let s = st();

    // Preset FE streams stomp the live pitch mailbox — never poll while tuning.
    if !from_initial_sync && is_tuner_active() {
        if label.contains("Tuner") {
            s.pending_tuner_sync = true;
        }
        return;
    }

    if s.mode == NANO_RX_CURRENT {
        check_state_rx_timeout();
        if s.mode == NANO_RX_CURRENT {
            return;
        }
    }
    if !from_initial_sync && s.mode != NANO_RX_IDLE {
        if label.contains("Tuner") {
            s.pending_tuner_sync = true;
        }
        return;
    }

    let now = now_us();
    let force_probe = !s.state_probe_label.is_empty() || s.tuner_fast_sync_armed;
    if !from_initial_sync && !force_probe && now - s.last_refresh_us < REFRESH_MIN_INTERVAL_US {
        return;
    }
    s.tuner_fast_sync_armed = false;
    s.last_refresh_us = now;

    let mut buf = [0u8; 32];
    nano_rx_mode_set(NANO_RX_CURRENT);
    s.state_rx_since_us = now_us();
    let n = builders::request_current_state(&mut buf);
    if n > 0 {
        info!(target: TAG, "Sending preset state request");
        let label = if label.is_empty() {
            "Amp state refresh"
        } else {
            label
        };
        ble_write_c304(&c_label(label), &buf[..n]);
    }
}

pub(super) fn request_current_state_now(label: &str) {
    send_state_request(label, false);
}

pub(super) fn parse_state_buffer() {
    let s = st();
    if s.state_len == 0 {
        return;
    }

    let label = if !s.state_probe_label.is_empty() {
        s.state_probe_label.clone()
    } else if !s.incomplete_probe_label.is_empty() {
        s.incomplete_probe_label.clone()
    } else {
        String::from("ROUTINE")
    };
    s.state_probe_label.clear();

    let buf_len = s.state_len;
    let buf = state_slice(buf_len);
    info!(target: TAG, "Parsing preset state ({buf_len} bytes) [{label}]");
    if label == "INITIAL" {
        parsers::log_proto_fields(TAG, &label, buf);
    }

    let truncated = state_dump_truncated(buf);
    let has_gate = crate::fx::nano_fx_dump_has_gate_fields(buf);
    let wait_for_gate = truncated
        && !has_gate
        && s.mode == NANO_RX_CURRENT
        && s.incomplete_defer_count < STATE_INCOMPLETE_DEFER_MAX;

    let live_amp = amp_copy(&s.amp);
    let live_dirty = s.amp_live_dirty;
    let cap_dirty = s.cap_vol_live_dirty;
    if let Some(amp) = parsers::parse_amp(buf) {
        s.amp = NanoAmpC::from(&amp);
        /*
         * Field 44 often sits in the missing mid-dump gap. Keep a previously
         * known capture volume (notify / write / earlier full dump); only seed
         * the web-editor 0 dB default (raw 128) when still unknown.
         */
        if s.amp.have_capture_volume {
            s.cap_vol_kept = s.amp.capture_volume;
            s.cap_vol_kept_valid = true;
        } else if s.cap_vol_kept_valid {
            s.amp.capture_volume = s.cap_vol_kept;
            s.amp.have_capture_volume = true;
        } else {
            s.amp.capture_volume = 128;
            s.amp.have_capture_volume = true;
            s.cap_vol_kept = 128;
            s.cap_vol_kept_valid = true;
        }
        // Dump snapshot can be older than knobs turned during RX — keep live.
        if live_amp.valid && live_dirty != 0 {
            for knob in 0..5u8 {
                if live_dirty & (1u8 << knob) != 0 {
                    amp_knob_set(&mut s.amp, knob, amp_knob_get(&live_amp, knob));
                }
            }
        }
        if cap_dirty && live_amp.valid && live_amp.have_capture_volume {
            s.amp.capture_volume = live_amp.capture_volume;
            s.amp.have_capture_volume = true;
            s.cap_vol_kept = live_amp.capture_volume;
            s.cap_vol_kept_valid = true;
        }
        s.amp_live_dirty = 0;
        s.cap_vol_live_dirty = false;
        resolve_pending_level_from_dump();
        parsers::log_amp(TAG, &s.amp.to_amp());
        push_amp_ui();
        if !wait_for_gate {
            start_live_poll();
        }
    } else {
        warn!(target: TAG, "Could not find amp fields 3–7 in state blob");
    }

    if wait_for_gate {
        s.incomplete_defer_count += 1;
        set_label(&mut s.incomplete_probe_label, &label);
        info!(
            target: TAG,
            "Incomplete state dump ({buf_len} bytes, no gate tail) — waiting ({}/{})",
            s.incomplete_defer_count, STATE_INCOMPLETE_DEFER_MAX
        );
        // Apply whatever FX/gate/name we already have; keep RX open for late tails.
        crate::fx::nano_fx_sync_from_preset(buf);
        update_preset_label_locked(buf);
        // Prefer waiting for a real chunk; fall back to one short re-parse.
        schedule_parse_in(PARSE_TAIL_US);
        return;
    }

    finish_state_capture();

    match parsers::parse_tuner_state(buf) {
        Some((tuner_active, route)) => {
            let tuner_route = route as u32;
            if label == "ROUTE" || label == "TUNER" || tuner_active || is_tuner_active() {
                parsers::log_tuner_state_fields(TAG, buf);
            }
            crate::tuner::nano_tuner_sync_from_state(tuner_active, tuner_route);
        }
        None => {
            if label == "ROUTE" || label == "TUNER" || is_tuner_active() {
                parsers::log_tuner_state_fields(TAG, buf);
            }
            nano_state_tuner_watch_update(is_tuner_active());
        }
    }

    crate::fx::nano_fx_sync_from_preset(buf);

    update_preset_label_locked(buf);

    if let Some(bpm) = parsers::parse_bpm(buf) {
        info!(target: TAG, "BPM {bpm} (field 56) [{label}]");
        nano_state_on_bpm_found(bpm);
    } else {
        // Short dumps omit f56 — keep last live tap-tempo BPM instead of clearing.
        info!(target: TAG, "BPM field 56 absent len={buf_len} [{label}]");
        if label == "INITIAL" || buf_len != s.last_bpm_hunt_len {
            parsers::log_bpm_hunt(TAG, buf);
            s.last_bpm_hunt_len = buf_len;
        }
        if label == "INITIAL" && !s.bpm_known {
            warn!(
                target: TAG,
                "BPM still unknown after INITIAL (FE nested f56 should supply it)"
            );
        }
    }

    if !crate::fx::nano_fx_dump_has_gate_fields(buf)
        && s.gate_retry_count < STATE_GATE_RETRY_MAX
        && (label == "INITIAL" || label == "ROUTINE" || label == "PRESET")
    {
        s.gate_retry_count += 1;
        info!(
            target: TAG,
            "Gate fields absent after parse — retry {}/{}", s.gate_retry_count, STATE_GATE_RETRY_MAX
        );
        request_current_state_now("Gate sync retry");
    } else if crate::fx::nano_fx_dump_has_gate_fields(buf) {
        s.gate_retry_count = 0;
    }

    maybe_retry_tuner_sync();
}
