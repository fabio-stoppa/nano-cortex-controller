/* ---------------------------------------------------------------- amp tab */

use super::*;
use super::chain::update_chain_blocks_locked;
use super::consts::{AMP_PEDAL_PARAM_COUNT, CHAIN_AMP, KNOB_COUNT, NANO_AMP_CAPTURE_NAME_MAX};
use super::dispatch::refresh_preset_label_locked;
use super::header::sync_header_model_locked;
use super::helpers::{fx_state, pct, set_label_if_changed, set_slider_if_changed};
use super::params::update_pedal_params_grid_scroll;
use super::state::ui;
use super::tempo::set_slider_enabled_locked;
pub(super) fn apply_amp_locked(amp: *const NanoAmpC) {
    let u = ui();
    u.ui_sync = true;

    let amp = unsafe { amp.as_ref() }.filter(|a| a.valid);

    let Some(amp) = amp else {
        // Keep last knobs while connected — blanking to 0/"-" looks like a spike
        // during brief invalid windows around preset dumps.
        if !u.ble_connected {
            for i in 0..KNOB_COUNT {
                if i as i32 == u.drag_knob || u.knobs[i].slider.is_null() {
                    continue;
                }
                lv::slider_set_value(u.knobs[i].slider, 0, lv::ANIM_OFF);
                lv::label_set_text(u.knobs[i].value, "-");
                u.last_sent[i] = 0;
            }
            if !u.cap_vol.slider.is_null() && u.cap_vol_drag == 0 {
                lv::slider_set_value(u.cap_vol.slider, 128, lv::ANIM_OFF);
                lv::label_set_text(u.cap_vol.value, "-");
                lv::add_state(u.cap_vol.slider, lv::STATE_DISABLED);
            }
            set_slider_enabled_locked(false);
        }
        update_chain_blocks_locked(ptr::null(), fx_state());
        u.amp_chrome_valid = false;
        u.ui_sync = false;
        return;
    };

    let raw = [amp.gain, amp.level, amp.bass, amp.mid, amp.treble];

    for i in 0..KNOB_COUNT {
        u.last_sent[i] = raw[i];
        if i as i32 == u.drag_knob {
            continue;
        }
        set_slider_if_changed(u.knobs[i].slider, raw[i] as i32);
        set_label_if_changed(u.knobs[i].value, &format!("{}%", pct(raw[i])));
    }

    if !u.cap_vol.slider.is_null() && !u.cap_vol.value.is_null() {
        lv::set_enabled(u.cap_vol.slider, u.ble_connected);
        if u.cap_vol_drag == 0 {
            if amp.have_capture_volume {
                set_slider_if_changed(u.cap_vol.slider, amp.capture_volume as i32);
                set_label_if_changed(
                    u.cap_vol.value,
                    &format!("{}%", pct(amp.capture_volume)),
                );
                u.cap_vol_last_sent = amp.capture_volume as i32;
            } else {
                lv::slider_set_value(u.cap_vol.slider, 128, lv::ANIM_OFF);
                lv::label_set_text(u.cap_vol.value, "-");
            }
        }
    }

    set_slider_enabled_locked(u.ble_connected);
    update_pedal_params_grid_scroll(u.amp_params_row, AMP_PEDAL_PARAM_COUNT);

    // Knob ticks arrive many times per second. Chain / preset / header only
    // change with capture/slot metadata — skip them on pure gain/EQ/master.
    let name_bytes: &[u8] = unsafe {
        core::slice::from_raw_parts(
            amp.capture_name.as_ptr() as *const u8,
            NANO_AMP_CAPTURE_NAME_MAX,
        )
    };
    let chrome_dirty = !u.amp_chrome_valid
        || amp.have_capture_slot != u.amp_chrome_have_slot
        || (amp.have_capture_slot && amp.capture_slot != u.amp_chrome_slot)
        || name_bytes != u.amp_chrome_name;

    if chrome_dirty {
        u.amp_chrome_valid = true;
        u.amp_chrome_have_slot = amp.have_capture_slot;
        u.amp_chrome_slot = amp.capture_slot;
        u.amp_chrome_name.copy_from_slice(name_bytes);
        u.amp_chrome_name[NANO_AMP_CAPTURE_NAME_MAX - 1] = 0;
        update_chain_blocks_locked(amp as *const NanoAmpC, fx_state());
        refresh_preset_label_locked();
        if u.chain_selected == CHAIN_AMP && !u.tuner_view {
            sync_header_model_locked(fx_state());
        }
    }
    u.ui_sync = false;
}

