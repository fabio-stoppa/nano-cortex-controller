//! Small string helpers shared by FX models / params / engine.

use core::ffi::{c_char, CStr};

/// `snprintf(buf, cap, "%s", text)` — truncate and always NUL-terminate.
pub(crate) fn write_c_buf(buf: *mut c_char, cap: usize, text: &str) {
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
pub(crate) fn c_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}
