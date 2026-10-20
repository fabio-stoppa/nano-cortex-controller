//! Tempo Mode: BPM tracking, tap averaging and enter/exit notify handling.

use core::ffi::CStr;

use log::info;

use nano_wire::builders;
use nano_wire::types::{BPM_MAX, BPM_MIN};

use super::ctx::{ble_write_c304, now_us, st, TAG};

const TEMPO_DEFAULT_BPM: f32 = 120.0;

pub(super) fn tempo_bpm_f() -> f32 {
    let s = st();
    if s.bpm_known && s.last_bpm >= BPM_MIN && s.last_bpm <= BPM_MAX {
        return s.last_bpm as f32;
    }
    TEMPO_DEFAULT_BPM
}

pub(super) fn write_tempo_tap_bpm(bpm_f: f32, label: &CStr) -> bool {
    let mut buf = [0u8; 24];
    let n = builders::bpm_tap_payload(bpm_f, &mut buf);
    if n == 0 {
        return false;
    }
    ble_write_c304(label, &buf[..n])
}

pub(super) fn on_tempo_notify(bpm: u16, tap_marker: bool) {
    let s = st();
    if bpm < BPM_MIN || bpm > BPM_MAX {
        return;
    }

    s.bpm_known = true;
    s.last_bpm = bpm;

    if tap_marker {
        // Tap / keep-alive while in Tempo Mode — open UI + phase-lock blinker.
        info!(target: TAG, "Tempo Mode notify TAP @ {bpm} BPM");
        s.tempo_mode_active = true;
        crate::screen::nano_ui_set_tempo_mode(true);
        crate::screen::nano_ui_bpm_tap(bpm);
        return;
    }

    if now_us() < s.tempo_ignore_exit_echo_us {
        info!(target: TAG, "Tempo Mode notify EXIT @ {bpm} BPM (ignored echo)");
        crate::screen::nano_ui_set_bpm(bpm, true);
        return;
    }

    /*
     * Exit-shaped 0x91 (no 18 01):
     * - While active → leave Tempo Mode (Footswitch II / peer exit).
     * - While idle → enter announce (Footswitch I hold); Cloud app opens here too.
     */
    if s.tempo_mode_active {
        info!(target: TAG, "Tempo Mode notify EXIT @ {bpm} BPM");
        s.tempo_mode_active = false;
        s.tap_count = 0;
        crate::screen::nano_ui_set_bpm(bpm, true);
        crate::screen::nano_ui_set_tempo_mode(false);
    } else {
        info!(target: TAG, "Tempo Mode notify ENTER @ {bpm} BPM");
        s.tempo_mode_active = true;
        crate::screen::nano_ui_set_tempo_mode(true);
        crate::screen::nano_ui_bpm_tap(bpm);
    }
}
