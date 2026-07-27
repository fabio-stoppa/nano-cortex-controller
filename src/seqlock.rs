//! Odd/even seqlock for cross-task snapshots of `Copy` state.
//!
//! Writer: [`SeqLock::write`] (RAII) or [`SeqLock::begin_write`] / [`SeqLock::end_write`].
//! After an already-finished in-place update, [`SeqLock::publish`] advances the
//! version by 2 (stays even) so readers retry.
//!
//! Reader: [`SeqLock::read`] spins while the sequence is odd and retries on mismatch.

use core::sync::atomic::{AtomicU32, Ordering};

pub struct SeqLock {
    seq: AtomicU32,
}

impl SeqLock {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
        }
    }

    /// Enter a write section (sequence becomes odd).
    #[inline]
    #[allow(dead_code)] // Prefer [`Self::write`]; kept for explicit begin/end sites.
    pub fn begin_write(&self) {
        let prev = self.seq.fetch_add(1, Ordering::AcqRel);
        debug_assert!(
            prev & 1 == 0,
            "seqlock: begin_write while already writing"
        );
    }

    /// Leave a write section (sequence becomes even).
    #[inline]
    #[allow(dead_code)]
    pub fn end_write(&self) {
        let prev = self.seq.fetch_add(1, Ordering::Release);
        debug_assert!(
            prev & 1 == 1,
            "seqlock: end_write without matching begin_write"
        );
    }

    /// RAII write section — prefer this over begin/end pairs.
    #[inline]
    #[allow(dead_code)] // Adopt on multi-field Shared updates; `publish` covers mutate-then-push.
    pub fn write(&self) -> SeqWriteGuard<'_> {
        self.begin_write();
        SeqWriteGuard(self)
    }

    /// Version bump after an unprotected in-place update (leave sequence even).
    #[inline]
    pub fn publish(&self) {
        // +2 keeps the seq even and wakes readers that copied during the update.
        self.seq.fetch_add(2, Ordering::Release);
    }

    /// Consistent read: skip odd (write in progress), retry on seq mismatch.
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
