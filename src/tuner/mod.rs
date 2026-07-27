//! Tuner (pitch + mute/bypass route).

pub mod engine;
pub mod types;

pub use engine::{
    nano_tuner_cents_f, nano_tuner_feed_c305, nano_tuner_feed_pitch_fast, nano_tuner_get,
    nano_tuner_may_apply_state, nano_tuner_note_c305_rx, nano_tuner_pitch_age_ms,
    nano_tuner_pitch_dt_ms, nano_tuner_reset, nano_tuner_route_label, nano_tuner_set_active,
    nano_tuner_snapshot, nano_tuner_sync_from_state, nano_tuner_tick,
};
pub use types::NANO_TUNER_ROUTE_UNKNOWN;

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_tuner_reset as *const (),
        nano_tuner_tick as *const (),
        nano_tuner_feed_c305 as *const (),
        nano_tuner_feed_pitch_fast as *const (),
        nano_tuner_note_c305_rx as *const (),
        nano_tuner_pitch_age_ms as *const (),
        nano_tuner_pitch_dt_ms as *const (),
        nano_tuner_cents_f as *const (),
        nano_tuner_get as *const (),
        nano_tuner_snapshot as *const (),
        nano_tuner_route_label as *const (),
        nano_tuner_sync_from_state as *const (),
        nano_tuner_may_apply_state as *const (),
        nano_tuner_set_active as *const (),
    ];
    core::hint::black_box(fns);
}
