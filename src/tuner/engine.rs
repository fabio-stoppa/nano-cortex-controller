//! Tuner: C305 pitch / mode parsing, route tracking and the C304 on/off writes
//! (port of `components/nano/nano_tuner.c`).

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use esp_idf_svc::sys::{
    esp_timer_create, esp_timer_create_args_t, esp_timer_get_time, esp_timer_handle_t,
    esp_timer_start_periodic, ESP_OK,
};
use log::{debug, info};

use nano_wire::builders;
use nano_wire::varint::read_varint;

use super::types::*;

const TAG: &str = "nano_tuner";
const PITCH_MAGIC0: u8 = 0x23;
const PITCH_MAGIC1: u8 = 0x2D;
const RX_IGNORE_ACTIVE_US: i64 = 400 * 1000;
const PITCH_STALE_US: i64 = 500 * 1000;
const PITCH_CENTS_MAX: f32 = 1200.0;
const PITCH_LOG_US: i64 = 500 * 1000;
const PITCH_REJECT_LOG_US: i64 = 300 * 1000;
const PITCH_GAP_LOG_US: i64 = 2000 * 1000;
const TUNER_RAW_DEDUP_US: i64 = 40 * 1000;
const C305_TUNER_AUTHORITY_US: i64 = 15000 * 1000;
const TUNER_RAW_FP_MAX: usize = 64;
const GAIN_2D_FP_MAX: usize = 24;
const GAIN_2D_RETX_US: i64 = 400 * 1000;

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

struct Shared<T>(UnsafeCell<T>);

// Same contract as the C port: tuner state is touched from the GATT callback,
// the C305 worker, the esp_timer task and the UI task, ordered only by the
// BLE event flow (readers use the seqlock in `nano_tuner_snapshot`).
unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// Diagnostic fields mirror the C statics: some are written for future hunts only.
#[allow(dead_code)]
struct Tuner {
    tuner: NanoTunerC,
    last_pitch_log_us: i64,
    last_pitch_reject_us: i64,
    last_raw_sniff_us: i64,
    last_gap_log_us: i64,
    last_raw_fp: [u8; TUNER_RAW_FP_MAX],
    last_raw_fp_len: usize,
    last_logged_note: i8,
    last_logged_cents: i16,
    last_pitch_us: i64,
    last_pitch_packet_us: i64,
    prev_pitch_packet_us: i64,
    last_abs_pitch: f32,
    display_cents_f: f32,
    have_abs_pitch: bool,
    ignore_active_until_us: i64,
    tick_timer: esp_timer_handle_t,
    last_raw_tag: u8,
    last_pitch_kind: i8,
    last_raw_cents: f32,
    c305_authority_until_us: i64,
    c305_authority_active: bool,
    last_gain_fp: [u8; GAIN_2D_FP_MAX],
    last_gain_fp_len: usize,
    mode_shape_valid: bool,
    mode_has_38_01: bool,
    allow_same_shape_mode: bool,
    last_route_apply_us: i64,
}

impl Tuner {
    const fn new() -> Self {
        Self {
            tuner: NanoTunerC::new(),
            last_pitch_log_us: 0,
            last_pitch_reject_us: 0,
            last_raw_sniff_us: 0,
            last_gap_log_us: 0,
            last_raw_fp: [0; TUNER_RAW_FP_MAX],
            last_raw_fp_len: 0,
            last_logged_note: -1,
            last_logged_cents: 0,
            last_pitch_us: 0,
            last_pitch_packet_us: 0,
            prev_pitch_packet_us: 0,
            last_abs_pitch: 0.0,
            display_cents_f: 0.0,
            have_abs_pitch: false,
            ignore_active_until_us: 0,
            tick_timer: ptr::null_mut(),
            last_raw_tag: 0,
            last_pitch_kind: 0,
            last_raw_cents: 0.0,
            c305_authority_until_us: 0,
            c305_authority_active: false,
            last_gain_fp: [0; GAIN_2D_FP_MAX],
            last_gain_fp_len: 0,
            mode_shape_valid: false,
            mode_has_38_01: false,
            allow_same_shape_mode: false,
            last_route_apply_us: 0,
        }
    }
}

static TUNER: Shared<Tuner> = Shared::new(Tuner::new());
static TUNER_SEQ: AtomicU32 = AtomicU32::new(0);

fn tn() -> &'static mut Tuner {
    TUNER.get()
}

fn now_us() -> i64 {
    unsafe { esp_timer_get_time() }
}

fn tuner_publish() {
    TUNER_SEQ.fetch_add(1, Ordering::Release);
}

fn mark_tuner_live_from_c305() {
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
fn route_label(route: u32) -> &'static str {
    match route {
        NANO_TUNER_ROUTE_MUTE => "bypass",
        NANO_TUNER_ROUTE_BYPASS => "mute",
        _ => "?",
    }
}

fn route_name(route: u32) -> &'static str {
    route_label(route)
}

/*
 * Mode button alternates trailer shape (short 7F vs 38 01 7F); same shape repeat
 * while live = heartbeat. Footswitch ON announces route via gain-prefixed 2D.
 * Shape change while live toggles mute ↔ bypass.
 */
fn clear_gain_2d_route_state() {
    let s = tn();
    s.last_gain_fp_len = 0;
    s.mode_shape_valid = false;
    s.allow_same_shape_mode = false;
    s.last_route_apply_us = 0;
}

fn gain_2d_packet_is_duplicate(data: &[u8]) -> bool {
    let s = tn();
    if s.last_gain_fp_len == 0 || data.is_empty() {
        return false;
    }

    let n = data.len().min(GAIN_2D_FP_MAX);
    n == s.last_gain_fp_len && data[..n] == s.last_gain_fp[..n]
}

fn store_gain_2d_fingerprint(data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let s = tn();
    s.last_gain_fp_len = data.len().min(GAIN_2D_FP_MAX);
    s.last_gain_fp[..s.last_gain_fp_len].copy_from_slice(&data[..s.last_gain_fp_len]);
}

fn gain_2d_is_rapid_retransmit(data: &[u8]) -> bool {
    if !gain_2d_packet_is_duplicate(data) {
        return false;
    }

    now_us() - tn().last_route_apply_us < GAIN_2D_RETX_US
}

fn route_from_trailer(has_38_01: bool) -> u32 {
    if has_38_01 {
        NANO_TUNER_ROUTE_BYPASS
    } else {
        NANO_TUNER_ROUTE_MUTE
    }
}

/// Return true when a live gain-2D notify is a keepalive, not a mode/footswitch event.
fn gain_2d_is_heartbeat(has_38_01: bool, was_active: bool) -> bool {
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

fn route_toggle(current: u32) -> u32 {
    match current {
        NANO_TUNER_ROUTE_MUTE => NANO_TUNER_ROUTE_BYPASS,
        NANO_TUNER_ROUTE_BYPASS => NANO_TUNER_ROUTE_MUTE,
        _ => NANO_TUNER_ROUTE_UNKNOWN,
    }
}

fn resolve_gain_2d_route(has_38_01: bool, was_active: bool, same_shape_after_on: bool) -> u32 {
    let s = tn();
    if !was_active || s.tuner.route == NANO_TUNER_ROUTE_UNKNOWN {
        return route_from_trailer(has_38_01);
    }

    if same_shape_after_on {
        return route_toggle(s.tuner.route);
    }

    route_from_trailer(has_38_01)
}

fn trailer_shape_name(has_38_01: bool) -> &'static str {
    if has_38_01 {
        "long"
    } else {
        "short"
    }
}

fn is_direct_2d_off(data: &[u8], i_2d: usize) -> bool {
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

fn is_gain_2d_route(data: &[u8], i_2d: usize) -> Option<bool> {
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

fn note_c305_authority(active: bool) {
    let s = tn();
    s.c305_authority_active = active;
    s.c305_authority_until_us = now_us() + C305_TUNER_AUTHORITY_US;
}

fn apply_tuner_mode_c305(active: bool, route: u32) {
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

unsafe extern "C" fn tick_timer_cb(_arg: *mut c_void) {
    expire_stale_pitch();
}

fn ensure_tick_timer() {
    let s = tn();
    if !s.tick_timer.is_null() {
        return;
    }

    let mut args: esp_timer_create_args_t = unsafe { core::mem::zeroed() };
    args.callback = Some(tick_timer_cb);
    args.name = c"tuner_tick".as_ptr();
    if unsafe { esp_timer_create(&args, &mut s.tick_timer) } == ESP_OK {
        unsafe { esp_timer_start_periodic(s.tick_timer, 250 * 1000) };
    }
}

fn expire_stale_pitch() {
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

fn push_ui() {
    tuner_publish();
    unsafe { crate::ui::screen::nano_ui_set_tuner(&tn().tuner) };
}

fn rx_active_blocked() -> bool {
    now_us() < tn().ignore_active_until_us
}

fn read_f32_le(data: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn match_header(data: &[u8], i: usize, b3: u8, b4: u8) -> bool {
    i + 4 < data.len()
        && data[i] == 0xC0
        && data[i + 1] == 0x08
        && data[i + 2] == 0x01
        && data[i + 3] == b3
        && data[i + 4] == b4
}

/// Tag byte after 22 02 / 22 01 is an ASCII note letter (A–G).
fn note_anchor_from_tag(tag: u8) -> Option<i8> {
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

fn cents_valid(cents_f: f32) -> bool {
    cents_f.is_finite() && cents_f.abs() <= PITCH_CENTS_MAX
}

fn log_pitch_update(tag: u8, cents_f: f32, kind: &str) {
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

fn find_tuner_sub(data: &[u8]) -> Option<u8> {
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
fn sniff_tuner_c305(data: &[u8], parsed: bool) {
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

fn log_unhandled_tuner(data: &[u8]) {
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

fn log_pitch_reject(why: &str, tag: u8, cents_f: f32, data: &[u8], i: usize) {
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
fn abs_pitch_from_wire(anchor: i8, accidental: i32, cents_f: f32) -> f32 {
    anchor as f32 + accidental as f32 + cents_f / 100.0
}

/// Nearest note + clamped residual (note, display cents, fractional cents).
fn note_from_pitch(pitch: f32) -> (i8, i16, f32) {
    let rounded = (pitch + 0.5).floor() as i32;
    let residual = pitch - rounded as f32;
    let mut cf = residual * 100.0;
    if cf > 50.0 {
        cf = 50.0;
    } else if cf < -50.0 {
        cf = -50.0;
    }

    let note = (((rounded % 12) + 12) % 12) as i8;
    (note, cf.round() as i16, cf)
}

fn clear_pitch_reading() {
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

fn apply_pitch_reading(tag: u8, cents_f: f32, form_2201: bool, sharp: bool, anchor: i8) -> bool {
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
fn find_pitch_cents(data: &[u8], i: usize) -> Option<(usize, bool)> {
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

fn try_parse_pitch_at(data: &[u8], form: u8) -> bool {
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

fn try_parse_pitch(data: &[u8]) -> bool {
    if try_parse_pitch_at(data, 0x02) {
        return true;
    }
    try_parse_pitch_at(data, 0x01)
}

/**
 * Footswitch ON often sends gain then 2D in one notify:
 *   C0 08 01 20 01 2D <440> 7F        (short mute — no 38 01 prefix)
 * Footswitch OFF sends direct 2D:
 *   C0 08 01 2D <440> 7F
 */
fn gain_prefix_before_2d(data: &[u8], i_2d: usize) -> bool {
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

fn direct_2d_header_before(data: &[u8], i_2d: usize) -> bool {
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

fn apply_gain_2d_route(data: &[u8], has_38_01: bool, was_active: bool) {
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

/**
 * Scan for 2D + 440 Hz anywhere in the notify.
 * Handles combined packets: C0 08 01 20 01 2D 00 00 DC 43 38 01 7F
 */
fn try_parse_2d_events(data: &[u8]) -> bool {
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
fn try_parse_route_trailer(data: &[u8]) -> bool {
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

fn try_parse_on(data: &[u8]) -> bool {
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

/* ------------------------------------------------------------- public C ABI */

#[no_mangle]
pub extern "C" fn nano_tuner_reset() {
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

#[no_mangle]
pub extern "C" fn nano_tuner_tick() {
    expire_stale_pitch();
}

#[no_mangle]
pub unsafe extern "C" fn nano_tuner_feed_c305(data: *const u8, len: usize) -> bool {
    if data.is_null() || len == 0 {
        expire_stale_pitch();
        return false;
    }
    let data = unsafe { slice::from_raw_parts(data, len) };

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
#[no_mangle]
pub unsafe extern "C" fn nano_tuner_feed_pitch_fast(data: *const u8, len: usize) -> bool {
    if data.is_null() || len == 0 || rx_active_blocked() {
        return false;
    }
    let data = unsafe { slice::from_raw_parts(data, len) };

    // Pitch only — no sniff, no 2D/mode, safe to call from GATT notify context.
    if try_parse_pitch(data) {
        tn().last_raw_sniff_us = now_us();
        return true;
    }
    false
}

#[no_mangle]
pub extern "C" fn nano_tuner_pitch_age_ms() -> i32 {
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

#[no_mangle]
pub extern "C" fn nano_tuner_pitch_dt_ms() -> i32 {
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

#[no_mangle]
pub extern "C" fn nano_tuner_cents_f() -> f32 {
    tn().display_cents_f
}

#[no_mangle]
pub extern "C" fn nano_tuner_note_c305_rx() {
    tn().last_raw_sniff_us = now_us();
}

#[no_mangle]
pub extern "C" fn nano_tuner_get() -> *const NanoTunerC {
    &tn().tuner as *const NanoTunerC
}

/// Consistent copy for UI thread (seqlock).
#[no_mangle]
pub unsafe extern "C" fn nano_tuner_snapshot(out: *mut NanoTunerC) {
    if out.is_null() {
        return;
    }

    loop {
        let s1 = TUNER_SEQ.load(Ordering::Acquire);
        unsafe { *out = tn().tuner };
        let s2 = TUNER_SEQ.load(Ordering::Acquire);
        if s1 == s2 {
            return;
        }
    }
}

/// Human-readable mute/bypass label matching the Nano pedal (not raw wire enum).
#[no_mangle]
pub extern "C" fn nano_tuner_route_label(route: u32) -> *const c_char {
    match route {
        NANO_TUNER_ROUTE_MUTE => c"bypass".as_ptr(),
        NANO_TUNER_ROUTE_BYPASS => c"mute".as_ptr(),
        _ => c"?".as_ptr(),
    }
}

/// True when a recent C305 tuner event should override conflicting preset state.
#[no_mangle]
pub extern "C" fn nano_tuner_may_apply_state(state_active: bool) -> bool {
    let s = tn();
    if now_us() >= s.c305_authority_until_us {
        return true;
    }
    state_active == s.c305_authority_active
}

/// Apply tuner mode from preset state (field 45 = on, f51 = off).
#[no_mangle]
pub extern "C" fn nano_tuner_sync_from_state(active: bool, route: u32) {
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
#[no_mangle]
pub extern "C" fn nano_tuner_set_active(active: bool) -> bool {
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
        unsafe {
            nano_wire::ffi::nano_log_bytes(
                c"nano_tuner".as_ptr(),
                c"Tuner ON (tx)".as_ptr(),
                buf[..n2d].as_ptr(),
                n2d,
            )
        };
        if crate::ble::nano_ble_write_c304(
            c"Tuner ON".as_ptr(),
            buf[..n2d].as_ptr(),
            buf[..n2d].len(),
        ) != ESP_OK {
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
    unsafe {
        nano_wire::ffi::nano_log_bytes(
            c"nano_tuner".as_ptr(),
            c"Tuner OFF (tx)".as_ptr(),
            buf[..n].as_ptr(),
            n,
        )
    };
    if crate::ble::nano_ble_write_c304(
        c"Tuner OFF".as_ptr(),
        buf[..n].as_ptr(),
        buf[..n].len(),
    ) != ESP_OK {
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
