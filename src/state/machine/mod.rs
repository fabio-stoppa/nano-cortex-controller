//! Preset / amp state machine (port of `components/nano/nano_state.c`).
//!
//! Owns the C305 reassembly buffers, the debounce/refresh/poll timers and the
//! Tempo Mode helpers and preset/amp state machine.

mod amp_live;
mod api;
mod classify;
mod ctx;
mod rx;
mod tempo;
mod timers;

pub use api::*;
