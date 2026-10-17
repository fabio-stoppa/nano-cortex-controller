//! Public hooks used by the Nano BLE client (GAP / start / status).

use core::sync::atomic::Ordering;

use esp_idf_svc::sys::*;
use log::{error, info};

use super::ctx::*;
use super::worker;

pub fn profile_app_id() -> u16 {
    PROFILE_APP_ID
}

pub fn is_connected() -> bool {
    is_link_up()
}

/// True while Nano is up and we still want FootCtrl and/or FootCtrlPlus.
pub fn need_peer() -> bool {
    super::ctx::need_peer()
}

pub fn gattc_if() -> esp_gatt_if_t {
    let v = GATTC_IF.load(Ordering::Acquire);
    if v < 0 {
        ESP_GATT_IF_NONE as esp_gatt_if_t
    } else {
        v as esp_gatt_if_t
    }
}

/// Register the second GATTC app + MIDI worker. Call once after Bluedroid is up.
pub fn register_app() -> esp_err_t {
    let s = ctx();
    if !worker::queue_init(s) {
        return ESP_ERR_NO_MEM;
    }
    let err = unsafe { esp_ble_gattc_app_register(PROFILE_APP_ID) };
    if err != ESP_OK {
        error!(target: TAG, "gattc app register failed: {}", err_name(err));
    } else {
        info!(target: TAG, "MIDI footswitch GATTC app registered (id={PROFILE_APP_ID})");
    }
    err
}

/// True when the complete local name is a Chocolate / Chocolate Plus.
pub fn is_footctrl_name(adv_name: &[u8]) -> bool {
    name_matches(adv_name)
}

/// If `adv_name` is a Chocolate we still need, and Nano is already up, open GATT.
/// Returns true when a connect was started (caller should mark scan stopped).
pub fn on_adv_match(
    adv_name: &[u8],
    bda: *mut u8,
    addr_type: esp_ble_addr_type_t,
) -> bool {
    let Some(kind) = classify_name(adv_name) else {
        return false;
    };
    // Nano first — Bluedroid CONNECT is delivered to every GATTC app; connecting
    // FootCtrl before Nano made the Nano profile think it was linked and stop scanning.
    if !crate::ble::nano_ble_connected() {
        return false;
    }
    let s = ctx();
    if have_kind(s, kind) || s.pending_bda.is_some() {
        return false;
    }
    if free_peer_slot(s).is_none() {
        return false;
    }
    let gattc_if = gattc_if();
    if gattc_if == ESP_GATT_IF_NONE as esp_gatt_if_t {
        return false;
    }

    let label = kind.label();
    info!(target: TAG, "Found {label}, connecting...");
    let mut pending = [0u8; 6];
    unsafe {
        pending.copy_from_slice(core::slice::from_raw_parts(bda, 6));
    }
    s.pending_bda = Some(pending);
    s.pending_kind = kind;
    unsafe {
        esp_ble_gap_stop_scanning();
        esp_ble_gattc_open(gattc_if, bda, addr_type, true);
    }
    true
}

/// Resume BLE scan when Nano and/or Chocolate still need a peer.
pub fn maybe_resume_scan() {
    crate::ble::client::resume_scan_if_needed();
}
