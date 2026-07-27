/* ------------------------------------------------------------ build tree */

use super::*;
use super::ble::{ble_icon_timer_cb, refresh_ble_icon_locked};
use super::chain::{
    chain_block_btn_event_cb, chain_btn_prepare, chain_set_block_label, icon_kind_glyph,
    make_content_panel, show_content_for_chain, tuner_hdr_btn_event_cb, ui_bring_chrome_to_front,
    ui_no_scroll_only, ui_param_cell_container, ui_passive_container, update_chain_blocks_locked,
};
use super::consts::{
    CHAIN_AMP, CHAIN_BLOCK_COUNT, CHAIN_ICON_W, CHAIN_LABEL_MAX, CHAIN_RAIL_BOTTOM_PAD,
    CHAIN_RAIL_PAD, CHAIN_RAIL_W, CHAIN_TAB_GAP, CHAIN_TAB_H, CHAIN_TAB_W, CONTENT_CHAIN_GAP,
    HEADER_DD_H, HEADER_H, HEADER_RIGHT_PAD, IconKind, PARAM_CELL_AMP_TAG, PARAM_CELL_CAP_VOL_TAG,
    PARAM_CELL_GATE_ON_TAG, PARAM_CELL_GATE_TAG, PARAM_GRID_COLS, PARAM_GRID_GAP, PARAM_GRID_PAD,
    PARAM_GRID_ROWS, PARAM_GRID_SCROLLBAR_W, UI_EDGE_PAD,
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
use super::state::{ui, AmpKnobUi, GateCellUi, note_lvgl_task, BUILD_DONE};
use super::tempo::{build_tempo_panel, build_tuner_panel};
use super::tuner::{apply_tuner_locked, bpm_hdr_btn_event_cb};
pub(super) fn style_screen(screen: *mut lv_obj_t) {
    lv::set_style_bg_color(screen, lv::color_hex(0x121212), 0);
    lv::set_style_bg_opa(screen, lv::OPA_COVER, 0);
    lv::set_style_pad_all(screen, UI_EDGE_PAD, 0);
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

pub(super) fn build_chain_bar(parent: *mut lv_obj_t, tab_h: i32) {
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

pub(super) fn build_ui() {
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
    apply_tuner_locked(tuner_state());
    apply_fx_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

pub(super) unsafe extern "C" fn ui_build_timer_cb(timer: *mut lv_timer_t) {
    note_lvgl_task();
    build_ui();
    BUILD_DONE.store(true, Ordering::Release);
    lv::timer_delete(timer);
}

