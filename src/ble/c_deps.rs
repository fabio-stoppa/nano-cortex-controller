//! FFI surface the BLE client depends on: `nano_ui` (still C), the Rust state
//! and tuner ports, and the protocol helpers exported by the `nano-wire` crate.

use core::ffi::{c_char, c_void};

/// Log tag shared with the C helpers, which take a `const char *`.
pub const TAG: &str = "nano_ble";

pub fn tag_ptr() -> *const c_char {
    c"nano_ble".as_ptr()
}

unsafe extern "C" {
    pub fn nano_state_feed_c305(data: *const u8, len: usize);
    pub fn nano_state_try_amp_notify_fast(data: *const u8, len: usize) -> bool;
    pub fn nano_state_begin_sync();
    pub fn nano_state_reset();
    pub fn nano_state_on_bpm_found(bpm: u16);

    pub fn nano_ui_set_ble_connected(connected: bool);
    pub fn nano_ui_set_amp(amp: *const c_void);

    pub fn nano_log_bytes(tag: *const c_char, label: *const c_char, data: *const u8, len: usize);
    pub fn nano_log_bytes_full(
        tag: *const c_char,
        label: *const c_char,
        data: *const u8,
        len: usize,
    );
    pub fn nano_log_bpm_raw_scan(
        tag: *const c_char,
        label: *const c_char,
        data: *const u8,
        len: usize,
    );

    pub fn nano_parse_bpm(data: *const u8, len: usize, out_bpm: *mut u16) -> bool;
    pub fn nano_try_parse_bpm_notify(data: *const u8, len: usize, out_bpm: *mut u16) -> bool;
}

pub fn tuner_active() -> bool {
    let tuner = crate::tuner::nano_tuner_get();
    !tuner.is_null() && unsafe { (*tuner).active }
}

pub fn tuner_feed_pitch_fast(data: &[u8]) -> bool {
    unsafe { crate::tuner::nano_tuner_feed_pitch_fast(data.as_ptr(), data.len()) }
}

pub fn state_feed_c305(data: &[u8]) {
    unsafe { nano_state_feed_c305(data.as_ptr(), data.len()) }
}

pub fn state_try_amp_notify_fast(data: &[u8]) -> bool {
    unsafe { nano_state_try_amp_notify_fast(data.as_ptr(), data.len()) }
}

pub fn state_begin_sync() {
    unsafe { nano_state_begin_sync() }
}

pub fn state_reset() {
    unsafe { nano_state_reset() }
}

pub fn state_on_bpm_found(bpm: u16) {
    unsafe { nano_state_on_bpm_found(bpm) }
}

pub fn ui_set_ble_connected(connected: bool) {
    unsafe { nano_ui_set_ble_connected(connected) }
}

/// `nano_ui_set_amp(NULL)` — clears the amp panel on disconnect.
pub fn ui_clear_amp() {
    unsafe { nano_ui_set_amp(core::ptr::null()) }
}

pub fn log_bytes(label: &core::ffi::CStr, data: &[u8]) {
    unsafe { nano_log_bytes(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}

pub fn log_bytes_full(label: &core::ffi::CStr, data: &[u8]) {
    unsafe { nano_log_bytes_full(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}

pub fn log_bpm_raw_scan(label: &core::ffi::CStr, data: &[u8]) {
    unsafe { nano_log_bpm_raw_scan(tag_ptr(), label.as_ptr(), data.as_ptr(), data.len()) }
}

/// Tap-tempo shaped notify first, then the preset-field parser (order from the C port).
pub fn parse_bpm(data: &[u8]) -> Option<u16> {
    let mut bpm: u16 = 0;
    let hit = unsafe {
        nano_try_parse_bpm_notify(data.as_ptr(), data.len(), &mut bpm)
            || nano_parse_bpm(data.as_ptr(), data.len(), &mut bpm)
    };
    hit.then_some(bpm)
}
