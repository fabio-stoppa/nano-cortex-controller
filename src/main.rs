//! Nano Cortex editor — Rust entry on ESP-IDF.
//! Protocol + BLE + state + FX + tuner + amp catalog + UI are Rust.

mod amp;
mod ble;
mod fx;
mod memsafe;
mod state;
mod syncutil;
mod tuner;
mod ui;

use std::thread;
use std::time::Duration;

use esp_idf_svc::sys::{
    self, esp_err_t, esp_log_level_set, esp_log_level_t_ESP_LOG_INFO, esp_log_level_t_ESP_LOG_WARN,
    ESP_OK,
};
use log::info;

fn check(err: esp_err_t, what: &str) {
    if err != ESP_OK {
        panic!("{what} failed: {err}");
    }
}

/// Quiet ROUTINE BLE/state INFO so Amount / FX focus lines are readable.
/// Override via `sdkconfig` `CONFIG_NANO_LOG_QUIET` when the symbol is present;
/// this const is the always-available fallback (flip here if needed).
const LOG_QUIET_FALLBACK: bool = true;

fn apply_log_filters() {
    let quiet = sdkconfig_bool("CONFIG_NANO_LOG_QUIET").unwrap_or(LOG_QUIET_FALLBACK);
    let tuner_only = sdkconfig_bool("CONFIG_NANO_LOG_TUNER_ONLY").unwrap_or(false);

    unsafe {
        if tuner_only {
            esp_log_level_set(core::ptr::null(), esp_log_level_t_ESP_LOG_WARN);
            esp_log_level_set(c"nano_tuner".as_ptr(), esp_log_level_t_ESP_LOG_INFO);
            info!("log filter: TUNER_ONLY");
        } else if quiet {
            esp_log_level_set(c"nano_state".as_ptr(), esp_log_level_t_ESP_LOG_WARN);
            esp_log_level_set(c"nano_ble".as_ptr(), esp_log_level_t_ESP_LOG_WARN);
            info!("log filter: QUIET (state/BLE WARN; FX Amount INFO)");
        }
    }
}

fn sdkconfig_bool(name: &str) -> Option<bool> {
    // embuild / esp-idf-sys may expose CONFIG_* as compile-env strings ("y" / "1").
    let v = match name {
        "CONFIG_NANO_LOG_QUIET" => option_env!("CONFIG_NANO_LOG_QUIET"),
        "CONFIG_NANO_LOG_TUNER_ONLY" => option_env!("CONFIG_NANO_LOG_TUNER_ONLY"),
        _ => None,
    }?;
    Some(v == "y" || v == "1" || v == "Y")
}

fn main() {
    // Keep module entry symbols in the final binary for cross-module callers.
    ble::ensure_linked();
    state::ensure_linked();
    fx::ensure_linked();
    tuner::ensure_linked();
    amp::ensure_linked();
    ui::ensure_linked();

    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    apply_log_filters();

    // Banner runs after nano_ui_start: the USB-JTAG console drops stdout writes
    // issued this early in boot, so anything logged before it never ships.
    check(ui::nano_ui_start(), "nano_ui_start");

    info!("firmware build 2026-07-27-rust8 (syncutil + module splits; nano-wire + BLE + state + FX + tuner + amp + UI)");
    info!("Nano Cortex firmware starting");

    check(ble::nano_ble_start(), "nano_ble_start");

    loop {
        thread::sleep(Duration::from_secs(60));
        if !ble::nano_ble_connected() {
            info!("Waiting for Nano Cortex (power on, BT enabled, not paired elsewhere)");
        } else if !state::nano_state_amp_ready() {
            info!("BLE connected — waiting for preset state parse");
        }
    }
}
