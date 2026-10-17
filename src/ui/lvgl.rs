//! Thin façade over the LVGL 9.5 and Waveshare BSP C ABIs.
//!
//! Every call here must run on the LVGL task (or under `lvgl_port_lock`), which
//! is exactly the contract the old `nano_ui.c` lived under. The wrappers keep
//! `screen.rs` readable: they carry the `unsafe` and the bindgen name mangling
//! so the port itself reads like the C it replaces.

#![allow(dead_code)]

use core::ffi::{c_char, c_void, CStr};
use std::ffi::CString;

use esp_idf_svc::sys as sys;
pub use esp_idf_svc::sys::{
    lv_area_t, lv_color_t, lv_event_t, lv_font_t, lv_obj_t, lv_point_t, lv_style_t, lv_timer_t,
};
use esp_idf_svc::sys::{esp_err_t, lv_opa_t, lv_style_selector_t};

/* ------------------------------------------------------- Waveshare BSP */
// `bsp/board.h` and `bsp/lvgl_port.h` stay out of `src/bindings.h`: they pull in
// `esp_lcd_touch.h`, whose C23 `[[deprecated]]` attribute clang rejects during
// the bindgen pass. Both handles are opaque to the UI, so `*mut c_void` is
// enough to hand them straight back to `lvgl_port_init`.

unsafe extern "C" {
    fn waveshare_esp32_s3_rgb_lcd_init(
        ret_panel: *mut *mut c_void,
        ret_touch: *mut *mut c_void,
    ) -> esp_err_t;
    fn waveshare_rgb_lcd_bl_on() -> esp_err_t;
    fn lvgl_port_init(lcd_handle: *mut c_void, tp_handle: *mut c_void) -> esp_err_t;
    fn lvgl_port_set_rotation(degree: u16) -> esp_err_t;
    fn lvgl_port_lock(timeout_ms: i32) -> bool;
    fn lvgl_port_unlock();
}

pub fn rgb_lcd_init() -> (esp_err_t, *mut c_void, *mut c_void) {
    let mut panel: *mut c_void = core::ptr::null_mut();
    let mut touch: *mut c_void = core::ptr::null_mut();
    let err = unsafe { waveshare_esp32_s3_rgb_lcd_init(&mut panel, &mut touch) };
    (err, panel, touch)
}

pub fn port_init(panel: *mut c_void, touch: *mut c_void) -> esp_err_t {
    unsafe { lvgl_port_init(panel, touch) }
}

pub fn port_set_rotation(degree: u16) -> esp_err_t {
    unsafe { lvgl_port_set_rotation(degree) }
}

pub fn bl_on() -> esp_err_t {
    unsafe { waveshare_rgb_lcd_bl_on() }
}

pub fn port_lock(timeout_ms: i32) -> bool {
    unsafe { lvgl_port_lock(timeout_ms) }
}

pub fn port_unlock() {
    unsafe { lvgl_port_unlock() }
}

/* ------------------------------------------------------------ constants */

pub const OPA_TRANSP: lv_opa_t = sys::_lv_opacity_level_t_LV_OPA_TRANSP as lv_opa_t;
pub const OPA_30: lv_opa_t = sys::_lv_opacity_level_t_LV_OPA_30 as lv_opa_t;
pub const OPA_40: lv_opa_t = sys::_lv_opacity_level_t_LV_OPA_40 as lv_opa_t;
pub const OPA_70: lv_opa_t = sys::_lv_opacity_level_t_LV_OPA_70 as lv_opa_t;
pub const OPA_80: lv_opa_t = sys::_lv_opacity_level_t_LV_OPA_80 as lv_opa_t;
pub const OPA_COVER: lv_opa_t = sys::_lv_opacity_level_t_LV_OPA_COVER as lv_opa_t;

pub const ALIGN_TOP_LEFT: sys::lv_align_t = sys::lv_align_t_LV_ALIGN_TOP_LEFT;
pub const ALIGN_TOP_MID: sys::lv_align_t = sys::lv_align_t_LV_ALIGN_TOP_MID;
pub const ALIGN_BOTTOM_MID: sys::lv_align_t = sys::lv_align_t_LV_ALIGN_BOTTOM_MID;
pub const ALIGN_CENTER: sys::lv_align_t = sys::lv_align_t_LV_ALIGN_CENTER;

pub const FLAG_HIDDEN: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN;
pub const FLAG_CLICKABLE: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE;
pub const FLAG_CHECKABLE: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_CHECKABLE;
pub const FLAG_SCROLLABLE: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE;
pub const FLAG_SCROLL_CHAIN: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_SCROLL_CHAIN;
pub const FLAG_SCROLL_CHAIN_VER: sys::lv_obj_flag_t =
    sys::lv_obj_flag_t_LV_OBJ_FLAG_SCROLL_CHAIN_VER;
pub const FLAG_EVENT_BUBBLE: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE;
pub const FLAG_GESTURE_BUBBLE: sys::lv_obj_flag_t = sys::lv_obj_flag_t_LV_OBJ_FLAG_GESTURE_BUBBLE;

pub const STATE_DISABLED: sys::lv_state_t = sys::lv_state_t_LV_STATE_DISABLED;
pub const STATE_CHECKED: sys::lv_state_t = sys::lv_state_t_LV_STATE_CHECKED;
pub const STATE_ANY: sys::lv_state_t = sys::lv_state_t_LV_STATE_ANY;

pub const PART_MAIN: lv_style_selector_t = sys::lv_part_t_LV_PART_MAIN as lv_style_selector_t;
pub const PART_INDICATOR: lv_style_selector_t =
    sys::lv_part_t_LV_PART_INDICATOR as lv_style_selector_t;
pub const PART_KNOB: lv_style_selector_t = sys::lv_part_t_LV_PART_KNOB as lv_style_selector_t;
pub const PART_SCROLLBAR: lv_style_selector_t =
    sys::lv_part_t_LV_PART_SCROLLBAR as lv_style_selector_t;
pub const PART_ANY: lv_style_selector_t = sys::lv_part_t_LV_PART_ANY as lv_style_selector_t;
pub const SEL_DISABLED: lv_style_selector_t = STATE_DISABLED as lv_style_selector_t;
pub const SEL_CHECKED: lv_style_selector_t = STATE_CHECKED as lv_style_selector_t;
pub const SEL_STATE_ANY: lv_style_selector_t = STATE_ANY as lv_style_selector_t;

pub const EVENT_ALL: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_ALL;
pub const EVENT_PRESSED: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_PRESSED;
pub const EVENT_PRESSING: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_PRESSING;
pub const EVENT_RELEASED: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_RELEASED;
pub const EVENT_CLICKED: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_CLICKED;
pub const EVENT_LONG_PRESSED: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_LONG_PRESSED;
pub const EVENT_VALUE_CHANGED: sys::lv_event_code_t = sys::lv_event_code_t_LV_EVENT_VALUE_CHANGED;

pub const FLEX_FLOW_ROW: sys::lv_flex_flow_t = sys::lv_flex_flow_t_LV_FLEX_FLOW_ROW;
pub const FLEX_FLOW_COLUMN: sys::lv_flex_flow_t = sys::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN;
pub const FLEX_FLOW_ROW_WRAP: sys::lv_flex_flow_t = sys::lv_flex_flow_t_LV_FLEX_FLOW_ROW_WRAP;

pub const FLEX_ALIGN_START: sys::lv_flex_align_t = sys::lv_flex_align_t_LV_FLEX_ALIGN_START;
pub const FLEX_ALIGN_CENTER: sys::lv_flex_align_t = sys::lv_flex_align_t_LV_FLEX_ALIGN_CENTER;
pub const FLEX_ALIGN_END: sys::lv_flex_align_t = sys::lv_flex_align_t_LV_FLEX_ALIGN_END;
pub const FLEX_ALIGN_SPACE_EVENLY: sys::lv_flex_align_t =
    sys::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_EVENLY;
pub const FLEX_ALIGN_SPACE_BETWEEN: sys::lv_flex_align_t =
    sys::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN;

pub const DIR_VER: sys::lv_dir_t = sys::lv_dir_t_LV_DIR_VER;

pub const TEXT_ALIGN_LEFT: sys::lv_text_align_t = sys::lv_text_align_t_LV_TEXT_ALIGN_LEFT;
pub const TEXT_ALIGN_CENTER: sys::lv_text_align_t = sys::lv_text_align_t_LV_TEXT_ALIGN_CENTER;

pub const LABEL_LONG_WRAP: sys::lv_label_long_mode_t =
    sys::lv_label_long_mode_t_LV_LABEL_LONG_MODE_WRAP;
pub const LABEL_LONG_DOT: sys::lv_label_long_mode_t =
    sys::lv_label_long_mode_t_LV_LABEL_LONG_MODE_DOTS;
pub const LABEL_LONG_CLIP: sys::lv_label_long_mode_t =
    sys::lv_label_long_mode_t_LV_LABEL_LONG_MODE_CLIP;

pub const SCROLLBAR_MODE_ON: sys::lv_scrollbar_mode_t =
    sys::lv_scrollbar_mode_t_LV_SCROLLBAR_MODE_ON;

pub const BORDER_SIDE_FULL: sys::lv_border_side_t = sys::lv_border_side_t_LV_BORDER_SIDE_FULL;

pub const ANIM_OFF: sys::lv_anim_enable_t = false;

/// `LV_SIZE_CONTENT` — `LV_COORD_SET_SPEC(LV_COORD_MAX)`, which bindgen drops
/// because it is a macro over a macro.
pub const SIZE_CONTENT: i32 = (sys::LV_COORD_MAX | (1u32 << 29)) as i32;

/// `LV_SYMBOL_BLUETOOTH` — U+F293 in the Montserrat symbol range.
pub const SYMBOL_BLUETOOTH: &str = "\u{F293}";

/* ---------------------------------------------------------------- fonts */

pub fn font_xs() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_14
}

pub fn font_sm() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_16
}

pub fn font_chain() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_18
}

/// Chain tab label font — smaller in portrait so 6 top tabs fit.
pub fn font_chain_for_mode(portrait: bool) -> *const lv_font_t {
    if portrait {
        font_xs()
    } else {
        font_chain()
    }
}

pub fn font_md() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_20
}

pub fn font_lg() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_24
}

pub fn font_symbol() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_24
}

pub fn font_tuner_note() -> *const lv_font_t {
    &raw const sys::lv_font_montserrat_48
}

/* ---------------------------------------------------------------- misc */

pub fn color_hex(c: u32) -> lv_color_t {
    unsafe { sys::lv_color_hex(c) }
}

pub fn color_eq(a: lv_color_t, b: lv_color_t) -> bool {
    a.red == b.red && a.green == b.green && a.blue == b.blue
}

pub fn pct(v: i32) -> i32 {
    unsafe { sys::lv_pct(v) }
}

pub fn hor_res() -> i32 {
    unsafe { sys::lv_display_get_horizontal_resolution(sys::lv_display_get_default()) }
}

pub fn ver_res() -> i32 {
    unsafe { sys::lv_display_get_vertical_resolution(sys::lv_display_get_default()) }
}

pub fn screen_active() -> *mut lv_obj_t {
    unsafe { sys::lv_screen_active() }
}

/* ------------------------------------------------------------- widgets */

pub fn obj_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_obj_create(parent) }
}

pub fn label_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_label_create(parent) }
}

pub fn button_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_button_create(parent) }
}

pub fn slider_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_slider_create(parent) }
}

pub fn switch_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_switch_create(parent) }
}

pub fn image_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_image_create(parent) }
}

pub fn image_set_src(obj: *mut lv_obj_t, src: *const c_void) {
    unsafe { sys::lv_image_set_src(obj, src) }
}

/* ------------------------------------------------------------ geometry */

pub fn set_size(obj: *mut lv_obj_t, w: i32, h: i32) {
    unsafe { sys::lv_obj_set_size(obj, w, h) }
}

pub fn set_width(obj: *mut lv_obj_t, w: i32) {
    unsafe { sys::lv_obj_set_width(obj, w) }
}

pub fn set_height(obj: *mut lv_obj_t, h: i32) {
    unsafe { sys::lv_obj_set_height(obj, h) }
}

pub fn set_pos(obj: *mut lv_obj_t, x: i32, y: i32) {
    unsafe { sys::lv_obj_set_pos(obj, x, y) }
}

pub fn set_x(obj: *mut lv_obj_t, x: i32) {
    unsafe { sys::lv_obj_set_x(obj, x) }
}

pub fn get_x(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_obj_get_x(obj) }
}

pub fn get_width(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_obj_get_width(obj) }
}

pub fn get_height(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_obj_get_height(obj) }
}

pub fn get_content_width(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_obj_get_content_width(obj) }
}

pub fn get_coords(obj: *const lv_obj_t) -> lv_area_t {
    let mut area = lv_area_t::default();
    unsafe { sys::lv_obj_get_coords(obj, &mut area) };
    area
}

pub fn area_width(area: &lv_area_t) -> i32 {
    unsafe { sys::lv_area_get_width(area) }
}

pub fn align(obj: *mut lv_obj_t, a: sys::lv_align_t, x_ofs: i32, y_ofs: i32) {
    unsafe { sys::lv_obj_align(obj, a, x_ofs, y_ofs) }
}

pub fn center(obj: *mut lv_obj_t) {
    unsafe { sys::lv_obj_center(obj) }
}

pub fn update_layout(obj: *const lv_obj_t) {
    unsafe { sys::lv_obj_update_layout(obj) }
}

pub fn invalidate(obj: *const lv_obj_t) {
    unsafe { sys::lv_obj_invalidate(obj) };
}

pub fn get_scroll_bottom(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_obj_get_scroll_bottom(obj) }
}

pub fn scroll_to(obj: *mut lv_obj_t, x: i32, y: i32, anim: sys::lv_anim_enable_t) {
    unsafe { sys::lv_obj_scroll_to(obj, x, y, anim) }
}

pub fn set_scroll_dir(obj: *mut lv_obj_t, dir: sys::lv_dir_t) {
    unsafe { sys::lv_obj_set_scroll_dir(obj, dir) }
}

pub fn set_scrollbar_mode(obj: *mut lv_obj_t, mode: sys::lv_scrollbar_mode_t) {
    unsafe { sys::lv_obj_set_scrollbar_mode(obj, mode) }
}

pub fn set_flex_flow(obj: *mut lv_obj_t, flow: sys::lv_flex_flow_t) {
    unsafe { sys::lv_obj_set_flex_flow(obj, flow) }
}

pub fn set_flex_align(
    obj: *mut lv_obj_t,
    main: sys::lv_flex_align_t,
    cross: sys::lv_flex_align_t,
    track: sys::lv_flex_align_t,
) {
    unsafe { sys::lv_obj_set_flex_align(obj, main, cross, track) }
}

pub fn set_flex_grow(obj: *mut lv_obj_t, grow: u8) {
    unsafe { sys::lv_obj_set_flex_grow(obj, grow) }
}

/* --------------------------------------------------------- flags/states */

pub fn add_flag(obj: *mut lv_obj_t, f: sys::lv_obj_flag_t) {
    unsafe { sys::lv_obj_add_flag(obj, f) }
}

pub fn remove_flag(obj: *mut lv_obj_t, f: sys::lv_obj_flag_t) {
    unsafe { sys::lv_obj_remove_flag(obj, f) }
}

pub fn has_flag(obj: *const lv_obj_t, f: sys::lv_obj_flag_t) -> bool {
    unsafe { sys::lv_obj_has_flag(obj, f) }
}

pub fn add_state(obj: *mut lv_obj_t, state: sys::lv_state_t) {
    unsafe { sys::lv_obj_add_state(obj, state) }
}

pub fn remove_state(obj: *mut lv_obj_t, state: sys::lv_state_t) {
    unsafe { sys::lv_obj_remove_state(obj, state) }
}

pub fn has_state(obj: *const lv_obj_t, state: sys::lv_state_t) -> bool {
    unsafe { sys::lv_obj_has_state(obj, state) }
}

pub fn set_enabled(obj: *mut lv_obj_t, enabled: bool) {
    if enabled {
        remove_state(obj, STATE_DISABLED);
    } else {
        add_state(obj, STATE_DISABLED);
    }
}

pub fn set_hidden(obj: *mut lv_obj_t, hidden: bool) {
    if hidden {
        add_flag(obj, FLAG_HIDDEN);
    } else {
        remove_flag(obj, FLAG_HIDDEN);
    }
}

/* ------------------------------------------------------------- styles */

pub fn set_style_bg_color(obj: *mut lv_obj_t, v: lv_color_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_bg_color(obj, v, sel) }
}

pub fn set_style_bg_opa(obj: *mut lv_obj_t, v: lv_opa_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_bg_opa(obj, v, sel) }
}

pub fn set_style_border_color(obj: *mut lv_obj_t, v: lv_color_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_border_color(obj, v, sel) }
}

pub fn set_style_border_width(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_border_width(obj, v, sel) }
}

pub fn set_style_border_opa(obj: *mut lv_obj_t, v: lv_opa_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_border_opa(obj, v, sel) }
}

pub fn set_style_border_side(obj: *mut lv_obj_t, v: sys::lv_border_side_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_border_side(obj, v, sel) }
}

pub fn set_style_border_post(obj: *mut lv_obj_t, v: bool, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_border_post(obj, v, sel) }
}

pub fn set_style_pad_all(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    // `lv_obj_set_style_pad_all` is `static inline`, so bindgen never sees it.
    set_style_pad_left(obj, v, sel);
    set_style_pad_right(obj, v, sel);
    set_style_pad_top(obj, v, sel);
    set_style_pad_bottom(obj, v, sel);
}

pub fn set_style_pad_top(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_pad_top(obj, v, sel) }
}

pub fn set_style_pad_bottom(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_pad_bottom(obj, v, sel) }
}

pub fn set_style_pad_left(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_pad_left(obj, v, sel) }
}

pub fn set_style_pad_right(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_pad_right(obj, v, sel) }
}

pub fn set_style_pad_row(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_pad_row(obj, v, sel) }
}

pub fn set_style_pad_column(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_pad_column(obj, v, sel) }
}

pub fn set_style_radius(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_radius(obj, v, sel) }
}

pub fn set_style_shadow_width(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_shadow_width(obj, v, sel) }
}

pub fn set_style_text_color(obj: *mut lv_obj_t, v: lv_color_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_text_color(obj, v, sel) }
}

pub fn set_style_text_font(obj: *mut lv_obj_t, v: *const lv_font_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_text_font(obj, v, sel) }
}

pub fn set_style_text_align(obj: *mut lv_obj_t, v: sys::lv_text_align_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_text_align(obj, v, sel) }
}

pub fn set_style_opa(obj: *mut lv_obj_t, v: lv_opa_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_opa(obj, v, sel) }
}

pub fn set_style_width(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_width(obj, v, sel) }
}

pub fn set_style_height(obj: *mut lv_obj_t, v: i32, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_height(obj, v, sel) }
}

pub fn set_style_image_recolor(obj: *mut lv_obj_t, v: lv_color_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_image_recolor(obj, v, sel) }
}

pub fn set_style_image_recolor_opa(obj: *mut lv_obj_t, v: lv_opa_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_set_style_image_recolor_opa(obj, v, sel) }
}

pub fn remove_style_all(obj: *mut lv_obj_t) {
    unsafe { sys::lv_obj_remove_style_all(obj) }
}

pub fn remove_theme(obj: *mut lv_obj_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_remove_theme(obj, sel) }
}

pub fn add_style(obj: *mut lv_obj_t, style: *const lv_style_t, sel: lv_style_selector_t) {
    unsafe { sys::lv_obj_add_style(obj, style, sel) }
}

/// Allocate + `lv_style_init` a style that lives for the rest of the run.
pub fn style_new() -> *mut lv_style_t {
    let style = Box::leak(Box::new(unsafe { core::mem::zeroed::<lv_style_t>() }));
    unsafe { sys::lv_style_init(style) };
    style
}

pub fn style_set_bg_color(style: *mut lv_style_t, v: lv_color_t) {
    unsafe { sys::lv_style_set_bg_color(style, v) }
}

pub fn style_set_bg_opa(style: *mut lv_style_t, v: lv_opa_t) {
    unsafe { sys::lv_style_set_bg_opa(style, v) }
}

pub fn style_set_border_width(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_border_width(style, v) }
}

pub fn style_set_shadow_width(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_shadow_width(style, v) }
}

pub fn style_set_radius(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_radius(style, v) }
}

pub fn style_set_pad_top(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_pad_top(style, v) }
}

pub fn style_set_pad_bottom(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_pad_bottom(style, v) }
}

pub fn style_set_pad_left(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_pad_left(style, v) }
}

pub fn style_set_pad_right(style: *mut lv_style_t, v: i32) {
    unsafe { sys::lv_style_set_pad_right(style, v) }
}

/* -------------------------------------------------------------- labels */

pub fn label_set_text(obj: *mut lv_obj_t, text: &str) {
    let c = CString::new(text).unwrap_or_default();
    unsafe { sys::lv_label_set_text(obj, c.as_ptr()) }
}

pub fn label_set_text_c(obj: *mut lv_obj_t, text: *const c_char) {
    if text.is_null() {
        return;
    }
    unsafe { sys::lv_label_set_text(obj, text) }
}

pub fn label_get_text(obj: *const lv_obj_t) -> Option<&'static str> {
    let p = unsafe { sys::lv_label_get_text(obj) };
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

pub fn label_set_long_mode(obj: *mut lv_obj_t, mode: sys::lv_label_long_mode_t) {
    unsafe { sys::lv_label_set_long_mode(obj, mode) }
}

/* ------------------------------------------------------------- sliders */

pub fn slider_set_value(obj: *mut lv_obj_t, value: i32, anim: sys::lv_anim_enable_t) {
    unsafe { sys::lv_slider_set_value(obj, value, anim) }
}

pub fn slider_set_range(obj: *mut lv_obj_t, min: i32, max: i32) {
    unsafe { sys::lv_slider_set_range(obj, min, max) }
}

pub fn slider_get_value(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_slider_get_value(obj) }
}

pub fn slider_get_min_value(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_slider_get_min_value(obj) }
}

pub fn slider_get_max_value(obj: *const lv_obj_t) -> i32 {
    unsafe { sys::lv_slider_get_max_value(obj) }
}

/* ---------------------------------------------------------------- tree */

pub fn get_child(obj: *const lv_obj_t, idx: i32) -> *mut lv_obj_t {
    unsafe { sys::lv_obj_get_child(obj, idx) }
}

pub fn get_parent(obj: *const lv_obj_t) -> *mut lv_obj_t {
    unsafe { sys::lv_obj_get_parent(obj) }
}

pub fn obj_clean(obj: *mut lv_obj_t) {
    unsafe { sys::lv_obj_clean(obj) }
}

/// `lv_obj_move_foreground` is `static inline` in `lv_api_map_v8.h`, so it is
/// re-expressed here on top of the functions bindgen does see.
pub fn move_foreground(obj: *mut lv_obj_t) {
    let parent = unsafe { sys::lv_obj_get_parent(obj) };
    if parent.is_null() {
        return;
    }
    let count = unsafe { sys::lv_obj_get_child_count(parent) };
    unsafe { sys::lv_obj_move_to_index(obj, count as i32 - 1) };
}

/* -------------------------------------------------------------- timers */

pub fn timer_create(cb: sys::lv_timer_cb_t, period_ms: u32, user_data: *mut c_void) -> *mut lv_timer_t {
    unsafe { sys::lv_timer_create(cb, period_ms, user_data) }
}

pub fn timer_delete(timer: *mut lv_timer_t) {
    unsafe { sys::lv_timer_delete(timer) }
}

pub fn timer_set_repeat_count(timer: *mut lv_timer_t, count: i32) {
    unsafe { sys::lv_timer_set_repeat_count(timer, count) }
}

/* --------------------------------------------------------- input / events */

pub fn indev_point() -> Option<lv_point_t> {
    let indev = unsafe { sys::lv_indev_active() };
    if indev.is_null() {
        return None;
    }
    let mut pt = lv_point_t::default();
    unsafe { sys::lv_indev_get_point(indev, &mut pt) };
    Some(pt)
}

pub fn event_code(e: *mut lv_event_t) -> sys::lv_event_code_t {
    unsafe { sys::lv_event_get_code(e) }
}

pub fn event_user_data(e: *mut lv_event_t) -> usize {
    unsafe { sys::lv_event_get_user_data(e) as usize }
}

pub fn event_current_target(e: *mut lv_event_t) -> *mut lv_obj_t {
    unsafe { sys::lv_event_get_current_target_obj(e) }
}

pub fn add_event_cb(
    obj: *mut lv_obj_t,
    cb: extern "C" fn(*mut lv_event_t),
    filter: sys::lv_event_code_t,
    user_data: usize,
) {
    unsafe { sys::lv_obj_add_event_cb(obj, Some(cb), filter, user_data as *mut c_void) };
}
