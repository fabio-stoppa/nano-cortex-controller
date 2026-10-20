//! FX block state: preset parsing, the param-refresh queue and the C304 writes
//! (port of `components/nano/nano_fx.c`).

mod api;
mod parse;
mod refresh;
mod state;
mod write;

pub use api::*;

pub fn poll() {
    refresh::poll();
}
