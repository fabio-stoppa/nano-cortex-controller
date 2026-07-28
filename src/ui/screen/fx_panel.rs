/* -------------------------------------------------------------- FX panel */

use super::*;
use super::chain::{ui_param_cell_container, update_chain_blocks_locked};
use super::gestures::param_cell_event_cb;
use super::header::sync_header_model_locked;
use super::helpers::{amp_state, c_str, fx_state, set_label_if_changed, set_slider_if_changed};
use super::params::{
    create_pedal_params_grid, fx_short_param_name, param_slider_width, style_compact_h_slider,
    style_param_cell, style_param_name_label, style_param_value_label,
    update_pedal_params_grid_scroll,
};
use super::state::{ui, FxParamUi};
pub(super) fn apply_fx_param_locked(display_i: usize, block: &NanoFxBlock) {
    if display_i >= NANO_FX_MAX_PARAMS || block.model.is_null() {
        return;
    }

    let u = ui();
    let param = u.fx_params[display_i];
    if param.col.is_null() {
        return;
    }

    if display_i >= block.param_count as usize || block.param_count == 0 {
        lv::add_flag(param.col, lv::FLAG_HIDDEN);
        return;
    }

    let wire = unsafe { block.model.as_ref() }
        .map(|m| nano_fx_param_wire_index(m, display_i))
        .unwrap_or(display_i);
    if wire >= block.param_count as usize {
        lv::add_flag(param.col, lv::FLAG_HIDDEN);
        return;
    }

    lv::remove_flag(param.col, lv::FLAG_HIDDEN);

    let pname = c_str(unsafe { (*block.model).param_names.add(wire).read() });
    set_label_if_changed(param.name, &fx_short_param_name(pname));

    let smin = nano_fx_slider_min(pname);
    let smax = nano_fx_slider_max(pname);
    if lv::slider_get_min_value(param.slider) != smin
        || lv::slider_get_max_value(param.slider) != smax
    {
        lv::slider_set_range(param.slider, smin, smax);
    }

    lv::set_enabled(
        param.slider,
        u.ble_connected && block.on && block.param_count > 0 && block.params_loaded,
    );

    // Wait for real values — never paint midpoints (that was the preset spike).
    if !block.params_loaded || !block.params[wire].valid {
        return;
    }

    if display_i as i32 != u.fx_drag_param {
        let display = block.params[wire].display;
        let slider_val = nano_fx_display_to_slider(pname, display);
        set_slider_if_changed(param.slider, slider_val);
        set_label_if_changed(param.value, &nano_fx_format_param(block, wire as u8));
        u.fx_last_sent[display_i] = slider_val;
    }
}

pub(super) fn apply_fx_locked(fx: *const NanoFxState) {
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
            if known && u.gate_on_drag == 0 && u.gate_drag == 0 {
                let on = if fx_ref.unwrap().gate.on { 1 } else { 0 };
                set_slider_if_changed(u.gate_on.slider, on);
                set_label_if_changed(u.gate_on.value, if on != 0 { "On" } else { "Off" });
            }
        }

        if !u.gate.value.is_null() && !u.gate.slider.is_null() {
            let known = fx_ref
                .map(|f| f.gate.valid && f.gate.reduction_valid)
                .unwrap_or(false);
            if known && u.gate_drag == 0 {
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

    // After a preset change, leave the last painted knobs alone until the
    // visible block's refresh returns — then paint once with final values.
    if crate::fx::nano_fx_params_ui_held() {
        u.fx_ui_sync = false;
        return;
    }

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

pub(super) fn fx_panel_is_visible() -> bool {
    let panel = ui().fx_panel;
    !panel.is_null() && !lv::has_flag(panel, lv::FLAG_HIDDEN)
}

pub(super) fn build_fx_param_cell(i: usize) {
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

pub(super) unsafe extern "C" fn fx_build_timer_cb(timer: *mut lv_timer_t) {
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

pub(super) fn ensure_fx_params_up_to(count: usize) {
    let u = ui();
    let count = count.min(NANO_FX_MAX_PARAMS);

    let need_more = (0..count).any(|j| u.fx_params[j].col.is_null());

    if need_more && u.fx_build_timer.is_null() {
        u.fx_build_timer = lv::timer_create(Some(fx_build_timer_cb), 50, ptr::null_mut());
    }
}

pub(super) fn build_fx_panel(parent: *mut lv_obj_t) {
    ui().fx_params_row = create_pedal_params_grid(parent);
}

