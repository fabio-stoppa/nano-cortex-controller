//! Periodic tick timer (stale-pitch expiry) and the UI-push helper.

use core::ffi::c_void;

use esp_idf_svc::sys::{
    esp_timer_create, esp_timer_create_args_t, esp_timer_start_periodic, ESP_OK,
};

use super::ctx::*;
use super::pitch::expire_stale_pitch;

extern "C" fn tick_timer_cb(_arg: *mut c_void) {
    expire_stale_pitch();
}

pub(super) fn ensure_tick_timer() {
    let s = tn();
    if !s.tick_timer.is_null() {
        return;
    }

    let mut args: esp_timer_create_args_t = unsafe { core::mem::zeroed() };
    args.callback = Some(tick_timer_cb);
    args.name = c"tuner_tick".as_ptr();
    if unsafe { esp_timer_create(&args, &mut s.tick_timer) } == ESP_OK {
        unsafe { esp_timer_start_periodic(s.tick_timer, 250 * 1000) };
    }
}

pub(super) fn push_ui() {
    tuner_publish();
    crate::ui::screen::nano_ui_set_tuner(&tn().tuner);
}
