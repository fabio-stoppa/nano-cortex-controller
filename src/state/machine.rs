//! Preset / amp state machine (port of `components/nano/nano_state.c`).
//!
//! Owns the C305 reassembly buffers, the debounce/refresh/poll timers and the
//! Tempo Mode helpers and preset/amp state machine.

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void, CStr};
use core::ptr;
use core::slice;
use std::alloc::{alloc_zeroed, Layout};
use std::ffi::CString;

use esp_idf_svc::sys::*;
use log::{debug, error, info, warn};

use nano_wire::ffi::{NanoAmpC, NanoPresetCatalogC};
use nano_wire::types::{AMP_CAPTURE_NAME_MAX, BPM_MAX, BPM_MIN, PRESET_COUNT, PRESET_NAME_MAX};

const TAG: &str = "nano_state";

fn tag_ptr() -> *const c_char {
    c"nano_state".as_ptr()
}

const fn amp_zero() -> NanoAmpC {
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

fn amp_copy(a: &NanoAmpC) -> NanoAmpC {
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

const fn preset_catalog_zero() -> NanoPresetCatalogC {
    NanoPresetCatalogC {
        names: [[0; PRESET_NAME_MAX]; PRESET_COUNT],
        count: 0,
        valid: false,
    }
}

fn c_bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn is_tuner_active() -> bool {
    let tuner = crate::tuner::nano_tuner_get();
    !tuner.is_null() && unsafe { (*tuner).active }
}

fn ble_write_c304(label: &CStr, data: &[u8]) -> bool {
    crate::ble::nano_ble_write_c304(label.as_ptr(), data.as_ptr(), data.len()) == ESP_OK
}

const STATE_BUF_MAX: usize = 48 * 1024;
const PARSE_DEBOUNCE_US: u64 = 1200 * 1000;
const STATE_REQUEST_DELAY_US: u64 = 2200 * 1000;
const REFRESH_DEBOUNCE_US: u64 = 800 * 1000;
const REFRESH_MIN_INTERVAL_US: i64 = 1500 * 1000;
const LIVE_POLL_INTERVAL_US: u64 = 15000 * 1000;
const SNIFF_MIN_INTERVAL_US: i64 = 100 * 1000;
const TUNER_FAST_SYNC_US: u64 = 400 * 1000;
const TUNER_WATCH_POLL_US: u64 = 5000 * 1000;
const STATE_RX_TIMEOUT_US: i64 = 5000 * 1000;
/// Max times we defer finish while waiting for a gate tail after mid-f34 truncation.
const STATE_INCOMPLETE_DEFER_MAX: u32 = 2;
/// Re-request current state when INITIAL/ROUTINE dump still lacks gate fields.
const STATE_GATE_RETRY_MAX: u32 = 2;
const STATE_PROBE_LABEL_MAX: usize = 24;
const PARSE_TASK_STACK: u32 = 8192;
const PARSE_TASK_PRIO: UBaseType_t = 5;
const TAP_TIMEOUT_US: i64 = 2000 * 1000;
const TAP_HISTORY_MAX: usize = 4;
const TEMPO_DEFAULT_BPM: f32 = 120.0;
const TEMPO_EXIT_ECHO_IGNORE_US: i64 = 800 * 1000;

/// Receive-path mode while syncing / applying pedal traffic.
pub const NANO_RX_IDLE: u32 = 0;
pub const NANO_RX_METADATA: u32 = 1;
pub const NANO_RX_CURRENT: u32 = 2;

const NANO_KNOB_LEVEL: u8 = 1;
const NANO_KNOB_TREBLE: u8 = 4;

const KNOB_NAMES: [&str; 5] = ["gain", "level", "bass", "mid", "treble"];

const PD_TRUE: BaseType_t = 1;
const PD_PASS: BaseType_t = 1;
const NO_AFFINITY: BaseType_t = 0x7FFF_FFFF;
const NOTIFY_INDEX: UBaseType_t = 0;
const PORT_MAX_DELAY: TickType_t = TickType_t::MAX;

fn now_us() -> i64 {
    unsafe { esp_timer_get_time() }
}

fn err_name(err: esp_err_t) -> &'static str {
    unsafe { CStr::from_ptr(esp_err_to_name(err)) }
        .to_str()
        .unwrap_or("?")
}

fn check(err: esp_err_t, what: &str) {
    if err != ESP_OK {
        panic!("{what} failed: {} ({err})", err_name(err));
    }
}

fn c_label(label: &str) -> CString {
    CString::new(label).unwrap_or_default()
}

struct State {
    mode: u32,
    amp: NanoAmpC,
    cap_vol_kept: u8,
    cap_vol_kept_valid: bool,
    master_write_us: i64,
    master_write_raw: i32,
    /// Knobs / capture-vol updated live while a dump is in flight — keep over stale parse.
    amp_live_dirty: u8,
    cap_vol_live_dirty: bool,
    state_buf: *mut u8,
    fe_buf: *mut u8,
    state_len: usize,
    fe_len: usize,
    last_bpm_hunt_len: usize,
    preset_catalog: NanoPresetCatalogC,
    parse_timer: esp_timer_handle_t,
    sync_timer: esp_timer_handle_t,
    refresh_timer: esp_timer_handle_t,
    poll_timer: esp_timer_handle_t,
    tuner_fast_sync_timer: esp_timer_handle_t,
    tuner_watch_timer: esp_timer_handle_t,
    last_refresh_us: i64,
    last_sniff_us: i64,
    state_rx_since_us: i64,
    live_poll_active: bool,
    tuner_fast_sync_armed: bool,
    tuner_watch_running: bool,
    pending_tuner_sync: bool,
    bpm_known: bool,
    last_bpm: u16,
    tempo_mode_active: bool,
    tempo_ignore_exit_echo_us: i64,
    state_probe_label: String,
    incomplete_probe_label: String,
    incomplete_defer_count: u32,
    gate_retry_count: u32,
    parse_task: TaskHandle_t,
    tap_times: [i64; TAP_HISTORY_MAX],
    tap_count: usize,
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

struct Shared<T>(UnsafeCell<T>);

// Same contract as the C port: state is touched from the C305 worker, the parse
// worker, the esp_timer task and the UI task, ordered only by the BLE event flow.
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

static STATE: Shared<State> = Shared::new(State::new());

fn st() -> &'static mut State {
    STATE.get()
}

/// The reassembly buffers live for the whole session, exactly like the C `malloc`.
fn alloc_buf() -> *mut u8 {
    let layout = Layout::from_size_align(STATE_BUF_MAX, 1).expect("state buffer layout");
    unsafe { alloc_zeroed(layout) }
}

fn set_label(dst: &mut String, src: &str) {
    let n = src.len().min(STATE_PROBE_LABEL_MAX - 1);
    dst.clear();
    dst.push_str(src.get(..n).unwrap_or(src));
}

fn amp_knob_get(amp: &NanoAmpC, knob: u8) -> u8 {
    match knob {
        0 => amp.gain,
        1 => amp.level,
        2 => amp.bass,
        3 => amp.mid,
        _ => amp.treble,
    }
}

fn amp_knob_set(amp: &mut NanoAmpC, knob: u8, raw: u8) {
    match knob {
        0 => amp.gain = raw,
        1 => amp.level = raw,
        2 => amp.bass = raw,
        3 => amp.mid = raw,
        _ => amp.treble = raw,
    }
}

/* ------------------------------------------------------------ Tempo Mode */

fn tempo_bpm_f() -> f32 {
    let s = st();
    if s.bpm_known && s.last_bpm >= BPM_MIN && s.last_bpm <= BPM_MAX {
        return s.last_bpm as f32;
    }
    TEMPO_DEFAULT_BPM
}

fn write_tempo_tap_bpm(bpm_f: f32, label: &CStr) -> bool {
    let mut buf = [0u8; 24];
    let n = unsafe { nano_wire::ffi::nano_bpm_tap_payload(bpm_f, buf.as_mut_ptr(), buf.len()) };
    if n == 0 {
        return false;
    }
    ble_write_c304(label, &buf[..n])
}

fn on_tempo_notify(bpm: u16, tap_marker: bool) {
    let s = st();
    if bpm < BPM_MIN || bpm > BPM_MAX {
        return;
    }

    s.bpm_known = true;
    s.last_bpm = bpm;

    if tap_marker {
        // Tap / keep-alive while in Tempo Mode — open UI + phase-lock blinker.
        info!(target: TAG, "Tempo Mode notify TAP @ {bpm} BPM");
        s.tempo_mode_active = true;
        unsafe { crate::ui::screen::nano_ui_set_tempo_mode(true) };
        unsafe { crate::ui::screen::nano_ui_bpm_tap(bpm) };
        return;
    }

    if now_us() < s.tempo_ignore_exit_echo_us {
        info!(target: TAG, "Tempo Mode notify EXIT @ {bpm} BPM (ignored echo)");
        unsafe { crate::ui::screen::nano_ui_set_bpm(bpm, true) };
        return;
    }

    /*
     * Exit-shaped 0x91 (no 18 01):
     * - While active → leave Tempo Mode (Footswitch II / peer exit).
     * - While idle → enter announce (Footswitch I hold); Cloud app opens here too.
     */
    if s.tempo_mode_active {
        info!(target: TAG, "Tempo Mode notify EXIT @ {bpm} BPM");
        s.tempo_mode_active = false;
        s.tap_count = 0;
        unsafe { crate::ui::screen::nano_ui_set_bpm(bpm, true) };
        unsafe { crate::ui::screen::nano_ui_set_tempo_mode(false) };
    } else {
        info!(target: TAG, "Tempo Mode notify ENTER @ {bpm} BPM");
        s.tempo_mode_active = true;
        unsafe { crate::ui::screen::nano_ui_set_tempo_mode(true) };
        unsafe { crate::ui::screen::nano_ui_bpm_tap(bpm) };
    }
}

/* ------------------------------------------------------- FE / state RX */

fn append_fe_payload(data: &[u8]) {
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

fn finish_fe_capture() {
    let s = st();
    if s.fe_len > 0 {
        let fe = fe_slice();
        if unsafe {
            nano_wire::ffi::nano_parse_fe_preset_catalog(fe.as_ptr(), fe.len(), &mut s.preset_catalog)
        } {
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

        let mut fe_bpm: u16 = 0;
        if unsafe { nano_wire::ffi::nano_parse_bpm(fe.as_ptr(), fe.len(), &mut fe_bpm) } {
            info!(target: TAG, "BPM {fe_bpm} (field 56 in FE metadata)");
            nano_state_on_bpm_found(fe_bpm);
        } else {
            unsafe { nano_wire::ffi::nano_log_bpm_raw_scan(tag_ptr(), c"FE metadata".as_ptr(), fe.as_ptr(), fe.len()) };
        }
    }
    s.fe_len = 0;
}

fn update_preset_label_locked(state: &[u8]) {
    let s = st();
    let mut label = String::from("-");
    let mut direct = [0u8; PRESET_NAME_MAX];

    let have_direct = unsafe {
        nano_wire::ffi::nano_proto_first_string(
            state.as_ptr(),
            state.len(),
            2,
            direct.as_mut_ptr().cast::<c_char>(),
            direct.len(),
        )
    };
    if have_direct && direct[0] != 0 {
        label = c_bytes_to_string(&direct);
    } else {
        let mut slot: u32 = 0;
        if unsafe { nano_wire::ffi::nano_proto_field_varint(state.as_ptr(), state.len(), 1, &mut slot) } {
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
    }

    unsafe { crate::ui::screen::nano_ui_set_preset_name(c_label(&label).as_ptr()) };
}

fn append_state_payload(data: &[u8]) {
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
        let mut tag: u32 = 0;
        let mut next: usize = 0;
        if !unsafe { nano_wire::ffi::nano_read_varint(data.as_ptr(), len, i, &mut tag, &mut next) } {
            return i > 0;
        }
        i = next;
        let wire = tag & 0x07;
        if wire == 0 {
            if !unsafe { nano_wire::ffi::nano_read_varint(data.as_ptr(), len, i, ptr::null_mut(), &mut next) } {
                return true;
            }
            i = next;
        } else if wire == 1 {
            if i + 8 > len {
                return true;
            }
            i += 8;
        } else if wire == 2 {
            let mut flen: u32 = 0;
            if !unsafe { nano_wire::ffi::nano_read_varint(data.as_ptr(), len, i, &mut flen, &mut next) } {
                return true;
            }
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

fn check_state_rx_timeout() {
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

fn send_state_request(label: &str, from_initial_sync: bool) {
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
    let n = unsafe { nano_wire::ffi::nano_request_current_state(buf.as_mut_ptr(), buf.len()) };
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

fn request_current_state_now(label: &str) {
    send_state_request(label, false);
}

fn parse_state_buffer() {
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
        unsafe { nano_wire::ffi::nano_log_proto_fields(tag_ptr(), c_label(&label).as_ptr(), buf.as_ptr(), buf.len()) };
    }

    let truncated = state_dump_truncated(buf);
    let has_gate = unsafe { crate::fx::nano_fx_dump_has_gate_fields(buf.as_ptr(), buf.len()) };
    let wait_for_gate = truncated
        && !has_gate
        && s.mode == NANO_RX_CURRENT
        && s.incomplete_defer_count < STATE_INCOMPLETE_DEFER_MAX;

    let live_amp = amp_copy(&s.amp);
    let live_dirty = s.amp_live_dirty;
    let cap_dirty = s.cap_vol_live_dirty;
    if unsafe { nano_wire::ffi::nano_parse_amp(buf.as_ptr(), buf.len(), &mut s.amp) } {
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
        unsafe { nano_wire::ffi::nano_log_amp(tag_ptr(), &s.amp) };
        unsafe { crate::ui::screen::nano_ui_set_amp(&s.amp as *const _) };
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
        // Apply whatever FX/gate we already have; keep RX open for late 0x80 tails.
        unsafe { crate::fx::nano_fx_sync_from_preset(buf.as_ptr(), buf.len()) };
        schedule_parse();
        return;
    }

    finish_state_capture();

    let mut tuner_active = false;
    let mut tuner_route: u32 = crate::tuner::NANO_TUNER_ROUTE_UNKNOWN;
    if unsafe {
        nano_wire::ffi::nano_parse_tuner_state(buf.as_ptr(), buf.len(), &mut tuner_active, &mut tuner_route)
    } {
        if label == "ROUTE" || label == "TUNER" || tuner_active || is_tuner_active() {
            unsafe { nano_wire::ffi::nano_log_tuner_state_fields(tag_ptr(), buf.as_ptr(), buf.len()) };
        }
        crate::tuner::nano_tuner_sync_from_state(tuner_active, tuner_route);
    } else {
        if label == "ROUTE" || label == "TUNER" || is_tuner_active() {
            unsafe { nano_wire::ffi::nano_log_tuner_state_fields(tag_ptr(), buf.as_ptr(), buf.len()) };
        }
        nano_state_tuner_watch_update(is_tuner_active());
    }

    unsafe { crate::fx::nano_fx_sync_from_preset(buf.as_ptr(), buf.len()) };

    update_preset_label_locked(buf);

    let mut bpm: u16 = 0;
    if unsafe { nano_wire::ffi::nano_parse_bpm(buf.as_ptr(), buf.len(), &mut bpm) } {
        info!(target: TAG, "BPM {bpm} (field 56) [{label}]");
        nano_state_on_bpm_found(bpm);
    } else {
        // Short dumps omit f56 — keep last live tap-tempo BPM instead of clearing.
        info!(target: TAG, "BPM field 56 absent len={buf_len} [{label}]");
        if label == "INITIAL" || buf_len != s.last_bpm_hunt_len {
            unsafe { nano_wire::ffi::nano_log_bpm_hunt(tag_ptr(), buf.as_ptr(), buf.len()) };
            s.last_bpm_hunt_len = buf_len;
        }
        if label == "INITIAL" && !s.bpm_known {
            warn!(
                target: TAG,
                "BPM still unknown after INITIAL (FE nested f56 should supply it)"
            );
        }
    }

    if !unsafe { crate::fx::nano_fx_dump_has_gate_fields(buf.as_ptr(), buf.len()) }
        && s.gate_retry_count < STATE_GATE_RETRY_MAX
        && (label == "INITIAL" || label == "ROUTINE" || label == "PRESET")
    {
        s.gate_retry_count += 1;
        info!(
            target: TAG,
            "Gate fields absent after parse — retry {}/{}", s.gate_retry_count, STATE_GATE_RETRY_MAX
        );
        request_current_state_now("Gate sync retry");
    } else if unsafe { crate::fx::nano_fx_dump_has_gate_fields(buf.as_ptr(), buf.len()) } {
        s.gate_retry_count = 0;
    }

    maybe_retry_tuner_sync();
}

/* ------------------------------------------------------- worker / timers */

unsafe extern "C" fn parse_worker(_arg: *mut c_void) {
    loop {
        unsafe { ulTaskGenericNotifyTake(NOTIFY_INDEX, PD_TRUE, PORT_MAX_DELAY) };
        parse_state_buffer();
    }
}

fn ensure_parse_task() {
    let s = st();
    if !s.parse_task.is_null() {
        return;
    }
    let ok = unsafe {
        xTaskCreatePinnedToCore(
            Some(parse_worker),
            c"nano_parse".as_ptr(),
            PARSE_TASK_STACK,
            ptr::null_mut(),
            PARSE_TASK_PRIO,
            &mut s.parse_task,
            NO_AFFINITY,
        )
    };
    if ok != PD_PASS {
        error!(target: TAG, "Failed to create parse worker task");
        s.parse_task = ptr::null_mut();
    }
}

unsafe extern "C" fn parse_timer_cb(_arg: *mut c_void) {
    ensure_parse_task();
    let s = st();
    if !s.parse_task.is_null() {
        unsafe {
            xTaskGenericNotify(
                s.parse_task,
                NOTIFY_INDEX,
                0,
                eNotifyAction_eIncrement,
                ptr::null_mut(),
            )
        };
    } else {
        parse_state_buffer();
    }
}

unsafe extern "C" fn refresh_timer_cb(_arg: *mut c_void) {
    request_current_state_now("Debounced amp refresh");
}

unsafe extern "C" fn poll_timer_cb(_arg: *mut c_void) {
    let s = st();
    if is_tuner_active() {
        return;
    }
    if s.mode == NANO_RX_IDLE && s.amp.valid {
        request_current_state_now("Live amp poll");
    }
}

unsafe extern "C" fn tuner_fast_sync_timer_cb(_arg: *mut c_void) {
    let s = st();
    if is_tuner_active() {
        s.pending_tuner_sync = true;
        return;
    }
    if s.mode != NANO_RX_IDLE {
        s.pending_tuner_sync = true;
        return;
    }
    s.tuner_fast_sync_armed = true;
    set_label(&mut s.state_probe_label, "TUNER");
    send_state_request("Tuner fast sync", false);
}

unsafe extern "C" fn tuner_watch_poll_cb(_arg: *mut c_void) {
    let s = st();
    if is_tuner_active() {
        nano_state_tuner_watch_update(true);
        return;
    }
    if s.mode != NANO_RX_IDLE || !s.amp.valid {
        return;
    }

    request_current_state_now("Tuner watch");
}

fn schedule_parse() {
    let s = st();
    if s.parse_timer.is_null() {
        return;
    }
    unsafe {
        esp_timer_stop(s.parse_timer);
        esp_timer_start_once(s.parse_timer, PARSE_DEBOUNCE_US);
    }
}

fn schedule_refresh() {
    let s = st();
    if s.refresh_timer.is_null() || s.mode != NANO_RX_IDLE {
        return;
    }
    unsafe {
        esp_timer_stop(s.refresh_timer);
        esp_timer_start_once(s.refresh_timer, REFRESH_DEBOUNCE_US);
    }
}

unsafe extern "C" fn sync_timer_cb(_arg: *mut c_void) {
    finish_fe_capture();
    info!(target: TAG, "Initial sync timer — requesting preset state");
    send_state_request("Current preset state request", true);
}

fn create_timer(
    handle: &mut esp_timer_handle_t,
    cb: unsafe extern "C" fn(*mut c_void),
    name: &CStr,
) {
    if !handle.is_null() {
        return;
    }
    // Zeroed rest mirrors the C designated initializer (dispatch = ESP_TIMER_TASK).
    let mut args: esp_timer_create_args_t = unsafe { core::mem::zeroed() };
    args.callback = Some(cb);
    args.name = name.as_ptr();
    check(
        unsafe { esp_timer_create(&args, handle) },
        "esp_timer_create",
    );
}

fn ensure_timers() {
    let s = st();
    create_timer(&mut s.parse_timer, parse_timer_cb, c"nano_parse");
    create_timer(&mut s.sync_timer, sync_timer_cb, c"nano_sync");
    create_timer(&mut s.refresh_timer, refresh_timer_cb, c"nano_refresh");
    create_timer(&mut s.poll_timer, poll_timer_cb, c"nano_poll");
    create_timer(
        &mut s.tuner_fast_sync_timer,
        tuner_fast_sync_timer_cb,
        c"nano_tsync",
    );
    create_timer(&mut s.tuner_watch_timer, tuner_watch_poll_cb, c"nano_twatch");
}

/* ------------------------------------------------------ C305 classification */

fn is_fe_stream(data: &[u8], mode: u32) -> bool {
    if data.len() < 3 {
        return false;
    }
    if data[0] == 0xFE || data[0] == 0xFD || data[0] == 0xCE || data[0] == 0xD0 {
        return true;
    }
    /*
     * High-bit byte1 marks segmented FE/state continuations, but live notifies
     * use lead C0 (also high bit). Classifying C0 as FE stream drops gain/EQ/
     * master updates for the whole dump window — UI freezes until parse.
     */
    if (mode == NANO_RX_METADATA || mode == NANO_RX_CURRENT)
        && (data[1] & 0x80) != 0
        && data[1] != 0xC0
    {
        return true;
    }
    false
}

/// Preset state: FE 41 stream chunks, or single-packet dump with byte1 == 0xC1.
fn is_state_stream_chunk(data: &[u8]) -> bool {
    if data.len() < 3 {
        return false;
    }
    if data[0] == 0xFE && data[1] == 0x41 {
        return true;
    }
    // Desktop decoder: full current-state dump notifications use second byte 0xC1.
    if data[1] == 0xC1 {
        return true;
    }
    false
}

fn sniff_unknown_c305(data: &[u8]) {
    let s = st();
    let now = now_us();
    if now - s.last_sniff_us < SNIFF_MIN_INTERVAL_US {
        return;
    }
    s.last_sniff_us = now;
    info!(
        target: TAG,
        "BPM hunt: unknown C305 len={} lead={:02X} {:02X}",
        data.len(),
        data.first().copied().unwrap_or(0),
        data.get(1).copied().unwrap_or(0)
    );
    {
        let hunt = &data[..data.len().min(64)];
        unsafe {
            nano_wire::ffi::nano_log_bytes_full(
                tag_ptr(),
                c"BPM hunt C305".as_ptr(),
                hunt.as_ptr(),
                hunt.len(),
            )
        };
    }
}

/* --------------------------------------------------------- live amp path */

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
    unsafe { crate::ui::screen::nano_ui_set_amp(&s.amp as *const _) };
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

fn resolve_pending_level_from_dump() {
    // No-op: LEVEL notifies are master-only; capture volume uses id 0x0A.
}

fn try_inline_amp_update(data: &[u8]) -> bool {
    let s = st();
    let mut knob_id: u8 = 0;
    let mut raw: u8 = 0;

    if unsafe { nano_wire::ffi::nano_try_parse_capture_volume_notify(data.as_ptr(), data.len(), &mut raw) } {
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
        unsafe { crate::ui::screen::nano_ui_set_amp(&s.amp as *const _) };
        return true;
    }

    if unsafe {
        nano_wire::ffi::nano_try_parse_amp_hw_notify(data.as_ptr(), data.len(), &mut knob_id, &mut raw)
    } {
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

    if unsafe {
        nano_wire::ffi::nano_try_parse_amp_knob_notify(data.as_ptr(), data.len(), &mut knob_id, &mut raw)
    } {
        if knob_id == NANO_KNOB_LEVEL {
            on_shared_level_notify(raw);
            return true;
        }
        return apply_knob_update(knob_id, raw, "knob notify");
    }

    false
}

/* ---------------------------------------------------------------- reset */

fn reset_amp_sync() {
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

/* ------------------------------------------------------------------ C ABI */

#[no_mangle]
pub extern "C" fn nano_state_reset() {
    reset_amp_sync();
    crate::tuner::nano_tuner_reset();
    crate::fx::nano_fx_reset();
    let s = st();
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = 0;
    s.tap_count = 0;
    unsafe { crate::ui::screen::nano_ui_set_bpm(0, false) };
    unsafe { crate::ui::screen::nano_ui_set_tempo_mode(false) };
}

#[no_mangle]
pub extern "C" fn nano_rx_mode_set(mode: u32) {
    let s = st();
    s.mode = mode;
    if mode == NANO_RX_CURRENT {
        s.state_len = 0;
    }
}

#[no_mangle]
pub extern "C" fn nano_rx_mode_get() -> u32 {
    st().mode
}

/// Feed one C305 notification (handles FE stream reassembly + amp parse).
#[no_mangle]
pub unsafe extern "C" fn nano_state_feed_c305(data: *const u8, len: usize) {
    if data.is_null() || len == 0 {
        return;
    }
    feed_c305(unsafe { slice::from_raw_parts(data, len) });
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
        if unsafe { crate::tuner::nano_tuner_feed_c305(data.as_ptr(), data.len()) } {
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

    let mut live_bpm: u16 = 0;
    let mut tap_marker = false;
    if unsafe {
        nano_wire::ffi::nano_try_parse_bpm_notify_ex(data.as_ptr(), len, &mut live_bpm, &mut tap_marker)
    } {
        on_tempo_notify(live_bpm, tap_marker);
        return;
    }

    if try_inline_amp_update(data) {
        return;
    }

    if unsafe { crate::tuner::nano_tuner_feed_c305(data.as_ptr(), data.len()) } {
        return;
    }

    if unsafe { crate::fx::nano_fx_feed_c305(data.as_ptr(), data.len()) } {
        return;
    }

    /* Late dump tails (byte1 high-bit, often 0x80) carry gate f53/f54 after a
     * mid-f34 gap. When RX already closed, salvage gate directly from the payload. */
    if len > 2 {
        let tail = &data[2..];
        if unsafe { crate::fx::nano_fx_try_apply_gate_bytes(tail.as_ptr(), tail.len()) } {
            return;
        }
    }
    if unsafe { crate::fx::nano_fx_try_apply_gate_bytes(data.as_ptr(), data.len()) } {
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
            unsafe {
                nano_wire::ffi::nano_log_bytes_full(
                    tag_ptr(),
                    c"BPM hunt other C305".as_ptr(),
                    hunt.as_ptr(),
                    hunt.len(),
                )
            };
        }
    }
}

/// Apply live amp / capture-volume notifies from the GATT callback (no FIFO).
#[no_mangle]
pub unsafe extern "C" fn nano_state_try_amp_notify_fast(data: *const u8, len: usize) -> bool {
    if data.is_null() || len == 0 {
        return false;
    }
    try_inline_amp_update(unsafe { slice::from_raw_parts(data, len) })
}

#[no_mangle]
pub extern "C" fn nano_state_get_amp() -> *const NanoAmpC {
    let s = st();
    if s.amp.valid {
        &s.amp as *const NanoAmpC
    } else {
        ptr::null()
    }
}

/// Optimistically update amp state and send C304 knob write (touch UI).
#[no_mangle]
pub extern "C" fn nano_state_write_amp_knob(knob: u32, raw: u8) -> bool {
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

    let mut buf = [0u8; 32];
    let n = unsafe { nano_wire::ffi::nano_amp_knob_payload(knob, raw, buf.as_mut_ptr(), buf.len()) };
    if n == 0 {
        return false;
    }

    let name = KNOB_NAMES[knob_id as usize];
    debug!(target: TAG, "Amp {name} -> raw {raw} (touch)");
    ble_write_c304(&c_label(&format!("Amp {name} = {raw}")), &buf[..n])
}

/// Capture volume (field 44 / write id 0x0A), raw 0..255.
#[no_mangle]
pub extern "C" fn nano_state_write_capture_volume(raw: u8) -> bool {
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
    let n = unsafe { nano_wire::ffi::nano_capture_volume_payload(raw, buf.as_mut_ptr(), buf.len()) };
    if n == 0 {
        return false;
    }

    debug!(target: TAG, "Capture volume -> raw {raw} (touch)");
    unsafe { crate::ui::screen::nano_ui_set_amp(&s.amp as *const _) };
    ble_write_c304(c"Capture volume", &buf[..n])
}

/// Select factory/user capture slot 1..25 and refresh preset state.
#[no_mangle]
pub extern "C" fn nano_state_write_capture_slot(slot: u8) -> bool {
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
    unsafe { crate::ui::screen::nano_ui_set_amp(&s.amp as *const _) };

    let mut buf = [0u8; 16];
    let n = unsafe { nano_wire::ffi::nano_capture_select_payload(slot, buf.as_mut_ptr(), buf.len()) };
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
#[no_mangle]
pub extern "C" fn nano_state_begin_sync() {
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

    let n = unsafe { nano_wire::ffi::nano_fe_metadata_request(buf.as_mut_ptr(), buf.len()) };
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
#[no_mangle]
pub extern "C" fn nano_state_request_tuner_sync() {
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
#[no_mangle]
pub extern "C" fn nano_state_tuner_watch_update(tuner_active: bool) {
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
#[no_mangle]
pub unsafe extern "C" fn nano_state_request_preset_refresh(label: *const c_char) {
    let label = if label.is_null() {
        String::from("Preset refresh")
    } else {
        unsafe { CStr::from_ptr(label) }.to_string_lossy().into_owned()
    };
    request_preset_refresh(&label);
}

/// Apply discovered BPM and stop boot probes.
#[no_mangle]
pub extern "C" fn nano_state_on_bpm_found(bpm: u16) {
    let s = st();
    if bpm < BPM_MIN || bpm > BPM_MAX {
        return;
    }
    s.bpm_known = true;
    s.last_bpm = bpm;
    unsafe { crate::ui::screen::nano_ui_set_bpm(bpm, true) };
}

/// Last known BPM (0 if unknown).
#[no_mangle]
pub extern "C" fn nano_state_get_bpm() -> u16 {
    let s = st();
    if s.bpm_known {
        s.last_bpm
    } else {
        0
    }
}

/// True while Nano Tempo Mode is considered active (UI or pedal).
#[no_mangle]
pub extern "C" fn nano_state_tempo_active() -> bool {
    st().tempo_mode_active
}

/// Open Tempo Mode screen: enter Nano Tempo Mode via C304 tap frame.
#[no_mangle]
pub extern "C" fn nano_state_tempo_enter() -> bool {
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
        unsafe { crate::ui::screen::nano_ui_set_bpm(bpm, true) };
    }
    ok
}

/// Close Tempo Mode screen: best-effort exit-shaped C304 write.
#[no_mangle]
pub extern "C" fn nano_state_tempo_exit() -> bool {
    let s = st();
    s.tap_count = 0;
    s.tempo_mode_active = false;
    s.tempo_ignore_exit_echo_us = now_us() + TEMPO_EXIT_ECHO_IGNORE_US;
    if !crate::ble::nano_ble_connected() {
        return false;
    }
    let bpm_f = tempo_bpm_f();
    let mut buf = [0u8; 24];
    let n = unsafe { nano_wire::ffi::nano_bpm_exit_payload(bpm_f, buf.as_mut_ptr(), buf.len()) };
    if n == 0 {
        return false;
    }
    info!(target: TAG, "Tempo Mode exit @ {bpm_f:.0} BPM");
    ble_write_c304(c"Tempo Mode exit", &buf[..n])
}

/// TAP button: send C304 tap frame. From the 2nd interval onward average BPM
/// and update UI/blinker.
#[no_mangle]
pub extern "C" fn nano_state_tempo_tap() -> bool {
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
    unsafe { crate::ui::screen::nano_ui_set_bpm(bpm, true) };
    true
}
