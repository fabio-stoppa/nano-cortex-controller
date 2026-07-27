//! `#[repr(C)]` mirrors of the struct and enum in `components/nano/nano_tuner.h`.
//!
//! The C UI reads `nano_tuner_t` by pointer, so field order and padding must
//! match the header exactly.

/// `nano_tuner_route_t` — a C enum, i.e. `int`-sized on Xtensa.
pub const NANO_TUNER_ROUTE_UNKNOWN: u32 = 0;
pub const NANO_TUNER_ROUTE_MUTE: u32 = 1;
pub const NANO_TUNER_ROUTE_BYPASS: u32 = 2;

/// Mirror of `nano_tuner_t`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct NanoTunerC {
    pub active: bool,
    pub has_pitch: bool,
    /// 0–11 (C..B), or -1 if unknown.
    pub note: i8,
    pub cents: i16,
    pub route: u32,
}

impl NanoTunerC {
    pub const fn new() -> Self {
        Self {
            active: false,
            has_pitch: false,
            note: 0,
            cents: 0,
            route: NANO_TUNER_ROUTE_UNKNOWN,
        }
    }
}
