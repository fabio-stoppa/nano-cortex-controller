/* ----------------------------------------------------------- model picker */

use super::*;
use super::chain::{chain_slot_is_fx, ui_bring_chrome_to_front, ui_no_scroll_only};
use super::consts::{
    AMP_PEDAL_PARAM_COUNT, CHAIN_AMP, FX_MODEL_PICKER_COLS, FX_MODEL_PICKER_GAP,
    FX_MODEL_PICKER_PAD, FX_MODEL_PICKER_SCROLL_PAD, FX_MODEL_PICKER_TILE_H, HEADER_H,
    HEADER_PICKER_AMP, HEADER_PICKER_FX,
};
use super::fx_panel::fx_panel_is_visible;
use super::helpers::{amp_state, c_str, fx_state};
use super::params::{
    fx_model_picker_update_scrollbar, style_grid_scrollbar, update_pedal_params_grid_scroll,
};
use super::state::ui;
pub(super) fn fx_block_matches_model(
    block: Option<&NanoFxBlock>,
    model: &NanoFxModelDef,
) -> bool {
    let Some(block) = block else {
        return false;
    };
    if block.model == model as *const NanoFxModelDef {
        return true;
    }
    block.has_model_id
        && model.id_len > 0
        && block.model_id_len == model.id_len
        && block.model_id_raw[..model.id_len as usize] == model.id[..model.id_len as usize]
}

pub(super) extern "C" fn fx_model_picker_backdrop_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    fx_model_picker_close();
}

pub(super) extern "C" fn fx_model_picker_tile_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }

    let idx = lv::event_user_data(e);
    let u = ui();
    if u.ble_connected {
        if u.header_picker_mode == HEADER_PICKER_AMP {
            nano_state_write_capture_slot(idx as u8);
        } else {
            if let Some(model) = nano_fx_get_allowed_model(u.fx_selected, idx) {
                nano_fx_write_block_model(u.fx_selected, model);
            }
        }
    }
    fx_model_picker_close();
}

pub(super) fn fx_model_picker_lock_page_scroll(lock: bool) {
    if !lock {
        return;
    }
    let u = ui();
    let screen = lv::screen_active();
    if !screen.is_null() {
        lv::remove_flag(screen, lv::FLAG_SCROLLABLE);
    }
    for obj in [u.content_root, u.fx_params_row, u.amp_params_row] {
        if !obj.is_null() {
            lv::remove_flag(obj, lv::FLAG_SCROLLABLE);
        }
    }
}

pub(super) fn model_picker_add_tile(
    parent: *mut lv_obj_t,
    tile_w: i32,
    name: &str,
    picker_idx: usize,
    selected: bool,
) {
    let tile = lv::button_create(parent);
    lv::set_size(tile, tile_w, FX_MODEL_PICKER_TILE_H);
    lv::set_style_bg_color(
        tile,
        lv::color_hex(if selected { 0x1A3040 } else { 0x141414 }),
        0,
    );
    lv::set_style_bg_opa(tile, lv::OPA_COVER, 0);
    lv::set_style_border_color(
        tile,
        lv::color_hex(if selected { 0xFFFFFF } else { 0x383838 }),
        0,
    );
    lv::set_style_border_width(tile, if selected { 2 } else { 1 }, 0);
    lv::set_style_radius(tile, 8, 0);
    lv::set_style_shadow_width(tile, 0, 0);
    lv::add_event_cb(
        tile,
        fx_model_picker_tile_event_cb,
        lv::EVENT_CLICKED,
        picker_idx,
    );

    let lbl = lv::label_create(tile);
    lv::label_set_text(lbl, name);
    lv::set_width(lbl, tile_w - 16);
    lv::label_set_long_mode(lbl, lv::LABEL_LONG_WRAP);
    lv::set_style_text_font(lbl, lv::font_sm(), 0);
    lv::set_style_text_color(
        lbl,
        lv::color_hex(if selected { 0xFFFFFF } else { 0xD8D8D8 }),
        0,
    );
    lv::set_style_text_align(lbl, lv::TEXT_ALIGN_CENTER, 0);
    lv::center(lbl);
}

pub(super) fn picker_grid_tile_width(grid: *mut lv_obj_t) -> (i32, i32) {
    let mut grid_w = lv::get_content_width(grid);
    if grid_w < 80 {
        grid_w = lv::hor_res() - 4 * FX_MODEL_PICKER_PAD;
    }
    grid_w -= FX_MODEL_PICKER_SCROLL_PAD;
    let gaps = (FX_MODEL_PICKER_COLS - 1) * FX_MODEL_PICKER_GAP;
    let tile_w = ((grid_w - gaps) / FX_MODEL_PICKER_COLS).max(64);
    (grid_w, tile_w)
}

pub(super) fn picker_section_container(grid: *mut lv_obj_t, grid_w: i32) -> *mut lv_obj_t {
    let tiles = lv::obj_create(grid);
    lv::set_width(tiles, grid_w);
    lv::set_height(tiles, lv::SIZE_CONTENT);
    lv::set_style_bg_opa(tiles, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(tiles, 0, 0);
    lv::set_style_pad_all(tiles, 0, 0);
    lv::set_style_pad_row(tiles, FX_MODEL_PICKER_GAP, 0);
    lv::set_style_pad_column(tiles, FX_MODEL_PICKER_GAP, 0);
    lv::set_flex_flow(tiles, lv::FLEX_FLOW_ROW_WRAP);
    lv::set_flex_align(
        tiles,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
    );
    ui_no_scroll_only(tiles);
    tiles
}

pub(super) fn picker_section_header(grid: *mut lv_obj_t, grid_w: i32, text: &str, first: bool) {
    let hdr = lv::label_create(grid);
    lv::label_set_text(hdr, text);
    lv::set_width(hdr, grid_w);
    lv::set_style_text_font(hdr, lv::font_md(), 0);
    lv::set_style_text_color(hdr, lv::color_hex(0x909090), 0);
    lv::set_style_pad_top(hdr, if first { 0 } else { 10 }, 0);
    lv::set_style_pad_bottom(hdr, 4, 0);
}

pub(super) fn amp_model_picker_populate() {
    let grid = ui().fx_model_picker_grid;
    if grid.is_null() {
        return;
    }

    const BANK_ORDER: [u32; 5] = [
        crate::amp::NANO_AMP_BANK_CLEAN,
        crate::amp::NANO_AMP_BANK_2,
        crate::amp::NANO_AMP_BANK_3,
        crate::amp::NANO_AMP_BANK_HIGH_GAIN,
        crate::amp::NANO_AMP_BANK_BASS,
    ];

    lv::obj_clean(grid);
    lv::scroll_to(grid, 0, 0, lv::ANIM_OFF);

    let selected_slot = unsafe { amp_state().as_ref() }
        .filter(|a| a.have_capture_slot)
        .map(|a| a.capture_slot)
        .unwrap_or(0);

    let (grid_w, tile_w) = picker_grid_tile_width(grid);

    let catalog = crate::amp::catalog();
    let mut first = true;
    for bank in BANK_ORDER {
        if !catalog.iter().any(|c| c.bank == bank) {
            continue;
        }

        picker_section_header(
            grid,
            grid_w,
            crate::amp::nano_amp_bank_label(bank),
            first,
        );
        first = false;

        let tiles = picker_section_container(grid, grid_w);
        for capture in catalog {
            if capture.bank != bank || capture.name.is_null() {
                continue;
            }
            let selected = selected_slot > 0 && capture.slot == selected_slot;
            model_picker_add_tile(
                tiles,
                tile_w,
                c_str(capture.name),
                capture.slot as usize,
                selected,
            );
        }
    }

    fx_model_picker_update_scrollbar();
}

pub(super) fn fx_model_picker_populate() {
    let u = ui();
    let grid = u.fx_model_picker_grid;
    if grid.is_null() {
        return;
    }

    const CAT_ORDER: [u32; 10] = [
        NANO_FX_CAT_GUITAR_OD,
        NANO_FX_CAT_BASS_OD,
        NANO_FX_CAT_WAH,
        NANO_FX_CAT_FILTER,
        NANO_FX_CAT_COMPRESSOR,
        NANO_FX_CAT_PITCH,
        NANO_FX_CAT_MODULATION,
        NANO_FX_CAT_DELAY,
        NANO_FX_CAT_REVERB,
        NANO_FX_CAT_UTILITY,
    ];

    lv::obj_clean(grid);
    lv::scroll_to(grid, 0, 0, lv::ANIM_OFF);

    let fx = unsafe { fx_state().as_ref() };
    let cur = fx.map(|f| &f.blocks[u.fx_selected as usize]);
    let count = nano_fx_allowed_model_count(u.fx_selected);

    let (grid_w, tile_w) = picker_grid_tile_width(grid);

    let allowed = |i: usize| nano_fx_get_allowed_model(u.fx_selected, i);
    let mut first = true;

    for cat in CAT_ORDER {
        let has_models = (0..count).any(|i| {
            allowed(i).is_some_and(|m| !m.name.is_null() && nano_fx_model_category(m) == cat)
        });
        if !has_models {
            continue;
        }

        picker_section_header(
            grid,
            grid_w,
            nano_fx_category_label(cat),
            first,
        );
        first = false;

        let tiles = picker_section_container(grid, grid_w);
        for i in 0..count {
            let Some(model) = allowed(i) else {
                continue;
            };
            if model.name.is_null() || nano_fx_model_category(model) != cat {
                continue;
            }
            let selected = fx_block_matches_model(cur, model);
            model_picker_add_tile(tiles, tile_w, c_str(model.name), i, selected);
        }
    }

    fx_model_picker_update_scrollbar();
}

pub(super) fn fx_model_picker_close() {
    let u = ui();
    if u.fx_model_picker.is_null() || !u.fx_model_picker_open {
        return;
    }
    u.fx_model_picker_open = false;
    lv::add_flag(u.fx_model_picker, lv::FLAG_HIDDEN);
    fx_model_picker_lock_page_scroll(false);

    let block_params = unsafe { fx_state().as_ref() }.and_then(|fx| {
        let b = &fx.blocks[u.fx_selected as usize];
        if b.valid {
            Some(b.param_count)
        } else {
            None
        }
    });

    if let Some(count) = block_params {
        if fx_panel_is_visible() {
            u.pedal_params_scroll_inited = false;
            update_pedal_params_grid_scroll(u.fx_params_row, count);
            return;
        }
    }
    if u.chain_selected == CHAIN_AMP {
        u.pedal_params_scroll_inited = false;
        update_pedal_params_grid_scroll(u.amp_params_row, AMP_PEDAL_PARAM_COUNT);
    }
}

pub(super) fn fx_model_picker_show() {
    let u = ui();
    if u.fx_model_picker.is_null() || u.fx_model_btn.is_null() {
        return;
    }
    if lv::has_state(u.fx_model_btn, lv::STATE_DISABLED) {
        return;
    }

    if u.chain_selected == CHAIN_AMP {
        u.header_picker_mode = HEADER_PICKER_AMP;
        amp_model_picker_populate();
    } else if chain_slot_is_fx(u.chain_selected) {
        u.header_picker_mode = HEADER_PICKER_FX;
        fx_model_picker_populate();
    } else {
        return;
    }

    u.fx_model_picker_open = true;
    fx_model_picker_lock_page_scroll(true);
    lv::remove_flag(u.fx_model_picker, lv::FLAG_HIDDEN);
    ui_bring_chrome_to_front();
}

pub(super) extern "C" fn fx_model_header_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    if ui().fx_model_picker_open {
        fx_model_picker_close();
        return;
    }
    fx_model_picker_show();
}

pub(super) fn build_fx_model_picker(screen: *mut lv_obj_t) {
    let u = ui();

    let picker = lv::obj_create(screen);
    u.fx_model_picker = picker;
    lv::set_size(picker, lv::pct(100), lv::ver_res() - HEADER_H);
    lv::align(picker, lv::ALIGN_TOP_MID, 0, HEADER_H);
    lv::set_style_bg_color(picker, lv::color_hex(0x000000), 0);
    lv::set_style_bg_opa(picker, lv::OPA_70, 0);
    lv::set_style_border_width(picker, 0, 0);
    lv::set_style_pad_all(picker, FX_MODEL_PICKER_PAD, 0);
    lv::add_flag(picker, lv::FLAG_CLICKABLE);
    lv::add_event_cb(
        picker,
        fx_model_picker_backdrop_event_cb,
        lv::EVENT_CLICKED,
        0,
    );
    lv::add_flag(picker, lv::FLAG_HIDDEN);
    ui_no_scroll_only(picker);
    lv::remove_flag(picker, lv::FLAG_SCROLL_CHAIN);

    let panel = lv::obj_create(picker);
    lv::set_size(panel, lv::pct(98), lv::pct(96));
    lv::center(panel);
    lv::set_style_bg_color(panel, lv::color_hex(0x0A0A0A), 0);
    lv::set_style_bg_opa(panel, lv::OPA_COVER, 0);
    lv::set_style_border_width(panel, 1, 0);
    lv::set_style_border_color(panel, lv::color_hex(0x303030), 0);
    lv::set_style_radius(panel, 8, 0);
    lv::set_style_pad_all(panel, FX_MODEL_PICKER_PAD, 0);
    lv::remove_flag(panel, lv::FLAG_CLICKABLE);
    ui_no_scroll_only(panel);
    lv::remove_flag(panel, lv::FLAG_SCROLL_CHAIN);

    let grid = lv::obj_create(panel);
    u.fx_model_picker_grid = grid;
    lv::set_size(grid, lv::pct(100), lv::pct(100));
    lv::set_style_bg_opa(grid, lv::OPA_TRANSP, 0);
    lv::set_style_border_width(grid, 0, 0);
    lv::set_style_pad_all(grid, 0, 0);
    lv::set_style_pad_right(grid, FX_MODEL_PICKER_SCROLL_PAD, 0);
    lv::set_style_pad_row(grid, FX_MODEL_PICKER_GAP, 0);
    lv::set_flex_flow(grid, lv::FLEX_FLOW_COLUMN);
    lv::set_flex_align(
        grid,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
        lv::FLEX_ALIGN_START,
    );
    lv::set_scroll_dir(grid, lv::DIR_VER);
    lv::add_flag(grid, lv::FLAG_SCROLLABLE);
    lv::remove_flag(grid, lv::FLAG_SCROLL_CHAIN);
    style_grid_scrollbar(grid, false);
}

