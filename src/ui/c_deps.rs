//! Everything the UI needs that is not LVGL: the Rust ports of `nano_ble`,
//! `nano_state`, `nano_fx`, `nano_tuner` and `nano_amp`, plus the small
//! string/time helpers `nano_ui.c` used to get from libc.
//!
//! The `nano_*` entry points are C-ABI exports of sibling Rust modules, so they
//! are called directly instead of through an `extern` block. What lives here is
//! the safe shape of those calls: fixed C buffers become `String`, `NULL`
//! becomes `Option`, and the `const char *` param names stay raw because
//! `nano_fx` wants them back verbatim.

#![allow(dead_code)]

use core::ffi::{c_char, CStr};
use core::ptr;

use esp_idf_svc::sys as sys;

pub use nano_wire::ffi::NanoAmpC;
pub use nano_wire::types::{AMP_CAPTURE_NAME_MAX, BPM_MAX, BPM_MIN, PRESET_NAME_MAX};

pub use crate::amp::{
    NanoAmpCapture, NANO_AMP_BANK_2, NANO_AMP_BANK_3, NANO_AMP_BANK_BASS, NANO_AMP_BANK_CLEAN,
    NANO_AMP_BANK_HIGH_GAIN, NANO_AMP_CAPTURE_COUNT,
};
pub use crate::fx::types::{
    NanoFxBlock, NanoFxModelDef, NanoFxState, NANO_FX_BLOCK_COUNT, NANO_FX_CAT_BASS_OD,
    NANO_FX_CAT_COMPRESSOR, NANO_FX_CAT_DELAY, NANO_FX_CAT_FILTER, NANO_FX_CAT_GUITAR_OD,
    NANO_FX_CAT_MODULATION, NANO_FX_CAT_NONE, NANO_FX_CAT_PITCH, NANO_FX_CAT_REVERB,
    NANO_FX_CAT_UTILITY, NANO_FX_CAT_WAH, NANO_FX_MAX_PARAMS, NANO_FX_POST1, NANO_FX_POST2,
    NANO_FX_POST3, NANO_FX_PRE1, NANO_FX_PRE2,
};
pub use crate::tuner::types::NanoTunerC;

pub const NANO_AMP_CAPTURE_NAME_MAX: usize = AMP_CAPTURE_NAME_MAX;

/* ------------------------------------------------------------------ time */

pub fn now_us() -> i64 {
    unsafe { sys::esp_timer_get_time() }
}

/* --------------------------------------------------------------- strings */

/// Borrow a `const char *` as `&str` (empty on NULL / invalid UTF-8).
pub fn c_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}

/// Read a NUL-terminated fixed buffer coming from a C struct.
pub fn c_buf_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Run a `void f(char *out, size_t cap)`-shaped C formatter into a `String`.
fn with_buf<const N: usize>(fill: impl FnOnce(*mut c_char, usize)) -> String {
    let mut buf = [0u8; N];
    fill(buf.as_mut_ptr().cast::<c_char>(), N);
    c_buf_str(&buf)
}

/* ------------------------------------------------------------------- ble */

pub fn ble_connected() -> bool {
    crate::ble::nano_ble_connected()
}

pub fn ble_scanning() -> bool {
    crate::ble::nano_ble_scanning()
}

/* ----------------------------------------------------------------- state */

pub fn amp_get() -> *const NanoAmpC {
    crate::state::nano_state_get_amp()
}

pub fn amp_eq(a: &NanoAmpC, b: &NanoAmpC) -> bool {
    a.gain == b.gain
        && a.level == b.level
        && a.bass == b.bass
        && a.mid == b.mid
        && a.treble == b.treble
        && a.capture_volume == b.capture_volume
        && a.capture_slot == b.capture_slot
        && a.capture_name == b.capture_name
        && a.have_capture_slot == b.have_capture_slot
        && a.have_capture_name == b.have_capture_name
        && a.have_capture_volume == b.have_capture_volume
        && a.valid == b.valid
}

pub fn amp_copy(a: &NanoAmpC) -> NanoAmpC {
    crate::state::c_deps::amp_copy(a)
}

pub fn amp_zero() -> NanoAmpC {
    crate::state::c_deps::amp_zero()
}

pub fn state_write_amp_knob(knob: u32, raw: u8) -> bool {
    crate::state::nano_state_write_amp_knob(knob, raw)
}

pub fn state_write_capture_volume(raw: u8) -> bool {
    crate::state::nano_state_write_capture_volume(raw)
}

pub fn state_write_capture_slot(slot: u8) -> bool {
    crate::state::nano_state_write_capture_slot(slot)
}

pub fn state_get_bpm() -> u16 {
    crate::state::nano_state_get_bpm()
}

pub fn state_tempo_enter() -> bool {
    crate::state::nano_state_tempo_enter()
}

pub fn state_tempo_exit() -> bool {
    crate::state::nano_state_tempo_exit()
}

pub fn state_tempo_tap() -> bool {
    crate::state::nano_state_tempo_tap()
}

/* ----------------------------------------------------------------- tuner */

pub fn tuner_get() -> *const NanoTunerC {
    crate::tuner::nano_tuner_get()
}

pub fn tuner_snapshot() -> NanoTunerC {
    let mut out = NanoTunerC::new();
    unsafe { crate::tuner::nano_tuner_snapshot(&mut out) };
    out
}

pub fn tuner_cents_f() -> f32 {
    crate::tuner::nano_tuner_cents_f()
}

pub fn tuner_set_active(active: bool) -> bool {
    crate::tuner::nano_tuner_set_active(active)
}

/* -------------------------------------------------------------------- fx */

pub fn fx_get() -> *const NanoFxState {
    crate::fx::nano_fx_get()
}

pub fn fx_state_eq(a: &NanoFxState, b: &NanoFxState) -> bool {
    if a.valid != b.valid
        || a.gate.on != b.gate.on
        || a.gate.valid != b.gate.valid
        || a.gate.on_known != b.gate.on_known
        || a.gate.reduction_valid != b.gate.reduction_valid
        || a.gate.reduction_pct != b.gate.reduction_pct
    {
        return false;
    }

    for (x, y) in a.blocks.iter().zip(b.blocks.iter()) {
        if x.id != y.id
            || x.on != y.on
            || x.valid != y.valid
            || x.model != y.model
            || x.model_id_raw != y.model_id_raw
            || x.model_id_len != y.model_id_len
            || x.param_count != y.param_count
            || x.params_loaded != y.params_loaded
            || x.has_model_id != y.has_model_id
        {
            return false;
        }
        for (p, q) in x.params.iter().zip(y.params.iter()) {
            if p.valid != q.valid
                || p.normalized.to_bits() != q.normalized.to_bits()
                || p.display.to_bits() != q.display.to_bits()
            {
                return false;
            }
        }
    }
    true
}

pub fn fx_block_copy(block: &NanoFxBlock) -> NanoFxBlock {
    let mut out = NanoFxBlock::zeroed();
    out.id = block.id;
    out.on = block.on;
    out.valid = block.valid;
    out.model = block.model;
    out.model_id_raw = block.model_id_raw;
    out.model_id_len = block.model_id_len;
    out.params = block.params;
    out.param_count = block.param_count;
    out.params_loaded = block.params_loaded;
    out.has_model_id = block.has_model_id;
    out
}

pub fn fx_state_copy(fx: &NanoFxState) -> NanoFxState {
    let mut out = NanoFxState::zeroed();
    out.gate = fx.gate;
    out.valid = fx.valid;
    for (dst, src) in out.blocks.iter_mut().zip(fx.blocks.iter()) {
        *dst = fx_block_copy(src);
    }
    out
}

/// `block->model->param_names[idx]`, or NULL when either is missing.
pub fn fx_param_name(block: &NanoFxBlock, idx: usize) -> *const c_char {
    let Some(model) = (unsafe { block.model.as_ref() }) else {
        return ptr::null();
    };
    if model.param_names.is_null() || idx >= model.param_count as usize {
        return ptr::null();
    }
    unsafe { *model.param_names.add(idx) }
}

pub fn fx_model_name(model: &NanoFxModelDef) -> &'static str {
    c_str(model.name)
}

pub fn fx_model_category(model: *const NanoFxModelDef) -> u32 {
    unsafe { crate::fx::nano_fx_model_category(model) }
}

pub fn fx_category_label(cat: u32) -> &'static str {
    c_str(crate::fx::nano_fx_category_label(cat))
}

pub fn fx_block_model_text(block: &NanoFxBlock) -> String {
    with_buf::<56>(|buf, cap| unsafe {
        crate::fx::nano_fx_block_model_text(block as *const NanoFxBlock, buf, cap)
    })
}

pub fn fx_model_chain_acronym(block: &NanoFxBlock) -> String {
    with_buf::<12>(|buf, cap| unsafe {
        crate::fx::nano_fx_model_chain_acronym(block as *const NanoFxBlock, buf, cap)
    })
}

pub fn fx_format_param(block: &NanoFxBlock, idx: usize) -> String {
    with_buf::<24>(|buf, cap| unsafe {
        crate::fx::nano_fx_format_param(block as *const NanoFxBlock, idx as u8, buf, cap)
    })
}

/// Format a param against a value the pedal has not confirmed yet: the C port
/// formatted a stack copy of the block with `display` patched in.
pub fn fx_format_param_display(block: &NanoFxBlock, idx: usize, display: f32) -> String {
    let mut tmp = fx_block_copy(block);
    if idx < NANO_FX_MAX_PARAMS {
        tmp.params[idx].display = display;
        tmp.params[idx].valid = true;
    }
    fx_format_param(&tmp, idx)
}

pub fn fx_allowed_model_count(block: u32) -> usize {
    crate::fx::nano_fx_allowed_model_count(block)
}

pub fn fx_get_allowed_model(block: u32, index: usize) -> *const NanoFxModelDef {
    crate::fx::nano_fx_get_allowed_model(block, index)
}

pub fn fx_write_gate_enabled(enabled: bool) -> bool {
    crate::fx::nano_fx_write_gate_enabled(enabled)
}

pub fn fx_write_gate_reduction(pct: u8) -> bool {
    crate::fx::nano_fx_write_gate_reduction(pct)
}

pub fn fx_write_block_enabled(block: u32, enabled: bool) -> bool {
    crate::fx::nano_fx_write_block_enabled(block, enabled)
}

pub fn fx_write_block_model(block: u32, model: *const NanoFxModelDef) -> bool {
    unsafe { crate::fx::nano_fx_write_block_model(block, model) }
}

pub fn fx_write_param_display(block: u32, param_index: u8, display: f32) -> bool {
    crate::fx::nano_fx_write_param_display(block, param_index, display)
}

pub fn fx_param_is_enum(name: *const c_char) -> bool {
    unsafe { crate::fx::nano_fx_param_is_enum(name) }
}

pub fn fx_param_enum_count(name: *const c_char) -> i32 {
    unsafe { crate::fx::nano_fx_param_enum_count(name) }
}

pub fn fx_slider_min(name: *const c_char) -> i32 {
    unsafe { crate::fx::nano_fx_slider_min(name) }
}

pub fn fx_slider_max(name: *const c_char) -> i32 {
    unsafe { crate::fx::nano_fx_slider_max(name) }
}

pub fn fx_display_to_slider(name: *const c_char, display: f32) -> i32 {
    unsafe { crate::fx::nano_fx_display_to_slider(name, display) }
}

pub fn fx_slider_to_display(name: *const c_char, slider_value: i32) -> f32 {
    unsafe { crate::fx::nano_fx_slider_to_display(name, slider_value) }
}

/* ------------------------------------------------------------------- amp */

pub fn amp_catalog() -> &'static [NanoAmpCapture] {
    crate::amp::catalog()
}

pub fn amp_bank_label(bank: u32) -> &'static str {
    c_str(crate::amp::nano_amp_bank_label(bank))
}

pub fn amp_format_bank_line(slot: u8) -> String {
    with_buf::<28>(|buf, cap| unsafe { crate::amp::nano_amp_format_bank_line(slot, buf, cap) })
}

pub fn amp_capture_header_text(amp: *const NanoAmpC) -> String {
    with_buf::<56>(|buf, cap| unsafe { crate::amp::nano_amp_capture_header_text(amp, buf, cap) })
}
