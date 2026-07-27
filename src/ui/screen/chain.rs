/* ------------------------------------------------------------ chain rail */

use super::*;
use super::consts::{
    AMP_PEDAL_PARAM_COUNT, CHAIN_AMP, CHAIN_BLOCK_COUNT, CHAIN_PFX1, CHAIN_PFX2,
    CHAIN_PFX3, CHAIN_PRE1, CHAIN_PRE2, CHAIN_TAB_BORDER_SEL, CHAIN_TAB_BORDER_W, HEADER_H,
    IconKind, UI_EDGE_PAD,
};
use super::dispatch::{apply_bpm_locked, refresh_preset_label_locked};
use super::fx_panel::apply_fx_locked;
use super::header::sync_header_model_locked;
use super::helpers::{amp_state, c_buf_str, copy_into, fx_state};
use super::params::update_pedal_params_grid_scroll;
use super::picker::fx_model_picker_close;
use super::state::{assert_lvgl_task, ui, with_inbox};
pub(super) fn category_to_icon(cat: u32) -> IconKind {
    match cat {
        NANO_FX_CAT_GUITAR_OD => IconKind::GuitarOd,
        NANO_FX_CAT_BASS_OD => IconKind::BassOd,
        NANO_FX_CAT_COMPRESSOR => IconKind::Compressor,
        NANO_FX_CAT_PITCH => IconKind::Pitch,
        NANO_FX_CAT_FILTER => IconKind::Filter,
        NANO_FX_CAT_WAH => IconKind::Wah,
        _ => IconKind::Utility,
    }
}

pub(super) fn icon_kind_color(kind: IconKind) -> lv_color_t {
    lv::color_hex(match kind {
        IconKind::Gate => 0xB0B8C0,
        IconKind::Amp => 0xE85040,
        IconKind::GuitarOd => 0xE87030,
        IconKind::BassOd => 0xC87828,
        IconKind::Compressor => 0x5090E8,
        IconKind::Pitch => 0x40C8E0,
        IconKind::Filter => 0xE0C040,
        IconKind::Wah => 0xA868E8,
        IconKind::Utility => 0x909090,
    })
}

pub(super) fn icon_kind_glyph(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Gate => "GATE",
        IconKind::Amp => "AMP",
        IconKind::GuitarOd => "GTR OD",
        IconKind::BassOd => "BASS",
        IconKind::Compressor => "COMP",
        IconKind::Pitch => "PITCH",
        IconKind::Filter => "FILTER",
        IconKind::Wah => "WAH",
        IconKind::Utility => "UTIL",
    }
}

pub(super) fn ui_passive_container(obj: *mut lv_obj_t) {
    if obj.is_null() {
        return;
    }
    lv::remove_flag(obj, lv::FLAG_CLICKABLE);
    lv::remove_flag(
        obj,
        lv::FLAG_SCROLLABLE | lv::FLAG_SCROLL_CHAIN | lv::FLAG_GESTURE_BUBBLE,
    );
}

pub(super) fn ui_scroll_container(obj: *mut lv_obj_t, dir: sys::lv_dir_t) {
    if obj.is_null() {
        return;
    }
    lv::add_flag(obj, lv::FLAG_SCROLLABLE | lv::FLAG_CLICKABLE);
    lv::set_scroll_dir(obj, dir);
    lv::remove_flag(obj, lv::FLAG_SCROLL_CHAIN);
    lv::add_flag(obj, lv::FLAG_GESTURE_BUBBLE);
}

pub(super) fn ui_param_cell_container(obj: *mut lv_obj_t) {
    if obj.is_null() {
        return;
    }
    lv::add_flag(obj, lv::FLAG_CLICKABLE);
    lv::remove_flag(obj, lv::FLAG_SCROLLABLE);
    lv::add_flag(obj, lv::FLAG_SCROLL_CHAIN_VER | lv::FLAG_GESTURE_BUBBLE);
}

pub(super) fn ui_no_scroll_only(obj: *mut lv_obj_t) {
    if obj.is_null() {
        return;
    }
    lv::remove_flag(obj, lv::FLAG_SCROLLABLE);
}

pub(super) fn ui_bring_chrome_to_front() {
    let u = ui();
    if !u.chain_bar.is_null() {
        lv::move_foreground(u.chain_bar);
    }
    if u.fx_model_picker_open && !u.fx_model_picker.is_null() {
        lv::move_foreground(u.fx_model_picker);
    }
    if !u.header_bar.is_null() {
        lv::move_foreground(u.header_bar);
    }
}

pub(super) fn make_content_panel(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    let panel = lv::obj_create(parent);
    lv::set_size(panel, lv::pct(100), lv::pct(100));
    lv::align(panel, lv::ALIGN_TOP_MID, 0, 0);
    lv::set_style_bg_opa(panel, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(panel, 0, 0);
    lv::set_style_pad_all(panel, 0, 0);
    ui_passive_container(panel);
    panel
}

pub(super) fn chain_set_block_label(idx: usize, text: &str, color: lv_color_t) {
    let block = &mut ui().chain[idx];
    if block.icon.is_null() {
        return;
    }

    if c_buf_str(&block.icon_text) == text && lv::color_eq(block.icon_color, color) {
        return;
    }

    copy_into(&mut block.icon_text, text);
    block.icon_color = color;
    lv::label_set_text(block.icon, text);
    lv::set_style_text_font(block.icon, lv::font_chain(), 0);
    lv::set_style_text_color(block.icon, color, 0);
    lv::set_style_shadow_width(block.icon, 0, 0);
}

pub(super) fn chain_btn_prepare(btn: *mut lv_obj_t) {
    lv::remove_style_all(btn);
    lv::remove_theme(btn, lv::PART_ANY | lv::SEL_STATE_ANY);
    lv::add_flag(btn, lv::FLAG_CLICKABLE);
    lv::remove_flag(btn, lv::FLAG_SCROLLABLE);
    lv::set_style_bg_opa(btn, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(btn, 0, 0);
    lv::set_style_shadow_width(btn, 0, 0);
    lv::set_style_pad_all(btn, 0, 0);
}

pub(super) fn chain_style_block(idx: usize, selected: bool, active: bool, accent: lv_color_t) {
    let block = &mut ui().chain[idx];
    if block.btn.is_null() {
        return;
    }

    if block.last_selected == selected
        && block.last_active == active
        && lv::color_eq(block.last_accent, accent)
    {
        return;
    }

    block.last_selected = selected;
    block.last_active = active;
    block.last_accent = accent;

    let opa = if selected || active {
        lv::OPA_COVER
    } else {
        lv::OPA_40
    };
    lv::set_style_opa(block.col, opa, 0);

    let outline = if active {
        accent
    } else {
        lv::color_hex(0x404040)
    };

    lv::set_style_bg_opa(block.col, lv::OPA_TRANSP, 0);
    if selected {
        lv::set_style_border_color(block.col, lv::color_hex(0xFFFFFF), 0);
        lv::set_style_border_width(block.col, CHAIN_TAB_BORDER_SEL, 0);
    } else {
        lv::set_style_border_color(block.col, outline, 0);
        lv::set_style_border_width(block.col, CHAIN_TAB_BORDER_W, 0);
    }
    lv::set_style_border_opa(block.col, lv::OPA_COVER, 0);
    lv::set_style_border_side(block.col, lv::BORDER_SIDE_FULL, 0);
    lv::set_style_border_post(block.col, selected, 0);

    lv::invalidate(block.col);
}

pub(super) fn chain_slot_to_fx_block(slot: usize) -> u32 {
    match slot {
        CHAIN_PRE1 => NANO_FX_PRE1,
        CHAIN_PRE2 => NANO_FX_PRE2,
        CHAIN_PFX1 => NANO_FX_POST1,
        CHAIN_PFX2 => NANO_FX_POST2,
        CHAIN_PFX3 => NANO_FX_POST3,
        _ => NANO_FX_PRE1,
    }
}

pub(super) fn chain_slot_is_fx(slot: usize) -> bool {
    matches!(slot, CHAIN_PRE1 | CHAIN_PRE2 | CHAIN_PFX1 | CHAIN_PFX2 | CHAIN_PFX3)
}

pub(super) fn show_content_for_chain(slot: usize) {
    let u = ui();
    if u.tempo_view {
        u.tempo_view = false;
        nano_state_tempo_exit();
        layout_chrome_for_tuner(false);
    }
    u.tuner_view = false;
    fx_model_picker_close();

    for panel in [u.amp_panel, u.fx_panel, u.tuner_panel, u.tempo_panel] {
        if !panel.is_null() {
            lv::add_flag(panel, lv::FLAG_HIDDEN);
        }
    }

    let shown = match slot {
        CHAIN_AMP => u.amp_panel,
        CHAIN_PRE1 | CHAIN_PRE2 | CHAIN_PFX1 | CHAIN_PFX2 | CHAIN_PFX3 => u.fx_panel,
        _ => ptr::null_mut(),
    };

    if !shown.is_null() {
        lv::remove_flag(shown, lv::FLAG_HIDDEN);
        lv::move_foreground(shown);
    }

    ui_bring_chrome_to_front();
    u.pedal_params_scroll_inited = false;
    if slot == CHAIN_AMP {
        update_pedal_params_grid_scroll(u.amp_params_row, AMP_PEDAL_PARAM_COUNT);
    } else if chain_slot_is_fx(slot) {
        let count = unsafe { fx_state().as_ref() }
            .map(|fx| {
                let b = &fx.blocks[u.fx_selected as usize];
                if b.valid {
                    b.param_count
                } else {
                    0
                }
            })
            .unwrap_or(0);
        update_pedal_params_grid_scroll(u.fx_params_row, count);
    }
    sync_header_model_locked(fx_state());
}

pub(super) fn layout_chrome_for_tuner(tuner_on: bool) {
    let u = ui();
    if u.content_root.is_null() {
        return;
    }

    if !u.chain_bar.is_null() {
        lv::set_hidden(u.chain_bar, tuner_on);
    }

    let h = if u.body_h > 0 {
        u.body_h
    } else {
        lv::get_height(u.content_root)
    };

    if tuner_on {
        let full_w = lv::hor_res() - 2 * UI_EDGE_PAD;
        lv::set_size(u.content_root, full_w, h);
        lv::align(u.content_root, lv::ALIGN_TOP_LEFT, 0, HEADER_H);
    } else {
        let w = if u.content_w_normal > 0 {
            u.content_w_normal
        } else {
            lv::get_width(u.content_root)
        };
        lv::set_size(u.content_root, w, h);
        lv::align(u.content_root, lv::ALIGN_TOP_LEFT, u.content_x_normal, HEADER_H);
    }
}

pub(super) fn show_tuner_view(on: bool) {
    let u = ui();
    if on && u.tempo_view {
        u.tempo_view = false;
        nano_state_tempo_exit();
        if !u.tempo_panel.is_null() {
            lv::add_flag(u.tempo_panel, lv::FLAG_HIDDEN);
        }
    }
    u.tuner_view = on;
    if on {
        fx_model_picker_close();
    }
    if !u.tuner_hdr_btn.is_null() {
        if on {
            lv::add_state(u.tuner_hdr_btn, lv::STATE_CHECKED);
        } else {
            lv::remove_state(u.tuner_hdr_btn, lv::STATE_CHECKED);
        }
    }
    for panel in [u.amp_panel, u.fx_panel, u.tempo_panel] {
        if !panel.is_null() {
            lv::add_flag(panel, lv::FLAG_HIDDEN);
        }
    }
    if u.tuner_panel.is_null() {
        return;
    }

    if on {
        lv::remove_flag(u.tuner_panel, lv::FLAG_HIDDEN);
        lv::move_foreground(u.tuner_panel);
        layout_chrome_for_tuner(true);
        sync_header_model_locked(fx_state());
        refresh_preset_label_locked();
        ui_bring_chrome_to_front();
    } else {
        lv::add_flag(u.tuner_panel, lv::FLAG_HIDDEN);
        layout_chrome_for_tuner(false);
        show_content_for_chain(u.chain_selected);
    }
}

pub(super) fn show_tempo_view(on: bool, write_pedal: bool) {
    let u = ui();
    if on && u.tuner_view {
        u.tuner_view = false;
        if !u.tuner_hdr_btn.is_null() {
            lv::remove_state(u.tuner_hdr_btn, lv::STATE_CHECKED);
        }
        if !u.tuner_panel.is_null() {
            lv::add_flag(u.tuner_panel, lv::FLAG_HIDDEN);
        }
    }

    if on == u.tempo_view {
        return;
    }

    u.tempo_view = on;
    if on {
        fx_model_picker_close();
    }

    for panel in [u.amp_panel, u.fx_panel, u.tuner_panel] {
        if !panel.is_null() {
            lv::add_flag(panel, lv::FLAG_HIDDEN);
        }
    }

    if u.tempo_panel.is_null() {
        return;
    }

    if on {
        lv::remove_flag(u.tempo_panel, lv::FLAG_HIDDEN);
        lv::move_foreground(u.tempo_panel);
        layout_chrome_for_tuner(true);
        sync_header_model_locked(fx_state());
        refresh_preset_label_locked();
        if !u.tempo_tap_btn.is_null() {
            lv::set_enabled(u.tempo_tap_btn, u.ble_connected);
        }
        ui_bring_chrome_to_front();
        if u.ui_bpm_cache_valid {
            apply_bpm_locked(u.ui_bpm_cache, true);
        }
        if write_pedal {
            nano_state_tempo_enter();
        }
        update_chain_blocks_locked(amp_state(), fx_state());
    } else {
        lv::add_flag(u.tempo_panel, lv::FLAG_HIDDEN);
        if write_pedal {
            nano_state_tempo_exit();
        }
        layout_chrome_for_tuner(false);
        show_content_for_chain(u.chain_selected);
        update_chain_blocks_locked(amp_state(), fx_state());
    }
}

pub(super) fn async_apply_tempo_mode_cb() {
    assert_lvgl_task("async_apply_tempo_mode");
    let pending = with_inbox(|u| {
        if !u.async_tempo_mode_pending {
            return None;
        }
        u.async_tempo_mode_pending = false;
        Some(u.async_tempo_mode)
    });
    let Some(on) = pending else {
        return;
    };
    let u = ui();
    if on && !u.tempo_view {
        show_tempo_view(true, false);
    } else if !on && u.tempo_view {
        show_tempo_view(false, false);
    }
}

pub(super) fn update_chain_blocks_locked(amp: *const NanoAmpC, fx: *const NanoFxState) {
    let amp = unsafe { amp.as_ref() };
    let fx = unsafe { fx.as_ref() };

    for i in 0..CHAIN_BLOCK_COUNT {
        if ui().chain[i].btn.is_null() {
            continue;
        }

        let u = ui();
        let selected = i == u.chain_selected && !u.tuner_view && !u.tempo_view;

        let (active, accent, block_label) = if i == CHAIN_AMP {
            (
                amp.map(|a| a.valid).unwrap_or(false),
                icon_kind_color(IconKind::Amp),
                "AMP".to_string(),
            )
        } else {
            let bid = chain_slot_to_fx_block(i);
            let b = fx.map(|f| &f.blocks[bid as usize]);
            let cat = match b {
                Some(b) if !b.model.is_null() => {
                    nano_fx_model_category(unsafe { &*b.model })
                }
                _ => NANO_FX_CAT_NONE,
            };
            let icon = category_to_icon(cat);
            let label = match b {
                Some(b) if b.valid => nano_fx_model_chain_acronym(b),
                _ => icon_kind_glyph(icon).to_string(),
            };
            (
                b.map(|b| b.valid && b.on).unwrap_or(false),
                icon_kind_color(icon),
                label,
            )
        };

        let label_color = if selected {
            if active {
                accent
            } else {
                lv::color_hex(0x909090)
            }
        } else if active {
            accent
        } else {
            lv::color_hex(0x505050)
        };

        chain_set_block_label(i, &block_label, label_color);
        chain_style_block(i, selected, active || i == CHAIN_AMP, accent);
    }
}

pub(super) fn chain_toggle_slot(slot: usize) {
    if !ui().ble_connected {
        return;
    }

    if !chain_slot_is_fx(slot) {
        return;
    }

    let Some(fx) = (unsafe { fx_state().as_ref() }) else {
        return;
    };
    let bid = chain_slot_to_fx_block(slot);
    nano_fx_write_block_enabled(bid, !fx.blocks[bid as usize].on);
}

pub(super) unsafe extern "C" fn chain_block_btn_event_cb(e: *mut lv_event_t) {
    let code = lv::event_code(e);
    let idx = lv::event_user_data(e);
    if idx >= CHAIN_BLOCK_COUNT {
        return;
    }

    if code == lv::EVENT_LONG_PRESSED {
        if idx == CHAIN_AMP {
            return;
        }
        ui().chain_suppress_click = idx as i32;
        chain_toggle_slot(idx);
        update_chain_blocks_locked(amp_state(), fx_state());
        return;
    }

    if code != lv::EVENT_CLICKED {
        return;
    }

    let u = ui();
    if u.chain_suppress_click == idx as i32 {
        u.chain_suppress_click = -1;
        return;
    }

    u.chain_selected = idx;
    if u.tuner_view || u.tempo_view {
        layout_chrome_for_tuner(false);
    }
    u.tuner_view = false;

    if chain_slot_is_fx(idx) {
        u.fx_selected = chain_slot_to_fx_block(idx);
    }

    show_content_for_chain(idx);

    if chain_slot_is_fx(idx) {
        apply_fx_locked(fx_state());
    }

    update_chain_blocks_locked(amp_state(), fx_state());
}

pub(super) unsafe extern "C" fn tuner_hdr_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    show_tuner_view(!ui().tuner_view);
    sync_header_model_locked(fx_state());
    update_chain_blocks_locked(amp_state(), fx_state());
}

