/* -------------------------------------------------------------- dispatch */

use super::*;
use super::ble::async_apply_ble_cb;
use super::chain::{apply_fx_focus_locked, async_apply_tempo_mode_cb, show_tuner_view};
use super::consts::{
    NANO_AMP_CAPTURE_COUNT, UI_DISPATCH_PERIOD_MS, UI_DISP_AMP, UI_DISP_BLE, UI_DISP_BPM,
    UI_DISP_FX, UI_DISP_FX_FOCUS, UI_DISP_PRESET, UI_DISP_TEMPO, UI_DISP_TUNER,
};
use super::fx_panel::apply_fx_locked;
use super::helpers::{amp_state, c_buf_str, set_label_if_changed};
use super::state::{assert_lvgl_task, note_lvgl_task, ui, with_inbox};
use super::tuner::{apply_tuner_locked, async_apply_amp_cb, async_apply_tuner_cb, paint_tuner_live_locked};
pub(super) fn ui_run_pending_updates(mask: u8) {
    assert_lvgl_task("ui_run_pending_updates");
    let ble = with_inbox(|u| (mask & UI_DISP_BLE) != 0 && u.async_ble_pending);
    if ble {
        async_apply_ble_cb();
    }
    if (mask & UI_DISP_AMP) != 0 {
        async_apply_amp_cb();
    }
    if (mask & UI_DISP_TUNER) != 0 {
        async_apply_tuner_cb();
    }
    if (mask & UI_DISP_FX_FOCUS) != 0 {
        let focus = with_inbox(|u| {
            if !u.async_fx_focus_pending {
                return None;
            }
            u.async_fx_focus_pending = false;
            let block = u.async_fx_focus;
            (block >= 0).then_some(block as u8)
        });
        if let Some(block) = focus {
            apply_fx_focus_locked(block);
        }
    }
    if (mask & UI_DISP_FX) != 0 {
        async_apply_fx_cb();
    }
    if (mask & UI_DISP_TEMPO) != 0 {
        async_apply_tempo_mode_cb();
    }
    if (mask & UI_DISP_BPM) != 0 {
        async_apply_bpm_cb();
    }
    if (mask & UI_DISP_PRESET) != 0 {
        async_apply_preset_cb();
    }
}

/// Drain pending UI updates; call from the LVGL task only.
pub fn nano_ui_dispatch_if_pending() {
    note_lvgl_task();
    assert_lvgl_task("nano_ui_dispatch_if_pending");

    let mask = with_inbox(|u| {
        if !u.dispatch_ready {
            return 0;
        }

        let mut mask = u.dispatch_mask;
        if mask == 0 {
            // Dirty bits can be lost against the setters; still paint hot snapshots.
            if u.async_tuner_pending {
                mask = UI_DISP_TUNER;
            } else if u.async_ble_pending {
                mask = UI_DISP_BLE;
            } else if u.async_fx_focus_pending {
                mask = UI_DISP_FX_FOCUS;
            } else if u.async_fx_pending {
                mask = UI_DISP_FX;
            } else if u.async_tempo_mode_pending {
                mask = UI_DISP_TEMPO;
            } else if u.async_bpm_pending {
                mask = UI_DISP_BPM;
            } else if u.async_preset_pending {
                mask = UI_DISP_PRESET;
            } else {
                return 0;
            }
        }

        u.dispatch_mask = 0;
        mask
    });

    if mask != 0 {
        ui_run_pending_updates(mask);
    }
}

pub(super) unsafe extern "C" fn ui_dispatch_timer_cb(_timer: *mut lv_timer_t) {
    note_lvgl_task();
    nano_ui_dispatch_if_pending();

    let mut t = NanoTunerC::default();
    nano_tuner_snapshot(&mut t);
    if t.active && !ui().tuner_view {
        show_tuner_view(true);
        apply_tuner_locked(&t);
    } else if !t.active && ui().tuner_view {
        show_tuner_view(false);
        apply_tuner_locked(&t);
    }

    paint_tuner_live_locked();
}

pub(super) fn ui_request_dispatch() {
    // The 8 ms dispatch timer picks the dirty bits up; the C wake queue only
    // existed to nudge a task that is already polling.
}

pub(super) fn ui_init_dispatch() {
    let u = ui();
    u.dispatch_ready = true;
    if u.dispatch_timer.is_null() {
        u.dispatch_timer = lv::timer_create(
            Some(ui_dispatch_timer_cb),
            UI_DISPATCH_PERIOD_MS,
            ptr::null_mut(),
        );
        if u.dispatch_timer.is_null() {
            error!("UI dispatch timer create failed");
        }
    }
}

pub(super) fn async_apply_fx_cb() {
    assert_lvgl_task("async_apply_fx");
    let pending = with_inbox(|u| {
        if !u.async_fx_pending || u.fx_model_picker_open {
            return None;
        }
        let fx = u.async_fx;
        u.async_fx_pending = false;
        Some(fx)
    });
    if let Some(fx) = pending {
        apply_fx_locked(&fx);
    }
}

pub(super) fn apply_bpm_locked(bpm: u16, valid: bool) {
    let u = ui();
    if u.bpm_lbl.is_null() && u.tempo_bpm_lbl.is_null() {
        return;
    }

    if !valid {
        if !u.bpm_lbl.is_null() {
            set_label_if_changed(u.bpm_lbl, "- BPM");
        }
        if !u.tempo_bpm_lbl.is_null() {
            set_label_if_changed(u.tempo_bpm_lbl, "-");
        }
        return;
    }

    if !u.bpm_lbl.is_null() {
        set_label_if_changed(u.bpm_lbl, &format!("{bpm} BPM"));
    }
    if !u.tempo_bpm_lbl.is_null() {
        set_label_if_changed(u.tempo_bpm_lbl, &format!("{bpm}"));
    }
}

pub(super) fn refresh_preset_label_locked() {
    let u = ui();
    if u.preset_lbl.is_null() {
        return;
    }

    let preset = if u.ui_preset_cache_valid && u.ui_preset_cache[0] != 0 {
        Some(c_buf_str(&u.ui_preset_cache).to_string())
    } else {
        None
    };

    let mut bank_line = String::new();
    if let Some(amp) = unsafe { amp_state().as_ref() } {
        if amp.have_capture_slot
            && amp.capture_slot >= 1
            && amp.capture_slot <= NANO_AMP_CAPTURE_COUNT
        {
            bank_line = crate::amp::nano_amp_format_bank_line(amp.capture_slot);
        }
    }

    let text = match (bank_line.is_empty(), preset) {
        (false, Some(p)) => format!("{bank_line} / {p}"),
        (false, None) => bank_line,
        (true, Some(p)) => p,
        (true, None) if u.tuner_view => "Tuner".to_string(),
        (true, None) if u.tempo_view => "Tempo".to_string(),
        (true, None) => "-".to_string(),
    };

    set_label_if_changed(u.preset_lbl, &text);
}

pub(super) fn async_apply_preset_cb() {
    assert_lvgl_task("async_apply_preset");
    // Keep the inbox lock while reading the preset cache so setters cannot tear
    // the fixed buffer mid-label update.
    let pending = with_inbox(|u| {
        if !u.async_preset_pending {
            return false;
        }
        u.async_preset_pending = false;
        true
    });
    if pending {
        refresh_preset_label_locked();
    }
}

pub(super) fn async_apply_bpm_cb() {
    assert_lvgl_task("async_apply_bpm");
    let pending = with_inbox(|u| {
        if !u.async_bpm_pending {
            return None;
        }
        u.async_bpm_pending = false;
        Some((u.async_bpm, u.async_bpm_valid))
    });
    if let Some((bpm, valid)) = pending {
        apply_bpm_locked(bpm, valid);
    }
}

