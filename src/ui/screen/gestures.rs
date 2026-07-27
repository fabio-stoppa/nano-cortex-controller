/* --------------------------------------------------------------- gestures */

use super::*;
use super::consts::{
    KNOB_COUNT, PARAM_CELL_AMP_TAG, PARAM_CELL_CAP_VOL_TAG, PARAM_CELL_GATE_ON_TAG,
    PARAM_CELL_GATE_TAG, PARAM_GESTURE_ADJUST, PARAM_GESTURE_MOVE_THRESH, PARAM_GESTURE_NONE,
    PARAM_GESTURE_PENDING, PARAM_GESTURE_SCROLL, PARAM_GESTURE_SCROLL_MIN_DY,
    PARAM_GESTURE_SETTLE_MS, PARAM_GESTURE_TAP_THRESH,
};
use super::helpers::now_us;
use super::params::{
    param_cell_apply_cap_vol, param_cell_apply_gate, param_cell_apply_gate_on,
    param_cell_apply_slider, pedal_params_can_scroll, pedal_params_row_active,
};
use super::state::ui;
pub(super) fn param_gesture_begin() {
    let u = ui();
    let Some(pt) = lv::indev_point() else {
        u.param_gesture.active = false;
        u.param_gesture.mode = PARAM_GESTURE_NONE;
        return;
    };

    u.param_gesture.active = true;
    u.param_gesture.mode = PARAM_GESTURE_PENDING;
    u.param_gesture.press_us = now_us();
    u.param_gesture.origin = pt;
}

pub(super) fn param_gesture_cancel_drag() {
    let u = ui();
    u.fx_drag_param = -1;
    u.drag_knob = -1;
    u.gate_drag = 0;
    u.gate_on_drag = 0;
    u.cap_vol_drag = 0;
}

pub(super) fn param_gesture_motion() -> (i32, i32) {
    let u = ui();
    if !u.param_gesture.active {
        return (0, 0);
    }
    let Some(pt) = lv::indev_point() else {
        return (0, 0);
    };
    (
        (pt.x - u.param_gesture.origin.x).abs(),
        (pt.y - u.param_gesture.origin.y).abs(),
    )
}

pub(super) fn param_gesture_update() -> u8 {
    let u = ui();
    if !u.param_gesture.active {
        return PARAM_GESTURE_NONE;
    }

    let (adx, ady) = param_gesture_motion();

    if u.param_gesture.mode == PARAM_GESTURE_SCROLL {
        return PARAM_GESTURE_SCROLL;
    }

    if u.param_gesture.mode == PARAM_GESTURE_ADJUST {
        if pedal_params_can_scroll() && ady > adx + 4 {
            u.param_gesture.mode = PARAM_GESTURE_SCROLL;
            param_gesture_cancel_drag();
            return PARAM_GESTURE_SCROLL;
        }
        return PARAM_GESTURE_ADJUST;
    }

    let settled = (now_us() - u.param_gesture.press_us) >= PARAM_GESTURE_SETTLE_MS * 1000;

    if pedal_params_can_scroll()
        && !pedal_params_row_active().is_null()
        && ady >= PARAM_GESTURE_SCROLL_MIN_DY
        && ady > adx
    {
        u.param_gesture.mode = PARAM_GESTURE_SCROLL;
        param_gesture_cancel_drag();
        return PARAM_GESTURE_SCROLL;
    }

    if adx >= PARAM_GESTURE_MOVE_THRESH && adx > ady + 6 {
        u.param_gesture.mode = PARAM_GESTURE_ADJUST;
        return PARAM_GESTURE_ADJUST;
    }

    if settled && adx >= PARAM_GESTURE_MOVE_THRESH && adx >= ady {
        u.param_gesture.mode = PARAM_GESTURE_ADJUST;
        return PARAM_GESTURE_ADJUST;
    }

    PARAM_GESTURE_PENDING
}

pub(super) fn param_gesture_should_apply_on_release() -> bool {
    let mode = param_gesture_update();
    let (adx, ady) = param_gesture_motion();

    if mode == PARAM_GESTURE_SCROLL {
        return false;
    }
    if mode == PARAM_GESTURE_ADJUST {
        return true;
    }
    if adx >= PARAM_GESTURE_TAP_THRESH || ady >= PARAM_GESTURE_TAP_THRESH {
        return false;
    }
    if pedal_params_can_scroll() && ady > adx {
        return false;
    }
    true
}

pub(super) fn param_gesture_end() {
    let u = ui();
    u.param_gesture.active = false;
    u.param_gesture.mode = PARAM_GESTURE_NONE;
}

pub(super) unsafe extern "C" fn param_cell_event_cb(e: *mut lv_event_t) {
    let packed = lv::event_user_data(e);
    let cell = lv::event_current_target(e);
    let code = lv::event_code(e);
    let u = ui();

    if packed & PARAM_CELL_GATE_ON_TAG != 0 {
        if u.gate_on.slider.is_null() || lv::has_flag(u.gate_on.slider, lv::FLAG_HIDDEN) {
            return;
        }
        if code == lv::EVENT_PRESSED {
            param_gesture_begin();
        } else if code == lv::EVENT_PRESSING {
            let mode = param_gesture_update();
            if mode == PARAM_GESTURE_ADJUST {
                u.gate_on_drag = 1;
                param_cell_apply_gate_on(cell, false);
            }
        } else if code == lv::EVENT_RELEASED {
            // Binary half-tap: apply even without a drag (ADJUST) gesture.
            if param_gesture_update() != PARAM_GESTURE_SCROLL {
                param_cell_apply_gate_on(cell, true);
            }
            u.gate_on_drag = 0;
            param_gesture_end();
        }
        return;
    }

    if packed & PARAM_CELL_GATE_TAG != 0 {
        if u.gate.slider.is_null() || lv::has_flag(u.gate.slider, lv::FLAG_HIDDEN) {
            return;
        }
        if code == lv::EVENT_PRESSED {
            param_gesture_begin();
        } else if code == lv::EVENT_PRESSING {
            let mode = param_gesture_update();
            if mode == PARAM_GESTURE_ADJUST {
                u.gate_drag = 1;
                param_cell_apply_gate(cell, false);
            }
        } else if code == lv::EVENT_RELEASED {
            // Same as amp/fx params: arm drag so apply_gate can write on tap.
            if param_gesture_should_apply_on_release() {
                u.gate_drag = 1;
                param_cell_apply_gate(cell, true);
            }
            u.gate_drag = 0;
            param_gesture_end();
        }
        return;
    }

    if packed & PARAM_CELL_CAP_VOL_TAG != 0 {
        if u.cap_vol.slider.is_null() || lv::has_flag(u.cap_vol.slider, lv::FLAG_HIDDEN) {
            return;
        }
        if code == lv::EVENT_PRESSED {
            param_gesture_begin();
        } else if code == lv::EVENT_PRESSING {
            let mode = param_gesture_update();
            if mode == PARAM_GESTURE_ADJUST {
                u.cap_vol_drag = 1;
                param_cell_apply_cap_vol(cell, false);
            }
        } else if code == lv::EVENT_RELEASED {
            if param_gesture_should_apply_on_release() {
                u.cap_vol_drag = 1;
                param_cell_apply_cap_vol(cell, true);
            }
            u.cap_vol_drag = 0;
            param_gesture_end();
        }
        return;
    }

    let idx = packed & 0xFF;
    let is_amp = packed & PARAM_CELL_AMP_TAG != 0;
    if (is_amp && idx >= KNOB_COUNT) || (!is_amp && idx >= NANO_FX_MAX_PARAMS) {
        return;
    }

    let slider = if is_amp {
        u.knobs[idx].slider
    } else {
        u.fx_params[idx].slider
    };
    if slider.is_null() || lv::has_flag(slider, lv::FLAG_HIDDEN) {
        return;
    }

    if code == lv::EVENT_PRESSED {
        param_gesture_begin();
        return;
    }

    if code == lv::EVENT_PRESSING {
        if param_gesture_update() == PARAM_GESTURE_ADJUST {
            if is_amp {
                u.drag_knob = idx as i32;
            } else {
                u.fx_drag_param = idx as i32;
            }
            param_cell_apply_slider(cell, slider, is_amp, idx, false);
        }
        return;
    }

    if code == lv::EVENT_RELEASED {
        if param_gesture_should_apply_on_release() {
            if is_amp {
                u.drag_knob = idx as i32;
            } else {
                u.fx_drag_param = idx as i32;
            }
            param_cell_apply_slider(cell, slider, is_amp, idx, true);
        }
        u.drag_knob = -1;
        u.fx_drag_param = -1;
        param_gesture_end();
    }
}

