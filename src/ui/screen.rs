//! Port of `components/nano/nano_ui.c`.
//!
//! Everything below the `nano_ui_*` entry points runs on the LVGL task, which
//! is the same contract the C file lived under: the public setters only stage a
//! snapshot plus a dirty bit, and the 8 ms dispatch timer paints it.

#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::ffi::{c_char, CStr};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use esp_idf_svc::sys as sys;
use esp_idf_svc::sys::{
    esp_err_t, lv_color_t, lv_event_t, lv_obj_t, lv_point_t, lv_style_t, lv_timer_t, ESP_ERR_NO_MEM,
    ESP_FAIL, ESP_OK,
};
use log::{error, info};
use nano_wire::ffi::NanoAmpC;

use crate::fx::{
    nano_fx_allowed_model_count, nano_fx_block_model_text, nano_fx_category_label,
    nano_fx_display_to_slider, nano_fx_format_param, nano_fx_get, nano_fx_get_allowed_model,
    nano_fx_model_category, nano_fx_model_chain_acronym, nano_fx_param_enum_count,
    nano_fx_param_is_enum, nano_fx_slider_max, nano_fx_slider_min, nano_fx_slider_to_display,
    nano_fx_write_block_enabled, nano_fx_write_block_model, nano_fx_write_gate_enabled,
    nano_fx_write_gate_reduction, nano_fx_write_param_display,
};
use crate::fx::types::{
    NanoFxBlock, NanoFxModelDef, NanoFxState, NANO_FX_CAT_BASS_OD, NANO_FX_CAT_COMPRESSOR,
    NANO_FX_CAT_DELAY, NANO_FX_CAT_FILTER, NANO_FX_CAT_GUITAR_OD, NANO_FX_CAT_MODULATION,
    NANO_FX_CAT_NONE, NANO_FX_CAT_PITCH, NANO_FX_CAT_REVERB, NANO_FX_CAT_UTILITY, NANO_FX_CAT_WAH,
    NANO_FX_MAX_PARAMS, NANO_FX_POST1, NANO_FX_POST2, NANO_FX_POST3, NANO_FX_PRE1, NANO_FX_PRE2,
};
use crate::state::{
    nano_state_get_amp, nano_state_get_bpm, nano_state_tempo_enter, nano_state_tempo_exit,
    nano_state_tempo_tap, nano_state_write_amp_knob, nano_state_write_capture_slot,
    nano_state_write_capture_volume,
};
use crate::tuner::types::NanoTunerC;
use crate::tuner::{nano_tuner_cents_f, nano_tuner_get, nano_tuner_set_active, nano_tuner_snapshot};
use crate::ui::icons;
use crate::ui::lvgl as lv;

/* ------------------------------------------------------------- constants */

const KNOB_COUNT: usize = 5;
const DRAG_WRITE_INTERVAL_US: i64 = 60 * 1000;

const FX_MODEL_PICKER_COLS: i32 = 3;
const FX_MODEL_PICKER_GAP: i32 = 8;
const FX_MODEL_PICKER_PAD: i32 = 12;
const FX_MODEL_PICKER_TILE_H: i32 = 64;
const FX_MODEL_PICKER_SCROLL_PAD: i32 = 16;

const UI_DISP_AMP: u8 = 1 << 0;
const UI_DISP_BLE: u8 = 1 << 1;
const UI_DISP_TUNER: u8 = 1 << 2;
const UI_DISP_FX: u8 = 1 << 3;
const UI_DISP_BPM: u8 = 1 << 4;
const UI_DISP_PRESET: u8 = 1 << 5;
const UI_DISP_TEMPO: u8 = 1 << 6;

const CHAIN_BLOCK_COUNT: usize = 6;
const UI_EDGE_PAD: i32 = 1;
const HEADER_H: i32 = 48;
const HEADER_DD_H: i32 = HEADER_H - 4;
const HEADER_RIGHT_PAD: i32 = 4;
const CHAIN_RAIL_PAD: i32 = 4;
const CHAIN_TAB_GAP: i32 = 8;
const CHAIN_RAIL_BOTTOM_PAD: i32 = 4;
const CONTENT_CHAIN_GAP: i32 = 4;
const CHAIN_TAB_BORDER_W: i32 = 1;
const CHAIN_TAB_BORDER_SEL: i32 = 2;

const CHAIN_RAIL_W: i32 = 112;
const CHAIN_TAB_W: i32 = 104;
const CHAIN_TAB_H: i32 = 64;
const CHAIN_ICON_W: i32 = 94;

const CHAIN_PRE1: usize = 0;
const CHAIN_PRE2: usize = 1;
const CHAIN_AMP: usize = 2;
const CHAIN_PFX1: usize = 3;
const CHAIN_PFX2: usize = 4;
const CHAIN_PFX3: usize = 5;

const CHAIN_LABEL_MAX: usize = 12;

const PARAM_GRID_COLS: i32 = 4;
const PARAM_GRID_ROWS: i32 = 4;
const PEDAL_PARAM_GRID_VISIBLE: u8 = (PARAM_GRID_COLS * PARAM_GRID_ROWS) as u8;
const AMP_PEDAL_PARAM_COUNT: u8 = 8;
const PARAM_GRID_GAP: i32 = 4;
const PARAM_GRID_PAD: i32 = 2;
const PARAM_CELL_PAD: i32 = 4;
const PARAM_CELL_ROW_GAP: i32 = 7;
const PARAM_LABEL_MAX_LEN: usize = 11;
const PARAM_SLIDER_INSET_H: i32 = 14;
const PARAM_GESTURE_SETTLE_MS: i64 = 150;
const PARAM_GESTURE_MOVE_THRESH: i32 = 12;
const PARAM_GESTURE_SCROLL_MIN_DY: i32 = 8;
const PARAM_GESTURE_TAP_THRESH: i32 = 10;
const PARAM_GRID_SCROLLBAR_W: i32 = 8;

const PARAM_CELL_AMP_TAG: usize = 0x100;
const PARAM_CELL_GATE_TAG: usize = 0x200;
const PARAM_CELL_GATE_ON_TAG: usize = 0x400;
const PARAM_CELL_CAP_VOL_TAG: usize = 0x800;

const TUNER_METER_W: i32 = 560;
const TUNER_METER_H: i32 = 36;
const TUNER_NEEDLE_W: i32 = 8;
const TUNER_CENTS_SPAN: f32 = 50.0;
const TUNER_IN_TUNE_CENTS: f32 = 2.0;
const TUNER_COLOR_OK: u32 = 0x3DCC7A;
const TUNER_COLOR_IDLE: u32 = 0x606060;
const TUNER_COLOR_NEEDLE: u32 = 0xFFFFFF;

const NANO_PRESET_NAME_MAX: usize = 48;
const NANO_AMP_CAPTURE_NAME_MAX: usize = 40;
const NANO_AMP_CAPTURE_COUNT: u8 = 25;
const NANO_BPM_MIN: u16 = 40;
const NANO_BPM_MAX: u16 = 240;

/// Keep LVGL awake for live tuner (~12.5 Hz pitch → paint a bit faster).
const UI_DISPATCH_PERIOD_MS: u32 = 8;

const HEADER_PICKER_FX: u8 = 0;
const HEADER_PICKER_AMP: u8 = 1;

const PARAM_GESTURE_NONE: u8 = 0;
const PARAM_GESTURE_PENDING: u8 = 1;
const PARAM_GESTURE_SCROLL: u8 = 2;
const PARAM_GESTURE_ADJUST: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum IconKind {
    Gate,
    Amp,
    GuitarOd,
    BassOd,
    Compressor,
    Pitch,
    Filter,
    Wah,
    Utility,
}

/* ----------------------------------------------------------------- state */

#[derive(Clone, Copy)]
struct AmpKnobUi {
    slider: *mut lv_obj_t,
    value: *mut lv_obj_t,
}

#[derive(Clone, Copy)]
struct GateCellUi {
    col: *mut lv_obj_t,
    name: *mut lv_obj_t,
    slider: *mut lv_obj_t,
    value: *mut lv_obj_t,
}

#[derive(Clone, Copy)]
struct FxParamUi {
    col: *mut lv_obj_t,
    name: *mut lv_obj_t,
    slider: *mut lv_obj_t,
    value: *mut lv_obj_t,
}

#[derive(Clone, Copy)]
struct ChainBlockUi {
    col: *mut lv_obj_t,
    btn: *mut lv_obj_t,
    icon: *mut lv_obj_t,
    icon_text: [u8; CHAIN_LABEL_MAX],
    icon_color: lv_color_t,
    last_active: bool,
    last_selected: bool,
    last_accent: lv_color_t,
}

#[derive(Clone, Copy)]
struct ParamGesture {
    active: bool,
    mode: u8,
    origin: lv_point_t,
    press_us: i64,
}

struct Ui {
    ble_icon: *mut lv_obj_t,
    bpm_lbl: *mut lv_obj_t,
    ble_icon_timer: *mut lv_timer_t,
    ble_blink_on: bool,

    tuner_note: *mut lv_obj_t,
    tuner_cents: *mut lv_obj_t,
    tuner_state: *mut lv_obj_t,
    tuner_btn: *mut lv_obj_t,
    tuner_meter: *mut lv_obj_t,
    tuner_meter_center: *mut lv_obj_t,
    tuner_meter_needle: *mut lv_obj_t,

    knobs: [AmpKnobUi; KNOB_COUNT],
    ui_sync: bool,
    ble_connected: bool,
    drag_knob: i32,
    last_sent: [u8; KNOB_COUNT],
    last_send_us: [i64; KNOB_COUNT],

    async_amp: NanoAmpC,
    async_amp_have: bool,
    async_tuner: NanoTunerC,
    async_tuner_pending: bool,
    async_ble_pending: bool,
    async_ble_connected: bool,

    gate_on: GateCellUi,
    gate: GateCellUi,
    cap_vol: GateCellUi,
    gate_on_drag: i32,
    gate_on_last_sent: i32,
    gate_on_last_send_us: i64,
    gate_drag: i32,
    gate_last_sent: i32,
    gate_last_send_us: i64,
    cap_vol_drag: i32,
    cap_vol_last_sent: i32,
    cap_vol_last_send_us: i64,
    chain_suppress_click: i32,

    amp_chrome_valid: bool,
    amp_chrome_have_slot: bool,
    amp_chrome_slot: u8,
    amp_chrome_name: [u8; NANO_AMP_CAPTURE_NAME_MAX],

    header_bar: *mut lv_obj_t,
    preset_lbl: *mut lv_obj_t,
    chain_bar: *mut lv_obj_t,
    content_root: *mut lv_obj_t,
    content_w_normal: i32,
    content_x_normal: i32,
    body_h: i32,
    amp_panel: *mut lv_obj_t,
    fx_panel: *mut lv_obj_t,
    tuner_panel: *mut lv_obj_t,
    tuner_hdr_btn: *mut lv_obj_t,
    tuner_view: bool,
    tempo_panel: *mut lv_obj_t,
    tempo_bpm_lbl: *mut lv_obj_t,
    tempo_tap_btn: *mut lv_obj_t,
    tempo_scope_sw: *mut lv_obj_t,
    tempo_scope_lbl: *mut lv_obj_t,
    tempo_view: bool,
    tempo_scope_global: bool,

    chain: [ChainBlockUi; CHAIN_BLOCK_COUNT],
    chain_selected: usize,

    fx_model_btn: *mut lv_obj_t,
    fx_model_btn_lbl: *mut lv_obj_t,
    fx_model_picker: *mut lv_obj_t,
    fx_model_picker_grid: *mut lv_obj_t,
    fx_model_picker_open: bool,
    header_picker_mode: u8,
    fx_model_btn_style: *mut lv_style_t,

    amp_params_row: *mut lv_obj_t,
    fx_params_row: *mut lv_obj_t,
    fx_build_timer: *mut lv_timer_t,
    param_cell_w: i32,
    param_cell_h: i32,
    fx_params: [FxParamUi; NANO_FX_MAX_PARAMS],
    fx_selected: u32,
    fx_scroll_anchor: u32,
    fx_ui_sync: bool,
    fx_drag_param: i32,
    fx_last_sent: [i32; NANO_FX_MAX_PARAMS],
    fx_last_send_us: [i64; NANO_FX_MAX_PARAMS],

    async_fx: NanoFxState,
    async_fx_pending: bool,
    async_bpm: u16,
    async_bpm_valid: bool,
    async_bpm_pending: bool,
    ui_bpm_cache: u16,
    ui_bpm_cache_valid: bool,
    async_tempo_mode: bool,
    async_tempo_mode_pending: bool,
    async_preset: [u8; NANO_PRESET_NAME_MAX],
    ui_preset_cache: [u8; NANO_PRESET_NAME_MAX],
    ui_preset_cache_valid: bool,
    async_preset_pending: bool,
    ui_amp_cache: NanoAmpC,
    ui_amp_cache_valid: bool,
    ui_fx_cache: NanoFxState,
    ui_fx_cache_valid: bool,

    pedal_params_scroll_row: *mut lv_obj_t,
    pedal_params_scroll_on: bool,
    pedal_params_scroll_inited: bool,
    gate_style_valid: bool,
    gate_style_on: bool,

    param_gesture: ParamGesture,

    dispatch_ready: bool,
    dispatch_mask: u8,
    dispatch_timer: *mut lv_timer_t,
}

impl Ui {
    const fn new() -> Ui {
        // Every field is a pointer, integer, bool or POD C struct, so the C
        // file's implicit zero-init for statics carries over verbatim.
        let mut u: Ui = unsafe { core::mem::zeroed() };
        u.drag_knob = -1;
        u.gate_on_last_sent = -1;
        u.gate_last_sent = -1;
        u.cap_vol_last_sent = -1;
        u.chain_suppress_click = -1;
        u.chain_selected = CHAIN_AMP;
        u.tempo_scope_global = true;
        u.fx_drag_param = -1;
        u
    }
}

struct UiCell(UnsafeCell<Ui>);

// Guarded by the LVGL task the same way `nano_ui.c`'s statics were.
unsafe impl Sync for UiCell {}

static UI: UiCell = UiCell(UnsafeCell::new(Ui::new()));

#[inline]
#[allow(clippy::mut_from_ref)]
fn ui() -> &'static mut Ui {
    unsafe { &mut *UI.0.get() }
}

static BUILD_DONE: AtomicBool = AtomicBool::new(false);

/* ---------------------------------------------------------------- helpers */

fn now_us() -> i64 {
    unsafe { sys::esp_timer_get_time() }
}

fn fx_state() -> *const NanoFxState {
    unsafe { nano_fx_get() }
}

fn amp_state() -> *const NanoAmpC {
    unsafe { nano_state_get_amp() }
}

fn pct(raw: u8) -> u32 {
    (raw as u32 * 100 + 127) / 255
}

fn note_label(note: i8) -> &'static str {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    if !(0..=11).contains(&note) {
        return "?";
    }
    NAMES[note as usize]
}

/// Read a NUL-terminated fixed C buffer.
fn c_buf_str(buf: &[u8]) -> &str {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..end]).unwrap_or("")
}

fn c_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}

/// `strncpy(dst, text, cap - 1)` + explicit NUL.
fn copy_into(dst: &mut [u8], text: &str) {
    dst.fill(0);
    let n = text.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&text.as_bytes()[..n]);
}

fn struct_bytes<T>(v: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts(v as *const T as *const u8, core::mem::size_of::<T>()) }
}

fn set_label_if_changed(lbl: *mut lv_obj_t, txt: &str) {
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

fn set_slider_if_changed(slider: *mut lv_obj_t, val: i32) {
    if slider.is_null() {
        return;
    }
    if lv::slider_get_value(slider) == val {
        return;
    }
    lv::slider_set_value(slider, val, lv::ANIM_OFF);
}

/* ----------------------------------------------------------------- tuner */

fn paint_tuner_meter_locked(cents_f: f32, has_pitch: bool) {
    let u = ui();
    if u.tuner_meter.is_null() || u.tuner_meter_needle.is_null() {
        return;
    }

    let c = cents_f.clamp(-TUNER_CENTS_SPAN, TUNER_CENTS_SPAN);
    let in_tune = has_pitch && c.abs() <= TUNER_IN_TUNE_CENTS;

    let (x, color) = if !has_pitch {
        (
            (TUNER_METER_W - TUNER_NEEDLE_W) / 2,
            lv::color_hex(TUNER_COLOR_IDLE),
        )
    } else {
        let t = (c + TUNER_CENTS_SPAN) / (2.0 * TUNER_CENTS_SPAN);
        let travel = TUNER_METER_W - TUNER_NEEDLE_W;
        let x = (t * travel as f32).round() as i32;
        let color = lv::color_hex(if in_tune {
            TUNER_COLOR_OK
        } else {
            TUNER_COLOR_NEEDLE
        });
        (x, color)
    };

    if lv::get_x(u.tuner_meter_needle) != x {
        lv::set_x(u.tuner_meter_needle, x);
    }
    lv::set_style_bg_color(u.tuner_meter_needle, color, 0);
    if !u.tuner_meter_center.is_null() {
        lv::set_style_bg_opa(
            u.tuner_meter_center,
            if in_tune { lv::OPA_80 } else { lv::OPA_30 },
            0,
        );
    }
    if !u.tuner_note.is_null() {
        lv::set_style_text_color(
            u.tuner_note,
            lv::color_hex(if in_tune { TUNER_COLOR_OK } else { 0xFFFFFF }),
            0,
        );
    }
}

unsafe extern "C" fn tuner_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED || !ui().ble_connected {
        return;
    }

    let t = unsafe { nano_tuner_get() };
    let Some(t) = (unsafe { t.as_ref() }) else {
        return;
    };
    unsafe { nano_tuner_set_active(!t.active) };
}

unsafe extern "C" fn bpm_hdr_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED || !ui().ble_connected {
        return;
    }
    show_tempo_view(!ui().tempo_view, true);
    sync_header_model_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

unsafe extern "C" fn tempo_tap_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED || !ui().ble_connected {
        return;
    }
    unsafe { nano_state_tempo_tap() };
}

unsafe extern "C" fn tempo_scope_event_cb(e: *mut lv_event_t) {
    let u = ui();
    if lv::event_code(e) != lv::EVENT_VALUE_CHANGED || u.tempo_scope_sw.is_null() {
        return;
    }
    u.tempo_scope_global = lv::has_state(u.tempo_scope_sw, lv::STATE_CHECKED);
    if !u.tempo_scope_lbl.is_null() {
        lv::label_set_text(
            u.tempo_scope_lbl,
            if u.tempo_scope_global {
                "Global tempo"
            } else {
                "Preset tempo"
            },
        );
    }
    // BLE write for global/preset scope is unknown — UI-local until sniffed.
    info!(
        "Tempo scope UI: {} (no BLE write yet)",
        if u.tempo_scope_global {
            "global"
        } else {
            "preset"
        }
    );
}

fn apply_tuner_locked(tuner: *const NanoTunerC) {
    let u = ui();
    if u.tuner_note.is_null() {
        return;
    }

    lv::set_enabled(u.tuner_btn, u.ble_connected);

    let tuner = unsafe { tuner.as_ref() };
    let active = tuner.map(|t| t.active).unwrap_or(false);
    if !active {
        set_label_if_changed(u.tuner_note, "-");
        lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
        set_label_if_changed(u.tuner_cents, "Tuner off");
        paint_tuner_meter_locked(0.0, false);
        lv::label_set_text(lv::get_child(u.tuner_btn, 0), "ON");
        return;
    }

    let t = tuner.unwrap();
    if t.has_pitch {
        set_label_if_changed(u.tuner_note, note_label(t.note));
        let cf = unsafe { nano_tuner_cents_f() };
        set_label_if_changed(u.tuner_cents, &format!("{cf:+.1} cents"));
        paint_tuner_meter_locked(cf, true);
    } else {
        set_label_if_changed(u.tuner_note, "...");
        lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
        set_label_if_changed(u.tuner_cents, "Listening...");
        paint_tuner_meter_locked(0.0, false);
    }
    lv::label_set_text(lv::get_child(u.tuner_btn, 0), "OFF");
}

fn async_apply_amp_cb() {
    let u = ui();
    if u.async_amp_have {
        let amp = u.async_amp;
        apply_amp_locked(&amp);
    } else {
        apply_amp_locked(ptr::null());
    }
}

fn async_apply_tuner_cb() {
    let u = ui();
    // Mode on/off still arrives via snapshot; pitch is sampled live below.
    if u.async_tuner_pending {
        let t = u.async_tuner;
        u.async_tuner_pending = false;
        apply_tuner_locked(&t);
        if t.active && !u.tuner_view {
            show_tuner_view(true);
        } else if !t.active && u.tuner_view {
            show_tuner_view(false);
        }
    }
}

fn paint_tuner_live_locked() {
    let u = ui();
    if u.tuner_note.is_null() || !u.tuner_view {
        return;
    }

    let mut t = NanoTunerC::default();
    unsafe { nano_tuner_snapshot(&mut t) };
    if !t.active {
        return;
    }

    if t.has_pitch {
        set_label_if_changed(u.tuner_note, note_label(t.note));
        let cf = unsafe { nano_tuner_cents_f() };
        set_label_if_changed(u.tuner_cents, &format!("{cf:+.1} cents"));
        paint_tuner_meter_locked(cf, true);
    } else {
        set_label_if_changed(u.tuner_note, "...");
        lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
        set_label_if_changed(u.tuner_cents, "Listening...");
        paint_tuner_meter_locked(0.0, false);
    }
}

/* ------------------------------------------------------------------- BLE */

fn refresh_ble_icon_locked() {
    let u = ui();
    if u.ble_icon.is_null() {
        return;
    }

    if u.ble_connected {
        lv::set_style_opa(u.ble_icon, lv::OPA_COVER, 0);
        lv::set_style_text_color(u.ble_icon, lv::color_hex(0x80C0FF), 0);
        return;
    }

    if unsafe { crate::ble::nano_ble_scanning() } {
        u.ble_blink_on = !u.ble_blink_on;
        lv::set_style_opa(
            u.ble_icon,
            if u.ble_blink_on {
                lv::OPA_COVER
            } else {
                lv::OPA_30
            },
            0,
        );
        lv::set_style_text_color(u.ble_icon, lv::color_hex(0x80C0FF), 0);
        return;
    }

    lv::set_style_opa(u.ble_icon, lv::OPA_30, 0);
    lv::set_style_text_color(u.ble_icon, lv::color_hex(0x505050), 0);
}

unsafe extern "C" fn ble_icon_timer_cb(_timer: *mut lv_timer_t) {
    refresh_ble_icon_locked();
}

fn async_apply_ble_cb() {
    let u = ui();
    if !u.async_ble_pending {
        return;
    }
    u.async_ble_pending = false;
    u.ble_connected = u.async_ble_connected;
    refresh_ble_icon_locked();
    set_slider_enabled_locked(u.ble_connected);
    if !u.tempo_tap_btn.is_null() {
        lv::set_enabled(u.tempo_tap_btn, u.ble_connected);
    }
    if u.async_tuner_pending {
        let t = u.async_tuner;
        apply_tuner_locked(&t);
    } else {
        apply_tuner_locked(unsafe { nano_tuner_get() });
    }
    if u.async_fx_pending {
        let fx = u.async_fx;
        apply_fx_locked(&fx);
    } else {
        apply_fx_locked(fx_state());
    }
}

/* -------------------------------------------------------------- dispatch */

fn ui_run_pending_updates(mask: u8) {
    let u = ui();
    if (mask & UI_DISP_BLE) != 0 && u.async_ble_pending {
        async_apply_ble_cb();
    }
    if (mask & UI_DISP_AMP) != 0 {
        async_apply_amp_cb();
    }
    if (mask & UI_DISP_TUNER) != 0 {
        async_apply_tuner_cb();
    }
    if (mask & UI_DISP_FX) != 0 {
        async_apply_fx_cb();
    }
    if (mask & UI_DISP_TEMPO) != 0 {
        async_apply_tempo_mode_cb();
    }
    if (mask & UI_DISP_BPM) != 0 {
        async_apply_bpm_cb();
    }
    if (mask & UI_DISP_PRESET) != 0 {
        async_apply_preset_cb();
    }
}

/// Drain pending UI updates; call from the LVGL task only.
#[no_mangle]
pub unsafe extern "C" fn nano_ui_dispatch_if_pending() {
    let u = ui();
    if !u.dispatch_ready {
        return;
    }

    let mut mask = u.dispatch_mask;
    if mask == 0 {
        // Dirty bits can be lost against the setters; still paint hot snapshots.
        if u.async_tuner_pending {
            mask = UI_DISP_TUNER;
        } else if u.async_ble_pending {
            mask = UI_DISP_BLE;
        } else if u.async_fx_pending {
            mask = UI_DISP_FX;
        } else if u.async_tempo_mode_pending {
            mask = UI_DISP_TEMPO;
        } else if u.async_bpm_pending {
            mask = UI_DISP_BPM;
        } else if u.async_preset_pending {
            mask = UI_DISP_PRESET;
        } else {
            return;
        }
    }

    u.dispatch_mask = 0;
    ui_run_pending_updates(mask);
}

unsafe extern "C" fn ui_dispatch_timer_cb(_timer: *mut lv_timer_t) {
    unsafe { nano_ui_dispatch_if_pending() };

    let mut t = NanoTunerC::default();
    unsafe { nano_tuner_snapshot(&mut t) };
    if t.active && !ui().tuner_view {
        show_tuner_view(true);
        apply_tuner_locked(&t);
    } else if !t.active && ui().tuner_view {
        show_tuner_view(false);
        apply_tuner_locked(&t);
    }

    paint_tuner_live_locked();
}

fn ui_request_dispatch() {
    // The 8 ms dispatch timer picks the dirty bits up; the C wake queue only
    // existed to nudge a task that is already polling.
}

fn ui_init_dispatch() {
    let u = ui();
    u.dispatch_ready = true;
    if u.dispatch_timer.is_null() {
        u.dispatch_timer = lv::timer_create(
            Some(ui_dispatch_timer_cb),
            UI_DISPATCH_PERIOD_MS,
            ptr::null_mut(),
        );
        if u.dispatch_timer.is_null() {
            error!("UI dispatch timer create failed");
        }
    }
}

fn async_apply_fx_cb() {
    let u = ui();
    if !u.async_fx_pending || u.fx_model_picker_open {
        return;
    }
    let fx = u.async_fx;
    apply_fx_locked(&fx);
    u.async_fx_pending = false;
}

fn apply_bpm_locked(bpm: u16, valid: bool) {
    let u = ui();
    if u.bpm_lbl.is_null() && u.tempo_bpm_lbl.is_null() {
        return;
    }

    if !valid {
        if !u.bpm_lbl.is_null() {
            set_label_if_changed(u.bpm_lbl, "- BPM");
        }
        if !u.tempo_bpm_lbl.is_null() {
            set_label_if_changed(u.tempo_bpm_lbl, "-");
        }
        return;
    }

    if !u.bpm_lbl.is_null() {
        set_label_if_changed(u.bpm_lbl, &format!("{bpm} BPM"));
    }
    if !u.tempo_bpm_lbl.is_null() {
        set_label_if_changed(u.tempo_bpm_lbl, &format!("{bpm}"));
    }
}

fn refresh_preset_label_locked() {
    let u = ui();
    if u.preset_lbl.is_null() {
        return;
    }

    let preset = if u.ui_preset_cache_valid && u.ui_preset_cache[0] != 0 {
        Some(c_buf_str(&u.ui_preset_cache).to_string())
    } else {
        None
    };

    let mut bank_line = String::new();
    if let Some(amp) = unsafe { amp_state().as_ref() } {
        if amp.have_capture_slot
            && amp.capture_slot >= 1
            && amp.capture_slot <= NANO_AMP_CAPTURE_COUNT
        {
            let mut buf = [0u8; 28];
            unsafe {
                crate::amp::nano_amp_format_bank_line(
                    amp.capture_slot,
                    buf.as_mut_ptr() as *mut c_char,
                    buf.len(),
                )
            };
            bank_line = c_buf_str(&buf).to_string();
        }
    }

    let text = match (bank_line.is_empty(), preset) {
        (false, Some(p)) => format!("{bank_line} / {p}"),
        (false, None) => bank_line,
        (true, Some(p)) => p,
        (true, None) if u.tuner_view => "Tuner".to_string(),
        (true, None) if u.tempo_view => "Tempo".to_string(),
        (true, None) => "-".to_string(),
    };

    set_label_if_changed(u.preset_lbl, &text);
}

fn async_apply_preset_cb() {
    let u = ui();
    if !u.async_preset_pending {
        return;
    }
    u.async_preset_pending = false;
    refresh_preset_label_locked();
}

fn async_apply_bpm_cb() {
    let u = ui();
    if !u.async_bpm_pending {
        return;
    }
    u.async_bpm_pending = false;
    apply_bpm_locked(u.async_bpm, u.async_bpm_valid);
}

/* ------------------------------------------------------------ chain rail */

fn category_to_icon(cat: u32) -> IconKind {
    match cat {
        NANO_FX_CAT_GUITAR_OD => IconKind::GuitarOd,
        NANO_FX_CAT_BASS_OD => IconKind::BassOd,
        NANO_FX_CAT_COMPRESSOR => IconKind::Compressor,
        NANO_FX_CAT_PITCH => IconKind::Pitch,
        NANO_FX_CAT_FILTER => IconKind::Filter,
        NANO_FX_CAT_WAH => IconKind::Wah,
        _ => IconKind::Utility,
    }
}

fn icon_kind_color(kind: IconKind) -> lv_color_t {
    lv::color_hex(match kind {
        IconKind::Gate => 0xB0B8C0,
        IconKind::Amp => 0xE85040,
        IconKind::GuitarOd => 0xE87030,
        IconKind::BassOd => 0xC87828,
        IconKind::Compressor => 0x5090E8,
        IconKind::Pitch => 0x40C8E0,
        IconKind::Filter => 0xE0C040,
        IconKind::Wah => 0xA868E8,
        IconKind::Utility => 0x909090,
    })
}

fn icon_kind_glyph(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Gate => "GATE",
        IconKind::Amp => "AMP",
        IconKind::GuitarOd => "GTR OD",
        IconKind::BassOd => "BASS",
        IconKind::Compressor => "COMP",
        IconKind::Pitch => "PITCH",
        IconKind::Filter => "FILTER",
        IconKind::Wah => "WAH",
        IconKind::Utility => "UTIL",
    }
}

fn ui_passive_container(obj: *mut lv_obj_t) {
    if obj.is_null() {
        return;
    }
    lv::remove_flag(obj, lv::FLAG_CLICKABLE);
    lv::remove_flag(
        obj,
        lv::FLAG_SCROLLABLE | lv::FLAG_SCROLL_CHAIN | lv::FLAG_GESTURE_BUBBLE,
    );
}

fn ui_scroll_container(obj: *mut lv_obj_t, dir: sys::lv_dir_t) {
    if obj.is_null() {
        return;
    }
    lv::add_flag(obj, lv::FLAG_SCROLLABLE | lv::FLAG_CLICKABLE);
    lv::set_scroll_dir(obj, dir);
    lv::remove_flag(obj, lv::FLAG_SCROLL_CHAIN);
    lv::add_flag(obj, lv::FLAG_GESTURE_BUBBLE);
}

fn ui_param_cell_container(obj: *mut lv_obj_t) {
    if obj.is_null() {
        return;
    }
    lv::add_flag(obj, lv::FLAG_CLICKABLE);
    lv::remove_flag(obj, lv::FLAG_SCROLLABLE);
    lv::add_flag(obj, lv::FLAG_SCROLL_CHAIN_VER | lv::FLAG_GESTURE_BUBBLE);
}

fn ui_no_scroll_only(obj: *mut lv_obj_t) {
    if obj.is_null() {
        return;
    }
    lv::remove_flag(obj, lv::FLAG_SCROLLABLE);
}

fn ui_bring_chrome_to_front() {
    let u = ui();
    if !u.chain_bar.is_null() {
        lv::move_foreground(u.chain_bar);
    }
    if u.fx_model_picker_open && !u.fx_model_picker.is_null() {
        lv::move_foreground(u.fx_model_picker);
    }
    if !u.header_bar.is_null() {
        lv::move_foreground(u.header_bar);
    }
}

fn make_content_panel(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    let panel = lv::obj_create(parent);
    lv::set_size(panel, lv::pct(100), lv::pct(100));
    lv::align(panel, lv::ALIGN_TOP_MID, 0, 0);
    lv::set_style_bg_opa(panel, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(panel, 0, 0);
    lv::set_style_pad_all(panel, 0, 0);
    ui_passive_container(panel);
    panel
}

fn chain_set_block_label(idx: usize, text: &str, color: lv_color_t) {
    let block = &mut ui().chain[idx];
    if block.icon.is_null() {
        return;
    }

    if c_buf_str(&block.icon_text) == text && lv::color_eq(block.icon_color, color) {
        return;
    }

    copy_into(&mut block.icon_text, text);
    block.icon_color = color;
    lv::label_set_text(block.icon, text);
    lv::set_style_text_font(block.icon, lv::font_chain(), 0);
    lv::set_style_text_color(block.icon, color, 0);
    lv::set_style_shadow_width(block.icon, 0, 0);
}

fn chain_btn_prepare(btn: *mut lv_obj_t) {
    lv::remove_style_all(btn);
    lv::remove_theme(btn, lv::PART_ANY | lv::SEL_STATE_ANY);
    lv::add_flag(btn, lv::FLAG_CLICKABLE);
    lv::remove_flag(btn, lv::FLAG_SCROLLABLE);
    lv::set_style_bg_opa(btn, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(btn, 0, 0);
    lv::set_style_shadow_width(btn, 0, 0);
    lv::set_style_pad_all(btn, 0, 0);
}

fn chain_style_block(idx: usize, selected: bool, active: bool, accent: lv_color_t) {
    let block = &mut ui().chain[idx];
    if block.btn.is_null() {
        return;
    }

    if block.last_selected == selected
        && block.last_active == active
        && lv::color_eq(block.last_accent, accent)
    {
        return;
    }

    block.last_selected = selected;
    block.last_active = active;
    block.last_accent = accent;

    let opa = if selected || active {
        lv::OPA_COVER
    } else {
        lv::OPA_40
    };
    lv::set_style_opa(block.col, opa, 0);

    let outline = if active {
        accent
    } else {
        lv::color_hex(0x404040)
    };

    lv::set_style_bg_opa(block.col, lv::OPA_TRANSP, 0);
    if selected {
        lv::set_style_border_color(block.col, lv::color_hex(0xFFFFFF), 0);
        lv::set_style_border_width(block.col, CHAIN_TAB_BORDER_SEL, 0);
    } else {
        lv::set_style_border_color(block.col, outline, 0);
        lv::set_style_border_width(block.col, CHAIN_TAB_BORDER_W, 0);
    }
    lv::set_style_border_opa(block.col, lv::OPA_COVER, 0);
    lv::set_style_border_side(block.col, lv::BORDER_SIDE_FULL, 0);
    lv::set_style_border_post(block.col, selected, 0);

    lv::invalidate(block.col);
}

fn chain_slot_to_fx_block(slot: usize) -> u32 {
    match slot {
        CHAIN_PRE1 => NANO_FX_PRE1,
        CHAIN_PRE2 => NANO_FX_PRE2,
        CHAIN_PFX1 => NANO_FX_POST1,
        CHAIN_PFX2 => NANO_FX_POST2,
        CHAIN_PFX3 => NANO_FX_POST3,
        _ => NANO_FX_PRE1,
    }
}

fn chain_slot_is_fx(slot: usize) -> bool {
    matches!(slot, CHAIN_PRE1 | CHAIN_PRE2 | CHAIN_PFX1 | CHAIN_PFX2 | CHAIN_PFX3)
}

fn show_content_for_chain(slot: usize) {
    let u = ui();
    if u.tempo_view {
        u.tempo_view = false;
        unsafe { nano_state_tempo_exit() };
        layout_chrome_for_tuner(false);
    }
    u.tuner_view = false;
    fx_model_picker_close();

    for panel in [u.amp_panel, u.fx_panel, u.tuner_panel, u.tempo_panel] {
        if !panel.is_null() {
            lv::add_flag(panel, lv::FLAG_HIDDEN);
        }
    }

    let shown = match slot {
        CHAIN_AMP => u.amp_panel,
        CHAIN_PRE1 | CHAIN_PRE2 | CHAIN_PFX1 | CHAIN_PFX2 | CHAIN_PFX3 => u.fx_panel,
        _ => ptr::null_mut(),
    };

    if !shown.is_null() {
        lv::remove_flag(shown, lv::FLAG_HIDDEN);
        lv::move_foreground(shown);
    }

    ui_bring_chrome_to_front();
    u.pedal_params_scroll_inited = false;
    if slot == CHAIN_AMP {
        update_pedal_params_grid_scroll(u.amp_params_row, AMP_PEDAL_PARAM_COUNT);
    } else if chain_slot_is_fx(slot) {
        let count = unsafe { fx_state().as_ref() }
            .map(|fx| {
                let b = &fx.blocks[u.fx_selected as usize];
                if b.valid {
                    b.param_count
                } else {
                    0
                }
            })
            .unwrap_or(0);
        update_pedal_params_grid_scroll(u.fx_params_row, count);
    }
    sync_header_model_locked(fx_state());
}

fn layout_chrome_for_tuner(tuner_on: bool) {
    let u = ui();
    if u.content_root.is_null() {
        return;
    }

    if !u.chain_bar.is_null() {
        lv::set_hidden(u.chain_bar, tuner_on);
    }

    let h = if u.body_h > 0 {
        u.body_h
    } else {
        lv::get_height(u.content_root)
    };

    if tuner_on {
        let full_w = lv::hor_res() - 2 * UI_EDGE_PAD;
        lv::set_size(u.content_root, full_w, h);
        lv::align(u.content_root, lv::ALIGN_TOP_LEFT, 0, HEADER_H);
    } else {
        let w = if u.content_w_normal > 0 {
            u.content_w_normal
        } else {
            lv::get_width(u.content_root)
        };
        lv::set_size(u.content_root, w, h);
        lv::align(u.content_root, lv::ALIGN_TOP_LEFT, u.content_x_normal, HEADER_H);
    }
}

fn show_tuner_view(on: bool) {
    let u = ui();
    if on && u.tempo_view {
        u.tempo_view = false;
        unsafe { nano_state_tempo_exit() };
        if !u.tempo_panel.is_null() {
            lv::add_flag(u.tempo_panel, lv::FLAG_HIDDEN);
        }
    }
    u.tuner_view = on;
    if on {
        fx_model_picker_close();
    }
    if !u.tuner_hdr_btn.is_null() {
        if on {
            lv::add_state(u.tuner_hdr_btn, lv::STATE_CHECKED);
        } else {
            lv::remove_state(u.tuner_hdr_btn, lv::STATE_CHECKED);
        }
    }
    for panel in [u.amp_panel, u.fx_panel, u.tempo_panel] {
        if !panel.is_null() {
            lv::add_flag(panel, lv::FLAG_HIDDEN);
        }
    }
    if u.tuner_panel.is_null() {
        return;
    }

    if on {
        lv::remove_flag(u.tuner_panel, lv::FLAG_HIDDEN);
        lv::move_foreground(u.tuner_panel);
        layout_chrome_for_tuner(true);
        sync_header_model_locked(fx_state());
        refresh_preset_label_locked();
        ui_bring_chrome_to_front();
    } else {
        lv::add_flag(u.tuner_panel, lv::FLAG_HIDDEN);
        layout_chrome_for_tuner(false);
        show_content_for_chain(u.chain_selected);
    }
}

fn show_tempo_view(on: bool, write_pedal: bool) {
    let u = ui();
    if on && u.tuner_view {
        u.tuner_view = false;
        if !u.tuner_hdr_btn.is_null() {
            lv::remove_state(u.tuner_hdr_btn, lv::STATE_CHECKED);
        }
        if !u.tuner_panel.is_null() {
            lv::add_flag(u.tuner_panel, lv::FLAG_HIDDEN);
        }
    }

    if on == u.tempo_view {
        return;
    }

    u.tempo_view = on;
    if on {
        fx_model_picker_close();
    }

    for panel in [u.amp_panel, u.fx_panel, u.tuner_panel] {
        if !panel.is_null() {
            lv::add_flag(panel, lv::FLAG_HIDDEN);
        }
    }

    if u.tempo_panel.is_null() {
        return;
    }

    if on {
        lv::remove_flag(u.tempo_panel, lv::FLAG_HIDDEN);
        lv::move_foreground(u.tempo_panel);
        layout_chrome_for_tuner(true);
        sync_header_model_locked(fx_state());
        refresh_preset_label_locked();
        if !u.tempo_tap_btn.is_null() {
            lv::set_enabled(u.tempo_tap_btn, u.ble_connected);
        }
        ui_bring_chrome_to_front();
        if u.ui_bpm_cache_valid {
            apply_bpm_locked(u.ui_bpm_cache, true);
        }
        if write_pedal {
            unsafe { nano_state_tempo_enter() };
        }
        update_chain_blocks_locked(amp_state(), fx_state());
    } else {
        lv::add_flag(u.tempo_panel, lv::FLAG_HIDDEN);
        if write_pedal {
            unsafe { nano_state_tempo_exit() };
        }
        layout_chrome_for_tuner(false);
        show_content_for_chain(u.chain_selected);
        update_chain_blocks_locked(amp_state(), fx_state());
    }
}

fn async_apply_tempo_mode_cb() {
    let u = ui();
    if !u.async_tempo_mode_pending {
        return;
    }
    u.async_tempo_mode_pending = false;
    let on = u.async_tempo_mode;
    if on && !u.tempo_view {
        show_tempo_view(true, false);
    } else if !on && u.tempo_view {
        show_tempo_view(false, false);
    }
}

fn update_chain_blocks_locked(amp: *const NanoAmpC, fx: *const NanoFxState) {
    let amp = unsafe { amp.as_ref() };
    let fx = unsafe { fx.as_ref() };

    for i in 0..CHAIN_BLOCK_COUNT {
        if ui().chain[i].btn.is_null() {
            continue;
        }

        let u = ui();
        let selected = i == u.chain_selected && !u.tuner_view && !u.tempo_view;

        let (active, accent, block_label) = if i == CHAIN_AMP {
            (
                amp.map(|a| a.valid).unwrap_or(false),
                icon_kind_color(IconKind::Amp),
                "AMP".to_string(),
            )
        } else {
            let bid = chain_slot_to_fx_block(i);
            let b = fx.map(|f| &f.blocks[bid as usize]);
            let cat = match b {
                Some(b) if !b.model.is_null() => unsafe { nano_fx_model_category(b.model) },
                _ => NANO_FX_CAT_NONE,
            };
            let icon = category_to_icon(cat);
            let label = match b {
                Some(b) if b.valid => {
                    let mut buf = [0u8; CHAIN_LABEL_MAX];
                    unsafe {
                        nano_fx_model_chain_acronym(
                            b as *const NanoFxBlock,
                            buf.as_mut_ptr() as *mut c_char,
                            buf.len(),
                        )
                    };
                    c_buf_str(&buf).to_string()
                }
                _ => icon_kind_glyph(icon).to_string(),
            };
            (
                b.map(|b| b.valid && b.on).unwrap_or(false),
                icon_kind_color(icon),
                label,
            )
        };

        let label_color = if selected {
            if active {
                accent
            } else {
                lv::color_hex(0x909090)
            }
        } else if active {
            accent
        } else {
            lv::color_hex(0x505050)
        };

        chain_set_block_label(i, &block_label, label_color);
        chain_style_block(i, selected, active || i == CHAIN_AMP, accent);
    }
}

fn chain_toggle_slot(slot: usize) {
    if !ui().ble_connected {
        return;
    }

    if !chain_slot_is_fx(slot) {
        return;
    }

    let Some(fx) = (unsafe { fx_state().as_ref() }) else {
        return;
    };
    let bid = chain_slot_to_fx_block(slot);
    unsafe { nano_fx_write_block_enabled(bid, !fx.blocks[bid as usize].on) };
}

unsafe extern "C" fn chain_block_btn_event_cb(e: *mut lv_event_t) {
    let code = lv::event_code(e);
    let idx = lv::event_user_data(e);
    if idx >= CHAIN_BLOCK_COUNT {
        return;
    }

    if code == lv::EVENT_LONG_PRESSED {
        if idx == CHAIN_AMP {
            return;
        }
        ui().chain_suppress_click = idx as i32;
        chain_toggle_slot(idx);
        update_chain_blocks_locked(amp_state(), fx_state());
        return;
    }

    if code != lv::EVENT_CLICKED {
        return;
    }

    let u = ui();
    if u.chain_suppress_click == idx as i32 {
        u.chain_suppress_click = -1;
        return;
    }

    u.chain_selected = idx;
    if u.tuner_view || u.tempo_view {
        layout_chrome_for_tuner(false);
    }
    u.tuner_view = false;

    if chain_slot_is_fx(idx) {
        u.fx_selected = chain_slot_to_fx_block(idx);
    }

    show_content_for_chain(idx);

    if chain_slot_is_fx(idx) {
        apply_fx_locked(fx_state());
    }

    update_chain_blocks_locked(amp_state(), fx_state());
}

unsafe extern "C" fn tuner_hdr_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    show_tuner_view(!ui().tuner_view);
    sync_header_model_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

/* ---------------------------------------------------------- device writes */

fn try_send_gate_locked(pct_val: u8, force_send: bool) {
    let u = ui();
    if u.fx_ui_sync || !u.ble_connected {
        return;
    }

    let now = now_us();
    if !force_send
        && pct_val as i32 == u.gate_last_sent
        && (now - u.gate_last_send_us) < DRAG_WRITE_INTERVAL_US
    {
        return;
    }

    if unsafe { nano_fx_write_gate_reduction(pct_val) } {
        u.gate_last_sent = pct_val as i32;
        u.gate_last_send_us = now;
    }
}

fn try_send_cap_vol_locked(raw: u8, force_send: bool) {
    let u = ui();
    if u.fx_ui_sync || !u.ble_connected {
        return;
    }

    let now = now_us();
    if !force_send
        && raw as i32 == u.cap_vol_last_sent
        && (now - u.cap_vol_last_send_us) < DRAG_WRITE_INTERVAL_US
    {
        return;
    }

    if unsafe { nano_state_write_capture_volume(raw) } {
        u.cap_vol_last_sent = raw as i32;
        u.cap_vol_last_send_us = now;
    }
}

fn try_send_gate_on_locked(on: bool, force_send: bool) {
    let u = ui();
    let now = now_us();
    let sent = if on { 1 } else { 0 };
    if !force_send
        && sent == u.gate_on_last_sent
        && (now - u.gate_on_last_send_us) < DRAG_WRITE_INTERVAL_US
    {
        return;
    }

    if unsafe { nano_fx_write_gate_enabled(on) } {
        u.gate_on_last_sent = sent;
        u.gate_on_last_send_us = now;
    }
}

fn try_send_knob_locked(idx: usize, raw: u8, force: bool) {
    let u = ui();
    if u.ui_sync || !u.ble_connected || idx >= KNOB_COUNT {
        return;
    }
    if !force && raw == u.last_sent[idx] {
        return;
    }

    let now = now_us();
    if !force && now - u.last_send_us[idx] < DRAG_WRITE_INTERVAL_US {
        return;
    }

    if nano_state_write_amp_knob(idx as u32, raw) {
        u.last_sent[idx] = raw;
        u.last_send_us[idx] = now;
    }
}

fn try_send_fx_param_locked(idx: usize, slider_val: i32, force: bool) {
    let u = ui();
    if u.fx_ui_sync || !u.ble_connected || idx >= NANO_FX_MAX_PARAMS {
        return;
    }

    let Some(fx) = (unsafe { fx_state().as_ref() }) else {
        return;
    };
    let block = &fx.blocks[u.fx_selected as usize];
    if block.model.is_null() || idx >= block.param_count as usize {
        return;
    }

    let pname = unsafe { (*block.model).param_names.add(idx).read() };
    let now = now_us();
    if !force
        && slider_val == u.fx_last_sent[idx]
        && now - u.fx_last_send_us[idx] < DRAG_WRITE_INTERVAL_US
    {
        return;
    }

    let display = unsafe { nano_fx_slider_to_display(pname, slider_val) };
    if unsafe { nano_fx_write_param_display(u.fx_selected, idx as u8, display) } {
        u.fx_last_sent[idx] = slider_val;
        u.fx_last_send_us[idx] = now;
    }
}

/* ----------------------------------------------------------- model picker */

fn fx_block_matches_model(
    block: Option<&NanoFxBlock>,
    model: *const NanoFxModelDef,
) -> bool {
    let Some(block) = block else {
        return false;
    };
    if model.is_null() {
        return false;
    }
    if block.model == model {
        return true;
    }
    let m = unsafe { &*model };
    block.has_model_id
        && m.id_len > 0
        && block.model_id_len == m.id_len
        && block.model_id_raw[..m.id_len as usize] == m.id[..m.id_len as usize]
}

unsafe extern "C" fn fx_model_picker_backdrop_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    fx_model_picker_close();
}

unsafe extern "C" fn fx_model_picker_tile_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }

    let idx = lv::event_user_data(e);
    let u = ui();
    if u.ble_connected {
        if u.header_picker_mode == HEADER_PICKER_AMP {
            unsafe { nano_state_write_capture_slot(idx as u8) };
        } else {
            let model = unsafe { nano_fx_get_allowed_model(u.fx_selected, idx) };
            if !model.is_null() {
                unsafe { nano_fx_write_block_model(u.fx_selected, model) };
            }
        }
    }
    fx_model_picker_close();
}

fn fx_model_picker_lock_page_scroll(lock: bool) {
    if !lock {
        return;
    }
    let u = ui();
    let screen = lv::screen_active();
    if !screen.is_null() {
        lv::remove_flag(screen, lv::FLAG_SCROLLABLE);
    }
    for obj in [u.content_root, u.fx_params_row, u.amp_params_row] {
        if !obj.is_null() {
            lv::remove_flag(obj, lv::FLAG_SCROLLABLE);
        }
    }
}

fn model_picker_add_tile(
    parent: *mut lv_obj_t,
    tile_w: i32,
    name: &str,
    picker_idx: usize,
    selected: bool,
) {
    let tile = lv::button_create(parent);
    lv::set_size(tile, tile_w, FX_MODEL_PICKER_TILE_H);
    lv::set_style_bg_color(
        tile,
        lv::color_hex(if selected { 0x1A3040 } else { 0x141414 }),
        0,
    );
    lv::set_style_bg_opa(tile, lv::OPA_COVER, 0);
    lv::set_style_border_color(
        tile,
        lv::color_hex(if selected { 0xFFFFFF } else { 0x383838 }),
        0,
    );
    lv::set_style_border_width(tile, if selected { 2 } else { 1 }, 0);
    lv::set_style_radius(tile, 8, 0);
    lv::set_style_shadow_width(tile, 0, 0);
    lv::add_event_cb(
        tile,
        fx_model_picker_tile_event_cb,
        lv::EVENT_CLICKED,
        picker_idx,
    );

    let lbl = lv::label_create(tile);
    lv::label_set_text(lbl, name);
    lv::set_width(lbl, tile_w - 16);
    lv::label_set_long_mode(lbl, lv::LABEL_LONG_WRAP);
    lv::set_style_text_font(lbl, lv::font_sm(), 0);
    lv::set_style_text_color(
        lbl,
        lv::color_hex(if selected { 0xFFFFFF } else { 0xD8D8D8 }),
        0,
    );
    lv::set_style_text_align(lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::center(lbl);
}

fn picker_grid_tile_width(grid: *mut lv_obj_t) -> (i32, i32) {
    let mut grid_w = lv::get_content_width(grid);
    if grid_w < 80 {
        grid_w = lv::hor_res() - 4 * FX_MODEL_PICKER_PAD;
    }
    grid_w -= FX_MODEL_PICKER_SCROLL_PAD;
    let gaps = (FX_MODEL_PICKER_COLS - 1) * FX_MODEL_PICKER_GAP;
    let tile_w = ((grid_w - gaps) / FX_MODEL_PICKER_COLS).max(64);
    (grid_w, tile_w)
}

fn picker_section_container(grid: *mut lv_obj_t, grid_w: i32) -> *mut lv_obj_t {
    let tiles = lv::obj_create(grid);
    lv::set_width(tiles, grid_w);
    lv::set_height(tiles, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(tiles, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(tiles, 0, 0);
    lv::set_style_pad_all(tiles, 0, 0);
    lv::set_style_pad_row(tiles, FX_MODEL_PICKER_GAP, 0);
    lv::set_style_pad_column(tiles, FX_MODEL_PICKER_GAP, 0);
    lv::set_flex_flow(tiles, lv::FLEX_FLOW_ROW_WRAP);
    lv::set_flex_align(
        tiles,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
    );
    ui_no_scroll_only(tiles);
    tiles
}

fn picker_section_header(grid: *mut lv_obj_t, grid_w: i32, text: &str, first: bool) {
    let hdr = lv::label_create(grid);
    lv::label_set_text(hdr, text);
    lv::set_width(hdr, grid_w);
    lv::set_style_text_font(hdr, lv::font_md(), 0);
    lv::set_style_text_color(hdr, lv::color_hex(0x909090), 0);
    lv::set_style_pad_top(hdr, if first { 0 } else { 10 }, 0);
    lv::set_style_pad_bottom(hdr, 4, 0);
}

fn amp_model_picker_populate() {
    let grid = ui().fx_model_picker_grid;
    if grid.is_null() {
        return;
    }

    const BANK_ORDER: [u32; 5] = [
        crate::amp::NANO_AMP_BANK_CLEAN,
        crate::amp::NANO_AMP_BANK_2,
        crate::amp::NANO_AMP_BANK_3,
        crate::amp::NANO_AMP_BANK_HIGH_GAIN,
        crate::amp::NANO_AMP_BANK_BASS,
    ];

    lv::obj_clean(grid);
    lv::scroll_to(grid, 0, 0, lv::ANIM_OFF);

    let selected_slot = unsafe { amp_state().as_ref() }
        .filter(|a| a.have_capture_slot)
        .map(|a| a.capture_slot)
        .unwrap_or(0);

    let (grid_w, tile_w) = picker_grid_tile_width(grid);

    let catalog = crate::amp::catalog();
    let mut first = true;
    for bank in BANK_ORDER {
        if !catalog.iter().any(|c| c.bank == bank) {
            continue;
        }

        picker_section_header(
            grid,
            grid_w,
            c_str(crate::amp::nano_amp_bank_label(bank)),
            first,
        );
        first = false;

        let tiles = picker_section_container(grid, grid_w);
        for capture in catalog {
            if capture.bank != bank || capture.name.is_null() {
                continue;
            }
            let selected = selected_slot > 0 && capture.slot == selected_slot;
            model_picker_add_tile(
                tiles,
                tile_w,
                c_str(capture.name),
                capture.slot as usize,
                selected,
            );
        }
    }

    fx_model_picker_update_scrollbar();
}

fn fx_model_picker_populate() {
    let u = ui();
    let grid = u.fx_model_picker_grid;
    if grid.is_null() {
        return;
    }

    const CAT_ORDER: [u32; 10] = [
        NANO_FX_CAT_GUITAR_OD,
        NANO_FX_CAT_BASS_OD,
        NANO_FX_CAT_WAH,
        NANO_FX_CAT_FILTER,
        NANO_FX_CAT_COMPRESSOR,
        NANO_FX_CAT_PITCH,
        NANO_FX_CAT_MODULATION,
        NANO_FX_CAT_DELAY,
        NANO_FX_CAT_REVERB,
        NANO_FX_CAT_UTILITY,
    ];

    lv::obj_clean(grid);
    lv::scroll_to(grid, 0, 0, lv::ANIM_OFF);

    let fx = unsafe { fx_state().as_ref() };
    let cur = fx.map(|f| &f.blocks[u.fx_selected as usize]);
    let count = unsafe { nano_fx_allowed_model_count(u.fx_selected) };

    let (grid_w, tile_w) = picker_grid_tile_width(grid);

    let allowed = |i: usize| unsafe { nano_fx_get_allowed_model(u.fx_selected, i) };
    let mut first = true;

    for cat in CAT_ORDER {
        let has_models = (0..count).any(|i| {
            let m = allowed(i);
            !m.is_null()
                && !unsafe { (*m).name }.is_null()
                && unsafe { nano_fx_model_category(m) } == cat
        });
        if !has_models {
            continue;
        }

        picker_section_header(
            grid,
            grid_w,
            c_str(unsafe { nano_fx_category_label(cat) }),
            first,
        );
        first = false;

        let tiles = picker_section_container(grid, grid_w);
        for i in 0..count {
            let model = allowed(i);
            if model.is_null()
                || unsafe { (*model).name }.is_null()
                || unsafe { nano_fx_model_category(model) } != cat
            {
                continue;
            }
            let selected = fx_block_matches_model(cur, model);
            model_picker_add_tile(tiles, tile_w, c_str(unsafe { (*model).name }), i, selected);
        }
    }

    fx_model_picker_update_scrollbar();
}

fn fx_model_picker_close() {
    let u = ui();
    if u.fx_model_picker.is_null() || !u.fx_model_picker_open {
        return;
    }
    u.fx_model_picker_open = false;
    lv::add_flag(u.fx_model_picker, lv::FLAG_HIDDEN);
    fx_model_picker_lock_page_scroll(false);

    let block_params = unsafe { fx_state().as_ref() }.and_then(|fx| {
        let b = &fx.blocks[u.fx_selected as usize];
        if b.valid {
            Some(b.param_count)
        } else {
            None
        }
    });

    if let Some(count) = block_params {
        if fx_panel_is_visible() {
            u.pedal_params_scroll_inited = false;
            update_pedal_params_grid_scroll(u.fx_params_row, count);
            return;
        }
    }
    if u.chain_selected == CHAIN_AMP {
        u.pedal_params_scroll_inited = false;
        update_pedal_params_grid_scroll(u.amp_params_row, AMP_PEDAL_PARAM_COUNT);
    }
}

fn fx_model_picker_show() {
    let u = ui();
    if u.fx_model_picker.is_null() || u.fx_model_btn.is_null() {
        return;
    }
    if lv::has_state(u.fx_model_btn, lv::STATE_DISABLED) {
        return;
    }

    if u.chain_selected == CHAIN_AMP {
        u.header_picker_mode = HEADER_PICKER_AMP;
        amp_model_picker_populate();
    } else if chain_slot_is_fx(u.chain_selected) {
        u.header_picker_mode = HEADER_PICKER_FX;
        fx_model_picker_populate();
    } else {
        return;
    }

    u.fx_model_picker_open = true;
    fx_model_picker_lock_page_scroll(true);
    lv::remove_flag(u.fx_model_picker, lv::FLAG_HIDDEN);
    ui_bring_chrome_to_front();
}

unsafe extern "C" fn fx_model_header_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    if ui().fx_model_picker_open {
        fx_model_picker_close();
        return;
    }
    fx_model_picker_show();
}

fn build_fx_model_picker(screen: *mut lv_obj_t) {
    let u = ui();

    let picker = lv::obj_create(screen);
    u.fx_model_picker = picker;
    lv::set_size(picker, lv::pct(100), lv::ver_res() - HEADER_H);
    lv::align(picker, lv::ALIGN_TOP_MID, 0, HEADER_H);
    lv::set_style_bg_color(picker, lv::color_hex(0x000000), 0);
    lv::set_style_bg_opa(picker, lv::OPA_70, 0);
    lv::set_style_border_width(picker, 0, 0);
    lv::set_style_pad_all(picker, FX_MODEL_PICKER_PAD, 0);
    lv::add_flag(picker, lv::FLAG_CLICKABLE);
    lv::add_event_cb(
        picker,
        fx_model_picker_backdrop_event_cb,
        lv::EVENT_CLICKED,
        0,
    );
    lv::add_flag(picker, lv::FLAG_HIDDEN);
    ui_no_scroll_only(picker);
    lv::remove_flag(picker, lv::FLAG_SCROLL_CHAIN);

    let panel = lv::obj_create(picker);
    lv::set_size(panel, lv::pct(98), lv::pct(96));
    lv::center(panel);
    lv::set_style_bg_color(panel, lv::color_hex(0x0A0A0A), 0);
    lv::set_style_bg_opa(panel, lv::OPA_COVER, 0);
    lv::set_style_border_width(panel, 1, 0);
    lv::set_style_border_color(panel, lv::color_hex(0x303030), 0);
    lv::set_style_radius(panel, 8, 0);
    lv::set_style_pad_all(panel, FX_MODEL_PICKER_PAD, 0);
    lv::remove_flag(panel, lv::FLAG_CLICKABLE);
    ui_no_scroll_only(panel);
    lv::remove_flag(panel, lv::FLAG_SCROLL_CHAIN);

    let grid = lv::obj_create(panel);
    u.fx_model_picker_grid = grid;
    lv::set_size(grid, lv::pct(100), lv::pct(100));
    lv::set_style_bg_opa(grid, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(grid, 0, 0);
    lv::set_style_pad_all(grid, 0, 0);
    lv::set_style_pad_right(grid, FX_MODEL_PICKER_SCROLL_PAD, 0);
    lv::set_style_pad_row(grid, FX_MODEL_PICKER_GAP, 0);
    lv::set_flex_flow(grid, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        grid,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
    );
    lv::set_scroll_dir(grid, lv::DIR_VER);
    lv::add_flag(grid, lv::FLAG_SCROLLABLE);
    lv::remove_flag(grid, lv::FLAG_SCROLL_CHAIN);
    style_grid_scrollbar(grid, false);
}

/* --------------------------------------------------------- param plumbing */

fn fx_short_param_name(full: &str) -> String {
    if full == "Feedback Depth" {
        return "Fbk Depth".to_string();
    }

    let cut = full.split('(').next().unwrap_or(full).trim_end_matches(' ');
    let n = cut.len().min(PARAM_LABEL_MAX_LEN);
    if n == 0 {
        return full[..full.len().min(PARAM_LABEL_MAX_LEN)].to_string();
    }
    cut[..n].to_string()
}

fn param_label_width() -> i32 {
    ui().param_cell_w - 2 * PARAM_CELL_PAD
}

fn style_param_name_label(lbl: *mut lv_obj_t) {
    lv::set_width(lbl, param_label_width());
    lv::label_set_long_mode(lbl, lv::LABEL_LONG_DOT);
    lv::set_style_text_align(lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::set_style_text_color(lbl, lv::color_hex(0xB0B0B0), 0);
    lv::set_style_text_font(lbl, lv::font_md(), 0);
}

fn style_param_value_label(lbl: *mut lv_obj_t) {
    lv::set_width(lbl, param_label_width());
    lv::set_style_text_align(lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::set_style_text_color(lbl, lv::color_hex(0xFFFFFF), 0);
    lv::set_style_text_font(lbl, lv::font_md(), 0);
}

fn style_grid_scrollbar(obj: *mut lv_obj_t, active: bool) {
    if obj.is_null() {
        return;
    }

    lv::set_scrollbar_mode(obj, lv::SCROLLBAR_MODE_ON);
    lv::set_style_width(obj, 6, lv::PART_SCROLLBAR);
    lv::set_style_pad_all(obj, 0, lv::PART_SCROLLBAR);
    lv::set_style_radius(obj, 3, lv::PART_SCROLLBAR);
    lv::set_style_bg_color(
        obj,
        lv::color_hex(if active { 0x686868 } else { 0x252525 }),
        lv::PART_SCROLLBAR,
    );
    lv::set_style_bg_opa(
        obj,
        if active { lv::OPA_COVER } else { lv::OPA_30 },
        lv::PART_SCROLLBAR,
    );
}

fn scroll_area_has_overflow(obj: *mut lv_obj_t) -> bool {
    if obj.is_null() {
        return false;
    }
    lv::update_layout(obj);
    lv::get_scroll_bottom(obj) > 0
}

fn fx_model_picker_update_scrollbar() {
    let grid = ui().fx_model_picker_grid;
    if grid.is_null() {
        return;
    }
    style_grid_scrollbar(grid, scroll_area_has_overflow(grid));
}

fn update_pedal_params_grid_scroll(row: *mut lv_obj_t, param_count: u8) {
    if row.is_null() {
        return;
    }

    let u = ui();
    let overflow = param_count > PEDAL_PARAM_GRID_VISIBLE;
    if u.pedal_params_scroll_inited
        && u.pedal_params_scroll_row == row
        && overflow == u.pedal_params_scroll_on
    {
        return;
    }

    u.pedal_params_scroll_row = row;
    u.pedal_params_scroll_on = overflow;
    u.pedal_params_scroll_inited = true;

    ui_scroll_container(row, lv::DIR_VER);
    style_grid_scrollbar(row, overflow);
    if !overflow {
        lv::scroll_to(row, 0, 0, lv::ANIM_OFF);
    }
}

fn pedal_params_row_active() -> *mut lv_obj_t {
    let u = ui();
    if u.chain_selected == CHAIN_AMP {
        return u.amp_params_row;
    }
    if chain_slot_is_fx(u.chain_selected) {
        return u.fx_params_row;
    }
    ptr::null_mut()
}

fn pedal_params_can_scroll() -> bool {
    let u = ui();
    let row = pedal_params_row_active();
    !row.is_null() && u.pedal_params_scroll_on && u.pedal_params_scroll_row == row
}

fn style_param_grid_container(obj: *mut lv_obj_t) {
    lv::set_style_pad_all(obj, PARAM_GRID_PAD, 0);
    lv::set_style_pad_row(obj, PARAM_GRID_GAP, 0);
    lv::set_style_pad_column(obj, PARAM_GRID_GAP, 0);
    lv::set_flex_flow(obj, lv::FLEX_FLOW_ROW_WRAP);
    lv::set_flex_align(
        obj,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
    );
}

fn create_pedal_params_grid(panel: *mut lv_obj_t) -> *mut lv_obj_t {
    lv::set_size(panel, lv::pct(100), lv::pct(100));
    lv::set_style_bg_opa(panel, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(panel, 0, 0);
    lv::set_style_pad_all(panel, 0, 0);
    ui_passive_container(panel);

    let row = lv::obj_create(panel);
    lv::set_size(row, lv::pct(100), lv::pct(100));
    lv::set_style_bg_opa(row, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(row, 0, 0);
    style_param_grid_container(row);
    ui_scroll_container(row, lv::DIR_VER);
    style_grid_scrollbar(row, false);
    row
}

fn style_param_cell(cell: *mut lv_obj_t, w: i32, h: i32) {
    lv::set_size(cell, w, h);
    lv::set_style_bg_color(cell, lv::color_hex(0x1A1A1A), 0);
    lv::set_style_bg_opa(cell, lv::OPA_COVER, 0);
    lv::set_style_border_color(cell, lv::color_hex(0x404040), 0);
    lv::set_style_border_width(cell, 1, 0);
    lv::set_style_radius(cell, 4, 0);
    lv::set_style_pad_all(cell, PARAM_CELL_PAD, 0);
    lv::set_style_pad_row(cell, PARAM_CELL_ROW_GAP, 0);
}

fn style_compact_h_slider(slider: *mut lv_obj_t, width: i32, min_v: i32, max_v: i32) {
    lv::set_width(slider, width);
    lv::set_height(slider, 18);
    lv::slider_set_range(slider, min_v, max_v);
    lv::set_style_bg_color(slider, lv::color_hex(0x1E1E1E), lv::PART_MAIN);
    lv::set_style_bg_opa(slider, lv::OPA_COVER, lv::PART_MAIN);
    lv::set_style_border_color(slider, lv::color_hex(0x707070), lv::PART_MAIN);
    lv::set_style_border_width(slider, 1, lv::PART_MAIN);
    lv::set_style_radius(slider, 3, lv::PART_MAIN);
    lv::set_style_bg_color(slider, lv::color_hex(0xB8C0C8), lv::PART_INDICATOR);
    lv::set_style_radius(slider, 2, lv::PART_INDICATOR);
    lv::set_style_pad_all(slider, 0, lv::PART_KNOB);
    lv::set_style_width(slider, 0, lv::PART_KNOB);
    lv::set_style_height(slider, 0, lv::PART_KNOB);
    lv::set_style_bg_opa(slider, lv::OPA_TRANSP, lv::PART_KNOB);
    lv::set_style_border_width(slider, 0, lv::PART_KNOB);
    lv::remove_flag(slider, lv::FLAG_CLICKABLE);
}

fn param_slider_width() -> i32 {
    (ui().param_cell_w - 2 * PARAM_CELL_PAD - 2 * PARAM_SLIDER_INSET_H).max(48)
}

fn param_cell_x_to_slider(cell: *mut lv_obj_t, slider: *mut lv_obj_t, abs_x: i32) -> i32 {
    let area = lv::get_coords(cell);
    let mut rel_x = abs_x - area.x1 - PARAM_CELL_PAD - PARAM_SLIDER_INSET_H;
    let track_w =
        (ui().param_cell_w - 2 * PARAM_CELL_PAD - 2 * PARAM_SLIDER_INSET_H).max(1);
    rel_x = rel_x.clamp(0, track_w);

    let min_v = lv::slider_get_min_value(slider);
    let max_v = lv::slider_get_max_value(slider);
    if max_v <= min_v {
        return min_v;
    }

    min_v + rel_x * (max_v - min_v) / track_w
}

fn param_cell_value_from_x(
    cell: *mut lv_obj_t,
    slider: *mut lv_obj_t,
    abs_x: i32,
    binary_half: bool,
) -> i32 {
    if !binary_half {
        return param_cell_x_to_slider(cell, slider, abs_x);
    }

    let area = lv::get_coords(cell);
    let rel_x = abs_x - area.x1;
    let cell_w = lv::area_width(&area).max(1);

    let min_v = lv::slider_get_min_value(slider);
    let max_v = lv::slider_get_max_value(slider);
    if rel_x >= cell_w / 2 {
        max_v
    } else {
        min_v
    }
}

fn param_cell_apply_gate_on(cell: *mut lv_obj_t, force_send: bool) {
    let u = ui();
    if u.gate_on.slider.is_null() {
        return;
    }
    let Some(pt) = lv::indev_point() else {
        return;
    };

    let val = param_cell_value_from_x(cell, u.gate_on.slider, pt.x, true);
    lv::slider_set_value(u.gate_on.slider, val, lv::ANIM_OFF);
    lv::label_set_text(u.gate_on.value, if val != 0 { "On" } else { "Off" });

    if u.gate_on_drag != 0 || force_send {
        try_send_gate_on_locked(val != 0, force_send);
    }
}

fn param_cell_apply_gate(cell: *mut lv_obj_t, force_send: bool) {
    let u = ui();
    if u.gate.slider.is_null() {
        return;
    }
    let Some(pt) = lv::indev_point() else {
        return;
    };

    let val = param_cell_x_to_slider(cell, u.gate.slider, pt.x);
    lv::slider_set_value(u.gate.slider, val, lv::ANIM_OFF);
    lv::label_set_text(u.gate.value, &format!("{val}%"));

    if u.gate_drag != 0 {
        try_send_gate_locked(val as u8, force_send);
    }
}

fn param_cell_apply_cap_vol(cell: *mut lv_obj_t, force_send: bool) {
    let u = ui();
    if u.cap_vol.slider.is_null() {
        return;
    }
    let Some(pt) = lv::indev_point() else {
        return;
    };

    let val = param_cell_x_to_slider(cell, u.cap_vol.slider, pt.x);
    lv::slider_set_value(u.cap_vol.slider, val, lv::ANIM_OFF);
    lv::label_set_text(u.cap_vol.value, &format!("{}%", pct(val as u8)));

    if u.cap_vol_drag != 0 || force_send {
        try_send_cap_vol_locked(val as u8, force_send);
    }
}

fn param_cell_apply_slider(
    cell: *mut lv_obj_t,
    slider: *mut lv_obj_t,
    is_amp: bool,
    idx: usize,
    force_send: bool,
) {
    let Some(pt) = lv::indev_point() else {
        return;
    };
    let u = ui();

    let mut binary_half = false;
    if !is_amp {
        if let Some(fx) = unsafe { fx_state().as_ref() } {
            let block = &fx.blocks[u.fx_selected as usize];
            if !block.model.is_null() && idx < block.param_count as usize {
                let pname = unsafe { (*block.model).param_names.add(idx).read() };
                if unsafe { nano_fx_param_is_enum(pname) }
                    && unsafe { nano_fx_param_enum_count(pname) } == 2
                {
                    binary_half = true;
                }
            }
        }
    }

    let val = param_cell_value_from_x(cell, slider, pt.x, binary_half);
    lv::slider_set_value(slider, val, lv::ANIM_OFF);

    if is_amp {
        lv::label_set_text(u.knobs[idx].value, &format!("{}%", pct(val as u8)));
        if u.drag_knob == idx as i32 {
            try_send_knob_locked(idx, val as u8, force_send);
        }
        return;
    }

    if let Some(fx) = unsafe { fx_state().as_ref() } {
        let block = &fx.blocks[u.fx_selected as usize];
        if !block.model.is_null() && idx < block.param_count as usize {
            let pname = unsafe { (*block.model).param_names.add(idx).read() };
            let mut tmp = *block;
            tmp.params[idx].display = unsafe { nano_fx_slider_to_display(pname, val) };
            tmp.params[idx].valid = true;
            let mut buf = [0u8; 24];
            unsafe {
                nano_fx_format_param(
                    &tmp,
                    idx as u8,
                    buf.as_mut_ptr() as *mut c_char,
                    buf.len(),
                )
            };
            lv::label_set_text(u.fx_params[idx].value, c_buf_str(&buf));
        }
    }

    if u.fx_drag_param == idx as i32 {
        try_send_fx_param_locked(idx, val, force_send);
    }
}

/* --------------------------------------------------------------- gestures */

fn param_gesture_begin() {
    let u = ui();
    let Some(pt) = lv::indev_point() else {
        u.param_gesture.active = false;
        u.param_gesture.mode = PARAM_GESTURE_NONE;
        return;
    };

    u.param_gesture.active = true;
    u.param_gesture.mode = PARAM_GESTURE_PENDING;
    u.param_gesture.press_us = now_us();
    u.param_gesture.origin = pt;
}

fn param_gesture_cancel_drag() {
    let u = ui();
    u.fx_drag_param = -1;
    u.drag_knob = -1;
    u.gate_drag = 0;
    u.gate_on_drag = 0;
    u.cap_vol_drag = 0;
}

fn param_gesture_motion() -> (i32, i32) {
    let u = ui();
    if !u.param_gesture.active {
        return (0, 0);
    }
    let Some(pt) = lv::indev_point() else {
        return (0, 0);
    };
    (
        (pt.x - u.param_gesture.origin.x).abs(),
        (pt.y - u.param_gesture.origin.y).abs(),
    )
}

fn param_gesture_update() -> u8 {
    let u = ui();
    if !u.param_gesture.active {
        return PARAM_GESTURE_NONE;
    }

    let (adx, ady) = param_gesture_motion();

    if u.param_gesture.mode == PARAM_GESTURE_SCROLL {
        return PARAM_GESTURE_SCROLL;
    }

    if u.param_gesture.mode == PARAM_GESTURE_ADJUST {
        if pedal_params_can_scroll() && ady > adx + 4 {
            u.param_gesture.mode = PARAM_GESTURE_SCROLL;
            param_gesture_cancel_drag();
            return PARAM_GESTURE_SCROLL;
        }
        return PARAM_GESTURE_ADJUST;
    }

    let settled = (now_us() - u.param_gesture.press_us) >= PARAM_GESTURE_SETTLE_MS * 1000;

    if pedal_params_can_scroll()
        && !pedal_params_row_active().is_null()
        && ady >= PARAM_GESTURE_SCROLL_MIN_DY
        && ady > adx
    {
        u.param_gesture.mode = PARAM_GESTURE_SCROLL;
        param_gesture_cancel_drag();
        return PARAM_GESTURE_SCROLL;
    }

    if adx >= PARAM_GESTURE_MOVE_THRESH && adx > ady + 6 {
        u.param_gesture.mode = PARAM_GESTURE_ADJUST;
        return PARAM_GESTURE_ADJUST;
    }

    if settled && adx >= PARAM_GESTURE_MOVE_THRESH && adx >= ady {
        u.param_gesture.mode = PARAM_GESTURE_ADJUST;
        return PARAM_GESTURE_ADJUST;
    }

    PARAM_GESTURE_PENDING
}

fn param_gesture_should_apply_on_release() -> bool {
    let mode = param_gesture_update();
    let (adx, ady) = param_gesture_motion();

    if mode == PARAM_GESTURE_SCROLL {
        return false;
    }
    if mode == PARAM_GESTURE_ADJUST {
        return true;
    }
    if adx >= PARAM_GESTURE_TAP_THRESH || ady >= PARAM_GESTURE_TAP_THRESH {
        return false;
    }
    if pedal_params_can_scroll() && ady > adx {
        return false;
    }
    true
}

fn param_gesture_end() {
    let u = ui();
    u.param_gesture.active = false;
    u.param_gesture.mode = PARAM_GESTURE_NONE;
}

unsafe extern "C" fn param_cell_event_cb(e: *mut lv_event_t) {
    let packed = lv::event_user_data(e);
    let cell = lv::event_current_target(e);
    let code = lv::event_code(e);
    let u = ui();

    if packed & PARAM_CELL_GATE_ON_TAG != 0 {
        if u.gate_on.slider.is_null() || lv::has_flag(u.gate_on.slider, lv::FLAG_HIDDEN) {
            return;
        }
        if code == lv::EVENT_PRESSED {
            param_gesture_begin();
        } else if code == lv::EVENT_PRESSING {
            let mode = param_gesture_update();
            if mode == PARAM_GESTURE_ADJUST {
                u.gate_on_drag = 1;
                param_cell_apply_gate_on(cell, false);
            }
        } else if code == lv::EVENT_RELEASED {
            // Binary half-tap: apply even without a drag (ADJUST) gesture.
            if param_gesture_update() != PARAM_GESTURE_SCROLL {
                param_cell_apply_gate_on(cell, true);
            }
            u.gate_on_drag = 0;
            param_gesture_end();
        }
        return;
    }

    if packed & PARAM_CELL_GATE_TAG != 0 {
        if u.gate.slider.is_null() || lv::has_flag(u.gate.slider, lv::FLAG_HIDDEN) {
            return;
        }
        if code == lv::EVENT_PRESSED {
            param_gesture_begin();
        } else if code == lv::EVENT_PRESSING {
            let mode = param_gesture_update();
            if mode == PARAM_GESTURE_ADJUST {
                u.gate_drag = 1;
                param_cell_apply_gate(cell, false);
            }
        } else if code == lv::EVENT_RELEASED {
            // Same as amp/fx params: arm drag so apply_gate can write on tap.
            if param_gesture_should_apply_on_release() {
                u.gate_drag = 1;
                param_cell_apply_gate(cell, true);
            }
            u.gate_drag = 0;
            param_gesture_end();
        }
        return;
    }

    if packed & PARAM_CELL_CAP_VOL_TAG != 0 {
        if u.cap_vol.slider.is_null() || lv::has_flag(u.cap_vol.slider, lv::FLAG_HIDDEN) {
            return;
        }
        if code == lv::EVENT_PRESSED {
            param_gesture_begin();
        } else if code == lv::EVENT_PRESSING {
            let mode = param_gesture_update();
            if mode == PARAM_GESTURE_ADJUST {
                u.cap_vol_drag = 1;
                param_cell_apply_cap_vol(cell, false);
            }
        } else if code == lv::EVENT_RELEASED {
            if param_gesture_should_apply_on_release() {
                u.cap_vol_drag = 1;
                param_cell_apply_cap_vol(cell, true);
            }
            u.cap_vol_drag = 0;
            param_gesture_end();
        }
        return;
    }

    let idx = packed & 0xFF;
    let is_amp = packed & PARAM_CELL_AMP_TAG != 0;
    if (is_amp && idx >= KNOB_COUNT) || (!is_amp && idx >= NANO_FX_MAX_PARAMS) {
        return;
    }

    let slider = if is_amp {
        u.knobs[idx].slider
    } else {
        u.fx_params[idx].slider
    };
    if slider.is_null() || lv::has_flag(slider, lv::FLAG_HIDDEN) {
        return;
    }

    if code == lv::EVENT_PRESSED {
        param_gesture_begin();
        return;
    }

    if code == lv::EVENT_PRESSING {
        if param_gesture_update() == PARAM_GESTURE_ADJUST {
            if is_amp {
                u.drag_knob = idx as i32;
            } else {
                u.fx_drag_param = idx as i32;
            }
            param_cell_apply_slider(cell, slider, is_amp, idx, false);
        }
        return;
    }

    if code == lv::EVENT_RELEASED {
        if param_gesture_should_apply_on_release() {
            if is_amp {
                u.drag_knob = idx as i32;
            } else {
                u.fx_drag_param = idx as i32;
            }
            param_cell_apply_slider(cell, slider, is_amp, idx, true);
        }
        u.drag_knob = -1;
        u.fx_drag_param = -1;
        param_gesture_end();
    }
}

/* ----------------------------------------------------------------- header */

fn sync_header_model_locked(fx: *const NanoFxState) {
    let u = ui();
    if u.fx_model_btn.is_null() || u.fx_model_btn_lbl.is_null() {
        return;
    }

    if u.tuner_view {
        set_label_if_changed(u.fx_model_btn_lbl, "Tuner");
        lv::remove_state(u.fx_model_btn, lv::STATE_DISABLED);
        lv::remove_flag(u.fx_model_btn, lv::FLAG_CLICKABLE);
        lv::set_style_bg_color(u.fx_model_btn, lv::color_hex(0x080808), lv::PART_MAIN);
        lv::set_style_bg_opa(u.fx_model_btn, lv::OPA_COVER, lv::PART_MAIN);
        lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0xECECEC), 0);
        return;
    }

    lv::add_flag(u.fx_model_btn, lv::FLAG_CLICKABLE);
    lv::set_style_bg_color(u.fx_model_btn, lv::color_hex(0x080808), lv::PART_MAIN);
    lv::set_style_bg_opa(u.fx_model_btn, lv::OPA_COVER, lv::PART_MAIN);
    lv::set_style_bg_opa(
        u.fx_model_btn,
        lv::OPA_40,
        lv::PART_MAIN | lv::SEL_DISABLED,
    );

    let mut enabled = false;
    let mut text = String::from("-");

    if u.chain_selected == CHAIN_AMP {
        let amp = amp_state();
        if unsafe { amp.as_ref() }.map(|a| a.valid).unwrap_or(false) {
            let mut buf = [0u8; 56];
            unsafe {
                crate::amp::nano_amp_capture_header_text(
                    amp as *const nano_wire::ffi::NanoAmpC,
                    buf.as_mut_ptr() as *mut c_char,
                    buf.len(),
                )
            };
            text = c_buf_str(&buf).to_string();
            enabled = u.ble_connected;
        } else {
            text = "Amp".to_string();
        }
    } else if chain_slot_is_fx(u.chain_selected) {
        if let Some(fx) = unsafe { fx.as_ref() } {
            let block = &fx.blocks[u.fx_selected as usize];
            if block.valid {
                let mut buf = [0u8; 56];
                unsafe {
                    nano_fx_block_model_text(
                        block as *const NanoFxBlock,
                        buf.as_mut_ptr() as *mut c_char,
                        buf.len(),
                    )
                };
                text = c_buf_str(&buf).to_string();
                enabled = u.ble_connected;
            }
        }
    }

    set_label_if_changed(u.fx_model_btn_lbl, &text);

    if enabled {
        lv::remove_state(u.fx_model_btn, lv::STATE_DISABLED);
        lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0xECECEC), 0);
    } else {
        lv::add_state(u.fx_model_btn, lv::STATE_DISABLED);
        lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0x606060), 0);
    }
}

/* -------------------------------------------------------------- FX panel */

fn apply_fx_param_locked(i: usize, block: &NanoFxBlock) {
    if i >= NANO_FX_MAX_PARAMS || block.model.is_null() {
        return;
    }

    let u = ui();
    let param = u.fx_params[i];
    if param.col.is_null() {
        return;
    }

    if i >= block.param_count as usize || block.param_count == 0 {
        lv::add_flag(param.col, lv::FLAG_HIDDEN);
        return;
    }

    lv::remove_flag(param.col, lv::FLAG_HIDDEN);

    let pname = unsafe { (*block.model).param_names.add(i).read() };
    set_label_if_changed(param.name, &fx_short_param_name(c_str(pname)));

    let display = if block.params[i].valid {
        block.params[i].display
    } else {
        let mid = (unsafe { nano_fx_slider_min(pname) }
            + unsafe { nano_fx_slider_max(pname) })
            / 2;
        unsafe { nano_fx_slider_to_display(pname, mid) }
    };

    let smin = unsafe { nano_fx_slider_min(pname) };
    let smax = unsafe { nano_fx_slider_max(pname) };
    if lv::slider_get_min_value(param.slider) != smin
        || lv::slider_get_max_value(param.slider) != smax
    {
        lv::slider_set_range(param.slider, smin, smax);
    }

    if i as i32 != u.fx_drag_param {
        let slider_val = unsafe { nano_fx_display_to_slider(pname, display) };
        set_slider_if_changed(param.slider, slider_val);

        let mut tmp = *block;
        if !tmp.params[i].valid {
            tmp.params[i].display = display;
            tmp.params[i].valid = true;
        }
        let mut buf = [0u8; 24];
        unsafe {
            nano_fx_format_param(&tmp, i as u8, buf.as_mut_ptr() as *mut c_char, buf.len())
        };
        set_label_if_changed(param.value, c_buf_str(&buf));
        u.fx_last_sent[i] = slider_val;
    }

    lv::set_enabled(
        param.slider,
        u.ble_connected && block.on && block.param_count > 0,
    );
}

fn apply_fx_locked(fx: *const NanoFxState) {
    let u = ui();
    u.fx_ui_sync = true;

    let fx_ref = unsafe { fx.as_ref() };

    if !u.gate_on.col.is_null() || !u.gate.col.is_null() {
        if !u.gate_on.slider.is_null() {
            lv::set_enabled(u.gate_on.slider, u.ble_connected);
        }
        if !u.gate.slider.is_null() {
            lv::set_enabled(u.gate.slider, u.ble_connected);
        }

        // Keep gate cells at full opacity — ON/Off is shown by the switch itself.
        // Dimming both cells with bypass made AMOUNT drags look like they were
        // toggling gate state whenever a live poll flipped gate.on.
        if !u.gate_style_valid {
            u.gate_style_valid = true;
            u.gate_style_on = true;
            if !u.gate_on.col.is_null() {
                lv::set_style_opa(u.gate_on.col, lv::OPA_COVER, 0);
            }
            if !u.gate.col.is_null() {
                lv::set_style_opa(u.gate.col, lv::OPA_COVER, 0);
            }
            if !u.gate_on.name.is_null() {
                lv::set_style_text_color(u.gate_on.name, lv::color_hex(0xB0B0B0), 0);
            }
            if !u.gate.name.is_null() {
                lv::set_style_text_color(u.gate.name, lv::color_hex(0xB0B0B0), 0);
            }
        }

        if !u.gate_on.value.is_null() && !u.gate_on.slider.is_null() {
            let known = fx_ref
                .map(|f| f.gate.valid && f.gate.on_known)
                .unwrap_or(false);
            if !known {
                if u.gate_on_drag == 0 && u.gate_drag == 0 {
                    lv::slider_set_value(u.gate_on.slider, 0, lv::ANIM_OFF);
                    lv::label_set_text(u.gate_on.value, "-");
                }
            } else if u.gate_on_drag == 0 && u.gate_drag == 0 {
                let on = if fx_ref.unwrap().gate.on { 1 } else { 0 };
                set_slider_if_changed(u.gate_on.slider, on);
                set_label_if_changed(u.gate_on.value, if on != 0 { "On" } else { "Off" });
            }
        }

        if !u.gate.value.is_null() && !u.gate.slider.is_null() {
            let known = fx_ref
                .map(|f| f.gate.valid && f.gate.reduction_valid)
                .unwrap_or(false);
            if !known {
                if u.gate_drag == 0 {
                    lv::slider_set_value(u.gate.slider, 50, lv::ANIM_OFF);
                    lv::label_set_text(u.gate.value, "-");
                }
            } else if u.gate_drag == 0 {
                let reduction = fx_ref.unwrap().gate.reduction_pct;
                set_slider_if_changed(u.gate.slider, reduction as i32);
                set_label_if_changed(u.gate.value, &format!("{reduction}%"));
            }
        }
    }

    update_chain_blocks_locked(amp_state(), fx);

    if u.fx_model_btn.is_null() {
        u.fx_ui_sync = false;
        return;
    }

    let block = fx_ref.map(|f| f.blocks[u.fx_selected as usize]);

    let Some(block) = block.filter(|b| b.valid) else {
        for i in 0..NANO_FX_MAX_PARAMS {
            if !u.fx_params[i].col.is_null() {
                lv::add_flag(u.fx_params[i].col, lv::FLAG_HIDDEN);
            }
        }
        sync_header_model_locked(fx);
        u.fx_ui_sync = false;
        return;
    };

    if !fx_panel_is_visible() {
        sync_header_model_locked(fx);
        u.fx_ui_sync = false;
        return;
    }

    if !u.fx_params_row.is_null() && u.fx_selected != u.fx_scroll_anchor {
        lv::scroll_to(u.fx_params_row, 0, 0, lv::ANIM_OFF);
        u.fx_scroll_anchor = u.fx_selected;
        u.pedal_params_scroll_inited = false;
    }

    ensure_fx_params_up_to(block.param_count as usize);
    update_pedal_params_grid_scroll(u.fx_params_row, block.param_count);

    for i in 0..NANO_FX_MAX_PARAMS {
        if i >= block.param_count as usize {
            if !u.fx_params[i].col.is_null() {
                lv::add_flag(u.fx_params[i].col, lv::FLAG_HIDDEN);
            }
            continue;
        }
        apply_fx_param_locked(i, &block);
    }

    sync_header_model_locked(fx);
    u.fx_ui_sync = false;
}

fn fx_panel_is_visible() -> bool {
    let panel = ui().fx_panel;
    !panel.is_null() && !lv::has_flag(panel, lv::FLAG_HIDDEN)
}

fn build_fx_param_cell(i: usize) {
    let u = ui();
    if i >= NANO_FX_MAX_PARAMS || !u.fx_params[i].col.is_null() || u.fx_params_row.is_null() {
        return;
    }

    let slider_w = param_slider_width();
    let cell_w = u.param_cell_w;
    let cell_h = u.param_cell_h;

    let col = lv::obj_create(u.fx_params_row);
    style_param_cell(col, cell_w, cell_h);
    lv::set_flex_flow(col, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        col,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::add_flag(col, lv::FLAG_HIDDEN);
    ui_param_cell_container(col);
    lv::add_event_cb(col, param_cell_event_cb, lv::EVENT_ALL, i);

    let name = lv::label_create(col);
    lv::label_set_text(name, "Param");
    style_param_name_label(name);

    let slider = lv::slider_create(col);
    style_compact_h_slider(slider, slider_w, 0, 100);
    lv::add_state(slider, lv::STATE_DISABLED);

    let value = lv::label_create(col);
    lv::label_set_text(value, "-");
    style_param_value_label(value);

    u.fx_params[i] = FxParamUi {
        col,
        name,
        slider,
        value,
    };
}

unsafe extern "C" fn fx_build_timer_cb(timer: *mut lv_timer_t) {
    let u = ui();
    let fx = unsafe { fx_state().as_ref() };
    let Some(fx) = fx.filter(|_| fx_panel_is_visible()) else {
        lv::timer_delete(timer);
        u.fx_build_timer = ptr::null_mut();
        return;
    };

    let block = fx.blocks[u.fx_selected as usize];
    let count = if block.valid {
        (block.param_count as usize).min(NANO_FX_MAX_PARAMS)
    } else {
        0
    };

    let mut built_idx: i32 = -1;
    for j in 0..count {
        if u.fx_params[j].col.is_null() {
            build_fx_param_cell(j);
            built_idx = j as i32;
            break;
        }
    }

    if built_idx >= 0 {
        apply_fx_param_locked(built_idx as usize, &block);
        return;
    }

    lv::timer_delete(timer);
    u.fx_build_timer = ptr::null_mut();
    apply_fx_locked(fx as *const NanoFxState);
}

fn ensure_fx_params_up_to(count: usize) {
    let u = ui();
    let count = count.min(NANO_FX_MAX_PARAMS);

    let need_more = (0..count).any(|j| u.fx_params[j].col.is_null());

    if need_more && u.fx_build_timer.is_null() {
        u.fx_build_timer = lv::timer_create(Some(fx_build_timer_cb), 50, ptr::null_mut());
    }
}

fn build_fx_panel(parent: *mut lv_obj_t) {
    ui().fx_params_row = create_pedal_params_grid(parent);
}

/* ------------------------------------------------------- tuner / tempo UI */

fn build_tuner_panel(parent: *mut lv_obj_t) {
    let u = ui();

    lv::set_style_bg_opa(parent, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(parent, 0, 0);
    lv::set_flex_flow(parent, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        parent,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::set_style_pad_row(parent, 0, 0);
    lv::set_style_pad_top(parent, 0, 0);
    lv::set_style_pad_bottom(parent, 20, 0);
    lv::set_size(parent, lv::pct(100), lv::pct(100));

    let body = lv::obj_create(parent);
    lv::set_width(body, lv::pct(100));
    lv::set_flex_grow(body, 1);
    lv::set_style_bg_opa(body, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(body, 0, 0);
    lv::set_style_pad_all(body, 0, 0);
    lv::set_style_pad_row(body, 10, 0);
    lv::set_flex_flow(body, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        body,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::remove_flag(body, lv::FLAG_SCROLLABLE);
    lv::remove_flag(body, lv::FLAG_CLICKABLE);

    u.tuner_note = lv::label_create(body);
    lv::label_set_text(u.tuner_note, "-");
    lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
    lv::set_style_text_font(u.tuner_note, lv::font_tuner_note(), 0);

    u.tuner_cents = lv::label_create(body);
    lv::label_set_text(u.tuner_cents, "Tuner off");
    lv::set_style_text_color(u.tuner_cents, lv::color_hex(0xC0C0C0), 0);
    lv::set_style_text_font(u.tuner_cents, lv::font_lg(), 0);
    lv::set_style_pad_bottom(u.tuner_cents, 16, 0);

    u.tuner_meter = lv::obj_create(body);
    lv::set_size(u.tuner_meter, TUNER_METER_W, TUNER_METER_H);
    lv::set_style_bg_color(u.tuner_meter, lv::color_hex(0x1A1A1A), 0);
    lv::set_style_bg_opa(u.tuner_meter, lv::OPA_COVER, 0);
    lv::set_style_border_width(u.tuner_meter, 1, 0);
    lv::set_style_border_color(u.tuner_meter, lv::color_hex(0x404040), 0);
    lv::set_style_radius(u.tuner_meter, 8, 0);
    lv::set_style_pad_all(u.tuner_meter, 0, 0);
    lv::remove_flag(u.tuner_meter, lv::FLAG_SCROLLABLE);
    lv::remove_flag(u.tuner_meter, lv::FLAG_CLICKABLE);

    u.tuner_meter_center = lv::obj_create(u.tuner_meter);
    lv::set_size(u.tuner_meter_center, 48, TUNER_METER_H - 4);
    lv::set_style_bg_color(u.tuner_meter_center, lv::color_hex(TUNER_COLOR_OK), 0);
    lv::set_style_bg_opa(u.tuner_meter_center, lv::OPA_30, 0);
    lv::set_style_border_width(u.tuner_meter_center, 0, 0);
    lv::set_style_radius(u.tuner_meter_center, 4, 0);
    lv::set_style_pad_all(u.tuner_meter_center, 0, 0);
    lv::align(u.tuner_meter_center, lv::ALIGN_CENTER, 0, 0);
    lv::remove_flag(u.tuner_meter_center, lv::FLAG_SCROLLABLE);
    lv::remove_flag(u.tuner_meter_center, lv::FLAG_CLICKABLE);

    let tick = lv::obj_create(u.tuner_meter);
    lv::set_size(tick, 2, TUNER_METER_H);
    lv::set_style_bg_color(tick, lv::color_hex(0xA0A0A0), 0);
    lv::set_style_bg_opa(tick, lv::OPA_COVER, 0);
    lv::set_style_border_width(tick, 0, 0);
    lv::set_style_radius(tick, 0, 0);
    lv::set_style_pad_all(tick, 0, 0);
    lv::align(tick, lv::ALIGN_CENTER, 0, 0);
    lv::remove_flag(tick, lv::FLAG_SCROLLABLE);
    lv::remove_flag(tick, lv::FLAG_CLICKABLE);

    u.tuner_meter_needle = lv::obj_create(u.tuner_meter);
    lv::set_size(u.tuner_meter_needle, TUNER_NEEDLE_W, TUNER_METER_H);
    lv::set_style_bg_color(u.tuner_meter_needle, lv::color_hex(TUNER_COLOR_IDLE), 0);
    lv::set_style_bg_opa(u.tuner_meter_needle, lv::OPA_COVER, 0);
    lv::set_style_border_width(u.tuner_meter_needle, 0, 0);
    lv::set_style_radius(u.tuner_meter_needle, 3, 0);
    lv::set_style_pad_all(u.tuner_meter_needle, 0, 0);
    lv::set_pos(u.tuner_meter_needle, (TUNER_METER_W - TUNER_NEEDLE_W) / 2, 0);
    lv::remove_flag(u.tuner_meter_needle, lv::FLAG_SCROLLABLE);
    lv::remove_flag(u.tuner_meter_needle, lv::FLAG_CLICKABLE);

    u.tuner_state = lv::label_create(body);
    lv::label_set_text(u.tuner_state, "");
    lv::add_flag(u.tuner_state, lv::FLAG_HIDDEN);

    u.tuner_btn = lv::button_create(parent);
    lv::set_size(u.tuner_btn, 160, 48);
    lv::add_event_cb(u.tuner_btn, tuner_btn_event_cb, lv::EVENT_CLICKED, 0);
    lv::add_state(u.tuner_btn, lv::STATE_DISABLED);
    let btn_label = lv::label_create(u.tuner_btn);
    lv::label_set_text(btn_label, "ON");
    lv::center(btn_label);
}

fn build_tempo_panel(parent: *mut lv_obj_t) {
    let u = ui();

    lv::set_style_bg_opa(parent, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(parent, 0, 0);
    lv::set_flex_flow(parent, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        parent,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::set_style_pad_row(parent, 0, 0);
    lv::set_style_pad_top(parent, 0, 0);
    lv::set_style_pad_bottom(parent, 20, 0);
    lv::set_size(parent, lv::pct(100), lv::pct(100));

    let body = lv::obj_create(parent);
    lv::set_width(body, lv::pct(100));
    lv::set_flex_grow(body, 1);
    lv::set_style_bg_opa(body, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(body, 0, 0);
    lv::set_style_pad_all(body, 0, 0);
    lv::set_style_pad_row(body, 16, 0);
    lv::set_flex_flow(body, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        body,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::remove_flag(body, lv::FLAG_SCROLLABLE);
    lv::remove_flag(body, lv::FLAG_CLICKABLE);

    u.tempo_bpm_lbl = lv::label_create(body);
    lv::label_set_text(u.tempo_bpm_lbl, "-");
    lv::set_style_text_color(u.tempo_bpm_lbl, lv::color_hex(0xFFFFFF), 0);
    lv::set_style_text_font(u.tempo_bpm_lbl, lv::font_tuner_note(), 0);

    let bpm_unit = lv::label_create(body);
    lv::label_set_text(bpm_unit, "BPM");
    lv::set_style_text_color(bpm_unit, lv::color_hex(0x909090), 0);
    lv::set_style_text_font(bpm_unit, lv::font_lg(), 0);

    let scope_row = lv::obj_create(body);
    lv::set_width(scope_row, lv::SIZE_CONTENT);
    lv::set_height(scope_row, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(scope_row, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(scope_row, 0, 0);
    lv::set_style_pad_all(scope_row, 0, 0);
    lv::set_style_pad_column(scope_row, 12, 0);
    lv::set_flex_flow(scope_row, lv::FLEX_FLOW_ROW);
    lv::set_flex_align(
        scope_row,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_passive_container(scope_row);

    u.tempo_scope_lbl = lv::label_create(scope_row);
    lv::label_set_text(u.tempo_scope_lbl, "Global tempo");
    lv::set_style_text_color(u.tempo_scope_lbl, lv::color_hex(0xC0C0C0), 0);
    lv::set_style_text_font(u.tempo_scope_lbl, lv::font_md(), 0);

    u.tempo_scope_sw = lv::switch_create(scope_row);
    lv::add_state(u.tempo_scope_sw, lv::STATE_CHECKED);
    lv::add_event_cb(
        u.tempo_scope_sw,
        tempo_scope_event_cb,
        lv::EVENT_VALUE_CHANGED,
        0,
    );

    u.tempo_tap_btn = lv::button_create(parent);
    lv::set_size(u.tempo_tap_btn, 220, 72);
    lv::add_event_cb(
        u.tempo_tap_btn,
        tempo_tap_btn_event_cb,
        lv::EVENT_CLICKED,
        0,
    );
    lv::add_state(u.tempo_tap_btn, lv::STATE_DISABLED);
    let tap_lbl = lv::label_create(u.tempo_tap_btn);
    lv::label_set_text(tap_lbl, "TAP");
    lv::set_style_text_font(tap_lbl, lv::font_lg(), 0);
    lv::center(tap_lbl);

    let bpm = unsafe { nano_state_get_bpm() };
    if bpm >= NANO_BPM_MIN {
        lv::label_set_text(u.tempo_bpm_lbl, &format!("{bpm}"));
    }
}

fn set_slider_enabled_locked(enabled: bool) {
    let u = ui();
    for i in 0..KNOB_COUNT {
        if !u.knobs[i].slider.is_null() {
            lv::set_enabled(u.knobs[i].slider, enabled);
        }
    }
}

/* ------------------------------------------------------------ build tree */

fn style_screen(screen: *mut lv_obj_t) {
    lv::set_style_bg_color(screen, lv::color_hex(0x121212), 0);
    lv::set_style_bg_opa(screen, lv::OPA_COVER, 0);
    lv::set_style_pad_all(screen, UI_EDGE_PAD, 0);
}

fn calc_param_cell_width(area_w: i32, cols: i32) -> i32 {
    let cols = cols.max(1);
    let area = area_w - 2 * PARAM_GRID_PAD;
    let gaps = (cols - 1) * PARAM_GRID_GAP;
    (area - gaps) / cols
}

fn calc_param_cell_height(area_h: i32, rows: i32) -> i32 {
    let rows = rows.max(1);
    let area = area_h - 2 * PARAM_GRID_PAD;
    let gaps = (rows - 1) * PARAM_GRID_GAP;
    (area - gaps) / rows
}

fn make_tuner_hdr_btn(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    let btn = lv::button_create(parent);
    lv::set_size(btn, 36, 36);
    lv::set_style_bg_opa(btn, lv::OPA_TRANSP, lv::PART_MAIN);
    lv::set_style_bg_opa(btn, lv::OPA_TRANSP, lv::PART_MAIN | lv::SEL_CHECKED);
    lv::set_style_border_width(btn, 1, lv::PART_MAIN);
    lv::set_style_border_color(btn, lv::color_hex(0x404040), lv::PART_MAIN);
    lv::set_style_border_color(
        btn,
        lv::color_hex(0x8090A0),
        lv::PART_MAIN | lv::SEL_CHECKED,
    );
    lv::set_style_shadow_width(btn, 0, lv::PART_MAIN);
    lv::set_style_radius(btn, 6, lv::PART_MAIN);
    lv::add_flag(btn, lv::FLAG_CHECKABLE);
    lv::add_event_cb(btn, tuner_hdr_btn_event_cb, lv::EVENT_CLICKED, 0);

    let img = lv::image_create(btn);
    lv::image_set_src(img, icons::TUNER_FORK.as_src());
    lv::set_style_image_recolor(img, lv::color_hex(0xB0B8C0), 0);
    lv::set_style_image_recolor_opa(img, lv::OPA_COVER, 0);
    lv::center(img);
    btn
}

fn init_fx_model_btn_style() -> *mut lv_style_t {
    let u = ui();
    if !u.fx_model_btn_style.is_null() {
        return u.fx_model_btn_style;
    }

    let style = lv::style_new();
    lv::style_set_bg_color(style, lv::color_hex(0x080808));
    lv::style_set_bg_opa(style, lv::OPA_COVER);
    lv::style_set_border_width(style, 0);
    lv::style_set_shadow_width(style, 0);
    lv::style_set_radius(style, 4);
    lv::style_set_pad_top(style, 8);
    lv::style_set_pad_bottom(style, 8);
    lv::style_set_pad_left(style, 12);
    lv::style_set_pad_right(style, 12);

    u.fx_model_btn_style = style;
    style
}

fn style_fx_model_header_btn(btn: *mut lv_obj_t) {
    if btn.is_null() {
        return;
    }

    let style = init_fx_model_btn_style();
    lv::remove_style_all(btn);
    lv::add_style(btn, style, lv::PART_MAIN | lv::SEL_STATE_ANY);
    lv::set_style_bg_opa(btn, lv::OPA_40, lv::PART_MAIN | lv::SEL_DISABLED);
    lv::set_width(btn, lv::pct(100));
    lv::set_height(btn, HEADER_DD_H);
    lv::set_flex_grow(btn, 1);
}

fn build_header_bar(screen: *mut lv_obj_t) {
    let u = ui();

    let header = lv::obj_create(screen);
    u.header_bar = header;
    lv::set_size(header, lv::pct(100), HEADER_H);
    lv::set_style_bg_opa(header, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(header, 0, 0);
    lv::set_style_pad_all(header, 0, 0);
    lv::set_flex_flow(header, lv::FLEX_FLOW_ROW);
    lv::set_flex_align(
        header,
        lv::FLEX_ALIGN_SPACE_BETWEEN,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::align(header, lv::ALIGN_TOP_MID, 0, 0);
    ui_passive_container(header);

    let left = lv::obj_create(header);
    lv::set_size(left, 196, HEADER_H);
    lv::set_style_bg_opa(left, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(left, 0, 0);
    lv::set_style_pad_left(left, 8, 0);
    lv::set_style_pad_right(left, 4, 0);
    lv::set_flex_flow(left, lv::FLEX_FLOW_ROW);
    lv::set_flex_align(
        left,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_passive_container(left);

    u.preset_lbl = lv::label_create(left);
    lv::label_set_text(u.preset_lbl, "-");
    lv::set_width(u.preset_lbl, 184);
    lv::label_set_long_mode(u.preset_lbl, lv::LABEL_LONG_DOT);
    lv::set_style_text_color(u.preset_lbl, lv::color_hex(0x909090), 0);
    lv::set_style_text_font(u.preset_lbl, lv::font_sm(), 0);
    lv::set_style_text_align(u.preset_lbl, lv::TEXT_ALIGN_LEFT, 0);

    let center = lv::obj_create(header);
    lv::set_height(center, HEADER_H);
    lv::set_flex_grow(center, 1);
    lv::set_style_bg_opa(center, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(center, 0, 0);
    lv::set_style_pad_all(center, 0, 0);
    lv::set_flex_flow(center, lv::FLEX_FLOW_ROW);
    lv::set_flex_align(
        center,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_passive_container(center);

    let dd_host = lv::obj_create(center);
    lv::set_size(dd_host, lv::pct(100), HEADER_DD_H);
    lv::set_flex_grow(dd_host, 1);
    lv::set_style_bg_opa(dd_host, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(dd_host, 0, 0);
    lv::set_style_pad_all(dd_host, 0, 0);
    ui_passive_container(dd_host);

    u.fx_model_btn = lv::button_create(dd_host);
    lv::set_size(u.fx_model_btn, lv::pct(100), lv::pct(100));
    style_fx_model_header_btn(u.fx_model_btn);
    lv::add_event_cb(
        u.fx_model_btn,
        fx_model_header_btn_event_cb,
        lv::EVENT_CLICKED,
        0,
    );
    lv::add_state(u.fx_model_btn, lv::STATE_DISABLED);

    u.fx_model_btn_lbl = lv::label_create(u.fx_model_btn);
    lv::label_set_text(u.fx_model_btn_lbl, "-");
    lv::set_width(u.fx_model_btn_lbl, lv::pct(100));
    lv::label_set_long_mode(u.fx_model_btn_lbl, lv::LABEL_LONG_DOT);
    lv::set_style_text_font(u.fx_model_btn_lbl, lv::font_lg(), 0);
    lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0x606060), 0);
    lv::set_style_text_align(u.fx_model_btn_lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::center(u.fx_model_btn_lbl);

    let right = lv::obj_create(header);
    lv::set_size(right, 200, HEADER_H);
    lv::set_style_bg_opa(right, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(right, 0, 0);
    lv::set_style_pad_all(right, 0, 0);
    lv::set_style_pad_right(right, HEADER_RIGHT_PAD, 0);
    lv::set_flex_flow(right, lv::FLEX_FLOW_ROW);
    lv::set_flex_align(
        right,
        lv::FLEX_ALIGN_END,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::set_style_pad_column(right, 14, 0);
    ui_passive_container(right);

    let bpm_row = lv::obj_create(right);
    lv::set_height(bpm_row, HEADER_H);
    lv::set_width(bpm_row, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(bpm_row, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(bpm_row, 0, 0);
    lv::set_style_pad_all(bpm_row, 0, 0);
    lv::set_flex_flow(bpm_row, lv::FLEX_FLOW_ROW);
    lv::set_flex_align(
        bpm_row,
        lv::FLEX_ALIGN_END,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::remove_flag(bpm_row, lv::FLAG_SCROLLABLE);
    lv::add_flag(bpm_row, lv::FLAG_CLICKABLE);
    lv::add_event_cb(bpm_row, bpm_hdr_btn_event_cb, lv::EVENT_CLICKED, 0);

    u.bpm_lbl = lv::label_create(bpm_row);
    lv::label_set_text(u.bpm_lbl, "- BPM");
    lv::set_style_text_color(u.bpm_lbl, lv::color_hex(0x909090), 0);
    lv::set_style_text_font(u.bpm_lbl, lv::font_sm(), 0);
    lv::set_width(u.bpm_lbl, lv::SIZE_CONTENT);
    lv::label_set_long_mode(u.bpm_lbl, lv::LABEL_LONG_CLIP);
    lv::set_style_text_align(u.bpm_lbl, lv::TEXT_ALIGN_LEFT, 0);
    lv::remove_flag(u.bpm_lbl, lv::FLAG_CLICKABLE);

    u.ble_icon = lv::label_create(right);
    lv::label_set_text(u.ble_icon, lv::SYMBOL_BLUETOOTH);
    lv::set_style_text_font(u.ble_icon, lv::font_symbol(), 0);
    lv::set_style_text_color(u.ble_icon, lv::color_hex(0x505050), 0);
    lv::set_style_opa(u.ble_icon, lv::OPA_30, 0);

    u.tuner_hdr_btn = make_tuner_hdr_btn(right);
}

fn make_knob_column(parent: *mut lv_obj_t, idx: usize, label: &str) {
    let u = ui();
    let slider_w = param_slider_width();

    let col = lv::obj_create(parent);
    style_param_cell(col, u.param_cell_w, u.param_cell_h);
    lv::set_flex_flow(col, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        col,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_param_cell_container(col);
    lv::add_event_cb(
        col,
        param_cell_event_cb,
        lv::EVENT_ALL,
        PARAM_CELL_AMP_TAG | idx,
    );

    let name = lv::label_create(col);
    lv::label_set_text(name, label);
    style_param_name_label(name);

    let slider = lv::slider_create(col);
    style_compact_h_slider(slider, slider_w, 0, 255);
    lv::add_state(slider, lv::STATE_DISABLED);

    let value = lv::label_create(col);
    lv::label_set_text(value, "-");
    style_param_value_label(value);

    u.knobs[idx] = AmpKnobUi { slider, value };
}

fn make_gate_slider_cell(
    parent: *mut lv_obj_t,
    label: &str,
    tag: usize,
    smin: i32,
    smax: i32,
) -> GateCellUi {
    let u = ui();
    let slider_w = param_slider_width();

    let col = lv::obj_create(parent);
    style_param_cell(col, u.param_cell_w, u.param_cell_h);
    lv::set_flex_flow(col, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        col,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_param_cell_container(col);
    lv::add_event_cb(col, param_cell_event_cb, lv::EVENT_ALL, tag);

    let name = lv::label_create(col);
    lv::label_set_text(name, label);
    style_param_name_label(name);

    let slider = lv::slider_create(col);
    style_compact_h_slider(slider, slider_w, smin, smax);
    lv::add_state(slider, lv::STATE_DISABLED);

    let value = lv::label_create(col);
    lv::label_set_text(value, "-");
    style_param_value_label(value);

    GateCellUi {
        col,
        name,
        slider,
        value,
    }
}

fn build_amp_panel(parent: *mut lv_obj_t) {
    let row = create_pedal_params_grid(parent);
    ui().amp_params_row = row;

    ui().gate_on = make_gate_slider_cell(row, "INPUT GATE", PARAM_CELL_GATE_ON_TAG, 0, 1);
    ui().gate = make_gate_slider_cell(row, "GATE AMNT", PARAM_CELL_GATE_TAG, 0, 100);
    make_knob_column(row, 0, "GAIN");
    ui().cap_vol = make_gate_slider_cell(row, "CAPTURE LV", PARAM_CELL_CAP_VOL_TAG, 0, 255);
    make_knob_column(row, 1, "MASTER LV");
    make_knob_column(row, 2, "BASS");
    make_knob_column(row, 3, "MID");
    make_knob_column(row, 4, "TREBLE");
}

fn build_chain_bar(parent: *mut lv_obj_t, tab_h: i32) {
    lv::set_style_bg_opa(parent, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(parent, 0, 0);
    lv::set_style_pad_all(parent, 0, 0);
    ui_passive_container(parent);

    let use_tab_h = if tab_h > 0 { tab_h } else { CHAIN_TAB_H };

    let list = lv::obj_create(parent);
    lv::set_size(list, lv::pct(100), lv::pct(100));
    lv::set_style_bg_opa(list, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(list, 0, 0);
    lv::set_style_pad_left(list, CHAIN_RAIL_PAD, 0);
    lv::set_style_pad_top(list, PARAM_GRID_PAD, 0);
    lv::set_style_pad_bottom(list, CHAIN_RAIL_BOTTOM_PAD, 0);
    lv::set_style_pad_right(list, 0, 0);
    lv::set_flex_flow(list, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        list,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::set_style_pad_row(list, CHAIN_TAB_GAP, 0);
    ui_passive_container(list);

    for i in 0..CHAIN_BLOCK_COUNT {
        let col = lv::obj_create(list);
        lv::set_size(col, CHAIN_TAB_W, use_tab_h);
        lv::set_style_bg_opa(col, lv::OPA_TRANSP, 0);
        lv::set_style_border_width(col, 0, 0);
        lv::set_style_pad_all(col, 0, 0);
        lv::set_style_radius(col, 6, 0);
        lv::set_flex_flow(col, lv::FLEX_FLOW_COLUMN);
        lv::set_flex_align(
            col,
            lv::FLEX_ALIGN_CENTER,
            lv::FLEX_ALIGN_CENTER,
            lv::FLEX_ALIGN_CENTER,
        );
        ui_passive_container(col);

        let btn = lv::obj_create(col);
        lv::set_size(btn, lv::pct(100), lv::pct(100));
        chain_btn_prepare(btn);
        lv::add_event_cb(btn, chain_block_btn_event_cb, lv::EVENT_ALL, i);
        ui_no_scroll_only(btn);

        let icon = lv::label_create(btn);
        lv::set_width(icon, CHAIN_ICON_W);
        lv::label_set_long_mode(icon, lv::LABEL_LONG_DOT);
        lv::set_style_text_align(icon, lv::TEXT_ALIGN_CENTER, 0);
        lv::set_style_text_font(icon, lv::font_chain(), 0);
        lv::set_style_text_color(icon, lv::color_hex(0x505050), 0);
        lv::add_flag(icon, lv::FLAG_EVENT_BUBBLE);
        lv::center(icon);

        let cb = &mut ui().chain[i];
        cb.col = col;
        cb.btn = btn;
        cb.icon = icon;
        cb.icon_text = [0; CHAIN_LABEL_MAX];
        cb.last_selected = false;
        cb.last_active = false;
        cb.last_accent = lv::color_hex(0x000000);

        let init_icon = if i == CHAIN_AMP {
            IconKind::Amp
        } else {
            IconKind::Utility
        };
        chain_set_block_label(i, icon_kind_glyph(init_icon), lv::color_hex(0x505050));
    }
}

fn build_ui() {
    let screen = lv::screen_active();
    style_screen(screen);

    build_header_bar(screen);
    build_fx_model_picker(screen);

    let inner_h = lv::ver_res() - 2 * UI_EDGE_PAD;
    let body_h = inner_h - HEADER_H;

    let chain_vpad = PARAM_GRID_PAD + CHAIN_RAIL_BOTTOM_PAD;
    let chain_gap_total = (CHAIN_BLOCK_COUNT as i32 - 1) * CHAIN_TAB_GAP;
    let chain_avail = body_h - chain_vpad - chain_gap_total;
    let chain_tab_h = chain_avail / CHAIN_BLOCK_COUNT as i32;

    let content_w = lv::hor_res() - 2 * UI_EDGE_PAD - CHAIN_RAIL_W - CONTENT_CHAIN_GAP;
    let content_x = CHAIN_RAIL_W + CONTENT_CHAIN_GAP;

    {
        let u = ui();
        u.body_h = body_h;
        u.content_w_normal = content_w;
        u.content_x_normal = content_x;
        u.param_cell_w = calc_param_cell_width(content_w - PARAM_GRID_SCROLLBAR_W, PARAM_GRID_COLS);
        u.param_cell_h = calc_param_cell_height(body_h, PARAM_GRID_ROWS);
    }

    let chain_bar = lv::obj_create(screen);
    ui().chain_bar = chain_bar;
    lv::set_size(chain_bar, CHAIN_RAIL_W, body_h);
    lv::align(chain_bar, lv::ALIGN_TOP_LEFT, 0, HEADER_H);
    build_chain_bar(chain_bar, chain_tab_h);

    let content_root = lv::obj_create(screen);
    ui().content_root = content_root;
    lv::set_size(content_root, content_w, body_h);
    lv::align(content_root, lv::ALIGN_TOP_LEFT, content_x, HEADER_H);
    lv::set_style_bg_opa(content_root, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(content_root, 0, 0);
    lv::set_style_pad_all(content_root, 0, 0);
    ui_passive_container(content_root);

    let amp_panel = make_content_panel(content_root);
    ui().amp_panel = amp_panel;
    build_amp_panel(amp_panel);

    let fx_panel = make_content_panel(content_root);
    ui().fx_panel = fx_panel;
    build_fx_panel(fx_panel);

    let tuner_panel = make_content_panel(content_root);
    ui().tuner_panel = tuner_panel;
    build_tuner_panel(tuner_panel);

    let tempo_panel = make_content_panel(content_root);
    ui().tempo_panel = tempo_panel;
    build_tempo_panel(tempo_panel);
    lv::add_flag(tempo_panel, lv::FLAG_HIDDEN);

    show_content_for_chain(CHAIN_AMP);
    ui_bring_chrome_to_front();

    ui_init_dispatch();
    if ui().ble_icon_timer.is_null() {
        ui().ble_icon_timer = lv::timer_create(Some(ble_icon_timer_cb), 400, ptr::null_mut());
    }
    refresh_ble_icon_locked();
    apply_tuner_locked(unsafe { nano_tuner_get() });
    apply_fx_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

unsafe extern "C" fn ui_build_timer_cb(timer: *mut lv_timer_t) {
    build_ui();
    BUILD_DONE.store(true, Ordering::Release);
    lv::timer_delete(timer);
}

/* ---------------------------------------------------------------- amp tab */

fn apply_amp_locked(amp: *const NanoAmpC) {
    let u = ui();
    u.ui_sync = true;

    let amp = unsafe { amp.as_ref() }.filter(|a| a.valid);

    let Some(amp) = amp else {
        for i in 0..KNOB_COUNT {
            if i as i32 == u.drag_knob || u.knobs[i].slider.is_null() {
                continue;
            }
            lv::slider_set_value(u.knobs[i].slider, 0, lv::ANIM_OFF);
            lv::label_set_text(u.knobs[i].value, "-");
            u.last_sent[i] = 0;
        }
        if !u.cap_vol.slider.is_null() && u.cap_vol_drag == 0 {
            lv::slider_set_value(u.cap_vol.slider, 128, lv::ANIM_OFF);
            lv::label_set_text(u.cap_vol.value, "-");
            lv::add_state(u.cap_vol.slider, lv::STATE_DISABLED);
        }
        set_slider_enabled_locked(false);
        update_chain_blocks_locked(ptr::null(), fx_state());
        u.amp_chrome_valid = false;
        u.ui_sync = false;
        return;
    };

    let raw = [amp.gain, amp.level, amp.bass, amp.mid, amp.treble];

    for i in 0..KNOB_COUNT {
        u.last_sent[i] = raw[i];
        if i as i32 == u.drag_knob {
            continue;
        }
        set_slider_if_changed(u.knobs[i].slider, raw[i] as i32);
        set_label_if_changed(u.knobs[i].value, &format!("{}%", pct(raw[i])));
    }

    if !u.cap_vol.slider.is_null() && !u.cap_vol.value.is_null() {
        lv::set_enabled(u.cap_vol.slider, u.ble_connected);
        if u.cap_vol_drag == 0 {
            if amp.have_capture_volume {
                set_slider_if_changed(u.cap_vol.slider, amp.capture_volume as i32);
                set_label_if_changed(
                    u.cap_vol.value,
                    &format!("{}%", pct(amp.capture_volume)),
                );
                u.cap_vol_last_sent = amp.capture_volume as i32;
            } else {
                lv::slider_set_value(u.cap_vol.slider, 128, lv::ANIM_OFF);
                lv::label_set_text(u.cap_vol.value, "-");
            }
        }
    }

    set_slider_enabled_locked(u.ble_connected);
    update_pedal_params_grid_scroll(u.amp_params_row, AMP_PEDAL_PARAM_COUNT);

    // Knob ticks arrive many times per second. Chain / preset / header only
    // change with capture/slot metadata — skip them on pure gain/EQ/master.
    let name_bytes: &[u8] = unsafe {
        core::slice::from_raw_parts(
            amp.capture_name.as_ptr() as *const u8,
            NANO_AMP_CAPTURE_NAME_MAX,
        )
    };
    let chrome_dirty = !u.amp_chrome_valid
        || amp.have_capture_slot != u.amp_chrome_have_slot
        || (amp.have_capture_slot && amp.capture_slot != u.amp_chrome_slot)
        || name_bytes != u.amp_chrome_name;

    if chrome_dirty {
        u.amp_chrome_valid = true;
        u.amp_chrome_have_slot = amp.have_capture_slot;
        u.amp_chrome_slot = amp.capture_slot;
        u.amp_chrome_name.copy_from_slice(name_bytes);
        u.amp_chrome_name[NANO_AMP_CAPTURE_NAME_MAX - 1] = 0;
        update_chain_blocks_locked(amp as *const NanoAmpC, fx_state());
        refresh_preset_label_locked();
        if u.chain_selected == CHAIN_AMP && !u.tuner_view {
            sync_header_model_locked(fx_state());
        }
    }
    u.ui_sync = false;
}

/* ------------------------------------------------------- public C entries */

/// Init Waveshare LCD + LVGL and show amp panel.
#[no_mangle]
pub unsafe extern "C" fn nano_ui_start() -> esp_err_t {
    let (err, panel, touch) = lv::rgb_lcd_init();
    if err != ESP_OK {
        return err;
    }
    let err = lv::port_init(panel, touch);
    if err != ESP_OK {
        return err;
    }
    let err = lv::bl_on();
    if err != ESP_OK {
        return err;
    }

    BUILD_DONE.store(false, Ordering::Release);

    if !lv::port_lock(-1) {
        return ESP_FAIL;
    }
    let build_timer = lv::timer_create(Some(ui_build_timer_cb), 1, ptr::null_mut());
    if build_timer.is_null() {
        lv::port_unlock();
        return ESP_ERR_NO_MEM;
    }
    lv::timer_set_repeat_count(build_timer, 1);
    lv::port_unlock();

    // 60 s budget, same as the C semaphore wait.
    for _ in 0..6000 {
        if BUILD_DONE.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !BUILD_DONE.load(Ordering::Acquire) {
        error!("UI build timed out");
        return ESP_FAIL;
    }

    info!("UI ready (800x480, left chain rail + panels)");
    ESP_OK
}

/// Refresh on-screen amp values (safe from any task).
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_amp(amp: *const NanoAmpC) {
    let u = ui();
    match unsafe { amp.as_ref() }.filter(|a| a.valid) {
        Some(amp) => {
            if u.ui_amp_cache_valid && struct_bytes(amp) == struct_bytes(&u.ui_amp_cache) {
                return;
            }
            u.ui_amp_cache = *amp;
            u.ui_amp_cache_valid = true;
            u.async_amp = *amp;
            u.async_amp_have = true;
        }
        None => {
            if !u.ui_amp_cache_valid {
                return;
            }
            u.ui_amp_cache_valid = false;
            u.async_amp_have = false;
        }
    }
    u.dispatch_mask |= UI_DISP_AMP;
    ui_request_dispatch();
}

/// Update BLE status line.
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_ble_connected(connected: bool) {
    let u = ui();
    u.async_ble_connected = connected;
    u.async_ble_pending = true;
    u.dispatch_mask |= UI_DISP_BLE;
    ui_request_dispatch();
}

/// Refresh tuner tab (note/cents/status).
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_tuner(tuner: *const NanoTunerC) {
    let u = ui();
    let Some(tuner) = (unsafe { tuner.as_ref() }) else {
        return;
    };
    if u.tuner_note.is_null() {
        return;
    }
    u.async_tuner = *tuner;
    u.async_tuner_pending = true;
    u.dispatch_mask |= UI_DISP_TUNER;
    ui_request_dispatch();
}

/// Update header preset name (safe from any task).
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_preset_name(name: *const c_char) {
    let u = ui();
    let incoming = c_str(name);

    if u.ui_preset_cache_valid && c_buf_str(&u.ui_preset_cache) == incoming {
        return;
    }

    copy_into(&mut u.ui_preset_cache, incoming);
    u.ui_preset_cache_valid = true;
    copy_into(&mut u.async_preset, incoming);
    u.async_preset_pending = true;
    u.dispatch_mask |= UI_DISP_PRESET;
    ui_request_dispatch();
}

/// Update header tempo readout (safe from any task).
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_bpm(bpm: u16, valid: bool) {
    let u = ui();
    if valid {
        if u.ui_bpm_cache_valid && u.ui_bpm_cache == bpm {
            return;
        }
        u.ui_bpm_cache = bpm;
        u.ui_bpm_cache_valid = true;
    } else {
        if !u.ui_bpm_cache_valid {
            return;
        }
        u.ui_bpm_cache_valid = false;
    }

    u.async_bpm = bpm;
    u.async_bpm_valid = valid;
    u.async_bpm_pending = true;
    u.dispatch_mask |= UI_DISP_BPM;
    ui_request_dispatch();
}

/// Live tap-tempo notify: update BPM and rephase the beat blinker to this
/// instant. Safe from any task.
#[no_mangle]
pub unsafe extern "C" fn nano_ui_bpm_tap(bpm: u16) {
    if !(NANO_BPM_MIN..=NANO_BPM_MAX).contains(&bpm) {
        return;
    }

    let u = ui();
    u.ui_bpm_cache = bpm;
    u.ui_bpm_cache_valid = true;
    u.async_bpm = bpm;
    u.async_bpm_valid = true;
    u.async_bpm_pending = true;
    u.dispatch_mask |= UI_DISP_BPM;
    ui_request_dispatch();
}

/// Pedal Tempo Mode on/off (Footswitch / peer notify). Shows or hides the Tempo
/// screen without writing enter/exit to the pedal. Safe from any task.
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_tempo_mode(active: bool) {
    let u = ui();
    u.async_tempo_mode = active;
    u.async_tempo_mode_pending = true;
    u.dispatch_mask |= UI_DISP_TEMPO;
    ui_request_dispatch();
}

/// Refresh gate + FX tabs.
#[no_mangle]
pub unsafe extern "C" fn nano_ui_set_fx(fx: *const NanoFxState) {
    let u = ui();
    match unsafe { fx.as_ref() } {
        Some(fx) => {
            if u.ui_fx_cache_valid && struct_bytes(fx) == struct_bytes(&u.ui_fx_cache) {
                return;
            }
            u.ui_fx_cache = *fx;
            u.ui_fx_cache_valid = true;
            u.async_fx = *fx;
            u.async_fx_pending = true;
        }
        None => {
            if !u.ui_fx_cache_valid {
                return;
            }
            u.ui_fx_cache_valid = false;
            u.async_fx_pending = false;
            u.async_fx = unsafe { core::mem::zeroed() };
        }
    }
    u.dispatch_mask |= UI_DISP_FX;
    ui_request_dispatch();
}
