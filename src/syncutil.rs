//! Cross-task sharing primitives used by BLE / state / FX / tuner / UI.
//!
//! # Contracts (must stay true or you have a data race)
//!
//! 1. **FreeRTOS queues / event flow** — GATT notify → C305 worker → parsers.
//!    Do not put long Rust mutexes on the GATT notify path.
//! 2. **[`Shared`]** — module-level interior mutability for a *single* owning
//!    task contract (documented at each `static`). Not a Mutex.
//! 3. **[`SeqLock`]** — consistent `Copy` snapshots for UI
//!    (`nano_fx_snapshot`, `nano_state_snapshot_amp`, `nano_tuner_snapshot`).
//! 4. **BLE atomics** — `LINK_UP` / handles in `ble` for cross-task readiness
//!    without reading live `Ctx`.
//! 5. **UI inbox mutex** — `ui::screen` stages paint snapshots; LVGL task only
//!    mutates widgets.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

/* ---------------------------------------------------------------- Shared */

/// Interior-mutability wrapper for module-level firmware state.
pub struct Shared<T>(UnsafeCell<T>);

unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// Exclusive access for the current task. Caller must uphold the module's
    /// task contract.
    #[allow(clippy::mut_from_ref)]
    pub fn get(&self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}

/* --------------------------------------------------------------- SeqLock */

/// Odd/even seqlock for cross-task snapshots of `Copy` state.
///
/// Writer: [`SeqLock::write`] (RAII) or begin/end. After an already-finished
/// in-place update, [`SeqLock::publish`] advances the version by 2 (stays even).
///
/// Reader: [`SeqLock::read`] spins while odd and retries on mismatch.
pub struct SeqLock {
    seq: AtomicU32,
}

impl SeqLock {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn begin_write(&self) {
        let prev = self.seq.fetch_add(1, Ordering::AcqRel);
        debug_assert!(prev & 1 == 0, "seqlock: begin_write while already writing");
    }

    #[inline]
    #[allow(dead_code)]
    pub fn end_write(&self) {
        let prev = self.seq.fetch_add(1, Ordering::Release);
        debug_assert!(prev & 1 == 1, "seqlock: end_write without matching begin_write");
    }

    #[inline]
    #[allow(dead_code)]
    pub fn write(&self) -> SeqWriteGuard<'_> {
        self.begin_write();
        SeqWriteGuard(self)
    }

    #[inline]
    pub fn publish(&self) {
        self.seq.fetch_add(2, Ordering::Release);
    }

    #[inline]
    pub fn read<T>(&self, mut load: impl FnMut() -> T) -> T {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            let val = load();
            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 {
                return val;
            }
        }
    }
}

pub struct SeqWriteGuard<'a>(&'a SeqLock);

impl Drop for SeqWriteGuard<'_> {
    fn drop(&mut self) {
        self.0.end_write();
    }
}
