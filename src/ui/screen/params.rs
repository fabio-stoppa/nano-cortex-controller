/* --------------------------------------------------------- param plumbing */

use super::*;
use super::chain::{chain_slot_is_fx, ui_passive_container, ui_scroll_container};
use super::consts::{
    CHAIN_AMP, PARAM_CELL_PAD, PARAM_CELL_ROW_GAP, PARAM_GRID_GAP, PARAM_GRID_PAD,
    PARAM_LABEL_MAX_LEN, PARAM_SLIDER_INSET_H, PEDAL_PARAM_GRID_VISIBLE,
};
use super::helpers::{c_str, fx_state, pct};
use super::state::ui;
use super::writes::{
    try_send_cap_vol_locked, try_send_fx_param_locked, try_send_gate_locked,
    try_send_gate_on_locked, try_send_knob_locked,
};
pub(super) fn fx_short_param_name(full: &str) -> String {
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

pub(super) fn param_label_width() -> i32 {
    ui().param_cell_w - 2 * PARAM_CELL_PAD
}

pub(super) fn style_param_name_label(lbl: *mut lv_obj_t) {
    lv::set_width(lbl, param_label_width());
    lv::label_set_long_mode(lbl, lv::LABEL_LONG_DOT);
    lv::set_style_text_align(lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::set_style_text_color(lbl, lv::color_hex(0xB0B0B0), 0);
    lv::set_style_text_font(lbl, lv::font_md(), 0);
}

pub(super) fn style_param_value_label(lbl: *mut lv_obj_t) {
    lv::set_width(lbl, param_label_width());
    lv::set_style_text_align(lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::set_style_text_color(lbl, lv::color_hex(0xFFFFFF), 0);
    lv::set_style_text_font(lbl, lv::font_md(), 0);
}

pub(super) fn style_grid_scrollbar(obj: *mut lv_obj_t, active: bool) {
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

pub(super) fn scroll_area_has_overflow(obj: *mut lv_obj_t) -> bool {
    if obj.is_null() {
        return false;
    }
    lv::update_layout(obj);
    lv::get_scroll_bottom(obj) > 0
}

pub(super) fn fx_model_picker_update_scrollbar() {
    let grid = ui().fx_model_picker_grid;
    if grid.is_null() {
        return;
    }
    style_grid_scrollbar(grid, scroll_area_has_overflow(grid));
}

pub(super) fn update_pedal_params_grid_scroll(row: *mut lv_obj_t, param_count: u8) {
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

pub(super) fn pedal_params_row_active() -> *mut lv_obj_t {
    let u = ui();
    if u.chain_selected == CHAIN_AMP {
        return u.amp_params_row;
    }
    if chain_slot_is_fx(u.chain_selected) {
        return u.fx_params_row;
    }
    ptr::null_mut()
}

pub(super) fn pedal_params_can_scroll() -> bool {
    let u = ui();
    let row = pedal_params_row_active();
    !row.is_null() && u.pedal_params_scroll_on && u.pedal_params_scroll_row == row
}

pub(super) fn style_param_grid_container(obj: *mut lv_obj_t) {
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

pub(super) fn create_pedal_params_grid(panel: *mut lv_obj_t) -> *mut lv_obj_t {
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

pub(super) fn style_param_cell(cell: *mut lv_obj_t, w: i32, h: i32) {
    lv::set_size(cell, w, h);
    lv::set_style_bg_color(cell, lv::color_hex(0x1A1A1A), 0);
    lv::set_style_bg_opa(cell, lv::OPA_COVER, 0);
    lv::set_style_border_color(cell, lv::color_hex(0x404040), 0);
    lv::set_style_border_width(cell, 1, 0);
    lv::set_style_radius(cell, 4, 0);
    lv::set_style_pad_all(cell, PARAM_CELL_PAD, 0);
    lv::set_style_pad_row(cell, PARAM_CELL_ROW_GAP, 0);
}

pub(super) fn style_compact_h_slider(slider: *mut lv_obj_t, width: i32, min_v: i32, max_v: i32) {
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

pub(super) fn param_slider_width() -> i32 {
    (ui().param_cell_w - 2 * PARAM_CELL_PAD - 2 * PARAM_SLIDER_INSET_H).max(48)
}

pub(super) fn param_cell_x_to_slider(cell: *mut lv_obj_t, slider: *mut lv_obj_t, abs_x: i32) -> i32 {
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

pub(super) fn param_cell_value_from_x(
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

pub(super) fn param_cell_apply_gate_on(cell: *mut lv_obj_t, force_send: bool) {
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

pub(super) fn param_cell_apply_gate(cell: *mut lv_obj_t, force_send: bool) {
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

pub(super) fn param_cell_apply_cap_vol(cell: *mut lv_obj_t, force_send: bool) {
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

pub(super) fn param_cell_apply_slider(
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
                let wire = unsafe { block.model.as_ref() }
                    .map(|m| nano_fx_param_wire_index(m, idx))
                    .unwrap_or(idx);
                if wire < block.param_count as usize {
                    let pname = c_str(unsafe { (*block.model).param_names.add(wire).read() });
                    if nano_fx_param_is_enum(pname) && nano_fx_param_enum_count(pname) == 2 {
                        binary_half = true;
                    }
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
            let wire = unsafe { block.model.as_ref() }
                .map(|m| nano_fx_param_wire_index(m, idx))
                .unwrap_or(idx);
            if wire < block.param_count as usize {
                let pname = c_str(unsafe { (*block.model).param_names.add(wire).read() });
                let mut tmp = *block;
                tmp.params[wire].display = nano_fx_slider_to_display(pname, val);
                tmp.params[wire].valid = true;
                lv::label_set_text(u.fx_params[idx].value, &nano_fx_format_param(&tmp, wire as u8));
            }
        }
    }

    if u.fx_drag_param == idx as i32 {
        try_send_fx_param_locked(idx, val, force_send);
    }
}

