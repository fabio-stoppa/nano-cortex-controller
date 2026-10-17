//! Public C ABI surface for the tuner engine.

use core::sync::atomic::Ordering;

use esp_idf_svc::sys::ESP_OK;
use log::{debug, info};

use nano_wire::builders;
use nano_wire::parsers;

use super::ctx::*;
use super::mode::{find_tuner_sub, log_unhandled_tuner, sniff_tuner_c305, try_parse_2d_events, try_parse_on, try_parse_route_trailer};
use super::pitch::{expire_stale_pitch, try_parse_pitch};
use super::route::{clear_gain_2d_route_state, note_c305_authority, route_name};
use super::tick::{ensure_tick_timer, push_ui};
use crate::tuner::types::*;

pub fn nano_tuner_reset() {
    ensure_tick_timer();

    let s = tn();
    s.tuner.active = false;
    s.tuner.has_pitch = false;
    s.tuner.note = -1;
    s.tuner.cents = 0;
    s.tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
    s.last_pitch_log_us = 0;
    s.last_pitch_reject_us = 0;
    s.last_raw_sniff_us = 0;
    s.last_raw_fp_len = 0;
    s.last_raw_fp = [0; TUNER_RAW_FP_MAX];
    s.last_logged_note = -1;
    s.last_logged_cents = 0;
    s.last_pitch_us = 0;
    s.last_pitch_packet_us = 0;
    s.prev_pitch_packet_us = 0;
    s.display_cents_f = 0.0;
    s.have_abs_pitch = false;
    s.last_abs_pitch = 0.0;
    s.ignore_active_until_us = 0;
    s.last_raw_tag = 0;
    s.last_pitch_kind = 0;
    s.last_raw_cents = 0.0;
    s.c305_authority_until_us = 0;
    s.c305_authority_active = false;
    clear_gain_2d_route_state();
    tuner_publish();
    push_ui();
    crate::state::nano_state_tuner_watch_update(false);
}

pub fn nano_tuner_tick() {
    expire_stale_pitch();
}

pub fn feed_c305_bytes(data: &[u8]) -> bool {
    expire_stale_pitch();

    let parsed = if try_parse_2d_events(data) {
        true
    } else if try_parse_route_trailer(data) {
        true
    } else if try_parse_pitch(data) {
        true
    } else if try_parse_on(data) {
        true
    } else {
        log_unhandled_tuner(data);
        false
    };

    sniff_tuner_c305(data, parsed);

    if parsed {
        return true;
    }

    find_tuner_sub(data).is_some()
}

/// Apply pitch immediately from the GATT notify path (no worker hop).
pub fn feed_pitch_fast_bytes(data: &[u8]) -> bool {
    if data.is_empty() || rx_active_blocked() {
        return false;
    }
    // Pitch only — no sniff, no 2D/mode, safe to call from GATT notify context.
    if try_parse_pitch(data) {
        tn().last_raw_sniff_us = now_us();
        return true;
    }
    false
}

pub fn nano_tuner_pitch_age_ms() -> i32 {
    let s = tn();
    if s.last_pitch_packet_us <= 0 {
        return -1;
    }
    let age = now_us() - s.last_pitch_packet_us;
    if age < 0 {
        return 0;
    }
    (age / 1000) as i32
}

pub fn nano_tuner_pitch_dt_ms() -> i32 {
    let s = tn();
    if s.last_pitch_packet_us <= 0 || s.prev_pitch_packet_us <= 0 {
        return -1;
    }
    let dt = s.last_pitch_packet_us - s.prev_pitch_packet_us;
    if dt < 0 {
        return -1;
    }
    (dt / 1000) as i32
}

pub fn nano_tuner_cents_f() -> f32 {
    tn().display_cents_f
}

pub fn nano_tuner_note_c305_rx() {
    tn().last_raw_sniff_us = now_us();
}

/// Consistent copy for UI thread (odd/even seqlock).
pub fn nano_tuner_snapshot(out: &mut NanoTunerC) {
    TUNER_SEQ.read(|| {
        *out = tn().tuner;
    });
}

/// Lock-free tuner-active check for BLE / worker hot paths.
pub fn nano_tuner_is_active() -> bool {
    TUNER_ACTIVE.load(Ordering::Acquire)
}

/// Human-readable mute/bypass label matching the Nano pedal (not raw wire enum).
pub fn nano_tuner_route_label(route: u32) -> &'static str {
    match route {
        NANO_TUNER_ROUTE_MUTE => "bypass",
        NANO_TUNER_ROUTE_BYPASS => "mute",
        _ => "?",
    }
}

/// True when a recent C305 tuner event should override conflicting preset state.
pub fn nano_tuner_may_apply_state(state_active: bool) -> bool {
    let s = tn();
    if now_us() >= s.c305_authority_until_us {
        return true;
    }
    state_active == s.c305_authority_active
}

/// Apply tuner mode from preset state (field 45 = on, f51 = off).
pub fn nano_tuner_sync_from_state(active: bool, route: u32) {
    ensure_tick_timer();

    let s = tn();

    // Mute/bypass can change in preset state while tuner stays on.
    if s.tuner.active && active && route != NANO_TUNER_ROUTE_UNKNOWN && s.tuner.route != route {
        s.tuner.route = route;
        info!(target: TAG, "Tuner route -> {} (preset state)", route_name(route));
        push_ui();
        crate::state::nano_state_tuner_watch_update(true);
        return;
    }

    if !nano_tuner_may_apply_state(active) {
        if s.tuner.active && route != NANO_TUNER_ROUTE_UNKNOWN && s.tuner.route != route {
            s.tuner.route = route;
            info!(
                target: TAG,
                "Tuner route from preset state -> {} (C305 held)",
                route_name(route)
            );
            push_ui();
        } else {
            debug!(
                target: TAG,
                "Preset state tuner {} ignored (C305 authority)",
                if active { "ON" } else { "OFF" }
            );
        }
        crate::state::nano_state_tuner_watch_update(s.tuner.active);
        return;
    }

    let mut ui = false;

    if s.tuner.active != active {
        s.tuner.active = active;
        ui = true;
        if active && route != NANO_TUNER_ROUTE_UNKNOWN {
            s.tuner.route = route;
        } else if !active {
            s.tuner.has_pitch = false;
            s.tuner.note = -1;
            s.tuner.cents = 0;
            s.tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
            s.last_raw_tag = 0;
            s.last_pitch_kind = 0;
            s.last_raw_cents = 0.0;
        }
    }

    if s.tuner.active && route != NANO_TUNER_ROUTE_UNKNOWN && s.tuner.route != route {
        s.tuner.route = route;
        ui = true;
    }

    if ui {
        if s.tuner.active && s.tuner.route != NANO_TUNER_ROUTE_UNKNOWN {
            info!(
                target: TAG,
                "Tuner sync from preset state -> ON ({})",
                route_name(s.tuner.route)
            );
        } else if s.tuner.active {
            info!(target: TAG, "Tuner sync from preset state -> ON (route unknown)");
        } else {
            info!(target: TAG, "Tuner sync from preset state -> OFF");
        }
        push_ui();
    } else if s.tuner.active
        && route != NANO_TUNER_ROUTE_UNKNOWN
        && s.tuner.route == NANO_TUNER_ROUTE_UNKNOWN
    {
        s.tuner.route = route;
        info!(target: TAG, "Tuner route -> {} (preset state)", route_name(route));
        push_ui();
    }

    crate::state::nano_state_tuner_watch_update(s.tuner.active);
}

/// Enable or disable tuner (confirmed C304 packets from sniff).
pub fn nano_tuner_set_active(active: bool) -> bool {
    if !crate::ble::nano_ble_connected() {
        return false;
    }

    let mut buf = [0u8; 24];

    if active {
        // BLE enable is unconfirmed; footswitch is reliable. Try 2D toggle once.
        let n2d = builders::tuner_enable_payload(&mut buf);
        if n2d == 0 {
            return false;
        }
        parsers::log_bytes("nano_tuner", "Tuner ON (tx)", &buf[..n2d]);
        if crate::ble::nano_ble_write_c304(c"Tuner ON", &buf[..n2d]) != ESP_OK {
            return false;
        }
        let s = tn();
        s.tuner.active = true;
        s.tuner.has_pitch = false;
        s.tuner.route = NANO_TUNER_ROUTE_MUTE;
        note_c305_authority(true);
        crate::ble::nano_ble_flush_c305_backlog();
        tuner_publish();
        push_ui();
        crate::state::nano_state_tuner_watch_update(true);
        return true;
    }

    let n = builders::tuner_disable_payload(&mut buf);
    if n == 0 {
        return false;
    }
    parsers::log_bytes("nano_tuner", "Tuner OFF (tx)", &buf[..n]);
    if crate::ble::nano_ble_write_c304(c"Tuner OFF", &buf[..n]) != ESP_OK {
        return false;
    }

    let s = tn();
    s.tuner.active = false;
    s.tuner.has_pitch = false;
    s.tuner.note = -1;
    s.tuner.cents = 0;
    s.tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
    s.ignore_active_until_us = now_us() + RX_IGNORE_ACTIVE_US;
    note_c305_authority(false);
    push_ui();
    crate::state::nano_state_tuner_watch_update(false);
    crate::state::nano_state_request_tuner_sync();
    true
}
