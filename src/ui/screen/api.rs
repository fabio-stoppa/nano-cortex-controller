/* ------------------------------------------------------- public C entries */

use super::*;
use super::build::ui_build_timer_cb;
use super::consts::{
    NANO_BPM_MAX, NANO_BPM_MIN, UI_DISP_AMP, UI_DISP_BLE, UI_DISP_BPM, UI_DISP_FX, UI_DISP_PRESET,
    UI_DISP_TEMPO, UI_DISP_TUNER,
};
use super::dispatch::ui_request_dispatch;
use super::helpers::{c_buf_str, copy_into, struct_bytes};
use super::state::{with_inbox, BUILD_DONE};
/// Init Waveshare LCD + LVGL and show amp panel.
pub fn nano_ui_start() -> esp_err_t {
    let (err, panel, touch) = lv::rgb_lcd_init();
    if err != ESP_OK {
        return err;
    }
    let err = lv::port_init(panel, touch);
    if err != ESP_OK {
        return err;
    }
    let err = lv::bl_on();
    if err != ESP_OK {
        return err;
    }

    BUILD_DONE.store(false, Ordering::Release);

    if !lv::port_lock(-1) {
        return ESP_FAIL;
    }
    let build_timer = lv::timer_create(Some(ui_build_timer_cb), 1, ptr::null_mut());
    if build_timer.is_null() {
        lv::port_unlock();
        return ESP_ERR_NO_MEM;
    }
    lv::timer_set_repeat_count(build_timer, 1);
    lv::port_unlock();

    // 60 s budget, same as the C semaphore wait.
    for _ in 0..6000 {
        if BUILD_DONE.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !BUILD_DONE.load(Ordering::Acquire) {
        error!("UI build timed out");
        return ESP_FAIL;
    }

    info!("UI ready (800x480, left chain rail + panels)");
    ESP_OK
}

/// Refresh on-screen amp values (safe from any task).
pub fn nano_ui_set_amp(amp: Option<&NanoAmpC>) {
    with_inbox(|u| {
        match amp.filter(|a| a.valid) {
            Some(amp) => {
                if u.ui_amp_cache_valid && struct_bytes(amp) == struct_bytes(&u.ui_amp_cache) {
                    return;
                }
                u.ui_amp_cache = *amp;
                u.ui_amp_cache_valid = true;
                u.async_amp = *amp;
                u.async_amp_have = true;
            }
            None => {
                if !u.ui_amp_cache_valid {
                    return;
                }
                u.ui_amp_cache_valid = false;
                u.async_amp_have = false;
            }
        }
        u.dispatch_mask |= UI_DISP_AMP;
        ui_request_dispatch();
    });
}

/// Update BLE status line.
pub fn nano_ui_set_ble_connected(connected: bool) {
    with_inbox(|u| {
        u.async_ble_connected = connected;
        u.async_ble_pending = true;
        u.dispatch_mask |= UI_DISP_BLE;
        ui_request_dispatch();
    });
}

/// Refresh tuner tab (note/cents/status).
pub fn nano_ui_set_tuner(tuner: &NanoTunerC) {
    with_inbox(|u| {
        if u.tuner_note.is_null() {
            return;
        }
        u.async_tuner = *tuner;
        u.async_tuner_pending = true;
        u.dispatch_mask |= UI_DISP_TUNER;
        ui_request_dispatch();
    });
}

/// Update header preset name (safe from any task).
pub fn nano_ui_set_preset_name(name: &str) {
    let incoming = name;
    with_inbox(|u| {
        if u.ui_preset_cache_valid && c_buf_str(&u.ui_preset_cache) == incoming {
            return;
        }

        copy_into(&mut u.ui_preset_cache, incoming);
        u.ui_preset_cache_valid = true;
        copy_into(&mut u.async_preset, incoming);
        u.async_preset_pending = true;
        u.dispatch_mask |= UI_DISP_PRESET;
        ui_request_dispatch();
    });
}

/// Update header tempo readout (safe from any task).
pub fn nano_ui_set_bpm(bpm: u16, valid: bool) {
    with_inbox(|u| {
        if valid {
            if u.ui_bpm_cache_valid && u.ui_bpm_cache == bpm {
                return;
            }
            u.ui_bpm_cache = bpm;
            u.ui_bpm_cache_valid = true;
        } else {
            if !u.ui_bpm_cache_valid {
                return;
            }
            u.ui_bpm_cache_valid = false;
        }

        u.async_bpm = bpm;
        u.async_bpm_valid = valid;
        u.async_bpm_pending = true;
        u.dispatch_mask |= UI_DISP_BPM;
        ui_request_dispatch();
    });
}

/// Live tap-tempo notify: update BPM and rephase the beat blinker to this
/// instant. Safe from any task.
pub fn nano_ui_bpm_tap(bpm: u16) {
    if !(NANO_BPM_MIN..=NANO_BPM_MAX).contains(&bpm) {
        return;
    }

    with_inbox(|u| {
        u.ui_bpm_cache = bpm;
        u.ui_bpm_cache_valid = true;
        u.async_bpm = bpm;
        u.async_bpm_valid = true;
        u.async_bpm_pending = true;
        u.dispatch_mask |= UI_DISP_BPM;
        ui_request_dispatch();
    });
}

/// Pedal Tempo Mode on/off (Footswitch / peer notify). Shows or hides the Tempo
/// screen without writing enter/exit to the pedal. Safe from any task.
pub fn nano_ui_set_tempo_mode(active: bool) {
    with_inbox(|u| {
        u.async_tempo_mode = active;
        u.async_tempo_mode_pending = true;
        u.dispatch_mask |= UI_DISP_TEMPO;
        ui_request_dispatch();
    });
}

/// Refresh gate + FX tabs.
pub fn nano_ui_set_fx(fx: &NanoFxState) {
    with_inbox(|u| {
        if u.ui_fx_cache_valid && struct_bytes(fx) == struct_bytes(&u.ui_fx_cache) {
            return;
        }
        u.ui_fx_cache = *fx;
        u.ui_fx_cache_valid = true;
        u.async_fx = *fx;
        u.async_fx_pending = true;
        u.dispatch_mask |= UI_DISP_FX;
        ui_request_dispatch();
    });
}
