//! Live amp / capture-volume knob updates from GATT notifies, plus sync reset.

use esp_idf_svc::sys::*;
use log::debug;

use nano_wire::parsers::{
    try_parse_amp_hw_notify, try_parse_amp_knob_notify, try_parse_capture_volume_notify,
};

use super::ctx::{
    amp_knob_get, amp_knob_set, amp_publish, amp_zero, now_us, push_amp_ui, st, KNOB_NAMES,
    NANO_KNOB_LEVEL, NANO_RX_CURRENT, NANO_RX_IDLE, TAG,
};

fn apply_knob_update(knob_id: u8, raw: u8, source: &str) -> bool {
    let s = st();
    if knob_id > 4 {
        return false;
    }

    if s.amp.valid && amp_knob_get(&s.amp, knob_id) == raw {
        return false;
    }

    amp_knob_set(&mut s.amp, knob_id, raw);
    s.amp.valid = true;
    if s.mode == NANO_RX_CURRENT {
        s.amp_live_dirty |= 1u8 << knob_id;
    }
    debug!(
        target: TAG,
        "Amp {} -> raw {raw} ({source})", KNOB_NAMES[knob_id as usize]
    );
    push_amp_ui();
    true
}

/**
 * Pedal LEVEL notifies: always C0 08 01 18 01 20 <v> 30 01 …
 * Same frame for normal LEVEL and capture-volume mode — no distinct BLE id.
 * Map LEVEL notifies to MASTER only. CAPTURE LV is driven by id 0x0A / field 44.
 */
fn on_shared_level_notify(raw: u8) {
    let s = st();
    let now = now_us();
    if s.master_write_raw >= 0
        && (now - s.master_write_us) < (500 * 1000)
        && raw as i32 == s.master_write_raw
    {
        apply_knob_update(NANO_KNOB_LEVEL, raw, "master echo");
        s.master_write_raw = -1;
        return;
    }

    apply_knob_update(NANO_KNOB_LEVEL, raw, "hw level");
}

pub(super) fn resolve_pending_level_from_dump() {
    // No-op: LEVEL notifies are master-only; capture volume uses id 0x0A.
}

pub(super) fn try_inline_amp_update(data: &[u8]) -> bool {
    let s = st();

    if let Some(raw) = try_parse_capture_volume_notify(data) {
        if s.amp.valid && s.amp.have_capture_volume && s.amp.capture_volume == raw {
            return true;
        }
        s.amp.capture_volume = raw;
        s.amp.have_capture_volume = true;
        s.amp.valid = true;
        s.cap_vol_kept = raw;
        s.cap_vol_kept_valid = true;
        if s.mode == NANO_RX_CURRENT {
            s.cap_vol_live_dirty = true;
        }
        debug!(target: TAG, "Capture volume -> raw {raw} (notify)");
        push_amp_ui();
        return true;
    }

    if let Some((knob_id, raw)) = try_parse_amp_hw_notify(data) {
        if knob_id == NANO_KNOB_LEVEL {
            on_shared_level_notify(raw);
            if !s.refresh_timer.is_null() {
                unsafe { esp_timer_stop(s.refresh_timer) };
            }
            return true;
        }
        if apply_knob_update(knob_id, raw, "hw knob") && !s.refresh_timer.is_null() {
            unsafe { esp_timer_stop(s.refresh_timer) };
        }
        return true;
    }

    if let Some((knob_id, raw)) = try_parse_amp_knob_notify(data) {
        if knob_id == NANO_KNOB_LEVEL {
            on_shared_level_notify(raw);
            return true;
        }
        return apply_knob_update(knob_id, raw, "knob notify");
    }

    false
}

pub(super) fn reset_amp_sync() {
    let s = st();
    s.mode = NANO_RX_IDLE;
    s.state_len = 0;
    s.live_poll_active = false;
    s.last_refresh_us = 0;
    s.last_sniff_us = 0;
    s.state_probe_label.clear();
    s.incomplete_probe_label.clear();
    s.incomplete_defer_count = 0;
    s.gate_retry_count = 0;
    s.tuner_watch_running = false;
    s.pending_tuner_sync = false;
    s.state_rx_since_us = 0;
    s.bpm_known = false;
    s.last_bpm = 0;
    s.tap_count = 0;
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = 0;
    s.amp = amp_zero();
    amp_publish();
    s.amp_live_dirty = 0;
    s.cap_vol_live_dirty = false;
    s.cap_vol_kept = 128;
    s.cap_vol_kept_valid = false;
    s.master_write_raw = -1;
    for timer in [
        s.parse_timer,
        s.sync_timer,
        s.refresh_timer,
        s.poll_timer,
        s.tuner_fast_sync_timer,
        s.tuner_watch_timer,
    ] {
        if !timer.is_null() {
            unsafe { esp_timer_stop(timer) };
        }
    }
    s.tuner_watch_running = false;
}
