/* ------------------------------------------------------- tuner / tempo UI */

use super::*;
use super::chain::ui_passive_container;
use super::consts::{
    KNOB_COUNT, NANO_BPM_MIN, TUNER_COLOR_IDLE, TUNER_COLOR_OK, TUNER_METER_H, TUNER_METER_W,
    TUNER_NEEDLE_W,
};
use super::state::ui;
use super::tuner::{tempo_scope_event_cb, tempo_tap_btn_event_cb, tuner_btn_event_cb};
pub(super) fn build_tuner_panel(parent: *mut lv_obj_t) {
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

pub(super) fn build_tempo_panel(parent: *mut lv_obj_t) {
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

    let bpm = nano_state_get_bpm();
    if bpm >= NANO_BPM_MIN {
        lv::label_set_text(u.tempo_bpm_lbl, &format!("{bpm}"));
    }
}

pub(super) fn set_slider_enabled_locked(enabled: bool) {
    let u = ui();
    for i in 0..KNOB_COUNT {
        if !u.knobs[i].slider.is_null() {
            lv::set_enabled(u.knobs[i].slider, enabled);
        }
    }
}

