/* ---------------------------------------------------------- device writes */

use super::*;
use super::consts::{DRAG_WRITE_INTERVAL_US, KNOB_COUNT};
use super::helpers::{c_str, fx_state, now_us};
use super::state::ui;
pub(super) fn try_send_gate_locked(pct_val: u8, force_send: bool) {
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

    if nano_fx_write_gate_reduction(pct_val) {
        u.gate_last_sent = pct_val as i32;
        u.gate_last_send_us = now;
    }
}

pub(super) fn try_send_cap_vol_locked(raw: u8, force_send: bool) {
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

    if nano_state_write_capture_volume(raw) {
        u.cap_vol_last_sent = raw as i32;
        u.cap_vol_last_send_us = now;
    }
}

pub(super) fn try_send_gate_on_locked(on: bool, force_send: bool) {
    let u = ui();
    let now = now_us();
    let sent = if on { 1 } else { 0 };
    if !force_send
        && sent == u.gate_on_last_sent
        && (now - u.gate_on_last_send_us) < DRAG_WRITE_INTERVAL_US
    {
        return;
    }

    if nano_fx_write_gate_enabled(on) {
        u.gate_on_last_sent = sent;
        u.gate_on_last_send_us = now;
    }
}

pub(super) fn try_send_knob_locked(idx: usize, raw: u8, force: bool) {
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

pub(super) fn try_send_fx_param_locked(display_i: usize, slider_val: i32, force: bool) {
    let u = ui();
    if u.fx_ui_sync || !u.ble_connected || display_i >= NANO_FX_MAX_PARAMS {
        return;
    }

    let Some(fx) = (unsafe { fx_state().as_ref() }) else {
        return;
    };
    let block = &fx.blocks[u.fx_selected as usize];
    if block.model.is_null() || display_i >= block.param_count as usize {
        return;
    }

    let wire = unsafe { block.model.as_ref() }
        .map(|m| nano_fx_param_wire_index(m, display_i))
        .unwrap_or(display_i);
    if wire >= block.param_count as usize {
        return;
    }

    let pname = c_str(unsafe { (*block.model).param_names.add(wire).read() });
    let now = now_us();
    if !force
        && slider_val == u.fx_last_sent[display_i]
        && now - u.fx_last_send_us[display_i] < DRAG_WRITE_INTERVAL_US
    {
        return;
    }

    let display = nano_fx_slider_to_display(pname, slider_val);
    if nano_fx_write_param_display(u.fx_selected, wire as u8, display) {
        u.fx_last_sent[display_i] = slider_val;
        u.fx_last_send_us[display_i] = now;
    }
}

