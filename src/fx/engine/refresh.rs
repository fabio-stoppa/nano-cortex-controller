//! Param-refresh queue and the esp_timer callbacks that drive it.

use core::ffi::{c_void, CStr};
use core::ptr;

use esp_idf_svc::sys::{
    esp_timer_create, esp_timer_create_args_t, esp_timer_handle_t, esp_timer_start_once,
    esp_timer_stop, ESP_OK,
};
use log::{debug, warn};

use nano_wire::builders;

use super::state::{
    fx, now_us, push_fx_ui, FX_AMOUNT_LIVE_SKIP_US, FX_AMOUNT_UI_GAP_US,
    FX_REFRESH_STALE_GUARD_US, FX_REFRESH_TIMEOUT_US, SLOT_CFG, TAG,
};
use crate::fx::models::{block_label, BLOCK_LABELS};
use crate::fx::types::*;

/// BLE write labels for param refresh — static so esp_timer callbacks avoid `format!`/`CString`.
const PARAM_REFRESH_LABELS: [&CStr; NANO_FX_BLOCK_COUNT] = [
    c"Pre1 param refresh",
    c"Pre2 param refresh",
    c"Post1 param refresh",
    c"Post2 param refresh",
    c"Post3 param refresh",
];

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
    create_timer(&mut s.amount_ui_timer, amount_ui_cb, c"nano_fx_amt");
}

/// Schedule a coalesced param poll for the Amount-focused FX block.
pub(super) fn schedule_amount_ui() {
    ensure_refresh_timers();
    let s = fx();
    unsafe {
        esp_timer_stop(s.amount_ui_timer);
        esp_timer_start_once(s.amount_ui_timer, FX_AMOUNT_UI_GAP_US);
    }
}

unsafe extern "C" fn amount_ui_cb(_arg: *mut c_void) {
    let s = fx();
    let block_id = if s.amount_log_block >= 0 {
        s.amount_log_block
    } else {
        s.pedal_fx_focus
    };
    let raw = s.last_amount_raw;
    let pending = s.amount_log_pending;
    s.amount_log_pending = false;

    if block_id < 0 || (block_id as usize) >= NANO_FX_BLOCK_COUNT {
        return;
    }

    // Live param echo already updated UI — skip a redundant BLE poll this tick.
    if s.live_param_block == block_id
        && s.live_param_us != 0
        && now_us() - s.live_param_us < FX_AMOUNT_LIVE_SKIP_US
    {
        return;
    }

    if pending {
        debug!(
            target: TAG,
            "Amount knob raw {raw} → poll {}",
            BLOCK_LABELS[block_id as usize]
        );
    }

    s.amount_refresh = true;
    enqueue_param_refresh(block_id as u32);
}

unsafe extern "C" fn refresh_timeout_cb(_arg: *mut c_void) {
    let s = fx();
    let was_amount = s.amount_refresh;
    if was_amount {
        debug!(
            target: TAG,
            "FX param refresh timed out for {} (Amount poll)",
            block_label(s.pending_block)
        );
    } else {
        warn!(
            target: TAG,
            "FX param refresh timed out for {}",
            block_label(s.pending_block)
        );
    }
    s.refresh_active = false;
    s.amount_refresh = false;
    // Absorb late untagged replies before the next request claims them.
    s.refresh_stale_until_us = now_us() + FX_REFRESH_STALE_GUARD_US as i64;
    unsafe {
        esp_timer_stop(s.refresh_gap_timer);
        esp_timer_start_once(s.refresh_gap_timer, FX_REFRESH_STALE_GUARD_US);
    }
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

        // Explicitly queued blocks (e.g. Amount poll) refresh even when bypassed.
        if s.fx.blocks[idx].params_loaded {
            continue;
        }

        s.pending_block = block_id;
        s.refresh_active = true;
        s.refresh_stale_until_us = 0;

        let mut buf = [0u8; 16];
        let n = builders::fx_param_refresh_payload(SLOT_CFG[idx].refresh_slot, &mut buf);
        if n == 0 || !crate::ble::nano_ble_connected() {
            s.refresh_active = false;
            s.amount_refresh = false;
            continue;
        }

        let label = BLOCK_LABELS[idx];
        debug!(target: TAG, "Refreshing params: {label}");
        let _ = crate::ble::nano_ble_write_c304(PARAM_REFRESH_LABELS[idx], &buf[..n]);

        unsafe {
            esp_timer_stop(s.refresh_timeout_timer);
            esp_timer_start_once(s.refresh_timeout_timer, FX_REFRESH_TIMEOUT_US);
        }
        return;
    }

    s.refresh_active = false;
    s.refresh_q_len = 0;
    s.refresh_q_head = 0;
    if s.suppress_fx_param_ui {
        s.suppress_fx_param_ui = false;
    }
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

    // Refresh the on-screen block first so the UI can paint final values ASAP.
    let priority = if s.pedal_fx_focus >= 0 {
        Some(s.pedal_fx_focus as usize)
    } else {
        None
    };
    if let Some(p) = priority {
        if block_needs_refresh(&s.fx.blocks[p]) {
            s.refresh_queue[s.refresh_q_len] = p as u32;
            s.refresh_q_len += 1;
        }
    }
    for i in 0..NANO_FX_BLOCK_COUNT {
        if priority == Some(i) {
            continue;
        }
        if block_needs_refresh(&s.fx.blocks[i]) {
            s.refresh_queue[s.refresh_q_len] = i as u32;
            s.refresh_q_len += 1;
        }
    }

    if s.refresh_q_len == 0 {
        debug!(target: TAG, "No active FX blocks to refresh");
        if s.suppress_fx_param_ui {
            s.suppress_fx_param_ui = false;
        }
        push_fx_ui();
        return;
    }

    debug!(
        target: TAG,
        "Queueing FX param refresh for {} block(s)", s.refresh_q_len
    );
    send_next_refresh();
}
