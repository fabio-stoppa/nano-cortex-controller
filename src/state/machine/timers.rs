//! Parse worker task, ESP timers and their callbacks.

use core::ffi::{c_void, CStr};
use core::ptr;

use esp_idf_svc::sys::*;
use log::{error, info};

use super::api::nano_state_tuner_watch_update;
use super::ctx::{check, is_tuner_active, set_label, st, NANO_RX_IDLE, TAG};
use super::rx::{
    finish_fe_capture, parse_state_buffer, request_current_state_now, send_state_request,
};

const REFRESH_DEBOUNCE_US: u64 = 50 * 1000;
const PARSE_TASK_STACK: u32 = 8192;
const PARSE_TASK_PRIO: UBaseType_t = 5;

const PD_TRUE: BaseType_t = 1;
const PD_PASS: BaseType_t = 1;
const NO_AFFINITY: BaseType_t = 0x7FFF_FFFF;
const NOTIFY_INDEX: UBaseType_t = 0;
const PORT_MAX_DELAY: TickType_t = TickType_t::MAX;

extern "C" fn parse_worker(_arg: *mut c_void) {
    loop {
        unsafe { ulTaskGenericNotifyTake(NOTIFY_INDEX, PD_TRUE, PORT_MAX_DELAY) };
        parse_state_buffer();
    }
}

fn ensure_parse_task() {
    let s = st();
    if !s.parse_task.is_null() {
        return;
    }
    // Internal RAM is tight once the Bluetooth controller stacks are large
    // enough for the Nano link. The parse stack can live in PSRAM.
    let caps = (MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT) as UBaseType_t;
    let ok = unsafe {
        xTaskCreatePinnedToCoreWithCaps(
            Some(parse_worker),
            c"nano_parse".as_ptr(),
            PARSE_TASK_STACK,
            ptr::null_mut(),
            PARSE_TASK_PRIO,
            &mut s.parse_task,
            NO_AFFINITY,
            caps,
        )
    };
    if ok != PD_PASS {
        error!(target: TAG, "Failed to create parse worker task");
        s.parse_task = ptr::null_mut();
    }
}

extern "C" fn parse_timer_cb(_arg: *mut c_void) {
    ensure_parse_task();
    let s = st();
    if !s.parse_task.is_null() {
        unsafe {
            xTaskGenericNotify(
                s.parse_task,
                NOTIFY_INDEX,
                0,
                eNotifyAction_eIncrement,
                ptr::null_mut(),
            )
        };
    } else {
        parse_state_buffer();
    }
}

extern "C" fn refresh_timer_cb(_arg: *mut c_void) {
    request_current_state_now("Debounced amp refresh");
}

extern "C" fn poll_timer_cb(_arg: *mut c_void) {
    let s = st();
    if is_tuner_active() {
        return;
    }
    if s.mode == NANO_RX_IDLE && s.amp.valid {
        request_current_state_now("Live amp poll");
    }
}

extern "C" fn tuner_fast_sync_timer_cb(_arg: *mut c_void) {
    let s = st();
    if is_tuner_active() {
        s.pending_tuner_sync = true;
        return;
    }
    if s.mode != NANO_RX_IDLE {
        s.pending_tuner_sync = true;
        return;
    }
    s.tuner_fast_sync_armed = true;
    set_label(&mut s.state_probe_label, "TUNER");
    send_state_request("Tuner fast sync", false);
}

extern "C" fn tuner_watch_poll_cb(_arg: *mut c_void) {
    let s = st();
    if is_tuner_active() {
        nano_state_tuner_watch_update(true);
        return;
    }
    if s.mode != NANO_RX_IDLE || !s.amp.valid {
        return;
    }

    request_current_state_now("Tuner watch");
}

/// Leave Tempo Mode after the last footswitch/UI tap goes idle.
extern "C" fn tempo_auto_exit_cb(_arg: *mut c_void) {
    if !st().tempo_mode_active {
        return;
    }
    info!(target: TAG, "Tempo Mode auto-exit (tap idle)");
    let _ = crate::state::nano_state_tempo_exit();
    crate::ui::screen::nano_ui_set_tempo_mode(false);
}

/// Re-arm auto-exit; each new tap postpones closing Tempo Mode.
pub(super) fn schedule_tempo_auto_exit() {
    let s = st();
    if s.tempo_auto_exit_timer.is_null() {
        return;
    }
    unsafe {
        esp_timer_stop(s.tempo_auto_exit_timer);
        esp_timer_start_once(s.tempo_auto_exit_timer, TEMPO_AUTO_EXIT_US);
    }
}

pub(super) fn cancel_tempo_auto_exit() {
    let s = st();
    if s.tempo_auto_exit_timer.is_null() {
        return;
    }
    unsafe {
        esp_timer_stop(s.tempo_auto_exit_timer);
    }
}

const TEMPO_AUTO_EXIT_US: u64 = 3500 * 1000;

/// Arm (or re-arm) the parse timer with an explicit quiet-window.
pub(super) fn schedule_parse_in(delay_us: u64) {
    let s = st();
    if s.parse_timer.is_null() {
        return;
    }
    let delay = delay_us.max(1);
    unsafe {
        esp_timer_stop(s.parse_timer);
        esp_timer_start_once(s.parse_timer, delay);
    }
}

pub(super) fn schedule_refresh() {
    let s = st();
    if s.refresh_timer.is_null() || s.mode != NANO_RX_IDLE {
        return;
    }
    unsafe {
        esp_timer_stop(s.refresh_timer);
        esp_timer_start_once(s.refresh_timer, REFRESH_DEBOUNCE_US);
    }
}

extern "C" fn sync_timer_cb(_arg: *mut c_void) {
    finish_fe_capture();
    info!(target: TAG, "Initial sync timer — requesting preset state");
    send_state_request("Current preset state request", true);
}

fn create_timer(
    handle: &mut esp_timer_handle_t,
    cb: extern "C" fn(*mut c_void),
    name: &CStr,
) {
    if !handle.is_null() {
        return;
    }
    // Zeroed rest mirrors the C designated initializer (dispatch = ESP_TIMER_TASK).
    let mut args: esp_timer_create_args_t = unsafe { core::mem::zeroed() };
    args.callback = Some(cb);
    args.name = name.as_ptr();
    check(
        unsafe { esp_timer_create(&args, handle) },
        "esp_timer_create",
    );
}

pub(super) fn ensure_timers() {
    let s = st();
    create_timer(&mut s.parse_timer, parse_timer_cb, c"nano_parse");
    create_timer(&mut s.sync_timer, sync_timer_cb, c"nano_sync");
    create_timer(&mut s.refresh_timer, refresh_timer_cb, c"nano_refresh");
    create_timer(&mut s.poll_timer, poll_timer_cb, c"nano_poll");
    create_timer(
        &mut s.tuner_fast_sync_timer,
        tuner_fast_sync_timer_cb,
        c"nano_tsync",
    );
    create_timer(&mut s.tuner_watch_timer, tuner_watch_poll_cb, c"nano_twatch");
    create_timer(
        &mut s.tempo_auto_exit_timer,
        tempo_auto_exit_cb,
        c"nano_texit",
    );
}
