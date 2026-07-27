//! FFI surface the state machine depends on: `nano_ui` (still C), the Rust
//! BLE/FX/tuner/amp ports, and the protocol helpers exported by the
//! `nano-wire` crate.

use core::ffi::{c_char, c_void, CStr};

use esp_idf_svc::sys::{esp_err_t, ESP_OK};

pub use nano_wire::ffi::{NanoAmpC, NanoPresetCatalogC};
pub use nano_wire::types::{AMP_CAPTURE_NAME_MAX, BPM_MAX, BPM_MIN, PRESET_COUNT, PRESET_NAME_MAX};

/// Log tag shared with the C helpers, which take a `const char *`.
pub const TAG: &str = "nano_state";

pub fn tag_ptr() -> *const c_char {
    c"nano_state".as_ptr()
}

pub use crate::tuner::NANO_TUNER_ROUTE_UNKNOWN;

pub const fn amp_zero() -> NanoAmpC {
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

pub fn amp_copy(a: &NanoAmpC) -> NanoAmpC {
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

pub const fn preset_catalog_zero() -> NanoPresetCatalogC {
    NanoPresetCatalogC {
        names: [[0; PRESET_NAME_MAX]; PRESET_COUNT],
        count: 0,
        valid: false,
    }
}

/// Read a NUL-terminated fixed buffer coming from a C struct.
pub fn c_bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

unsafe extern "C" {
    /* ---------------------------------------------------- components/nano */

    pub fn nano_ui_set_amp(amp: *const c_void);
    pub fn nano_ui_set_bpm(bpm: u16, valid: bool);
    pub fn nano_ui_set_tempo_mode(active: bool);
    pub fn nano_ui_bpm_tap(bpm: u16);
    pub fn nano_ui_set_preset_name(name: *const c_char);

    /* --------------------------------------------------------- nano-wire */

    pub fn nano_fe_metadata_request(buf: *mut u8, cap: usize) -> usize;
    pub fn nano_request_current_state(buf: *mut u8, cap: usize) -> usize;
    pub fn nano_amp_knob_payload(knob: u32, raw_value: u8, buf: *mut u8, cap: usize) -> usize;
    pub fn nano_capture_volume_payload(raw_value: u8, buf: *mut u8, cap: usize) -> usize;
    pub fn nano_capture_select_payload(slot: u8, buf: *mut u8, cap: usize) -> usize;
    pub fn nano_bpm_tap_payload(bpm: f32, buf: *mut u8, cap: usize) -> usize;
    pub fn nano_bpm_exit_payload(bpm: f32, buf: *mut u8, cap: usize) -> usize;

    pub fn nano_read_varint(
        data: *const u8,
        len: usize,
        offset: usize,
        value: *mut u32,
        next: *mut usize,
    ) -> bool;
    pub fn nano_proto_first_string(
        data: *const u8,
        len: usize,
        field_num: u32,
        out: *mut c_char,
        cap: usize,
    ) -> bool;
    pub fn nano_proto_field_varint(
        data: *const u8,
        len: usize,
        field_num: u32,
        out_val: *mut u32,
    ) -> bool;

    pub fn nano_parse_amp(data: *const u8, len: usize, out: *mut NanoAmpC) -> bool;
    pub fn nano_parse_bpm(data: *const u8, len: usize, out_bpm: *mut u16) -> bool;
    pub fn nano_parse_fe_preset_catalog(
        data: *const u8,
        len: usize,
        out: *mut NanoPresetCatalogC,
    ) -> bool;
    pub fn nano_parse_tuner_state(
        data: *const u8,
        len: usize,
        out_active: *mut bool,
        out_route: *mut u32,
    ) -> bool;

    pub fn nano_try_parse_capture_volume_notify(data: *const u8, len: usize, raw: *mut u8) -> bool;
    pub fn nano_try_parse_amp_hw_notify(
        data: *const u8,
        len: usize,
        knob_id: *mut u8,
        raw: *mut u8,
    ) -> bool;
    pub fn nano_try_parse_amp_knob_notify(
        data: *const u8,
        len: usize,
        knob_id: *mut u8,
        raw: *mut u8,
    ) -> bool;
    pub fn nano_try_parse_bpm_notify_ex(
        data: *const u8,
        len: usize,
        out_bpm: *mut u16,
        out_tap_marker: *mut bool,
    ) -> bool;

    pub fn nano_log_amp(tag: *const c_char, amp: *const NanoAmpC);
    pub fn nano_log_proto_fields(
        tag: *const c_char,
        label: *const c_char,
        data: *const u8,
        len: usize,
    );
    pub fn nano_log_bpm_hunt(tag: *const c_char, data: *const u8, len: usize);
    pub fn nano_log_bpm_raw_scan(
        tag: *const c_char,
        label: *const c_char,
        data: *const u8,
        len: usize,
    );
    pub fn nano_log_bytes_full(
        tag: *const c_char,
        label: *const c_char,
        data: *const u8,
        len: usize,
    );
    pub fn nano_log_tuner_state_fields(tag: *const c_char, data: *const u8, len: usize);
}

/* ------------------------------------------- nano_ble / fx / tuner / amp */
// `nano_ble_*`, `nano_fx_*`, `nano_tuner_*` and `nano_amp_*` are the C-ABI
// exports of the Rust ports, so they are called directly instead of through an
// extern block.

pub fn ble_connected() -> bool {
    crate::ble::nano_ble_connected()
}

pub fn ble_write_c304(label: &CStr, data: &[u8]) -> bool {
    let err: esp_err_t = crate::ble::nano_ble_write_c304(label.as_ptr(), data.as_ptr(), data.len());
    err == ESP_OK
}

/* ------------------------------------------------------- safe wrappers */

pub fn tuner_active() -> bool {
    let tuner = crate::tuner::nano_tuner_get();
    !tuner.is_null() && unsafe { (*tuner).active }
}

pub fn ui_set_amp(amp: &NanoAmpC) {
    unsafe { nano_ui_set_amp(amp as *const NanoAmpC as *const c_void) }
}

pub fn ui_set_bpm(bpm: u16, valid: bool) {
    unsafe { nano_ui_set_bpm(bpm, valid) }
}

pub fn ui_set_tempo_mode(active: bool) {
    unsafe { nano_ui_set_tempo_mode(active) }
}

pub fn ui_bpm_tap(bpm: u16) {
    unsafe { nano_ui_bpm_tap(bpm) }
}

pub fn ui_set_preset_name(name: &CStr) {
    unsafe { nano_ui_set_preset_name(name.as_ptr()) }
}

pub fn fx_sync_from_preset(data: &[u8]) {
    unsafe { crate::fx::nano_fx_sync_from_preset(data.as_ptr(), data.len()) }
}

pub fn fx_dump_has_gate_fields(data: &[u8]) -> bool {
    unsafe { crate::fx::nano_fx_dump_has_gate_fields(data.as_ptr(), data.len()) }
}

pub fn fx_feed_c305(data: &[u8]) -> bool {
    unsafe { crate::fx::nano_fx_feed_c305(data.as_ptr(), data.len()) }
}

pub fn fx_try_apply_gate_bytes(data: &[u8]) -> bool {
    unsafe { crate::fx::nano_fx_try_apply_gate_bytes(data.as_ptr(), data.len()) }
}

pub fn fx_reset() {
    crate::fx::nano_fx_reset()
}

pub fn tuner_feed_c305(data: &[u8]) -> bool {
    unsafe { crate::tuner::nano_tuner_feed_c305(data.as_ptr(), data.len()) }
}

pub fn tuner_note_c305_rx() {
    crate::tuner::nano_tuner_note_c305_rx()
}

pub fn tuner_sync_from_state(active: bool, route: u32) {
    crate::tuner::nano_tuner_sync_from_state(active, route)
}

pub fn tuner_reset() {
    crate::tuner::nano_tuner_reset()
}

/// Factory capture name for slot 1..25 (`None` when the slot is unknown).
pub fn amp_capture_name(slot: u8) -> Option<String> {
    crate::amp::capture_name(slot).map(|name| name.to_owned())
}

pub fn log_amp(amp: &NanoAmpC) {
    unsafe { nano_log_amp(tag_ptr(), amp) }
}

pub fn log_proto_fields(label: &CStr, data: &[u8]) {
    unsafe { nano_log_proto_fields(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}

pub fn log_bpm_hunt(data: &[u8]) {
    unsafe { nano_log_bpm_hunt(tag_ptr(), data.as_ptr(), data.len()) }
}

pub fn log_bpm_raw_scan(label: &CStr, data: &[u8]) {
    unsafe { nano_log_bpm_raw_scan(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}

pub fn log_bytes_full(label: &CStr, data: &[u8]) {
    unsafe { nano_log_bytes_full(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}

pub fn log_tuner_state_fields(data: &[u8]) {
    unsafe { nano_log_tuner_state_fields(tag_ptr(), data.as_ptr(), data.len()) }
}
