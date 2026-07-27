/* ------------------------------------------------------------------- BLE */

use super::*;
use super::fx_panel::apply_fx_locked;
use super::helpers::{fx_state, tuner_state};
use super::state::{assert_lvgl_task, ui, with_inbox};
use super::tempo::set_slider_enabled_locked;
use super::tuner::apply_tuner_locked;
pub(super) fn refresh_ble_icon_locked() {
    let u = ui();
    if u.ble_icon.is_null() {
        return;
    }

    if u.ble_connected {
        lv::set_style_opa(u.ble_icon, lv::OPA_COVER, 0);
        lv::set_style_text_color(u.ble_icon, lv::color_hex(0x80C0FF), 0);
        return;
    }

    if crate::ble::nano_ble_scanning() {
        u.ble_blink_on = !u.ble_blink_on;
        lv::set_style_opa(
            u.ble_icon,
            if u.ble_blink_on {
                lv::OPA_COVER
            } else {
                lv::OPA_30
            },
            0,
        );
        lv::set_style_text_color(u.ble_icon, lv::color_hex(0x80C0FF), 0);
        return;
    }

    lv::set_style_opa(u.ble_icon, lv::OPA_30, 0);
    lv::set_style_text_color(u.ble_icon, lv::color_hex(0x505050), 0);
}

pub(super) unsafe extern "C" fn ble_icon_timer_cb(_timer: *mut lv_timer_t) {
    refresh_ble_icon_locked();
}

pub(super) fn async_apply_ble_cb() {
    assert_lvgl_task("async_apply_ble");
    let snap = with_inbox(|u| {
        if !u.async_ble_pending {
            return None;
        }
        u.async_ble_pending = false;
        Some((
            u.async_ble_connected,
            u.async_tuner_pending.then_some(u.async_tuner),
            u.async_fx_pending.then_some(u.async_fx),
        ))
    });
    let Some((connected, tuner, fx)) = snap else {
        return;
    };

    let u = ui();
    u.ble_connected = connected;
    refresh_ble_icon_locked();
    set_slider_enabled_locked(u.ble_connected);
    if !u.tempo_tap_btn.is_null() {
        lv::set_enabled(u.tempo_tap_btn, u.ble_connected);
    }
    if let Some(t) = tuner {
        apply_tuner_locked(&t);
    } else {
        apply_tuner_locked(tuner_state());
    }
    if let Some(fx) = fx {
        apply_fx_locked(&fx);
    } else {
        apply_fx_locked(fx_state());
    }
}

