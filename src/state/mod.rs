//! Preset / amp state machine.

pub mod machine;

pub use machine::{
    nano_rx_mode_get, nano_rx_mode_set, nano_state_begin_sync, nano_state_feed_c305,
    nano_state_get_amp, nano_state_get_bpm, nano_state_on_bpm_found,
    nano_state_request_preset_refresh, nano_state_request_tuner_sync, nano_state_reset,
    nano_state_tempo_active, nano_state_tempo_enter, nano_state_tempo_exit, nano_state_tempo_tap,
    nano_state_try_amp_notify_fast, nano_state_tuner_watch_update, nano_state_write_amp_knob,
    nano_state_write_capture_slot, nano_state_write_capture_volume,
};

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_state_reset as *const (),
        nano_rx_mode_set as *const (),
        nano_rx_mode_get as *const (),
        nano_state_feed_c305 as *const (),
        nano_state_try_amp_notify_fast as *const (),
        nano_state_get_amp as *const (),
        nano_state_write_amp_knob as *const (),
        nano_state_write_capture_volume as *const (),
        nano_state_write_capture_slot as *const (),
        nano_state_begin_sync as *const (),
        nano_state_request_tuner_sync as *const (),
        nano_state_tuner_watch_update as *const (),
        nano_state_request_preset_refresh as *const (),
        nano_state_on_bpm_found as *const (),
        nano_state_get_bpm as *const (),
        nano_state_tempo_active as *const (),
        nano_state_tempo_enter as *const (),
        nano_state_tempo_exit as *const (),
        nano_state_tempo_tap as *const (),
    ];
    core::hint::black_box(fns);
}
