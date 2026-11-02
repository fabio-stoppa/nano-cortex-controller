//! Preset / amp state machine.
#![allow(dead_code, unused_imports)]

pub mod machine;

static SNAPSHOT_GENERATION: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static LIBRARY_GENERATION: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub fn library_generation() -> u32 {
    LIBRARY_GENERATION.load(core::sync::atomic::Ordering::Relaxed)
}
pub(crate) fn library_completed() {
    LIBRARY_GENERATION.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}
static CAB_GENERATION: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static PRESET_ACK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub fn preset_ack_generation() -> u32 {
    PRESET_ACK.load(core::sync::atomic::Ordering::Relaxed)
}
pub(crate) fn preset_ack_completed() {
    PRESET_ACK.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}
pub fn cab_generation() -> u32 {
    CAB_GENERATION.load(core::sync::atomic::Ordering::Relaxed)
}
pub(crate) fn cab_snapshot_completed() {
    CAB_GENERATION.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}
pub fn snapshot_generation() -> u32 {
    SNAPSHOT_GENERATION.load(core::sync::atomic::Ordering::Relaxed)
}
pub(crate) fn snapshot_completed() {
    SNAPSHOT_GENERATION.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub fn poll() {
    machine::poll();
}

pub use machine::{
    feed_c305_bytes, nano_rx_mode_get, nano_rx_mode_set, nano_state_amp_ready,
    nano_state_assign_footswitch, nano_state_begin_sync, nano_state_cab_settings_snapshot,
    nano_state_calibration_snapshot, nano_state_copy_current, nano_state_copy_preset,
    nano_state_delete_preset, nano_state_device_settings,
    nano_state_expression_assignments_snapshot, nano_state_expression_calibration_save,
    nano_state_expression_calibration_start, nano_state_get_bpm, nano_state_library_snapshot,
    nano_state_load_library_cab, nano_state_load_library_capture, nano_state_metering,
    nano_state_note_sound_edit, nano_state_on_bpm_found, nano_state_preset_dirty,
    nano_state_preset_name, nano_state_preset_name_taken, nano_state_preset_names,
    nano_state_preset_slot, nano_state_read_cab_settings, nano_state_read_expression,
    nano_state_read_settings, nano_state_rename_preset, nano_state_request_library,
    nano_state_request_lists, nano_state_request_preset_refresh, nano_state_request_tuner_sync,
    nano_state_reset, nano_state_save_preset, nano_state_select_preset,
    nano_state_set_tuner_reference, nano_state_snapshot_amp, nano_state_step_capture,
    nano_state_step_preset, nano_state_tempo_active, nano_state_tempo_enter, nano_state_tempo_exit,
    nano_state_tempo_tap, nano_state_tools_open, nano_state_transfer_preset,
    nano_state_tuner_settings, nano_state_tuner_watch_update, nano_state_verified_amp,
    nano_state_write_amp_knob, nano_state_write_cab_float, nano_state_write_cab_mic,
    nano_state_write_capture_slot, nano_state_write_capture_volume, nano_state_write_expression,
    nano_state_write_jack_mode, nano_state_write_led_usb, nano_state_write_mute,
    nano_state_write_usb_playback, try_amp_notify_fast_bytes,
};
