//! Mute/bypass route tracking: C305 gain+2D framing, trailer-shape heartbeat
//! detection, and applying resolved tuner mode/route state.

use log::{debug, info};

use nano_wire::varint::read_varint;

use super::ctx::*;
use super::tick::{ensure_tick_timer, push_ui};
use crate::tuner::types::*;

pub(super) fn mark_tuner_live_from_c305() {
    note_c305_authority(true);
    let s = tn();
    if s.tuner.active {
        return;
    }

    s.tuner.active = true;
    crate::ble::nano_ble_flush_c305_backlog();
    tuner_publish();
    push_ui();
    crate::state::nano_state_tuner_watch_update(true);
    info!(target: TAG, "live-path 2026-07-26h armed (age meter, no C306, LVGL low)");
}

/// Wire enum is inverted vs pedal on this unit; labels show pedal truth.
pub(super) fn route_label(route: u32) -> &'static str {
    match route {
        NANO_TUNER_ROUTE_MUTE => "bypass",
        NANO_TUNER_ROUTE_BYPASS => "mute",
        _ => "?",
    }
}

pub(super) fn route_name(route: u32) -> &'static str {
    route_label(route)
}

/*
 * Mode button alternates trailer shape (short 7F vs 38 01 7F); same shape repeat
 * while live = heartbeat. Footswitch ON announces route via gain-prefixed 2D.
 * Shape change while live toggles mute ↔ bypass.
 */
pub(super) fn clear_gain_2d_route_state() {
    let s = tn();
    s.last_gain_fp_len = 0;
    s.mode_shape_valid = false;
    s.allow_same_shape_mode = false;
    s.last_route_apply_us = 0;
}

pub(super) fn gain_2d_packet_is_duplicate(data: &[u8]) -> bool {
    let s = tn();
    if s.last_gain_fp_len == 0 || data.is_empty() {
        return false;
    }

    let n = data.len().min(GAIN_2D_FP_MAX);
    n == s.last_gain_fp_len && data[..n] == s.last_gain_fp[..n]
}

pub(super) fn store_gain_2d_fingerprint(data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let s = tn();
    s.last_gain_fp_len = data.len().min(GAIN_2D_FP_MAX);
    s.last_gain_fp[..s.last_gain_fp_len].copy_from_slice(&data[..s.last_gain_fp_len]);
}

pub(super) fn gain_2d_is_rapid_retransmit(data: &[u8]) -> bool {
    if !gain_2d_packet_is_duplicate(data) {
        return false;
    }

    now_us() - tn().last_route_apply_us < GAIN_2D_RETX_US
}

pub(super) fn route_from_trailer(has_38_01: bool) -> u32 {
    if has_38_01 {
        NANO_TUNER_ROUTE_BYPASS
    } else {
        NANO_TUNER_ROUTE_MUTE
    }
}

/// Return true when a live gain-2D notify is a keepalive, not a mode/footswitch event.
pub(super) fn gain_2d_is_heartbeat(has_38_01: bool, was_active: bool) -> bool {
    let s = tn();
    if !was_active || !s.mode_shape_valid {
        return false;
    }

    if has_38_01 != s.mode_has_38_01 {
        return false;
    }

    if s.allow_same_shape_mode {
        return false;
    }

    route_from_trailer(has_38_01) == s.tuner.route
}

pub(super) fn route_toggle(current: u32) -> u32 {
    match current {
        NANO_TUNER_ROUTE_MUTE => NANO_TUNER_ROUTE_BYPASS,
        NANO_TUNER_ROUTE_BYPASS => NANO_TUNER_ROUTE_MUTE,
        _ => NANO_TUNER_ROUTE_UNKNOWN,
    }
}

pub(super) fn resolve_gain_2d_route(has_38_01: bool, was_active: bool, same_shape_after_on: bool) -> u32 {
    let s = tn();
    if !was_active || s.tuner.route == NANO_TUNER_ROUTE_UNKNOWN {
        return route_from_trailer(has_38_01);
    }

    if same_shape_after_on {
        return route_toggle(s.tuner.route);
    }

    route_from_trailer(has_38_01)
}

pub(super) fn trailer_shape_name(has_38_01: bool) -> &'static str {
    if has_38_01 {
        "long"
    } else {
        "short"
    }
}

pub(super) fn is_direct_2d_off(data: &[u8], i_2d: usize) -> bool {
    let len = data.len();
    if !direct_2d_header_before(data, i_2d) || gain_prefix_before_2d(data, i_2d) {
        return false;
    }

    if i_2d + 8 <= len && data[i_2d + 5] == 0x38 && data[i_2d + 6] == 0x01 && data[i_2d + 7] == 0x7F
    {
        return true;
    }

    i_2d + 6 <= len && data[i_2d + 5] == 0x7F
}

pub(super) fn is_gain_2d_route(data: &[u8], i_2d: usize) -> Option<bool> {
    let len = data.len();
    if !gain_prefix_before_2d(data, i_2d) {
        return None;
    }

    if i_2d + 8 <= len && data[i_2d + 5] == 0x38 && data[i_2d + 6] == 0x01 {
        return Some(true);
    }

    if i_2d + 6 <= len && data[i_2d + 5] == 0x7F {
        return Some(false);
    }

    None
}

pub(super) fn note_c305_authority(active: bool) {
    let s = tn();
    s.c305_authority_active = active;
    s.c305_authority_until_us = now_us() + C305_TUNER_AUTHORITY_US;
}

pub(super) fn apply_tuner_mode_c305(active: bool, route: u32) {
    ensure_tick_timer();
    note_c305_authority(active);

    let s = tn();
    let mut changed = s.tuner.active != active;
    if active && route != NANO_TUNER_ROUTE_UNKNOWN && s.tuner.route != route {
        changed = true;
    }

    if !changed {
        if active && route != NANO_TUNER_ROUTE_UNKNOWN && s.tuner.route != route {
            s.tuner.route = route;
            info!(target: TAG, "Tuner route -> {} (C305)", route_name(route));
            push_ui();
        } else if active
            && route != NANO_TUNER_ROUTE_UNKNOWN
            && s.tuner.route == NANO_TUNER_ROUTE_UNKNOWN
        {
            s.tuner.route = route;
            info!(target: TAG, "Tuner route -> {} (C305)", route_name(route));
            push_ui();
        }
        if active {
            crate::state::nano_state_tuner_watch_update(true);
        }
        return;
    }

    let prev_route = s.tuner.route;
    let prev_active = s.tuner.active;

    s.tuner.active = active;
    if !active {
        s.tuner.has_pitch = false;
        s.tuner.note = -1;
        s.tuner.cents = 0;
        s.tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
        clear_gain_2d_route_state();
        s.last_raw_tag = 0;
        s.last_pitch_kind = 0;
        s.last_raw_cents = 0.0;
    } else if route != NANO_TUNER_ROUTE_UNKNOWN {
        s.tuner.route = route;
    }

    if !active && prev_active {
        info!(target: TAG, "Tuner OFF (C305)");
    } else if active && !prev_active {
        s.ignore_active_until_us = 0;
        crate::ble::nano_ble_flush_c305_backlog();
        if s.tuner.route != NANO_TUNER_ROUTE_UNKNOWN {
            info!(target: TAG, "Tuner ON — route: {}", route_name(s.tuner.route));
        } else {
            info!(target: TAG, "Tuner ON — route: unknown (awaiting C305 / preset state)");
        }
    } else if active && route != NANO_TUNER_ROUTE_UNKNOWN && route != prev_route {
        info!(target: TAG, "Tuner route -> {} (C305)", route_name(route));
    }

    tuner_publish();
    push_ui();
    crate::state::nano_state_tuner_watch_update(s.tuner.active);

    if !active {
        crate::state::nano_state_request_tuner_sync();
    }
}

/**
 * Footswitch ON often sends gain then 2D in one notify:
 *   C0 08 01 20 01 2D <440> 7F        (short mute — no 38 01 prefix)
 * Footswitch OFF sends direct 2D:
 *   C0 08 01 2D <440> 7F
 */
pub(super) fn gain_prefix_before_2d(data: &[u8], i_2d: usize) -> bool {
    if i_2d < 6 {
        return false;
    }

    let mut h = 0usize;
    while h + 5 <= i_2d {
        if data[h] == 0xC0 && data[h + 1] == 0x08 && data[h + 2] == 0x01 && data[h + 3] == 0x20 {
            if let Some((_, next)) = read_varint(data, h + 4) {
                if next == i_2d {
                    return true;
                }
            }
        }
        h += 1;
    }

    false
}

pub(super) fn direct_2d_header_before(data: &[u8], i_2d: usize) -> bool {
    if i_2d < 3 {
        return false;
    }

    // C0 08 01 2D — 2D sits at i_2d (may follow a length byte).
    let h = i_2d - 3;
    h + 4 <= data.len()
        && data[h] == 0xC0
        && data[h + 1] == 0x08
        && data[h + 2] == 0x01
        && data[h + 3] == 0x2D
}

pub(super) fn apply_gain_2d_route(data: &[u8], has_38_01: bool, was_active: bool) {
    {
        let s = tn();
        debug!(
            target: TAG,
            "Tuner gain-2D rx len={} {} trailer={} (last={} route={})",
            data.len(),
            if was_active { "live" } else { "ON" },
            trailer_shape_name(has_38_01),
            if s.mode_shape_valid { trailer_shape_name(s.mode_has_38_01) } else { "none" },
            route_name(s.tuner.route)
        );
    }

    if was_active && gain_2d_is_rapid_retransmit(data) {
        debug!(target: TAG, "Tuner route skip: BLE retransmit");
        return;
    }

    let s = tn();
    let same_shape = s.mode_shape_valid && has_38_01 == s.mode_has_38_01;
    let same_shape_after_on = same_shape && s.allow_same_shape_mode;

    if gain_2d_is_heartbeat(has_38_01, was_active) {
        debug!(
            target: TAG,
            "Tuner route skip: heartbeat (trailer={})",
            trailer_shape_name(has_38_01)
        );
        return;
    }

    s.allow_same_shape_mode = false;

    let route = resolve_gain_2d_route(has_38_01, was_active, same_shape_after_on);
    apply_tuner_mode_c305(true, route);

    if route == NANO_TUNER_ROUTE_UNKNOWN {
        debug!(target: TAG, "Tuner route skip: unknown");
        return;
    }

    let s = tn();
    s.mode_has_38_01 = has_38_01;
    s.mode_shape_valid = true;
    s.last_route_apply_us = now_us();
    store_gain_2d_fingerprint(data);
    if !was_active {
        tn().allow_same_shape_mode = true;
    }
}
