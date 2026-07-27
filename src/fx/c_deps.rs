//! FFI surface the FX module depends on: `nano_ui` (still C), the Rust BLE and
//! state ports, and the payload builders owned by the `nano-wire` crate.

use core::ffi::{c_char, CStr};

use esp_idf_svc::sys::{esp_err_t, ESP_OK};

use super::types::NanoFxState;

/// Log tag shared with the C helpers, which take a `const char *`.
pub const TAG: &str = "nano_fx";

unsafe extern "C" {
    /* ---------------------------------------------------- components/nano */

    pub fn nano_ui_set_fx(fx: *const NanoFxState);
}

/* ------------------------------------------------------- nano_ble / state */
// `nano_ble_*` and `nano_state_*` are the C-ABI exports of the Rust ports, so
// they are called directly instead of through an extern block.

pub fn ui_set_fx(fx: &NanoFxState) {
    unsafe { nano_ui_set_fx(fx as *const NanoFxState) }
}

pub fn ble_connected() -> bool {
    crate::ble::nano_ble_connected()
}

pub fn ble_write_c304(label: &CStr, data: &[u8]) -> bool {
    let err: esp_err_t = crate::ble::nano_ble_write_c304(label.as_ptr(), data.as_ptr(), data.len());
    err == ESP_OK
}

pub fn state_request_preset_refresh(label: &CStr) {
    unsafe { crate::state::nano_state_request_preset_refresh(label.as_ptr()) }
}

/* -------------------------------------------------------- string helpers */

/// `snprintf(buf, cap, "%s", text)` — truncate and always NUL-terminate.
pub fn write_c_buf(buf: *mut c_char, cap: usize, text: &str) {
    if buf.is_null() || cap == 0 {
        return;
    }
    let dst = unsafe { core::slice::from_raw_parts_mut(buf.cast::<u8>(), cap) };
    let bytes = text.as_bytes();
    let n = bytes.len().min(cap - 1);
    dst[..n].copy_from_slice(&bytes[..n]);
    dst[n] = 0;
}

/// Borrow a `const char *` as `&str` (empty on NULL / invalid UTF-8).
pub fn c_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}
