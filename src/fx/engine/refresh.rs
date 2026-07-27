//! Param-refresh queue and the esp_timer callbacks that drive it.

use core::ffi::{c_void, CStr};
use core::ptr;

use esp_idf_svc::sys::{
    esp_timer_create, esp_timer_create_args_t, esp_timer_handle_t, esp_timer_start_once,
    esp_timer_stop, ESP_OK,
};
use log::{info, warn};

use nano_wire::builders;

use super::state::{c_label, clip, fx, now_us, push_fx_ui, FX_REFRESH_TIMEOUT_US, SLOT_CFG, TAG};
use crate::fx::models::{block_label, block_model_text, BLOCK_LABELS};
use crate::fx::types::*;

fn block_needs_refresh(block: &NanoFxBlock) -> bool {
    block.on && !block.params_loaded
}

pub(super) fn block_model_write_recent(block: u32) -> bool {
    let s = fx();
    let idx = block as usize;
    if idx >= NANO_FX_BLOCK_COUNT || s.block_model_write_us[idx] == 0 {
        return false;
    }
    now_us() - s.block_model_write_us[idx] < super::state::FX_MODEL_WRITE_ASSERT_US
}

/// Model selected via UI/BLE write; honor only inside the assert window.
pub(super) fn trusted_model_for(block: u32) -> Option<&'static NanoFxModelDef> {
    let s = fx();
    let idx = block as usize;
    if idx >= NANO_FX_BLOCK_COUNT {
        return None;
    }
    let trusted = s.block_trusted_model[idx];
    if trusted.is_null() {
        return None;
    }
    if !block_model_write_recent(block) {
        s.block_trusted_model[idx] = ptr::null();
        s.block_model_write_us[idx] = 0;
        return None;
    }
    // SAFETY: pointer is always a `&'static NanoFxModelDef` from the catalog.
    Some(unsafe { &*trusted })
}

/// Drop write-assert trust so the next preset dump can replace models.
pub(super) fn clear_model_write_trust() {
    let s = fx();
    s.block_model_write_us = [0; NANO_FX_BLOCK_COUNT];
    s.block_trusted_model = [ptr::null(); NANO_FX_BLOCK_COUNT];
    s.block_on_asserted_us = [0; NANO_FX_BLOCK_COUNT];
}

fn create_timer(handle: &mut esp_timer_handle_t, cb: unsafe extern "C" fn(*mut c_void), name: &CStr) {
    if !handle.is_null() {
        return;
    }
    // Zeroed rest mirrors the C designated initializer (dispatch = ESP_TIMER_TASK).
    let mut args: esp_timer_create_args_t = unsafe { core::mem::zeroed() };
    args.callback = Some(cb);
    args.name = name.as_ptr();
    let err = unsafe { esp_timer_create(&args, handle) };
    if err != ESP_OK {
        panic!("esp_timer_create failed: {err}");
    }
}

pub(super) fn ensure_refresh_timers() {
    let s = fx();
    create_timer(&mut s.refresh_timeout_timer, refresh_timeout_cb, c"nano_fx_to");
    create_timer(&mut s.refresh_gap_timer, refresh_gap_cb, c"nano_fx_gap");
    create_timer(&mut s.begin_refresh_timer, begin_refresh_cb, c"nano_fx_begin");
}

unsafe extern "C" fn refresh_timeout_cb(_arg: *mut c_void) {
    let s = fx();
    warn!(
        target: TAG,
        "FX param refresh timed out for {}", block_label(s.pending_block)
    );
    s.refresh_active = false;
    send_next_refresh();
}

unsafe extern "C" fn refresh_gap_cb(_arg: *mut c_void) {
    send_next_refresh();
}

unsafe extern "C" fn begin_refresh_cb(_arg: *mut c_void) {
    run_begin_param_refresh();
}

fn block_in_refresh_queue(block: u32) -> bool {
    let s = fx();
    s.refresh_queue[s.refresh_q_head..s.refresh_q_len]
        .iter()
        .any(|&queued| queued == block)
}

fn merge_refresh_queue_from_blocks() {
    let s = fx();
    for i in 0..NANO_FX_BLOCK_COUNT {
        if !block_needs_refresh(&s.fx.blocks[i]) {
            continue;
        }
        if block_in_refresh_queue(i as u32) {
            continue;
        }
        if s.refresh_q_len < NANO_FX_BLOCK_COUNT {
            s.refresh_queue[s.refresh_q_len] = i as u32;
            s.refresh_q_len += 1;
        }
    }
}

pub(super) fn enqueue_param_refresh(block: u32) {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }

    ensure_refresh_timers();
    let s = fx();
    s.fx.blocks[block as usize].params_loaded = false;

    if block_in_refresh_queue(block) {
        if !s.refresh_active {
            send_next_refresh();
        }
        return;
    }

    if s.refresh_q_len < NANO_FX_BLOCK_COUNT {
        s.refresh_queue[s.refresh_q_len] = block;
        s.refresh_q_len += 1;
    }

    if !s.refresh_active {
        send_next_refresh();
    }
}

fn send_next_refresh() {
    ensure_refresh_timers();
    let s = fx();

    while s.refresh_q_head < s.refresh_q_len {
        let block_id = s.refresh_queue[s.refresh_q_head];
        let idx = block_id as usize;
        s.refresh_q_head += 1;

        if !s.fx.blocks[idx].on || !block_needs_refresh(&s.fx.blocks[idx]) {
            continue;
        }

        s.pending_block = block_id;
        s.refresh_active = true;

        let mut buf = [0u8; 16];
        let n = builders::fx_param_refresh_payload(SLOT_CFG[idx].refresh_slot, &mut buf);
        if n == 0 || !crate::ble::nano_ble_connected() {
            s.refresh_active = false;
            continue;
        }

        let label = BLOCK_LABELS[idx];
        let model_text = clip(&block_model_text(&s.fx.blocks[idx]), 31);
        info!(target: TAG, "Refreshing params: {label} ({model_text})");
        let label = c_label(&clip(&format!("{label} param refresh"), 31));
        let _ = crate::ble::nano_ble_write_c304(&label, &buf[..n]);

        unsafe {
            esp_timer_stop(s.refresh_timeout_timer);
            esp_timer_start_once(s.refresh_timeout_timer, FX_REFRESH_TIMEOUT_US);
        }
        return;
    }

    s.refresh_active = false;
    s.refresh_q_len = 0;
    s.refresh_q_head = 0;
    push_fx_ui();
}

fn run_begin_param_refresh() {
    let s = fx();
    if s.refresh_active {
        merge_refresh_queue_from_blocks();
        return;
    }

    s.refresh_q_len = 0;
    s.refresh_q_head = 0;

    for i in 0..NANO_FX_BLOCK_COUNT {
        if block_needs_refresh(&s.fx.blocks[i]) {
            s.refresh_queue[s.refresh_q_len] = i as u32;
            s.refresh_q_len += 1;
        }
    }

    if s.refresh_q_len == 0 {
        info!(target: TAG, "No active FX blocks to refresh");
        push_fx_ui();
        return;
    }

    info!(
        target: TAG,
        "Queueing FX param refresh for {} block(s)", s.refresh_q_len
    );
    send_next_refresh();
}
