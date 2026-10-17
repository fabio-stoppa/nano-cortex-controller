/* ---------------------------------------------------------------- helpers */

use super::*;
use super::state::on_lvgl_task;
pub(super) fn now_us() -> i64 {
    unsafe { sys::esp_timer_get_time() }
}

/// LVGL-task-only paint buffers. Snapshot APIs fill these so paint helpers never
/// hold a live pointer into FX / amp `Shared` state while the C305 worker writes.
pub(super) static mut FX_PAINT: NanoFxState = NanoFxState::zeroed();
pub(super) static mut AMP_PAINT: NanoAmpC = unsafe { core::mem::zeroed() };
pub(super) static mut TUNER_PAINT: NanoTunerC = NanoTunerC::new();

pub(super) fn tuner_snap() -> NanoTunerC {
    let mut out = NanoTunerC::new();
    nano_tuner_snapshot(&mut out);
    out
}

/// Seqlock snapshot into the LVGL paint buffer (must only run on the LVGL task).
pub(super) fn fx_state() -> *const NanoFxState {
    debug_assert!(on_lvgl_task(), "fx_state paint buffer is LVGL-task only");
    unsafe {
        let out = &mut *(&raw mut FX_PAINT);
        if nano_fx_snapshot(out) {
            &raw const FX_PAINT
        } else {
            ptr::null()
        }
    }
}

/// Seqlock snapshot into the LVGL paint buffer (must only run on the LVGL task).
pub(super) fn amp_state() -> *const NanoAmpC {
    debug_assert!(on_lvgl_task(), "amp_state paint buffer is LVGL-task only");
    unsafe {
        let out = &mut *(&raw mut AMP_PAINT);
        if nano_state_snapshot_amp(out) {
            &raw const AMP_PAINT
        } else {
            ptr::null()
        }
    }
}

pub(super) fn tuner_state() -> *const NanoTunerC {
    debug_assert!(on_lvgl_task(), "tuner_state paint buffer is LVGL-task only");
    unsafe {
        let out = &mut *(&raw mut TUNER_PAINT);
        nano_tuner_snapshot(out);
        &raw const TUNER_PAINT
    }
}

pub(super) fn pct(raw: u8) -> u32 {
    (raw as u32 * 100 + 127) / 255
}

pub(super) fn note_label(note: i8) -> &'static str {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    if !(0..=11).contains(&note) {
        return "?";
    }
    NAMES[note as usize]
}

/// Read a NUL-terminated fixed C buffer.
pub(super) fn c_buf_str(buf: &[u8]) -> &str {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..end]).unwrap_or("")
}

pub(super) fn c_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}

/// `strncpy(dst, text, cap - 1)` + explicit NUL.
pub(super) fn copy_into(dst: &mut [u8], text: &str) {
    dst.fill(0);
    let n = text.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&text.as_bytes()[..n]);
}

pub(super) fn struct_bytes<T>(v: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts(v as *const T as *const u8, core::mem::size_of::<T>()) }
}

pub(super) fn set_label_if_changed(lbl: *mut lv_obj_t, txt: &str) {
    if lbl.is_null() {
        return;
    }
    if let Some(cur) = lv::label_get_text(lbl) {
        if cur == txt {
            return;
        }
    }
    lv::label_set_text(lbl, txt);
}

pub(super) fn set_slider_if_changed(slider: *mut lv_obj_t, val: i32) {
    if slider.is_null() {
        return;
    }
    if lv::slider_get_value(slider) == val {
        return;
    }
    lv::slider_set_value(slider, val, lv::ANIM_OFF);
}

