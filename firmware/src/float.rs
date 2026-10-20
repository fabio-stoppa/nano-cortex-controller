//! `f32` rounding without the standard library.

pub fn round(value: f32) -> f32 {
    libm::roundf(value)
}

pub fn floor(value: f32) -> f32 {
    libm::floorf(value)
}
