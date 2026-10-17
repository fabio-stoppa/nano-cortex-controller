/* ------------------------------------------------------------ build tree */

use super::*;
use super::ble::{ble_icon_timer_cb, build_connecting_overlay, refresh_ble_icon_locked};
use super::chain::{
    chain_block_btn_event_cb, chain_btn_prepare, chain_set_block_label, icon_kind_glyph,
    make_content_panel, show_content_for_chain, show_tempo_view, show_tuner_view,
    tuner_hdr_btn_event_cb, ui_bring_chrome_to_front, ui_no_scroll_only, ui_param_cell_container,
    ui_passive_container, update_chain_blocks_locked,
};
use super::consts::{
    CHAIN_AMP, CHAIN_BLOCK_COUNT, CHAIN_LABEL_MAX, CHAIN_RAIL_BOTTOM_PAD, CHAIN_RAIL_H,
    CHAIN_RAIL_PAD, CHAIN_RAIL_W, CHAIN_TAB_GAP, CHAIN_TAB_H, CHAIN_TAB_H_VERT, CHAIN_TAB_W,
    CONTENT_CHAIN_GAP, HEADER_DD_H, HEADER_H, HEADER_RIGHT_PAD, IconKind, KNOB_COUNT, LayoutMode,
    PARAM_CELL_AMP_TAG, PARAM_CELL_CAP_VOL_TAG, PARAM_CELL_GATE_ON_TAG, PARAM_CELL_GATE_TAG,
    PARAM_GRID_COLS, PARAM_GRID_GAP, PARAM_GRID_PAD, PARAM_GRID_ROWS, PARAM_GRID_SCROLLBAR_W,
    UI_EDGE_PAD,
};
use super::dispatch::ui_init_dispatch;
use super::fx_panel::{apply_fx_locked, build_fx_panel};
use super::gestures::param_cell_event_cb;
use super::helpers::{amp_state, fx_state, tuner_state};
use super::params::{
    create_pedal_params_grid, param_slider_width, style_compact_h_slider, style_param_cell,
    style_param_name_label, style_param_value_label,
};
use super::picker::{build_fx_model_picker, fx_model_header_btn_event_cb};
use super::prefs::save_layout_mode;
use super::state::{ui, AmpKnobUi, ChainBlockUi, FxParamUi, GateCellUi, note_lvgl_task, BUILD_DONE};
use log::error;
use super::tempo::{build_tempo_panel, build_tuner_panel};
use super::tuner::{apply_tuner_locked, bpm_hdr_btn_event_cb};
pub(super) fn style_screen(screen: *mut lv_obj_t) {
    lv::set_style_bg_color(screen, lv::color_hex(0x121212), 0);
    lv::set_style_bg_opa(screen, lv::OPA_COVER, 0);
    lv::set_style_pad_all(screen, UI_EDGE_PAD, 0);
}

pub(super) fn param_grid_cols() -> i32 {
    if ui().layout_mode.is_portrait() {
        3
    } else {
        PARAM_GRID_COLS
    }
}

pub(super) fn param_grid_rows() -> i32 {
    // Portrait has more vertical room — more rows keep each cell shorter.
    if ui().layout_mode.is_portrait() {
        8
    } else {
        PARAM_GRID_ROWS
    }
}

pub(super) fn param_grid_visible() -> u8 {
    (param_grid_cols() * param_grid_rows()) as u8
}

pub(super) fn calc_param_cell_width(area_w: i32, cols: i32) -> i32 {
    let cols = cols.max(1);
    let area = area_w - 2 * PARAM_GRID_PAD;
    let gaps = (cols - 1) * PARAM_GRID_GAP;
    (area - gaps) / cols
}

pub(super) fn calc_param_cell_height(area_h: i32, rows: i32) -> i32 {
    let rows = rows.max(1);
    let area = area_h - 2 * PARAM_GRID_PAD;
    let gaps = (rows - 1) * PARAM_GRID_GAP;
    (area - gaps) / rows
}

pub(super) fn make_tuner_hdr_btn(parent: *mut lv_obj_t) -> *mut lv_obj_t {
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

pub(super) fn init_fx_model_btn_style() -> *mut lv_style_t {
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

pub(super) fn style_fx_model_header_btn(btn: *mut lv_obj_t) {
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

pub(super) fn build_header_bar(screen: *mut lv_obj_t) {
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

    let narrow = lv::hor_res() < 600;
    let left_w = if narrow { 108 } else { 196 };
    let preset_w = if narrow { 96 } else { 184 };
    let right_w = if narrow { 128 } else { 200 };

    let left = lv::obj_create(header);
    lv::set_size(left, left_w, HEADER_H);
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
    lv::set_width(u.preset_lbl, preset_w);
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
    lv::set_size(right, right_w, HEADER_H);
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
    lv::set_style_pad_column(right, if narrow { 8 } else { 14 }, 0);
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

pub(super) fn make_knob_column(parent: *mut lv_obj_t, idx: usize, label: &str) {
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

pub(super) fn make_gate_slider_cell(
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

pub(super) fn build_amp_panel(parent: *mut lv_obj_t) {
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

pub(super) fn build_chain_bar(parent: *mut lv_obj_t, mode: LayoutMode, tab_w: i32, tab_h: i32) {
    lv::set_style_bg_opa(parent, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(parent, 0, 0);
    lv::set_style_pad_all(parent, 0, 0);
    ui_passive_container(parent);

    let list = lv::obj_create(parent);
    ui().chain_list = list;
    lv::set_size(list, lv::pct(100), lv::pct(100));
    lv::set_style_bg_opa(list, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(list, 0, 0);
    style_chain_list(list, mode);
    ui_passive_container(list);

    let use_tab_w = if tab_w > 0 { tab_w } else { CHAIN_TAB_W };
    let use_tab_h = if tab_h > 0 { tab_h } else { CHAIN_TAB_H };
    let icon_w = (use_tab_w - 8).max(28);

    for i in 0..CHAIN_BLOCK_COUNT {
        let col = lv::obj_create(list);
        lv::set_size(col, use_tab_w, use_tab_h);
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
        lv::set_width(icon, icon_w);
        lv::label_set_long_mode(icon, lv::LABEL_LONG_DOT);
        lv::set_style_text_align(icon, lv::TEXT_ALIGN_CENTER, 0);
        lv::set_style_text_font(
            icon,
            lv::font_chain_for_mode(mode.is_portrait()),
            0,
        );
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

#[derive(Clone, Copy)]
pub(super) struct ChromeGeom {
    pub body_h: i32,
    pub chain_w: i32,
    pub chain_h: i32,
    pub chain_x: i32,
    pub chain_y: i32,
    pub content_w: i32,
    pub content_h: i32,
    pub content_x: i32,
    pub content_y: i32,
    pub chain_tab_w: i32,
    pub chain_tab_h: i32,
    pub param_cell_w: i32,
    pub param_cell_h: i32,
}

/// Landscape: left chain rail. Portrait: top horizontal chain strip.
pub(super) fn compute_chrome_geom() -> ChromeGeom {
    let mode = ui().layout_mode;
    let inner_w = lv::hor_res() - 2 * UI_EDGE_PAD;
    let inner_h = lv::ver_res() - 2 * UI_EDGE_PAD;
    let body_h = inner_h - HEADER_H;
    let cols = param_grid_cols();
    let rows = param_grid_rows();

    match mode {
        LayoutMode::Horizontal | LayoutMode::HorizontalFlip => {
            let chain_vpad = PARAM_GRID_PAD + CHAIN_RAIL_BOTTOM_PAD;
            let chain_gap_total = (CHAIN_BLOCK_COUNT as i32 - 1) * CHAIN_TAB_GAP;
            let chain_avail = body_h - chain_vpad - chain_gap_total;
            let chain_tab_h = chain_avail / CHAIN_BLOCK_COUNT as i32;
            let content_w = inner_w - CHAIN_RAIL_W - CONTENT_CHAIN_GAP;
            let content_x = CHAIN_RAIL_W + CONTENT_CHAIN_GAP;
            ChromeGeom {
                body_h,
                chain_w: CHAIN_RAIL_W,
                chain_h: body_h,
                chain_x: 0,
                chain_y: HEADER_H,
                content_w,
                content_h: body_h,
                content_x,
                content_y: HEADER_H,
                chain_tab_w: CHAIN_TAB_W,
                chain_tab_h,
                param_cell_w: calc_param_cell_width(content_w - PARAM_GRID_SCROLLBAR_W, cols),
                param_cell_h: calc_param_cell_height(body_h, rows),
            }
        }
        LayoutMode::Vertical | LayoutMode::VerticalFlip => {
            let chain_h = CHAIN_RAIL_H;
            let content_h = (body_h - chain_h - CONTENT_CHAIN_GAP).max(120);
            let content_y = HEADER_H + chain_h + CONTENT_CHAIN_GAP;
            let chain_hpad = PARAM_GRID_PAD * 2;
            let chain_tab_gap = 4;
            let chain_gap_total = (CHAIN_BLOCK_COUNT as i32 - 1) * chain_tab_gap;
            let chain_avail = (inner_w - chain_hpad - chain_gap_total).max(CHAIN_BLOCK_COUNT as i32);
            let chain_tab_w = chain_avail / CHAIN_BLOCK_COUNT as i32;
            ChromeGeom {
                body_h,
                chain_w: inner_w,
                chain_h,
                chain_x: 0,
                chain_y: HEADER_H,
                content_w: inner_w,
                content_h,
                content_x: 0,
                content_y,
                chain_tab_w,
                chain_tab_h: CHAIN_TAB_H_VERT,
                param_cell_w: calc_param_cell_width(inner_w - PARAM_GRID_SCROLLBAR_W, cols),
                // Cap height so portrait cells stay compact even with fewer params.
                param_cell_h: calc_param_cell_height(content_h, rows).min(78),
            }
        }
    }
}

fn style_chain_list(list: *mut lv_obj_t, mode: LayoutMode) {
    if list.is_null() {
        return;
    }
    match mode {
        LayoutMode::Horizontal | LayoutMode::HorizontalFlip => {
            lv::set_style_pad_left(list, CHAIN_RAIL_PAD, 0);
            lv::set_style_pad_top(list, PARAM_GRID_PAD, 0);
            lv::set_style_pad_bottom(list, CHAIN_RAIL_BOTTOM_PAD, 0);
            lv::set_style_pad_right(list, 0, 0);
            lv::set_style_pad_row(list, CHAIN_TAB_GAP, 0);
            lv::set_style_pad_column(list, 0, 0);
            lv::set_flex_flow(list, lv::FLEX_FLOW_COLUMN);
            lv::set_flex_align(
                list,
                lv::FLEX_ALIGN_START,
                lv::FLEX_ALIGN_CENTER,
                lv::FLEX_ALIGN_CENTER,
            );
        }
        LayoutMode::Vertical | LayoutMode::VerticalFlip => {
            lv::set_style_pad_left(list, PARAM_GRID_PAD, 0);
            lv::set_style_pad_right(list, PARAM_GRID_PAD, 0);
            lv::set_style_pad_top(list, 4, 0);
            lv::set_style_pad_bottom(list, 4, 0);
            lv::set_style_pad_row(list, 0, 0);
            lv::set_style_pad_column(list, 4, 0);
            lv::set_flex_flow(list, lv::FLEX_FLOW_ROW);
            lv::set_flex_align(
                list,
                lv::FLEX_ALIGN_START,
                lv::FLEX_ALIGN_CENTER,
                lv::FLEX_ALIGN_CENTER,
            );
        }
    }
}

fn clear_widget_ptrs_for_rebuild() {
    let u = ui();
    u.ble_icon = ptr::null_mut();
    u.bpm_lbl = ptr::null_mut();
    u.connecting_overlay = ptr::null_mut();
    u.connecting_icon = ptr::null_mut();
    u.connecting_status = ptr::null_mut();
    u.layout_horiz_btn = ptr::null_mut();
    u.layout_horiz_flip_btn = ptr::null_mut();
    u.layout_vert_btn = ptr::null_mut();
    u.layout_vert_flip_btn = ptr::null_mut();
    u.tuner_note = ptr::null_mut();
    u.tuner_cents = ptr::null_mut();
    u.tuner_state = ptr::null_mut();
    u.tuner_btn = ptr::null_mut();
    u.tuner_meter = ptr::null_mut();
    u.tuner_meter_center = ptr::null_mut();
    u.tuner_meter_needle = ptr::null_mut();
    u.knobs = [AmpKnobUi {
        slider: ptr::null_mut(),
        value: ptr::null_mut(),
    }; KNOB_COUNT];
    u.gate_on = GateCellUi {
        col: ptr::null_mut(),
        name: ptr::null_mut(),
        slider: ptr::null_mut(),
        value: ptr::null_mut(),
    };
    u.gate = u.gate_on;
    u.cap_vol = u.gate_on;
    u.header_bar = ptr::null_mut();
    u.preset_lbl = ptr::null_mut();
    u.chain_bar = ptr::null_mut();
    u.chain_list = ptr::null_mut();
    u.content_root = ptr::null_mut();
    u.amp_panel = ptr::null_mut();
    u.fx_panel = ptr::null_mut();
    u.tuner_panel = ptr::null_mut();
    u.tuner_hdr_btn = ptr::null_mut();
    u.tempo_panel = ptr::null_mut();
    u.tempo_bpm_lbl = ptr::null_mut();
    u.tempo_tap_btn = ptr::null_mut();
    u.tempo_scope_sw = ptr::null_mut();
    u.tempo_scope_lbl = ptr::null_mut();
    for i in 0..CHAIN_BLOCK_COUNT {
        u.chain[i] = ChainBlockUi {
            col: ptr::null_mut(),
            btn: ptr::null_mut(),
            icon: ptr::null_mut(),
            icon_text: [0; CHAIN_LABEL_MAX],
            icon_color: lv::color_hex(0),
            last_active: false,
            last_selected: false,
            last_accent: lv::color_hex(0),
        };
    }
    u.fx_model_btn = ptr::null_mut();
    u.fx_model_btn_lbl = ptr::null_mut();
    u.fx_model_picker = ptr::null_mut();
    u.fx_model_picker_grid = ptr::null_mut();
    u.fx_model_picker_open = false;
    u.amp_params_row = ptr::null_mut();
    u.fx_params_row = ptr::null_mut();
    for i in 0..NANO_FX_MAX_PARAMS {
        u.fx_params[i] = FxParamUi {
            col: ptr::null_mut(),
            name: ptr::null_mut(),
            slider: ptr::null_mut(),
            value: ptr::null_mut(),
        };
    }
    u.pedal_params_scroll_row = ptr::null_mut();
    u.pedal_params_scroll_inited = false;
    u.gate_style_valid = false;
    u.amp_chrome_valid = false;
}

fn layout_rotation_degrees(mode: LayoutMode) -> u16 {
    match mode {
        LayoutMode::Horizontal => 0,
        LayoutMode::Vertical => 90,
        LayoutMode::HorizontalFlip => 180,
        LayoutMode::VerticalFlip => 270,
    }
}

/// Switch landscape/portrait: rotate the panel 0°/90° and rebuild the UI tree.
pub(super) fn apply_layout_mode(mode: LayoutMode) {
    let u = ui();
    let prev = u.layout_mode;
    if prev == mode && !u.chain_bar.is_null() {
        refresh_layout_buttons_locked();
        return;
    }

    u.layout_mode = mode;
    let degree = layout_rotation_degrees(mode);
    let err = lv::port_set_rotation(degree);
    if err != ESP_OK {
        error!("lvgl_port_set_rotation({degree}) failed: {err}");
    }
    save_layout_mode(mode);

    if u.header_bar.is_null() && u.connecting_overlay.is_null() {
        return;
    }

    let ble_connected = u.ble_connected;
    let connecting_await_ready = u.connecting_await_ready;
    let chain_selected = u.chain_selected;
    let tuner_view = u.tuner_view;
    let tempo_view = u.tempo_view;
    let ble_blink_on = u.ble_blink_on;

    if !u.fx_build_timer.is_null() {
        lv::timer_delete(u.fx_build_timer);
        u.fx_build_timer = ptr::null_mut();
    }

    let screen = lv::screen_active();
    lv::obj_clean(screen);
    clear_widget_ptrs_for_rebuild();

    u.ble_connected = ble_connected;
    u.connecting_await_ready = connecting_await_ready;
    u.chain_selected = chain_selected;
    u.tuner_view = false;
    u.tempo_view = false;
    u.ble_blink_on = ble_blink_on;
    u.layout_mode = mode;

    build_ui();

    if tuner_view {
        show_tuner_view(true);
    } else if tempo_view {
        show_tempo_view(true, false);
    } else {
        show_content_for_chain(chain_selected);
    }
}

pub(super) fn refresh_layout_buttons_locked() {
    let u = ui();
    let mode = u.layout_mode;
    style_layout_choice_btn(u.layout_horiz_btn, matches!(mode, LayoutMode::Horizontal));
    style_layout_choice_btn(
        u.layout_horiz_flip_btn,
        matches!(mode, LayoutMode::HorizontalFlip),
    );
    style_layout_choice_btn(u.layout_vert_btn, matches!(mode, LayoutMode::Vertical));
    style_layout_choice_btn(
        u.layout_vert_flip_btn,
        matches!(mode, LayoutMode::VerticalFlip),
    );
}

pub(super) fn style_layout_choice_btn(btn: *mut lv_obj_t, selected: bool) {
    if btn.is_null() {
        return;
    }
    if selected {
        lv::add_state(btn, lv::STATE_CHECKED);
        lv::set_style_bg_color(btn, lv::color_hex(0x2A4060), 0);
        lv::set_style_border_color(btn, lv::color_hex(0x80C0FF), 0);
        lv::set_style_text_color(lv::get_child(btn, 0), lv::color_hex(0xE0E0E0), 0);
    } else {
        lv::remove_state(btn, lv::STATE_CHECKED);
        lv::set_style_bg_color(btn, lv::color_hex(0x1A1A1A), 0);
        lv::set_style_border_color(btn, lv::color_hex(0x404040), 0);
        lv::set_style_text_color(lv::get_child(btn, 0), lv::color_hex(0xA0A0A0), 0);
    }
}

pub(super) fn build_ui() {
    let screen = lv::screen_active();
    style_screen(screen);

    let _ = lv::port_set_rotation(layout_rotation_degrees(ui().layout_mode));

    build_header_bar(screen);
    build_fx_model_picker(screen);

    let g = compute_chrome_geom();
    {
        let u = ui();
        u.body_h = g.body_h;
        u.content_w_normal = g.content_w;
        u.content_h_normal = g.content_h;
        u.content_x_normal = g.content_x;
        u.content_y_normal = g.content_y;
        u.param_cell_w = g.param_cell_w;
        u.param_cell_h = g.param_cell_h;
    }

    let chain_bar = lv::obj_create(screen);
    ui().chain_bar = chain_bar;
    lv::set_size(chain_bar, g.chain_w, g.chain_h);
    lv::align(chain_bar, lv::ALIGN_TOP_LEFT, g.chain_x, g.chain_y);
    build_chain_bar(chain_bar, ui().layout_mode, g.chain_tab_w, g.chain_tab_h);

    let content_root = lv::obj_create(screen);
    ui().content_root = content_root;
    lv::set_size(content_root, g.content_w, g.content_h);
    lv::align(
        content_root,
        lv::ALIGN_TOP_LEFT,
        g.content_x,
        g.content_y,
    );
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

    show_content_for_chain(ui().chain_selected);
    build_connecting_overlay(screen);
    ui_bring_chrome_to_front();

    ui_init_dispatch();
    if ui().ble_icon_timer.is_null() {
        ui().ble_icon_timer = lv::timer_create(Some(ble_icon_timer_cb), 400, ptr::null_mut());
    }
    refresh_ble_icon_locked();
    apply_tuner_locked(tuner_state());
    apply_fx_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

pub(super) extern "C" fn ui_build_timer_cb(timer: *mut lv_timer_t) {
    note_lvgl_task();
    build_ui();
    BUILD_DONE.store(true, Ordering::Release);
    lv::timer_delete(timer);
}
