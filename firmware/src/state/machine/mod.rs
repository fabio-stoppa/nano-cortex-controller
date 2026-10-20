//! Preset / amp state machine (port of `components/nano/nano_state.c`).
//!
//! Owns the C305 reassembly buffers, the debounce/refresh/poll timers and the
//! Tempo Mode helpers and preset/amp state machine.

mod amp_live;
mod api;
mod classify;
mod ctx;
mod editor;
mod rx;
mod tempo;
mod timers;

pub use api::*;
pub fn poll() {
    timers::poll();
    crate::fx::poll();
    crate::tuner::poll();
}

pub use editor::{
    nano_state_assign_footswitch, nano_state_copy_current, nano_state_copy_preset,
    nano_state_delete_preset, nano_state_load_library_cab, nano_state_load_library_capture,
    nano_state_preset_dirty, nano_state_preset_name, nano_state_preset_names,
    nano_state_preset_slot, nano_state_read_expression, nano_state_read_settings,
    nano_state_rename_preset, nano_state_request_library, nano_state_request_lists,
    nano_state_save_preset, nano_state_select_preset, nano_state_set_tuner_reference,
    nano_state_step_capture, nano_state_step_preset, nano_state_write_cab_float,
    nano_state_write_cab_mic, nano_state_write_expression, nano_state_write_led_usb,
    nano_state_write_mute,
};
