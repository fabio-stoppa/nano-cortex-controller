//! Nano Cortex editor — Rust entry on ESP-IDF.
//! Protocol + BLE + state + FX + tuner + amp catalog + UI are Rust.

mod amp;
mod ble;
mod fx;
mod state;
mod tuner;
mod ui;

use std::thread;
use std::time::Duration;

use esp_idf_svc::sys::{self, esp_err_t, ESP_OK};
use log::info;

fn check(err: esp_err_t, what: &str) {
    if err != ESP_OK {
        panic!("{what} failed: {err}");
    }
}

fn main() {
    // Keep C-ABI symbols in the final binary for cross-module callers.
    nano_wire::ffi::ensure_linked();
    ble::ensure_linked();
    state::ensure_linked();
    fx::ensure_linked();
    tuner::ensure_linked();
    amp::ensure_linked();
    ui::ensure_linked();

    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Banner runs after nano_ui_start: the USB-JTAG console drops stdout writes
    // issued this early in boot, so anything logged before it never ships.
    check(ui::nano_ui_start(), "nano_ui_start");

    info!("firmware build 2026-07-26-rust7 (nano-wire + BLE + state + FX + tuner + amp + UI in Rust)");
    info!("Nano Cortex firmware starting");

    check(ble::nano_ble_start(), "nano_ble_start");

    loop {
        thread::sleep(Duration::from_secs(60));
        if !ble::nano_ble_connected() {
            info!("Waiting for Nano Cortex (power on, BT enabled, not paired elsewhere)");
        } else if state::nano_state_get_amp().is_null() {
            info!("BLE connected — waiting for preset state parse");
        }
    }
}
