//! FFI surface the tuner depends on: `nano_ui` (still C), the Rust BLE and
//! state ports, and the payload builders owned by the `nano-wire` crate.

use core::ffi::CStr;

use esp_idf_svc::sys::{esp_err_t, ESP_OK};

use super::types::NanoTunerC;

/// Log tag shared with the C helpers, which take a `const char *`.
pub const TAG: &str = "nano_tuner";

pub fn tag_ptr() -> *const core::ffi::c_char {
    c"nano_tuner".as_ptr()
}

unsafe extern "C" {
    /* ---------------------------------------------------- components/nano */

    pub fn nano_ui_set_tuner(tuner: *const NanoTunerC);

    /* ---------------------------------------------------------- nano-wire */

    pub fn nano_log_bytes(
        tag: *const core::ffi::c_char,
        label: *const core::ffi::c_char,
        data: *const u8,
        len: usize,
    );
}

/* -------------------------------------------------- nano_ble / nano_state */
// `nano_ble_*` and `nano_state_*` are the C-ABI exports of the Rust ports, so
// they are called directly instead of through an extern block.

pub fn ui_set_tuner(tuner: &NanoTunerC) {
    unsafe { nano_ui_set_tuner(tuner as *const NanoTunerC) }
}

pub fn ble_connected() -> bool {
    crate::ble::nano_ble_connected()
}

pub fn ble_write_c304(label: &CStr, data: &[u8]) -> bool {
    let err: esp_err_t = crate::ble::nano_ble_write_c304(label.as_ptr(), data.as_ptr(), data.len());
    err == ESP_OK
}

pub fn ble_flush_c305_backlog() {
    crate::ble::nano_ble_flush_c305_backlog()
}

pub fn state_tuner_watch_update(active: bool) {
    crate::state::nano_state_tuner_watch_update(active)
}

pub fn state_request_tuner_sync() {
    crate::state::nano_state_request_tuner_sync()
}

pub fn log_bytes(label: &CStr, data: &[u8]) {
    unsafe { nano_log_bytes(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}
