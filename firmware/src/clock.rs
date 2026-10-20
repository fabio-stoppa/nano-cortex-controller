//! Microsecond clock for the ported preset, effect, and tuner logic.
//!
//! Embassy's timer is the same clock the Bluetooth task already waits on.

pub fn now_us() -> i64 {
    embassy_time::Instant::now().as_micros() as i64
}
