//! Shared state, buffers and low-level helpers used by every `machine` submodule.

use core::ffi::CStr;
use core::ptr;
use std::alloc::{alloc_zeroed, Layout};
use std::ffi::CString;

use esp_idf_svc::sys::*;

use nano_wire::types::{NanoAmpC, NanoPresetCatalogC, AMP_CAPTURE_NAME_MAX, PRESET_COUNT, PRESET_NAME_MAX};

use crate::syncutil::{SeqLock, Shared};

pub(super) const TAG: &str = "nano_state";

pub(super) const fn amp_zero() -> NanoAmpC {
    NanoAmpC {
        gain: 0,
        level: 0,
        bass: 0,
        mid: 0,
        treble: 0,
        capture_volume: 0,
        capture_slot: 0,
        capture_name: [0; AMP_CAPTURE_NAME_MAX],
        have_capture_slot: false,
        have_capture_name: false,
        have_capture_volume: false,
        valid: false,
    }
}

pub(super) fn amp_copy(a: &NanoAmpC) -> NanoAmpC {
    NanoAmpC {
        gain: a.gain,
        level: a.level,
        bass: a.bass,
        mid: a.mid,
        treble: a.treble,
        capture_volume: a.capture_volume,
        capture_slot: a.capture_slot,
        capture_name: a.capture_name,
        have_capture_slot: a.have_capture_slot,
        have_capture_name: a.have_capture_name,
        have_capture_volume: a.have_capture_volume,
        valid: a.valid,
    }
}

pub(super) const fn preset_catalog_zero() -> NanoPresetCatalogC {
    NanoPresetCatalogC {
        names: [[0; PRESET_NAME_MAX]; PRESET_COUNT],
        count: 0,
        valid: false,
    }
}

pub(super) fn c_bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

pub(super) fn is_tuner_active() -> bool {
    crate::tuner::nano_tuner_is_active()
}

pub(super) fn ble_write_c304(label: &CStr, data: &[u8]) -> bool {
    crate::ble::nano_ble_write_c304(label, data) == ESP_OK
}

pub(super) const STATE_BUF_MAX: usize = 48 * 1024;
const STATE_PROBE_LABEL_MAX: usize = 24;
pub(super) const TAP_HISTORY_MAX: usize = 4;

/// Receive-path mode while syncing / applying pedal traffic.
pub(super) const NANO_RX_IDLE: u32 = 0;
pub(super) const NANO_RX_METADATA: u32 = 1;
pub(super) const NANO_RX_CURRENT: u32 = 2;

pub(super) const NANO_KNOB_LEVEL: u8 = 1;
pub(super) const NANO_KNOB_TREBLE: u8 = 4;

pub(super) const KNOB_NAMES: [&str; 5] = ["gain", "level", "bass", "mid", "treble"];

pub(super) fn now_us() -> i64 {
    unsafe { esp_timer_get_time() }
}

fn err_name(err: esp_err_t) -> &'static str {
    unsafe { CStr::from_ptr(esp_err_to_name(err)) }
        .to_str()
        .unwrap_or("?")
}

pub(super) fn check(err: esp_err_t, what: &str) {
    if err != ESP_OK {
        panic!("{what} failed: {} ({err})", err_name(err));
    }
}

pub(super) fn c_label(label: &str) -> CString {
    CString::new(label).unwrap_or_default()
}

pub(super) struct State {
    pub(super) mode: u32,
    pub(super) amp: NanoAmpC,
    pub(super) cap_vol_kept: u8,
    pub(super) cap_vol_kept_valid: bool,
    pub(super) master_write_us: i64,
    pub(super) master_write_raw: i32,
    /// Knobs / capture-vol updated live while a dump is in flight — keep over stale parse.
    pub(super) amp_live_dirty: u8,
    pub(super) cap_vol_live_dirty: bool,
    pub(super) state_buf: *mut u8,
    pub(super) fe_buf: *mut u8,
    pub(super) state_len: usize,
    pub(super) fe_len: usize,
    pub(super) last_bpm_hunt_len: usize,
    pub(super) preset_catalog: NanoPresetCatalogC,
    pub(super) parse_timer: esp_timer_handle_t,
    pub(super) sync_timer: esp_timer_handle_t,
    pub(super) refresh_timer: esp_timer_handle_t,
    pub(super) poll_timer: esp_timer_handle_t,
    pub(super) tuner_fast_sync_timer: esp_timer_handle_t,
    pub(super) tuner_watch_timer: esp_timer_handle_t,
    pub(super) last_refresh_us: i64,
    pub(super) last_sniff_us: i64,
    pub(super) state_rx_since_us: i64,
    pub(super) live_poll_active: bool,
    pub(super) tuner_fast_sync_armed: bool,
    pub(super) tuner_watch_running: bool,
    pub(super) pending_tuner_sync: bool,
    pub(super) bpm_known: bool,
    pub(super) last_bpm: u16,
    pub(super) tempo_mode_active: bool,
    pub(super) tempo_ignore_exit_echo_us: i64,
    pub(super) state_probe_label: String,
    pub(super) incomplete_probe_label: String,
    pub(super) incomplete_defer_count: u32,
    pub(super) gate_retry_count: u32,
    pub(super) parse_task: TaskHandle_t,
    pub(super) tap_times: [i64; TAP_HISTORY_MAX],
    pub(super) tap_count: usize,
}

impl State {
    const fn new() -> Self {
        Self {
            mode: NANO_RX_IDLE,
            amp: amp_zero(),
            cap_vol_kept: 128,
            cap_vol_kept_valid: false,
            master_write_us: 0,
            master_write_raw: -1,
            amp_live_dirty: 0,
            cap_vol_live_dirty: false,
            state_buf: ptr::null_mut(),
            fe_buf: ptr::null_mut(),
            state_len: 0,
            fe_len: 0,
            last_bpm_hunt_len: 0,
            preset_catalog: preset_catalog_zero(),
            parse_timer: ptr::null_mut(),
            sync_timer: ptr::null_mut(),
            refresh_timer: ptr::null_mut(),
            poll_timer: ptr::null_mut(),
            tuner_fast_sync_timer: ptr::null_mut(),
            tuner_watch_timer: ptr::null_mut(),
            last_refresh_us: 0,
            last_sniff_us: 0,
            state_rx_since_us: 0,
            live_poll_active: false,
            tuner_fast_sync_armed: false,
            tuner_watch_running: false,
            pending_tuner_sync: false,
            bpm_known: false,
            last_bpm: 0,
            tempo_mode_active: false,
            tempo_ignore_exit_echo_us: 0,
            state_probe_label: String::new(),
            incomplete_probe_label: String::new(),
            incomplete_defer_count: 0,
            gate_retry_count: 0,
            parse_task: ptr::null_mut(),
            tap_times: [0; TAP_HISTORY_MAX],
            tap_count: 0,
        }
    }
}

static STATE: Shared<State> = Shared::new(State::new());
/// Odd/even seqlock for consistent UI snapshots of `NanoAmpC`.
pub(super) static AMP_SEQ: SeqLock = SeqLock::new();

pub(super) fn st() -> &'static mut State {
    STATE.get()
}

pub(super) fn amp_publish() {
    AMP_SEQ.publish();
}

pub(super) fn push_amp_ui() {
    amp_publish();
    crate::ui::screen::nano_ui_set_amp(Some(&st().amp));
}

/// The reassembly buffers live for the whole session, exactly like the C `malloc`.
pub(super) fn alloc_buf() -> *mut u8 {
    let layout = Layout::from_size_align(STATE_BUF_MAX, 1).expect("state buffer layout");
    unsafe { alloc_zeroed(layout) }
}

pub(super) fn set_label(dst: &mut String, src: &str) {
    let n = src.len().min(STATE_PROBE_LABEL_MAX - 1);
    dst.clear();
    dst.push_str(src.get(..n).unwrap_or(src));
}

pub(super) fn amp_knob_get(amp: &NanoAmpC, knob: u8) -> u8 {
    match knob {
        0 => amp.gain,
        1 => amp.level,
        2 => amp.bass,
        3 => amp.mid,
        _ => amp.treble,
    }
}

pub(super) fn amp_knob_set(amp: &mut NanoAmpC, knob: u8, raw: u8) {
    match knob {
        0 => amp.gain = raw,
        1 => amp.level = raw,
        2 => amp.bass = raw,
        3 => amp.mid = raw,
        _ => amp.treble = raw,
    }
}
