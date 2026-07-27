//! Interior-mutability wrapper for module-level firmware state.
//!
//! # Task contract (must stay true or this is a data race)
//! - Writers: C305 worker, ESP-IDF / esp_timer callbacks, Bluedroid GATT callbacks.
//! - Readers/writers: LVGL task (UI) via staged snapshots where possible.
//! - Ordering relies on FreeRTOS queues / event flow, **not** a Rust Mutex.
//!   Do not add long critical sections on the GATT notify path.
//! - Cross-task UI reads of FX / amp / tuner use [`crate::seqlock::SeqLock`]
//!   snapshots (`nano_fx_snapshot`, `nano_state_snapshot_amp`, `nano_tuner_snapshot`).
//! - BLE link readiness is published via atomics in `ble::client` (no Ctx read
//!   from the UI / write path for `connected` / handles).

use core::cell::UnsafeCell;

pub struct Shared<T>(UnsafeCell<T>);

unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// Exclusive access for the current task. Caller must uphold the task contract above.
    #[allow(clippy::mut_from_ref)]
    pub fn get(&self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}
