//! Periodic stale-pitch check and the UI-push helper.

use super::ctx::*;
use super::pitch::expire_stale_pitch;

pub(super) fn poll() {
    if tn().tick_timer.poll() {
        expire_stale_pitch();
    }
}

pub(super) fn ensure_tick_timer() {
    let s = tn();
    if s.tick_timer.armed() {
        return;
    }
    s.tick_timer.start_periodic(250 * 1000);
}

pub(super) fn push_ui() {
    tuner_publish();
    crate::screen::nano_ui_set_tuner(&tn().tuner);
}
