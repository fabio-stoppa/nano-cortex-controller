//! Preset / amp state machine.

pub mod machine;

pub use machine::{
    feed_c305_bytes, try_amp_notify_fast_bytes, nano_rx_mode_get, nano_rx_mode_set,
    nano_state_amp_ready, nano_state_begin_sync, nano_state_get_bpm, nano_state_on_bpm_found,
    nano_state_request_preset_refresh, nano_state_request_tuner_sync, nano_state_reset,
    nano_state_snapshot_amp, nano_state_tempo_active, nano_state_tempo_enter, nano_state_tempo_exit,
    nano_state_tempo_tap, nano_state_tuner_watch_update, nano_state_write_amp_knob,
    nano_state_write_capture_slot, nano_state_write_capture_volume, nano_state_assign_footswitch,
    nano_state_copy_preset, nano_state_delete_preset, nano_state_load_library_cab,
    nano_state_load_library_capture, nano_state_preset_dirty, nano_state_preset_names,
    nano_state_preset_slot,
    nano_state_read_expression, nano_state_read_settings, nano_state_rename_preset,
    nano_state_request_library, nano_state_request_lists, nano_state_save_preset,
    nano_state_select_preset, nano_state_set_tuner_reference, nano_state_write_cab_float,
    nano_state_write_cab_mic, nano_state_write_expression, nano_state_write_led_usb,
    nano_state_write_mute,
};

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_state_reset as *const (),
        nano_rx_mode_set as *const (),
        nano_rx_mode_get as *const (),
        feed_c305_bytes as *const (),
        try_amp_notify_fast_bytes as *const (),
        nano_state_amp_ready as *const (),
        nano_state_snapshot_amp as *const (),
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
        nano_state_select_preset as *const (),
        nano_state_save_preset as *const (),
        nano_state_copy_preset as *const (),
        nano_state_rename_preset as *const (),
        nano_state_delete_preset as *const (),
        nano_state_assign_footswitch as *const (),
        nano_state_request_library as *const (),
        nano_state_load_library_capture as *const (),
        nano_state_load_library_cab as *const (),
        nano_state_write_cab_float as *const (),
        nano_state_write_cab_mic as *const (),
        nano_state_read_expression as *const (),
        nano_state_write_expression as *const (),
        nano_state_read_settings as *const (),
        nano_state_write_led_usb as *const (),
        nano_state_write_mute as *const (),
        nano_state_set_tuner_reference as *const (),
        nano_state_request_lists as *const (),
        nano_state_preset_names as *const (),
        nano_state_preset_slot as *const (),
        nano_state_preset_dirty as *const (),
    ];
    core::hint::black_box(fns);
}
