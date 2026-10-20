//! Deadlines the Bluetooth task polls in place of the ESP-IDF timers.
//!
//! Parsing runs on that same task. The old FreeRTOS worker existed so a
//! controller callback would not parse a preset dump; this task is already
//! off the controller.

use log::info;

use super::api::nano_state_tuner_watch_update;
use super::ctx::{is_tuner_active, set_label, st, NANO_RX_IDLE, TAG};
use super::rx::{
    finish_fe_capture, parse_state_buffer, request_current_state_now, send_state_request,
};

const REFRESH_DEBOUNCE_US: u64 = 50 * 1000;
const TEMPO_AUTO_EXIT_US: u64 = 3500 * 1000;

pub(super) fn poll() {
    if st().parse_timer.poll() {
        parse_state_buffer();
    }
    if st().sync_timer.poll() {
        finish_fe_capture();
        info!(target: TAG, "Initial sync timer — requesting preset state");
        send_state_request("Current preset state request", true);
    }
    if st().refresh_timer.poll() {
        request_current_state_now("Debounced amp refresh");
    }
    if st().poll_timer.poll() {
        let s = st();
        if !is_tuner_active() && s.mode == NANO_RX_IDLE && s.amp.valid {
            request_current_state_now("Live amp poll");
        }
    }
    if st().tuner_fast_sync_timer.poll() {
        let s = st();
        if is_tuner_active() || s.mode != NANO_RX_IDLE {
            s.pending_tuner_sync = true;
        } else {
            s.tuner_fast_sync_armed = true;
            set_label(&mut s.state_probe_label, "TUNER");
            send_state_request("Tuner fast sync", false);
        }
    }
    if st().tuner_watch_timer.poll() {
        let s = st();
        if is_tuner_active() {
            nano_state_tuner_watch_update(true);
        } else if s.mode == NANO_RX_IDLE && s.amp.valid {
            request_current_state_now("Tuner watch");
        }
    }
    if st().tempo_auto_exit_timer.poll() && st().tempo_mode_active {
        info!(target: TAG, "Tempo Mode auto-exit (tap idle)");
        let _ = crate::state::nano_state_tempo_exit();
        crate::screen::nano_ui_set_tempo_mode(false);
    }
}

pub(super) fn ensure_timers() {}

/// Re-arm auto-exit; each new tap postpones closing Tempo Mode.
pub(super) fn schedule_tempo_auto_exit() {
    st().tempo_auto_exit_timer.start_once(TEMPO_AUTO_EXIT_US);
}

pub(super) fn cancel_tempo_auto_exit() {
    st().tempo_auto_exit_timer.stop();
}

/// Arm (or re-arm) the parse timer with an explicit quiet-window.
pub(super) fn schedule_parse_in(delay_us: u64) {
    st().parse_timer.start_once(delay_us.max(1));
}

pub(super) fn schedule_refresh() {
    let s = st();
    if s.mode != NANO_RX_IDLE {
        return;
    }
    s.refresh_timer.start_once(REFRESH_DEBOUNCE_US);
}
