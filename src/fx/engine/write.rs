//! C304 write bodies for the `nano_fx_write_*` C ABI entry points.

use esp_idf_svc::sys::ESP_OK;
use log::info;

use nano_wire::builders;

use super::parse::{apply_model, store_catalog_model_id};
use super::refresh::enqueue_param_refresh;
use super::state::{c_label, clip, fx, fx_publish, now_us, push_fx_ui, SLOT_CFG, TAG};
use crate::fx::models::*;
use crate::fx::params::{clampf, display_to_normalized, format_param, normalized_to_display};
use crate::fx::types::*;
use crate::fx::util::c_str;

pub(super) fn write_gate_enabled(enabled: bool) -> bool {
    let s = fx();
    s.fx.gate.on = enabled;
    s.fx.gate.on_known = true;
    s.fx.gate.valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 16];
    let n = builders::global_toggle_payload(0x09, if enabled { 0x00 } else { 0x01 }, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }
    info!(target: TAG, "Gate {}", if enabled { "ON" } else { "OFF" });
    push_fx_ui();
    if crate::ble::nano_ble_write_c304(
        if enabled { c"Gate ON" } else { c"Gate OFF" },
        &buf[..n],
    ) != ESP_OK {
        return false;
    }
    s.gate_assert_us = now_us();
    s.gate_assert_on = i32::from(enabled);
    true
}

/// Tell the Nano which FX slot the Amount knob should control.
pub(super) fn write_fx_focus(model_slot: u8) -> bool {
    if model_slot as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }
    let mut buf = [0u8; 16];
    let n = builders::fx_slot_focus_payload(model_slot, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }
    let label = c_label(&clip(
        &format!("FX focus {}", BLOCK_LABELS[model_slot as usize]),
        31,
    ));
    crate::ble::nano_ble_write_c304(&label, &buf[..n]) == ESP_OK
}

pub(super) fn write_gate_reduction(pct: u8) -> bool {
    let pct = pct.min(100);
    let s = fx();
    s.fx.gate.reduction_pct = pct;
    s.fx.gate.reduction_valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 20];
    let n = builders::gate_reduction_payload(pct, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }

    push_fx_ui();
    if crate::ble::nano_ble_write_c304(&c_label(&format!("Gate amount {pct}%")), &buf[..n]) != ESP_OK {
        return false;
    }
    s.gate_amount_assert_us = now_us();
    s.gate_amount_assert_pct = pct as i32;
    // Amount writes must not let a stale poll flip bypass while the user is editing.
    if s.gate_assert_on < 0 {
        s.gate_assert_us = s.gate_amount_assert_us;
        s.gate_assert_on = i32::from(s.fx.gate.on);
    }
    true
}

/// Load model on device, enable block, queue param refresh.
pub(super) fn write_block_model(block: u32, model: &NanoFxModelDef) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT || !is_model_allowed(block, model) {
        return false;
    }

    if !crate::ble::nano_ble_connected() {
        return false;
    }

    let idx = block as usize;
    let mut buf = [0u8; 16];
    let n = builders::fx_model_select_payload(SLOT_CFG[idx].model_slot, model.id_bytes(), &mut buf);
    if n == 0 {
        return false;
    }

    let label = clip(
        &format!("{} model {}", BLOCK_LABELS[idx], model_name(model)),
        55,
    );
    info!(target: TAG, "{label}");
    if crate::ble::nano_ble_write_c304(&c_label(&label), &buf[..n]) != ESP_OK {
        return false;
    }

    let s = fx();
    let b = &mut s.fx.blocks[idx];
    store_catalog_model_id(b, model);
    let Some(model) = crate::fx::models::find_model(model.id_bytes()) else {
        return false;
    };
    apply_model(b, Some(model));
    b.valid = true;
    b.params_loaded = false;
    s.block_model_write_us[idx] = now_us();
    s.block_trusted_model[idx] = model as *const NanoFxModelDef;
    s.fx.valid = true;

    if !s.fx.blocks[idx].on {
        let mut en_buf = [0u8; 16];
        let en_n = builders::global_toggle_payload(SLOT_CFG[idx].enable_slot, 0x00, &mut en_buf);
        if en_n > 0 {
            s.fx.blocks[idx].on = true;
            s.block_on_asserted_us[idx] = now_us();
            let _ = crate::ble::nano_ble_write_c304(
                &c_label(&clip(&format!("{} ON", BLOCK_LABELS[idx]), 31)),
                &en_buf[..en_n],
            );
        }
    }

    enqueue_param_refresh(block);
    push_fx_ui();
    true
}

pub(super) fn write_block_enabled(block: u32, enabled: bool) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let idx = block as usize;
    let s = fx();
    s.fx.blocks[idx].on = enabled;
    s.fx.blocks[idx].valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 16];
    let n = builders::global_toggle_payload(
        SLOT_CFG[idx].enable_slot,
        if enabled { 0x00 } else { 0x01 },
        &mut buf,
    );
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }

    let label = clip(
        &format!(
            "{} {}",
            BLOCK_LABELS[idx],
            if enabled { "ON" } else { "OFF" }
        ),
        31,
    );
    info!(target: TAG, "{label}");
    push_fx_ui();

    if crate::ble::nano_ble_write_c304(&c_label(&label), &buf[..n]) != ESP_OK {
        return false;
    }

    if enabled {
        s.block_on_asserted_us[idx] = now_us();
        enqueue_param_refresh(block);
    } else {
        s.block_on_asserted_us[idx] = 0;
        // Keep last param values for the UI while bypassed; preset sync forces
        // a reload when values are actually stale.
    }
    true
}

pub(super) fn write_param_normalized(block: u32, param_index: u8, normalized: f32) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let idx = block as usize;
    let s = fx();
    let Some(model) = model_ref(s.fx.blocks[idx].model) else {
        return false;
    };
    if param_index >= s.fx.blocks[idx].param_count {
        return false;
    }

    let name = c_str(unsafe { *model.param_names.add(param_index as usize) });
    let normalized = clampf(normalized, 0.0, 1.0);
    let param = &mut s.fx.blocks[idx].params[param_index as usize];
    param.normalized = normalized;
    param.display = normalized_to_display(name, normalized);
    param.valid = true;
    s.fx.valid = true;
    fx_publish();

    let mut buf = [0u8; 24];
    let n = builders::fx_param_payload(SLOT_CFG[idx].model_slot, param_index, normalized, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }

    let value = clip(&format_param(&s.fx.blocks[idx], param_index), 15);
    let label = clip(&format!("{} {name}={value}", BLOCK_LABELS[idx]), 47);
    crate::ble::nano_ble_write_c304(&c_label(&label), &buf[..n]) == ESP_OK
}

pub(super) fn write_param_display(block: u32, param_index: u8, display_value: f32) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let s = fx();
    let b = &s.fx.blocks[block as usize];
    let Some(model) = model_ref(b.model) else {
        return false;
    };
    if param_index >= b.param_count {
        return false;
    }

    let name = c_str(unsafe { *model.param_names.add(param_index as usize) });
    let normalized = display_to_normalized(name, display_value);
    write_param_normalized(block, param_index, normalized)
}
