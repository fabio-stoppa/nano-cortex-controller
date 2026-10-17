//! C305 sniffing/logging helpers plus the 2D-event, route-trailer and 22/01
//! "on" wire parsers that drive tuner mode transitions.

use log::{debug, info};

use nano_wire::varint::read_varint;

use super::ctx::*;
use super::route::{apply_gain_2d_route, apply_tuner_mode_c305, gain_prefix_before_2d, is_direct_2d_off, is_gain_2d_route};
use crate::tuner::types::*;

pub(super) fn find_tuner_sub(data: &[u8]) -> Option<u8> {
    let len = data.len();

    let mut i = 0usize;
    while i + 4 <= len {
        if data[i] == 0xC0 && data[i + 1] == 0x08 && data[i + 2] == 0x01 {
            let sub = data[i + 3];
            if sub == 0x2D || sub == 0x22 {
                return Some(sub);
            }

            // Combined notify: C0 08 01 20 <varint> 2D <440 Hz> ...
            if sub == 0x20 {
                if let Some((_, next)) = read_varint(data, i + 4) {
                    if next + 5 <= len
                        && data[next] == 0x2D
                        && (read_f32_le(data, next + 1) - 440.0).abs() <= 1.0
                    {
                        return Some(0x2D);
                    }
                }
            }
        }
        i += 1;
    }

    // Bare 2D + A440 anywhere in the notify.
    let mut i = 0usize;
    while i + 6 <= len {
        if data[i] == 0x2D && (read_f32_le(data, i + 1) - 440.0).abs() <= 1.0 {
            return Some(0x2D);
        }
        i += 1;
    }

    None
}

/**
 * Track C305 activity for quiet detection. Full hex dumps were hunt-only and
 * stalled the RX worker on UART (~4s backlog through a 48-deep queue).
 */
pub(super) fn sniff_tuner_c305(data: &[u8], parsed: bool) {
    if data.is_empty() {
        return;
    }

    let s = tn();
    let sub = find_tuner_sub(data);
    let tuner_shaped = sub.is_some();
    if !s.tuner.active && !tuner_shaped {
        return;
    }

    let now = now_us();
    let fp_len = data.len().min(TUNER_RAW_FP_MAX);
    if fp_len == s.last_raw_fp_len
        && data[..fp_len] == s.last_raw_fp[..fp_len]
        && now - s.last_raw_sniff_us < TUNER_RAW_DEDUP_US
    {
        return;
    }

    s.last_raw_sniff_us = now;
    s.last_raw_fp_len = fp_len;
    s.last_raw_fp[..fp_len].copy_from_slice(&data[..fp_len]);

    // Parsed pitch is covered by Pitch … logs; skip wire spam.
    if parsed && sub == Some(0x22) {
        return;
    }

    // Mode / odd frames only — debug level so UART stays off the hot path.
    if tuner_shaped {
        debug!(
            target: TAG,
            "C305 RX [{}] sub=0x{:02X} len={} {}",
            if s.tuner.active { "live" } else { "idle" },
            sub.unwrap_or(0),
            data.len(),
            if parsed { "parsed" } else { "raw" }
        );
    } else if s.tuner.active {
        debug!(target: TAG, "C305 RX [live] len={} other", data.len());
    }
}

pub(super) fn log_unhandled_tuner(data: &[u8]) {
    let len = data.len();
    if len == 0 {
        return;
    }

    let mut i = 0usize;
    while i + 6 <= len {
        if data[i] == 0x2D && (read_f32_le(data, i + 1) - 440.0).abs() <= 1.0 {
            debug!(target: TAG, "Tuner C305 2D unparsed (len={len})");
            return;
        }
        i += 1;
    }
}

/**
 * Scan for 2D + 440 Hz anywhere in the notify.
 * Handles combined packets: C0 08 01 20 01 2D 00 00 DC 43 38 01 7F
 */
pub(super) fn try_parse_2d_events(data: &[u8]) -> bool {
    let len = data.len();
    let mut i = 0usize;
    while i + 6 <= len {
        if data[i] != 0x2D || (read_f32_le(data, i + 1) - 440.0).abs() > 1.0 {
            i += 1;
            continue;
        }

        if is_direct_2d_off(data, i) {
            tn().ignore_active_until_us = now_us() + RX_IGNORE_ACTIVE_US;
            apply_tuner_mode_c305(false, NANO_TUNER_ROUTE_UNKNOWN);
            return true;
        }

        let Some(has_38_01) = is_gain_2d_route(data, i) else {
            if gain_prefix_before_2d(data, i) {
                debug!(target: TAG, "Tuner 2D ignored: gain prefix but unknown trailer");
            }
            i += 1;
            continue;
        };

        apply_gain_2d_route(data, has_38_01, tn().tuner.active);
        return true;
    }

    false
}

/// Mute/bypass toggle may only change the 38 01 XX trailer on an existing 2D packet.
pub(super) fn try_parse_route_trailer(data: &[u8]) -> bool {
    let len = data.len();
    if len < 3 {
        return false;
    }

    let mut i = 0usize;
    while i + 3 <= len {
        if data[i] != 0x38 || data[i + 1] != 0x01 {
            i += 1;
            continue;
        }

        // Pitch keepalives also contain 38 01 80 — only treat as route in 2D context.
        let mut anchor_2d = 0usize;
        let mut in_2d = false;
        let mut j = if i >= 8 { i - 8 } else { 0 };
        while j + 5 <= i && j + 5 <= len {
            if data[j] == 0x2D && (read_f32_le(data, j + 1) - 440.0).abs() <= 1.0 {
                anchor_2d = j;
                in_2d = true;
                break;
            }
            j += 1;
        }
        if !in_2d || !gain_prefix_before_2d(data, anchor_2d) {
            i += 1;
            continue;
        }

        apply_gain_2d_route(data, true, tn().tuner.active);
        return true;
    }

    false
}

pub(super) fn try_parse_on(data: &[u8]) -> bool {
    if rx_active_blocked() {
        return false;
    }

    let len = data.len();
    let mut i = 0usize;
    while i + 5 <= len {
        if !match_header(data, i, 0x22, 0x01) {
            i += 1;
            continue;
        }

        if tn().tuner.active {
            return false;
        }

        info!(target: TAG, "Tuner ON (device, 22/01 header)");
        apply_tuner_mode_c305(true, NANO_TUNER_ROUTE_UNKNOWN);
        return true;
    }

    false
}
