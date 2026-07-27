//! LVGL front end: the Rust port of `components/nano/nano_ui.c` plus the
//! bitmap and LVGL/BSP glue it needs.

pub mod icons;
pub mod lvgl;
pub mod screen;

/// Bring up the display and block until the first frame is on screen.
pub fn nano_ui_start() -> esp_idf_svc::sys::esp_err_t {
    unsafe { screen::nano_ui_start() }
}

/// Touch every exported symbol so rustc does not dead-strip them from the
/// firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        screen::nano_ui_start as *const (),
        screen::nano_ui_set_amp as *const (),
        screen::nano_ui_set_ble_connected as *const (),
        screen::nano_ui_set_tuner as *const (),
        screen::nano_ui_set_preset_name as *const (),
        screen::nano_ui_set_bpm as *const (),
        screen::nano_ui_bpm_tap as *const (),
        screen::nano_ui_set_tempo_mode as *const (),
        screen::nano_ui_set_fx as *const (),
        screen::nano_ui_dispatch_if_pending as *const (),
    ];
    core::hint::black_box(fns);
}
