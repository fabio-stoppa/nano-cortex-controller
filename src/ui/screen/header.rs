/* ----------------------------------------------------------------- header */

use super::*;
use super::chain::chain_slot_is_fx;
use super::consts::CHAIN_AMP;
use super::helpers::{amp_state, set_label_if_changed};
use super::state::ui;
pub(super) fn sync_header_model_locked(fx: *const NanoFxState) {
    let u = ui();
    if u.fx_model_btn.is_null() || u.fx_model_btn_lbl.is_null() {
        return;
    }

    if u.tuner_view {
        set_label_if_changed(u.fx_model_btn_lbl, "Tuner");
        lv::remove_state(u.fx_model_btn, lv::STATE_DISABLED);
        lv::remove_flag(u.fx_model_btn, lv::FLAG_CLICKABLE);
        lv::set_style_bg_color(u.fx_model_btn, lv::color_hex(0x080808), lv::PART_MAIN);
        lv::set_style_bg_opa(u.fx_model_btn, lv::OPA_COVER, lv::PART_MAIN);
        lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0xECECEC), 0);
        return;
    }

    lv::add_flag(u.fx_model_btn, lv::FLAG_CLICKABLE);
    lv::set_style_bg_color(u.fx_model_btn, lv::color_hex(0x080808), lv::PART_MAIN);
    lv::set_style_bg_opa(u.fx_model_btn, lv::OPA_COVER, lv::PART_MAIN);
    lv::set_style_bg_opa(
        u.fx_model_btn,
        lv::OPA_40,
        lv::PART_MAIN | lv::SEL_DISABLED,
    );

    let mut enabled = false;
    let mut text = String::from("-");

    if u.chain_selected == CHAIN_AMP {
        let amp = amp_state();
        if let Some(a) = unsafe { amp.as_ref() }.filter(|a| a.valid) {
            text = crate::amp::nano_amp_capture_header_text(a);
            enabled = u.ble_connected;
        } else {
            text = "Amp".to_string();
        }
    } else if chain_slot_is_fx(u.chain_selected) {
        if let Some(fx) = unsafe { fx.as_ref() } {
            let block = &fx.blocks[u.fx_selected as usize];
            if block.valid {
                text = nano_fx_block_model_text(block);
                enabled = u.ble_connected;
            }
        }
    }

    set_label_if_changed(u.fx_model_btn_lbl, &text);

    if enabled {
        lv::remove_state(u.fx_model_btn, lv::STATE_DISABLED);
        lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0xECECEC), 0);
    } else {
        lv::add_state(u.fx_model_btn, lv::STATE_DISABLED);
        lv::set_style_text_color(u.fx_model_btn_lbl, lv::color_hex(0x606060), 0);
    }
}

