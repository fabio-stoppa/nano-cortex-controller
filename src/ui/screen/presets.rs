//! Preset list and save. The list sends a type 29 load; Save sends type 3.

use super::*;
use super::state::ui;
use crate::state::{nano_state_preset_names, nano_state_save_preset, nano_state_select_preset};

pub(super) fn paint_save_locked() {
    let u = ui();
    if u.save_btn.is_null() {
        return;
    }
    let color = if u.preset_dirty {
        0x2FB648
    } else {
        0x1A1A1A
    };
    lv::set_style_bg_color(u.save_btn, lv::color_hex(color), 0);
}

pub(super) extern "C" fn save_btn_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    let _ = nano_state_save_preset();
}

pub(super) extern "C" fn preset_label_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    open_preset_list();
}

extern "C" fn preset_row_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    let slot = lv::event_user_data(e) as u8;
    close_preset_list();
    let _ = nano_state_select_preset(slot);
}

extern "C" fn preset_list_backdrop_event_cb(e: *mut lv_event_t) {
    if lv::event_code(e) != lv::EVENT_CLICKED {
        return;
    }
    if lv::event_current_target(e) != ui().preset_list {
        return;
    }
    close_preset_list();
}

fn close_preset_list() {
    let u = ui();
    if u.preset_list.is_null() {
        return;
    }
    lv::obj_delete(u.preset_list);
    u.preset_list = ptr::null_mut();
}

fn open_preset_list() {
    close_preset_list();
    let names = nano_state_preset_names();
    if names.is_empty() {
        return;
    }

    let screen = lv::screen_active();
    let list = lv::obj_create(screen);
    lv::set_size(list, lv::hor_res(), lv::ver_res());
    lv::set_pos(list, 0, 0);
    lv::set_style_bg_color(list, lv::color_hex(0x000000), 0);
    lv::set_style_bg_opa(list, lv::OPA_70, 0);
    lv::set_style_border_width(list, 0, 0);
    lv::set_style_pad_all(list, 12, 0);
    lv::set_flex_flow(list, lv::FLEX_FLOW_COLUMN);
    lv::set_style_pad_row(list, 4, 0);
    lv::add_flag(list, lv::FLAG_CLICKABLE);
    lv::add_event_cb(list, preset_list_backdrop_event_cb, lv::EVENT_CLICKED, 0);
    ui().preset_list = list;

    for (i, name) in names.iter().enumerate() {
        let slot = (i + 1) as u8;
        let row = lv::button_create(list);
        lv::set_width(row, lv::pct(100));
        lv::set_height(row, 36);
        lv::set_style_bg_color(row, lv::color_hex(0x161616), 0);
        lv::set_style_border_width(row, 0, 0);
        lv::set_style_radius(row, 6, 0);
        let label = if name.is_empty() {
            format!("{slot}")
        } else {
            format!("{slot}  {name}")
        };
        let text = lv::label_create(row);
        lv::label_set_text(text, &label);
        lv::set_style_text_font(text, lv::font_sm(), 0);
        lv::set_style_text_color(text, lv::color_hex(0xE8E8E8), 0);
        lv::align(text, lv::ALIGN_TOP_LEFT, 8, 8);
        lv::add_event_cb(row, preset_row_event_cb, lv::EVENT_CLICKED, slot as usize);
    }
}
