//! Public C ABI: `nano_state_*` / `nano_rx_*` and the C305 feed entry points.

use esp_idf_svc::sys::*;
use log::{debug, error, info};

use nano_wire::builders;
use nano_wire::parsers;
use nano_wire::types::{Knob, NanoAmpC, BPM_MAX, BPM_MIN};

use super::amp_live::{reset_amp_sync, try_inline_amp_update};
use super::classify::{is_fe_stream, is_state_stream_chunk, sniff_unknown_c305};
use super::ctx::{
    alloc_buf, amp_copy, amp_knob_set, amp_publish, ble_write_c304, c_label, is_tuner_active,
    now_us, preset_catalog_zero, push_amp_ui, set_label, st, AMP_SEQ, KNOB_NAMES, NANO_KNOB_LEVEL,
    NANO_KNOB_TREBLE, NANO_RX_CURRENT, NANO_RX_IDLE, NANO_RX_METADATA, TAG, TAP_HISTORY_MAX,
};
use super::rx::{
    append_fe_payload, append_state_payload, check_state_rx_timeout, request_current_state_now,
};
use super::tempo::{on_tempo_notify, tempo_bpm_f, write_tempo_tap_bpm};
use super::timers::{ensure_timers, schedule_parse, schedule_refresh};

const STATE_REQUEST_DELAY_US: u64 = 2200 * 1000;
const TUNER_FAST_SYNC_US: u64 = 400 * 1000;
const TUNER_WATCH_POLL_US: u64 = 5000 * 1000;
const TAP_TIMEOUT_US: i64 = 2000 * 1000;
const TEMPO_EXIT_ECHO_IGNORE_US: i64 = 800 * 1000;

pub fn nano_state_reset() {
    reset_amp_sync();
    crate::tuner::nano_tuner_reset();
    crate::fx::nano_fx_reset();
    let s = st();
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = 0;
    s.tap_count = 0;
    crate::ui::screen::nano_ui_set_bpm(0, false);
    crate::ui::screen::nano_ui_set_tempo_mode(false);
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
    feed_c305(data);
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

    if len >= 2 && data[0] == 0x10 && data[1] == 0xC0 {
        if tuner_live {
            debug!(target: TAG, "C305 diverted (preset ACK) len={len}");
        } else {
            debug!(target: TAG, "Preset change ACK");
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
        schedule_parse();
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
        schedule_parse();
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
        info!(
            target: TAG,
            "BPM hunt: other C305 while mode={} len={len} lead={:02X}", s.mode, data[0]
        );
        {
            let hunt = &data[..len.min(48)];
            parsers::log_bytes_full(TAG, "BPM hunt other C305", hunt);
        }
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
    if knob > NANO_KNOB_TREBLE as u32 || !crate::ble::nano_ble_connected() {
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
    if !crate::ble::nano_ble_connected() {
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
    if !(1..=25).contains(&slot) || !crate::ble::nano_ble_connected() {
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

    unsafe {
        esp_timer_stop(s.sync_timer);
        esp_timer_start_once(s.sync_timer, STATE_REQUEST_DELAY_US);
    }
    set_label(&mut s.state_probe_label, "INITIAL");
    info!(target: TAG, "Sync started — preset state request in 2.2s");
}

/// Faster preset polling while tuner appears off (catches footswitch without C305).
pub fn nano_state_request_tuner_sync() {
    ensure_timers();
    let s = st();
    if s.tuner_fast_sync_timer.is_null() {
        return;
    }
    unsafe {
        esp_timer_stop(s.tuner_fast_sync_timer);
        esp_timer_start_once(s.tuner_fast_sync_timer, TUNER_FAST_SYNC_US);
    }
}

/// Faster preset polling while tuner appears off (catches footswitch without C305).
pub fn nano_state_tuner_watch_update(tuner_active: bool) {
    ensure_timers();
    let s = st();
    if s.tuner_watch_timer.is_null() {
        return;
    }

    if tuner_active {
        // Tuner live: route/mode come from C305 only — do not poll preset state.
        if s.tuner_watch_running {
            unsafe { esp_timer_stop(s.tuner_watch_timer) };
            s.tuner_watch_running = false;
        }
        if !s.tuner_fast_sync_timer.is_null() {
            unsafe { esp_timer_stop(s.tuner_fast_sync_timer) };
        }
        if !s.refresh_timer.is_null() {
            unsafe { esp_timer_stop(s.refresh_timer) };
        }
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
        unsafe { esp_timer_start_periodic(s.tuner_watch_timer, TUNER_WATCH_POLL_US) };
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
    let label = if label.is_empty() { "Preset refresh" } else { label };
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
    crate::ui::screen::nano_ui_set_bpm(bpm, true);
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
    if !crate::ble::nano_ble_connected() {
        return false;
    }
    s.tap_count = 0;
    let bpm_f = tempo_bpm_f();
    info!(target: TAG, "Tempo Mode enter @ {bpm_f:.0} BPM");
    let ok = write_tempo_tap_bpm(bpm_f, c"Tempo Mode enter");
    if ok {
        let bpm = bpm_f.round() as u16;
        s.bpm_known = true;
        s.last_bpm = bpm;
        s.tempo_mode_active = true;
        // Number only — blinker phase-locks on the pedal echo/notify.
        crate::ui::screen::nano_ui_set_bpm(bpm, true);
    }
    ok
}

/// Close Tempo Mode screen: best-effort exit-shaped C304 write.
pub fn nano_state_tempo_exit() -> bool {
    let s = st();
    s.tap_count = 0;
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = now_us() + TEMPO_EXIT_ECHO_IGNORE_US;
    if !crate::ble::nano_ble_connected() {
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
    if !crate::ble::nano_ble_connected() {
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

    let bpm = bpm_f.round() as u16;
    info!(target: TAG, "Tempo Mode tap #{} -> {bpm} BPM", s.tap_count);
    if !write_tempo_tap_bpm(bpm_f, c"Tempo Mode tap") {
        return false;
    }

    s.bpm_known = true;
    s.last_bpm = bpm;
    s.tempo_mode_active = true;
    // Update readout now; phase-lock on C305 echo so we match the Nano LED.
    crate::ui::screen::nano_ui_set_bpm(bpm, true);
    true
}
