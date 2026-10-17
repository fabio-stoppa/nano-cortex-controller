/* ------------------------------------------------------------------- BLE */

use super::*;
use super::build::{
    apply_layout_mode, refresh_layout_buttons_locked, style_layout_choice_btn,
};
use super::chain::{ui_bring_chrome_to_front, ui_no_scroll_only, ui_passive_container};
use super::consts::LayoutMode;
use super::fx_panel::apply_fx_locked;
use super::helpers::{fx_state, tuner_state};
use super::state::{assert_lvgl_task, ui, with_inbox};
use super::tempo::set_slider_enabled_locked;
use super::tuner::apply_tuner_locked;

fn session_ready() -> bool {
    crate::state::nano_state_amp_ready() && crate::fx::nano_fx_params_sync_ready()
}

pub(super) fn show_connecting_overlay(visible: bool) {
    let u = ui();
    if u.connecting_overlay.is_null() {
        return;
    }
    if visible {
        lv::remove_flag(u.connecting_overlay, lv::FLAG_HIDDEN);
        lv::move_foreground(u.connecting_overlay);
    } else {
        lv::add_flag(u.connecting_overlay, lv::FLAG_HIDDEN);
    }
}

/// Keep the cover up until BLE is up and amp + FX params have finished loading.
pub(super) fn update_connecting_overlay_locked() {
    let u = ui();
    if u.connecting_overlay.is_null() {
        return;
    }

    if !u.ble_connected {
        u.connecting_await_ready = true;
        show_connecting_overlay(true);
        refresh_connecting_status_locked();
        ui_bring_chrome_to_front();
        return;
    }

    if !u.connecting_await_ready {
        show_connecting_overlay(false);
        return;
    }

    if session_ready() {
        u.connecting_await_ready = false;
        show_connecting_overlay(false);
        return;
    }

    show_connecting_overlay(true);
    refresh_connecting_status_locked();
    ui_bring_chrome_to_front();
}

fn refresh_connecting_status_locked() {
    let u = ui();
    if u.connecting_status.is_null() || u.connecting_overlay.is_null() {
        return;
    }
    if lv::has_flag(u.connecting_overlay, lv::FLAG_HIDDEN) {
        return;
    }

    let scanning = !u.ble_connected && crate::ble::nano_ble_scanning();
    let (status, blink) = if !u.ble_connected {
        if scanning {
            ("Scanning for Nano Cortex...", true)
        } else {
            ("Waiting for Nano Cortex...", false)
        }
    } else if !crate::state::nano_state_amp_ready() {
        ("Connected - loading preset...", true)
    } else {
        ("Loading effects...", true)
    };
    lv::label_set_text(u.connecting_status, status);

    if !u.connecting_icon.is_null() {
        let dim = blink && !u.ble_blink_on;
        lv::set_style_opa(
            u.connecting_icon,
            if dim { lv::OPA_30 } else { lv::OPA_COVER },
            0,
        );
    }
}

fn make_layout_choice_btn(parent: *mut lv_obj_t, label: &str, mode: LayoutMode) -> *mut lv_obj_t {
    let btn = lv::button_create(parent);
    lv::set_size(btn, 148, 36);
    lv::set_style_radius(btn, 8, 0);
    lv::set_style_border_width(btn, 1, 0);
    lv::set_style_shadow_width(btn, 0, 0);
    lv::set_style_bg_opa(btn, lv::OPA_COVER, 0);
    lv::add_flag(btn, lv::FLAG_CHECKABLE);
    lv::add_event_cb(
        btn,
        layout_choice_btn_event_cb,
        lv::EVENT_CLICKED,
        mode as usize,
    );

    let lbl = lv::label_create(btn);
    lv::label_set_text(lbl, label);
    lv::set_style_text_font(lbl, lv::font_sm(), 0);
    lv::center(lbl);

    style_layout_choice_btn(btn, false);
    btn
}

pub(super) extern "C" fn layout_choice_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    let mode = LayoutMode::from_u8(lv::event_user_data(e) as u8);
    apply_layout_mode(mode);
}

pub(super) fn build_connecting_overlay(screen: *mut lv_obj_t) {
    let u = ui();

    let overlay = lv::obj_create(screen);
    u.connecting_overlay = overlay;
    lv::set_size(overlay, lv::pct(100), lv::pct(100));
    lv::align(overlay, lv::ALIGN_TOP_LEFT, 0, 0);
    lv::set_style_bg_color(overlay, lv::color_hex(0x121212), 0);
    lv::set_style_bg_opa(overlay, lv::OPA_COVER, 0);
    lv::set_style_border_width(overlay, 0, 0);
    lv::set_style_pad_all(overlay, 32, 0);
    lv::set_flex_flow(overlay, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        overlay,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    lv::set_style_pad_row(overlay, 18, 0);
    lv::add_flag(overlay, lv::FLAG_CLICKABLE);
    ui_no_scroll_only(overlay);

    u.connecting_icon = lv::label_create(overlay);
    lv::label_set_text(u.connecting_icon, lv::SYMBOL_BLUETOOTH);
    lv::set_style_text_font(u.connecting_icon, lv::font_tuner_note(), 0);
    lv::set_style_text_color(u.connecting_icon, lv::color_hex(0x80C0FF), 0);

    let title = lv::label_create(overlay);
    lv::label_set_text(title, "Connecting to Nano Cortex");
    lv::set_style_text_font(title, lv::font_lg(), 0);
    lv::set_style_text_color(title, lv::color_hex(0xE0E0E0), 0);
    lv::set_style_text_align(title, lv::TEXT_ALIGN_CENTER, 0);

    u.connecting_status = lv::label_create(overlay);
    lv::label_set_text(u.connecting_status, "Waiting for Nano Cortex...");
    lv::set_style_text_font(u.connecting_status, lv::font_md(), 0);
    lv::set_style_text_color(u.connecting_status, lv::color_hex(0x80C0FF), 0);
    lv::set_style_text_align(u.connecting_status, lv::TEXT_ALIGN_CENTER, 0);

    let layout_wrap = lv::obj_create(overlay);
    let box_w = (lv::hor_res() - 48).clamp(280, 520);
    lv::set_width(layout_wrap, box_w.min(420));
    lv::set_height(layout_wrap, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(layout_wrap, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(layout_wrap, 0, 0);
    lv::set_style_pad_all(layout_wrap, 0, 0);
    lv::set_style_pad_row(layout_wrap, 10, 0);
    lv::set_flex_flow(layout_wrap, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        layout_wrap,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_passive_container(layout_wrap);

    let layout_lbl = lv::label_create(layout_wrap);
    lv::label_set_text(layout_lbl, "Screen orientation");
    lv::set_style_text_font(layout_lbl, lv::font_sm(), 0);
    lv::set_style_text_color(layout_lbl, lv::color_hex(0x909090), 0);
    lv::set_style_text_align(layout_lbl, lv::TEXT_ALIGN_CENTER, 0);

    let layout_row = lv::obj_create(layout_wrap);
    lv::set_width(layout_row, lv::pct(100));
    lv::set_height(layout_row, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(layout_row, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(layout_row, 0, 0);
    lv::set_style_pad_all(layout_row, 0, 0);
    lv::set_style_pad_column(layout_row, 8, 0);
    lv::set_style_pad_row(layout_row, 8, 0);
    lv::set_flex_flow(layout_row, lv::FLEX_FLOW_ROW_WRAP);
    lv::set_flex_align(
        layout_row,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_passive_container(layout_row);

    u.layout_horiz_btn = make_layout_choice_btn(layout_row, "Landscape", LayoutMode::Horizontal);
    u.layout_horiz_flip_btn =
        make_layout_choice_btn(layout_row, "Landscape Flip", LayoutMode::HorizontalFlip);
    u.layout_vert_btn = make_layout_choice_btn(layout_row, "Portrait", LayoutMode::Vertical);
    u.layout_vert_flip_btn =
        make_layout_choice_btn(layout_row, "Portrait Flip", LayoutMode::VerticalFlip);
    refresh_layout_buttons_locked();

    let steps = lv::obj_create(overlay);
    lv::set_width(steps, box_w);
    lv::set_height(steps, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(steps, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(steps, 0, 0);
    lv::set_style_pad_all(steps, 0, 0);
    lv::set_style_pad_top(steps, 12, 0);
    lv::set_style_pad_row(steps, 14, 0);
    lv::set_flex_flow(steps, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        steps,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
        lv::FLEX_ALIGN_CENTER,
    );
    ui_passive_container(steps);

    let hold = lv::label_create(steps);
    lv::label_set_text(
        hold,
        "Hold EXIT + CAPTURE on the Nano\nuntil the GAIN ring spins.",
    );
    lv::set_width(hold, lv::pct(100));
    lv::label_set_long_mode(hold, lv::LABEL_LONG_WRAP);
    lv::set_style_text_font(hold, lv::font_md(), 0);
    lv::set_style_text_color(hold, lv::color_hex(0xC0C0C0), 0);
    lv::set_style_text_align(hold, lv::TEXT_ALIGN_CENTER, 0);

    let phone = lv::label_create(steps);
    lv::label_set_text(
        phone,
        "Make sure Nano is not paired\nwith a smartphone or Cortex Cloud.",
    );
    lv::set_width(phone, lv::pct(100));
    lv::label_set_long_mode(phone, lv::LABEL_LONG_WRAP);
    lv::set_style_text_font(phone, lv::font_md(), 0);
    lv::set_style_text_color(phone, lv::color_hex(0x909090), 0);
    lv::set_style_text_align(phone, lv::TEXT_ALIGN_CENTER, 0);

    u.connecting_await_ready = true;
    update_connecting_overlay_locked();
}

pub(super) fn refresh_ble_icon_locked() {
    let u = ui();
    if u.ble_icon.is_null() {
        return;
    }

    if u.ble_connected {
        lv::set_style_opa(u.ble_icon, lv::OPA_COVER, 0);
        lv::set_style_text_color(u.ble_icon, lv::color_hex(0x80C0FF), 0);
        if u.connecting_await_ready {
            // Blink the overlay icon while loading after connect.
            u.ble_blink_on = !u.ble_blink_on;
            update_connecting_overlay_locked();
        }
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
        update_connecting_overlay_locked();
        return;
    }

    lv::set_style_opa(u.ble_icon, lv::OPA_30, 0);
    lv::set_style_text_color(u.ble_icon, lv::color_hex(0x505050), 0);
    update_connecting_overlay_locked();
}

pub(super) extern "C" fn ble_icon_timer_cb(_timer: *mut lv_timer_t) {
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
    if !connected {
        u.connecting_await_ready = true;
    }
    update_connecting_overlay_locked();
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
