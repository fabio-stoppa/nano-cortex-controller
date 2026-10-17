/* ----------------------------------------------------------------- tuner */

use super::*;
use super::amp::apply_amp_locked;
use super::chain::{show_tempo_view, show_tuner_view, update_chain_blocks_locked};
use super::consts::{
    TUNER_CENTS_SPAN, TUNER_COLOR_IDLE, TUNER_COLOR_NEEDLE, TUNER_COLOR_OK, TUNER_IN_TUNE_CENTS,
    TUNER_NEEDLE_W,
};
use super::header::sync_header_model_locked;
use super::helpers::{amp_state, fx_state, note_label, set_label_if_changed, tuner_snap};
use super::state::{assert_lvgl_task, ui, with_inbox};
pub(super) fn paint_tuner_meter_locked(cents_f: f32, has_pitch: bool) {
    let u = ui();
    if u.tuner_meter.is_null() || u.tuner_meter_needle.is_null() {
        return;
    }

    let c = cents_f.clamp(-TUNER_CENTS_SPAN, TUNER_CENTS_SPAN);
    let in_tune = has_pitch && c.abs() <= TUNER_IN_TUNE_CENTS;

    let meter_w = if u.tuner_meter_w > 0 {
        u.tuner_meter_w
    } else {
        lv::get_width(u.tuner_meter).max(TUNER_NEEDLE_W + 1)
    };

    let (x, color) = if !has_pitch {
        (
            (meter_w - TUNER_NEEDLE_W) / 2,
            lv::color_hex(TUNER_COLOR_IDLE),
        )
    } else {
        let t = (c + TUNER_CENTS_SPAN) / (2.0 * TUNER_CENTS_SPAN);
        let travel = meter_w - TUNER_NEEDLE_W;
        let x = (t * travel as f32).round() as i32;
        let color = lv::color_hex(if in_tune {
            TUNER_COLOR_OK
        } else {
            TUNER_COLOR_NEEDLE
        });
        (x, color)
    };

    if lv::get_x(u.tuner_meter_needle) != x {
        lv::set_x(u.tuner_meter_needle, x);
    }
    lv::set_style_bg_color(u.tuner_meter_needle, color, 0);
    if !u.tuner_meter_center.is_null() {
        lv::set_style_bg_opa(
            u.tuner_meter_center,
            if in_tune { lv::OPA_80 } else { lv::OPA_30 },
            0,
        );
    }
    if !u.tuner_note.is_null() {
        lv::set_style_text_color(
            u.tuner_note,
            lv::color_hex(if in_tune { TUNER_COLOR_OK } else { 0xFFFFFF }),
            0,
        );
    }
}

pub(super) extern "C" fn tuner_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED || !ui().ble_connected {
        return;
    }

    let t = tuner_snap();
    nano_tuner_set_active(!t.active);
}

pub(super) extern "C" fn bpm_hdr_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED || !ui().ble_connected {
        return;
    }
    show_tempo_view(!ui().tempo_view, true);
    sync_header_model_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

pub(super) extern "C" fn tempo_tap_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED || !ui().ble_connected {
        return;
    }
    nano_state_tempo_tap();
}

pub(super) extern "C" fn tempo_scope_event_cb(e: *mut lv_event_t) {
    let u = ui();
    if lv::event_code(e) != lv::EVENT_VALUE_CHANGED || u.tempo_scope_sw.is_null() {
        return;
    }
    u.tempo_scope_global = lv::has_state(u.tempo_scope_sw, lv::STATE_CHECKED);
    if !u.tempo_scope_lbl.is_null() {
        lv::label_set_text(
            u.tempo_scope_lbl,
            if u.tempo_scope_global {
                "Global tempo"
            } else {
                "Preset tempo"
            },
        );
    }
    // BLE write for global/preset scope is unknown — UI-local until sniffed.
    info!(
        "Tempo scope UI: {} (no BLE write yet)",
        if u.tempo_scope_global {
            "global"
        } else {
            "preset"
        }
    );
}

pub(super) fn apply_tuner_locked(tuner: *const NanoTunerC) {
    let u = ui();
    if u.tuner_note.is_null() {
        return;
    }

    lv::set_enabled(u.tuner_btn, u.ble_connected);

    let tuner = unsafe { tuner.as_ref() };
    let active = tuner.map(|t| t.active).unwrap_or(false);
    if !active {
        set_label_if_changed(u.tuner_note, "-");
        lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
        set_label_if_changed(u.tuner_cents, "Tuner off");
        paint_tuner_meter_locked(0.0, false);
        lv::label_set_text(lv::get_child(u.tuner_btn, 0), "ON");
        return;
    }

    let t = tuner.unwrap();
    if t.has_pitch {
        set_label_if_changed(u.tuner_note, note_label(t.note));
        let cf = nano_tuner_cents_f();
        set_label_if_changed(u.tuner_cents, &format!("{cf:+.1} cents"));
        paint_tuner_meter_locked(cf, true);
    } else {
        set_label_if_changed(u.tuner_note, "...");
        lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
        set_label_if_changed(u.tuner_cents, "Listening...");
        paint_tuner_meter_locked(0.0, false);
    }
    lv::label_set_text(lv::get_child(u.tuner_btn, 0), "OFF");
}

pub(super) fn async_apply_amp_cb() {
    assert_lvgl_task("async_apply_amp");
    let pending = with_inbox(|u| {
        if u.async_amp_have {
            Some(u.async_amp)
        } else {
            None
        }
    });
    match pending {
        Some(amp) => apply_amp_locked(&amp),
        None => apply_amp_locked(ptr::null()),
    }
    super::ble::update_connecting_overlay_locked();
}

pub(super) fn async_apply_tuner_cb() {
    assert_lvgl_task("async_apply_tuner");
    // Mode on/off still arrives via snapshot; pitch is sampled live below.
    let pending = with_inbox(|u| {
        if !u.async_tuner_pending {
            return None;
        }
        u.async_tuner_pending = false;
        Some(u.async_tuner)
    });
    let Some(t) = pending else {
        return;
    };
    apply_tuner_locked(&t);
    let u = ui();
    if t.active && !u.tuner_view {
        show_tuner_view(true);
    } else if !t.active && u.tuner_view {
        show_tuner_view(false);
    }
}

pub(super) fn paint_tuner_live_locked() {
    let u = ui();
    if u.tuner_note.is_null() || !u.tuner_view {
        return;
    }

    let mut t = NanoTunerC::default();
    nano_tuner_snapshot(&mut t);
    if !t.active {
        return;
    }

    if t.has_pitch {
        set_label_if_changed(u.tuner_note, note_label(t.note));
        let cf = nano_tuner_cents_f();
        set_label_if_changed(u.tuner_cents, &format!("{cf:+.1} cents"));
        paint_tuner_meter_locked(cf, true);
    } else {
        set_label_if_changed(u.tuner_note, "...");
        lv::set_style_text_color(u.tuner_note, lv::color_hex(0xFFFFFF), 0);
        set_label_if_changed(u.tuner_cents, "Listening...");
        paint_tuner_meter_locked(0.0, false);
    }
}

