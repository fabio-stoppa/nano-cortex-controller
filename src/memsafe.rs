//! Small helpers for turning raw FFI pointers into bounded Rust slices.
//!
//! Callers still need `unsafe` at the boundary (the pointer comes from C / ESP-IDF),
//! but null checks and length caps live in one place instead of being rediscovered
//! at every call site.

use core::slice;

/// Largest single BLE ATT / notify payload we accept (matches C305 queue item).
pub const PKT_MAX: usize = 512;

/// Largest assembled preset / metadata blob we will form a slice over.
pub const BLOB_MAX: usize = 16 * 1024;

/// Borrow `ptr[..len]` when the pointer is non-null and `len > 0`.
///
/// # Safety
/// `ptr` must point to at least `len` readable bytes for the returned lifetime.
#[inline]
pub unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() || len == 0 {
        None
    } else {
        Some(unsafe { slice::from_raw_parts(ptr, len) })
    }
}

/// Like [`bytes`], but refuses anything larger than `max` (returns `None`).
///
/// # Safety
/// Same as [`bytes`].
#[inline]
pub unsafe fn bytes_at_most<'a>(ptr: *const u8, len: usize, max: usize) -> Option<&'a [u8]> {
    if len > max {
        None
    } else {
        unsafe { bytes(ptr, len) }
    }
}

/// Like [`bytes`], but truncates to `max` when the remote sent too much.
///
/// # Safety
/// `ptr` must point to at least `len.min(max)` readable bytes.
#[inline]
pub unsafe fn bytes_truncated<'a>(ptr: *const u8, len: usize, max: usize) -> Option<&'a [u8]> {
    unsafe { bytes(ptr, len.min(max)) }
}
