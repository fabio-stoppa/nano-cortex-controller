//! Pitch parsing (22/01, 22/02 wire forms) and applying the resulting note/cents
//! reading to the shared `Tuner` state.

use log::{debug, info};

use super::ctx::*;
use super::route::mark_tuner_live_from_c305;
use super::tick::push_ui;

/// Tag byte after 22 02 / 22 01 is an ASCII note letter (A–G).
pub(super) fn note_anchor_from_tag(tag: u8) -> Option<i8> {
    let c = (tag as char).to_ascii_uppercase();
    match c {
        'A' => Some(9),
        'B' => Some(11),
        'C' => Some(0),
        'D' => Some(2),
        'E' => Some(4),
        'F' => Some(5),
        'G' => Some(7),
        _ => None,
    }
}

pub(super) fn cents_valid(cents_f: f32) -> bool {
    cents_f.is_finite() && cents_f.abs() <= PITCH_CENTS_MAX
}

pub(super) fn log_pitch_update(tag: u8, cents_f: f32, kind: &str) {
    let s = tn();
    let now = now_us();
    if now - s.last_pitch_log_us < PITCH_LOG_US {
        return;
    }
    s.last_pitch_log_us = now;
    s.last_logged_note = s.tuner.note;
    s.last_logged_cents = s.tuner.cents;

    let proto_ch = if (0x20..0x7F).contains(&tag) { tag as char } else { '?' };
    let note = if s.tuner.note >= 0 && s.tuner.note <= 11 {
        NOTE_NAMES[s.tuner.note as usize]
    } else {
        "?"
    };
    /* Never info! on the pitch hot path — USB-JTAG logging in the GATT
     * callback stalls Bluedroid and recreates ~4s of notify backlog. */
    debug!(
        target: TAG,
        "Pitch {} {} raw {:+.1} -> {} {:+}",
        kind,
        proto_ch,
        cents_f,
        note,
        s.tuner.cents
    );
}

pub(super) fn log_pitch_reject(why: &str, tag: u8, cents_f: f32, data: &[u8], i: usize) {
    let s = tn();
    let now = now_us();
    if now - s.last_pitch_reject_us < PITCH_REJECT_LOG_US {
        return;
    }
    s.last_pitch_reject_us = now;

    let len = data.len();
    if i + 12 <= len {
        debug!(
            target: TAG,
            "Pitch reject ({}) tag 0x{:02X} cents={} b5..b11: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
            why,
            tag,
            cents_f,
            data[i + 5],
            data[i + 6],
            data[i + 7],
            data[i + 8],
            data[i + 9],
            data[i + 10],
            data[i + 11]
        );
    } else {
        debug!(
            target: TAG,
            "Pitch reject ({why}) tag 0x{tag:02X} cents={cents_f} (short {len})"
        );
    }
}

/**
 * Wire tag + accidental + cents → continuous pitch in semitone units (C=0).
 * 22/02# D +40 and 22/01 E −60 both become 3.40 — same sounding pitch.
 */
pub(super) fn abs_pitch_from_wire(anchor: i8, accidental: i32, cents_f: f32) -> f32 {
    anchor as f32 + accidental as f32 + cents_f / 100.0
}

/// Nearest note + clamped residual (note, display cents, fractional cents).
pub(super) fn note_from_pitch(pitch: f32) -> (i8, i16, f32) {
    let rounded = crate::float::floor(pitch + 0.5) as i32;
    let residual = pitch - rounded as f32;
    let mut cf = residual * 100.0;
    if cf > 50.0 {
        cf = 50.0;
    } else if cf < -50.0 {
        cf = -50.0;
    }

    let note = (((rounded % 12) + 12) % 12) as i8;
    (note, crate::float::round(cf) as i16, cf)
}

pub(super) fn clear_pitch_reading() {
    let s = tn();
    s.tuner.has_pitch = false;
    s.tuner.note = -1;
    s.tuner.cents = 0;
    s.last_raw_tag = 0;
    s.last_pitch_kind = 0;
    s.last_raw_cents = 0.0;
    s.display_cents_f = 0.0;
    s.have_abs_pitch = false;
    tuner_publish();
}

pub(super) fn apply_pitch_reading(tag: u8, cents_f: f32, form_2201: bool, sharp: bool, anchor: i8) -> bool {
    let kind: i8 = if form_2201 {
        1
    } else if sharp {
        3
    } else {
        2
    };
    let now = now_us();
    let accidental = if sharp { 1 } else { 0 };
    let abs_pitch = abs_pitch_from_wire(anchor, accidental, cents_f);

    /*
     * Always nearest-note — same path as a fresh pluck after silence.
     * Continuous hysteresis made sustained tuning feel seconds behind while
     * values were "correct" once the held spelling finally flipped.
     */
    let (note, display_cents, display_cents_f) = note_from_pitch(abs_pitch);

    let s = tn();
    s.prev_pitch_packet_us = s.last_pitch_packet_us;
    s.last_pitch_packet_us = now;
    s.last_pitch_us = now;
    s.last_abs_pitch = abs_pitch;
    s.display_cents_f = display_cents_f;
    s.have_abs_pitch = true;
    s.last_raw_tag = tag;
    s.last_pitch_kind = kind;
    s.last_raw_cents = cents_f;

    // Always publish — fractional cents move even when the int label does not.
    s.tuner.has_pitch = true;
    s.tuner.note = note;
    s.tuner.cents = display_cents;
    tuner_publish();

    let kind_name = if form_2201 {
        "22/01"
    } else if sharp {
        "22/02#"
    } else {
        "22/02"
    };
    log_pitch_update(tag, cents_f, kind_name);
    true
}

/// Cents float offset after a 22/xx header, plus the sharp accidental flag.
pub(super) fn find_pitch_cents(data: &[u8], i: usize) -> Option<(usize, bool)> {
    let len = data.len();

    // 22 02 <letter> '#' 2D <cents> — 0x23 is ASCII sharp.
    if i + 6 < len && data[i + 6] == PITCH_MAGIC0 && i + 7 < len && data[i + 7] == PITCH_MAGIC1 {
        return Some((i + 8, true));
    }
    if i + 6 < len && data[i + 6] == PITCH_MAGIC1 {
        return Some((i + 7, false));
    }
    let mut j = i + 5;
    while j + 5 <= len && j <= i + 8 {
        if data[j] == PITCH_MAGIC1 {
            let sharp = j > i + 5 && data[j - 1] == PITCH_MAGIC0;
            return Some((j + 1, sharp));
        }
        j += 1;
    }
    None
}

pub(super) fn try_parse_pitch_at(data: &[u8], form: u8) -> bool {
    if rx_active_blocked() {
        return false;
    }

    let len = data.len();
    let mut i = 0usize;
    while i + 10 <= len {
        if !match_header(data, i, 0x22, form) {
            i += 1;
            continue;
        }

        let tag = data[i + 5];
        let Some((cents_off, mut sharp)) = find_pitch_cents(data, i) else {
            log_pitch_reject(
                if form == 0x01 { "22/01 no 2D" } else { "22/02 no 2D" },
                tag,
                0.0,
                data,
                i,
            );
            i += 1;
            continue;
        };

        /*
         * 0x23 ('#') is a sharp accidental only on 22/02.
         * On 22/01 it is optional framing before the cents float — never a sharp.
         */
        if form == 0x01 {
            sharp = false;
        }

        if cents_off + 4 > len {
            log_pitch_reject("truncated float", tag, 0.0, data, i);
            i += 1;
            continue;
        }

        let wire = read_f32_le(data, cents_off);
        // Both forms carry real cents; 22/01 is just a narrow fine-lock window.
        let cents_f = wire;
        if !cents_valid(cents_f) {
            log_pitch_reject("bad cents", tag, wire, data, i);
            i += 1;
            continue;
        }

        let Some(anchor) = note_anchor_from_tag(tag) else {
            log_pitch_reject("bad tag", tag, cents_f, data, i);
            i += 1;
            continue;
        };

        if !tn().tuner.active {
            info!(target: TAG, "Tuner ON (device, 22/{form:02X} pitch)");
        }
        mark_tuner_live_from_c305();
        return apply_pitch_reading(tag, cents_f, form == 0x01, sharp, anchor);
    }

    false
}

pub(super) fn try_parse_pitch(data: &[u8]) -> bool {
    if try_parse_pitch_at(data, 0x02) {
        return true;
    }
    try_parse_pitch_at(data, 0x01)
}

pub(super) fn expire_stale_pitch() {
    let s = tn();
    if !s.tuner.active {
        return;
    }

    let now = now_us();

    // Heartbeat while live with no C305 — proves silence vs dropped logs.
    if s.last_raw_sniff_us > 0
        && now - s.last_raw_sniff_us >= PITCH_GAP_LOG_US
        && now - s.last_gap_log_us >= (PITCH_GAP_LOG_US * 4)
    {
        s.last_gap_log_us = now;
        debug!(
            target: TAG,
            "C305 quiet for {} ms (tuner live{})",
            (now - s.last_raw_sniff_us) / 1000,
            if s.tuner.has_pitch { ", holding last pitch" } else { "" }
        );
    }

    if !s.tuner.has_pitch {
        return;
    }

    if now - s.last_pitch_us <= PITCH_STALE_US {
        return;
    }

    clear_pitch_reading();
    push_ui();
}
