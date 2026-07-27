//! Small string helpers shared by FX models / params / engine.

use core::ffi::{c_char, CStr};

/// Borrow a `const char *` as `&str` (empty on NULL / invalid UTF-8).
pub(crate) fn c_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}
