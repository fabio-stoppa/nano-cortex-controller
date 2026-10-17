//! BLE GATT clients: Nano Cortex + optional M-Vave Chocolate MIDI footswitch.

pub mod client;
pub mod midi_fs;

pub use client::{
    nano_ble_connected, nano_ble_drain_rx_queue, nano_ble_flush_c305_backlog,
    nano_ble_read_bpm_probe_chars, nano_ble_scanning, nano_ble_start, nano_ble_write_c304,
};

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_ble_start as *const (),
        nano_ble_connected as *const (),
        nano_ble_scanning as *const (),
        nano_ble_write_c304 as *const (),
        nano_ble_read_bpm_probe_chars as *const (),
        nano_ble_drain_rx_queue as *const (),
        nano_ble_flush_c305_backlog as *const (),
        midi_fs::is_connected as *const (),
        midi_fs::register_app as *const (),
    ];
    core::hint::black_box(fns);
}
