//! Shared tuner state: the `Tuner` struct, its statics, and small byte/time helpers
//! used across the other `engine` submodules.

use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use esp_idf_svc::sys::{esp_timer_get_time, esp_timer_handle_t};

use crate::syncutil::{SeqLock, Shared};
use crate::tuner::types::*;

pub(super) const TAG: &str = "nano_tuner";
pub(super) const PITCH_MAGIC0: u8 = 0x23;
pub(super) const PITCH_MAGIC1: u8 = 0x2D;
pub(super) const RX_IGNORE_ACTIVE_US: i64 = 400 * 1000;
pub(super) const PITCH_STALE_US: i64 = 500 * 1000;
pub(super) const PITCH_CENTS_MAX: f32 = 1200.0;
pub(super) const PITCH_LOG_US: i64 = 500 * 1000;
pub(super) const PITCH_REJECT_LOG_US: i64 = 300 * 1000;
pub(super) const PITCH_GAP_LOG_US: i64 = 2000 * 1000;
pub(super) const TUNER_RAW_DEDUP_US: i64 = 40 * 1000;
pub(super) const C305_TUNER_AUTHORITY_US: i64 = 15000 * 1000;
pub(super) const TUNER_RAW_FP_MAX: usize = 64;
pub(super) const GAIN_2D_FP_MAX: usize = 24;
pub(super) const GAIN_2D_RETX_US: i64 = 400 * 1000;

pub(super) const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Diagnostic fields mirror the C statics: some are written for future hunts only.
#[allow(dead_code)]
pub(super) struct Tuner {
    pub(super) tuner: NanoTunerC,
    pub(super) last_pitch_log_us: i64,
    pub(super) last_pitch_reject_us: i64,
    pub(super) last_raw_sniff_us: i64,
    pub(super) last_gap_log_us: i64,
    pub(super) last_raw_fp: [u8; TUNER_RAW_FP_MAX],
    pub(super) last_raw_fp_len: usize,
    pub(super) last_logged_note: i8,
    pub(super) last_logged_cents: i16,
    pub(super) last_pitch_us: i64,
    pub(super) last_pitch_packet_us: i64,
    pub(super) prev_pitch_packet_us: i64,
    pub(super) last_abs_pitch: f32,
    pub(super) display_cents_f: f32,
    pub(super) have_abs_pitch: bool,
    pub(super) ignore_active_until_us: i64,
    pub(super) tick_timer: esp_timer_handle_t,
    pub(super) last_raw_tag: u8,
    pub(super) last_pitch_kind: i8,
    pub(super) last_raw_cents: f32,
    pub(super) c305_authority_until_us: i64,
    pub(super) c305_authority_active: bool,
    pub(super) last_gain_fp: [u8; GAIN_2D_FP_MAX],
    pub(super) last_gain_fp_len: usize,
    pub(super) mode_shape_valid: bool,
    pub(super) mode_has_38_01: bool,
    pub(super) allow_same_shape_mode: bool,
    pub(super) last_route_apply_us: i64,
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

pub(super) static TUNER: Shared<Tuner> = Shared::new(Tuner::new());
pub(super) static TUNER_SEQ: SeqLock = SeqLock::new();
/// Hot-path flag for BLE worker — avoid live `nano_tuner_get()` pointer races.
pub(super) static TUNER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(super) fn tn() -> &'static mut Tuner {
    TUNER.get()
}

pub(super) fn now_us() -> i64 {
    unsafe { esp_timer_get_time() }
}

pub(super) fn tuner_publish() {
    TUNER_ACTIVE.store(tn().tuner.active, Ordering::Release);
    TUNER_SEQ.publish();
}

pub(super) fn rx_active_blocked() -> bool {
    now_us() < tn().ignore_active_until_us
}

pub(super) fn read_f32_le(data: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

pub(super) fn match_header(data: &[u8], i: usize, b3: u8, b4: u8) -> bool {
    i + 4 < data.len()
        && data[i] == 0xC0
        && data[i + 1] == 0x08
        && data[i + 2] == 0x01
        && data[i + 3] == b3
        && data[i + 4] == b4
}
