//! Tuner (pitch + mute/bypass route).
#![allow(dead_code, unused_imports)]

pub mod engine;
pub mod types;

pub fn poll() {
    engine::poll();
}

pub use engine::{
    feed_c305_bytes, feed_pitch_fast_bytes, nano_tuner_cents_f, nano_tuner_is_active,
    nano_tuner_may_apply_state, nano_tuner_note_c305_rx, nano_tuner_pitch_age_ms,
    nano_tuner_pitch_dt_ms, nano_tuner_reset, nano_tuner_route_label, nano_tuner_set_active,
    nano_tuner_snapshot, nano_tuner_sync_from_state, nano_tuner_tick,
};
