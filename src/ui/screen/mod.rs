//! Port of `components/nano/nano_ui.c`.
//!
//! Everything below the `nano_ui_*` entry points runs on the LVGL task, which
//! is the same contract the C file lived under: the public setters only stage a
//! snapshot plus a dirty bit, and the 8 ms dispatch timer paints it.

#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void, CStr};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use esp_idf_svc::sys as sys;
use esp_idf_svc::sys::{
    esp_err_t, lv_color_t, lv_event_t, lv_obj_t, lv_point_t, lv_style_t, lv_timer_t,
    xTaskGetCurrentTaskHandle, ESP_ERR_NO_MEM, ESP_FAIL, ESP_OK,
};
use log::{error, info};
use nano_wire::types::NanoAmpC;

use crate::fx::{
    nano_fx_allowed_model_count, nano_fx_block_model_text, nano_fx_category_label,
    nano_fx_display_to_slider, nano_fx_format_param, nano_fx_get_allowed_model,
    nano_fx_model_category, nano_fx_model_chain_acronym, nano_fx_param_enum_count,
    nano_fx_param_is_enum, nano_fx_param_wire_index, nano_fx_slider_max, nano_fx_slider_min,
    nano_fx_slider_to_display,
    nano_fx_snapshot, nano_fx_write_block_enabled, nano_fx_write_block_model,
    nano_fx_write_gate_enabled, nano_fx_write_gate_reduction, nano_fx_write_param_display,
};
use crate::fx::types::{
    NanoFxBlock, NanoFxModelDef, NanoFxState, NANO_FX_CAT_BASS_OD, NANO_FX_CAT_COMPRESSOR,
    NANO_FX_CAT_DELAY, NANO_FX_CAT_FILTER, NANO_FX_CAT_GUITAR_OD, NANO_FX_CAT_MODULATION,
    NANO_FX_CAT_NONE, NANO_FX_CAT_PITCH, NANO_FX_CAT_REVERB, NANO_FX_CAT_UTILITY, NANO_FX_CAT_WAH,
    NANO_FX_MAX_PARAMS, NANO_FX_POST1, NANO_FX_POST2, NANO_FX_POST3, NANO_FX_PRE1, NANO_FX_PRE2,
};
use crate::state::{
    nano_state_get_bpm, nano_state_snapshot_amp, nano_state_tempo_enter, nano_state_tempo_exit,
    nano_state_tempo_tap, nano_state_write_amp_knob, nano_state_write_capture_slot,
    nano_state_write_capture_volume,
};
use crate::tuner::types::NanoTunerC;
use crate::tuner::{nano_tuner_cents_f, nano_tuner_set_active, nano_tuner_snapshot};
use crate::ui::icons;
use crate::ui::lvgl as lv;

mod amp;
mod api;
mod ble;
mod build;
mod chain;
mod consts;
mod dispatch;
mod fx_panel;
mod gestures;
mod header;
mod helpers;
mod params;
mod picker;
mod prefs;
mod presets;
mod state;
mod tempo;
mod tuner;
mod writes;

pub use api::{
    nano_ui_bpm_tap, nano_ui_select_fx_block, nano_ui_set_amp, nano_ui_set_ble_connected,
    nano_ui_set_bpm, nano_ui_set_fx, nano_ui_set_preset_dirty, nano_ui_set_preset_name,
    nano_ui_set_tempo_mode,
    nano_ui_set_tuner, nano_ui_start,
};
pub use dispatch::nano_ui_dispatch_if_pending;
