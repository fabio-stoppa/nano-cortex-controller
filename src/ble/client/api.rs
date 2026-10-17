//! Public `nano_ble_*` C ABI entry points.

use super::*;
use super::{ctx::*, gap::*, gatt::*, worker::*};

pub fn nano_ble_connected() -> bool {
    LINK_UP.load(Ordering::Acquire)
}

pub fn nano_ble_scanning() -> bool {
    SCANNING.load(Ordering::Acquire)
}

/// Drop pending C305 packets (call on disconnect).
pub fn nano_ble_drain_rx_queue() {
    drain_rx_queue(ctx());
}

/// Drop the FIFO backlog only — keeps the live pitch mailbox. Call when the tuner goes live.
pub fn nano_ble_flush_c305_backlog() {
    let s = ctx();
    if !s.queue.is_null() {
        unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
    }
}

/// Write payload to C304 (no-op if not connected).
pub fn nano_ble_write_c304(label: &CStr, data: &[u8]) -> esp_err_t {
    let Some(link) = link_snap() else {
        warn!(
            target: TAG,
            "Skip write ({}): not ready",
            label.to_str().unwrap_or("?")
        );
        return ESP_ERR_INVALID_STATE;
    };
    let len = data.len();
    if len == 0 || len > crate::memsafe::PKT_MAX {
        return ESP_ERR_INVALID_ARG;
    }

    nano_wire::log_bytes(TAG, label.to_str().unwrap_or("C304"), data);

    let err = unsafe {
        esp_ble_gattc_write_char(
            link.gattc_if,
            link.conn_id,
            link.c304,
            len as u16,
            data.as_ptr().cast_mut(),
            esp_gatt_write_type_t_ESP_GATT_WRITE_TYPE_RSP,
            esp_gatt_auth_req_t_ESP_GATT_AUTH_REQ_NONE,
        )
    };
    if err != ESP_OK {
        error!(target: TAG, "write_char failed: {}", err_name(err));
    }
    err
}

/// Read C300/C301/C307 (and log hex) — boot BPM hunt.
pub fn nano_ble_read_bpm_probe_chars() {
    let s = ctx();
    if !LINK_UP.load(Ordering::Acquire) {
        warn!(target: TAG, "BPM hunt: GATT read skipped (not connected)");
        return;
    }
    if s.bpm_read_count == 0 {
        warn!(target: TAG, "BPM hunt: no readable C300/C301/C307 handles");
        return;
    }
    s.bpm_read_next = 0;
    bpm_gatt_read_next(s);
}

/// Init NVS + BT controller, register the GATT client, start scanning.
pub fn nano_ble_start() -> esp_err_t {
    let s = ctx();
    s.gattc_if = ESP_GATT_IF_NONE as esp_gatt_if_t;
    s.conn_id = 0;
    s.remote_bda = [0; 6];
    s.connected = false;
    s.scanning = false;
    s.drop_count = 0;
    reset_handles(s);

    unsafe {
        let mut ret = nvs_flash_init();
        if ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND {
            check(nvs_flash_erase(), "nvs_flash_erase");
            ret = nvs_flash_init();
        }
        check(ret, "nvs_flash_init");

        check(
            esp_bt_controller_mem_release(esp_bt_mode_t_ESP_BT_MODE_CLASSIC_BT),
            "esp_bt_controller_mem_release",
        );

        let mut bt_cfg = MaybeUninit::<esp_bt_controller_config_t>::uninit();
        nano_bt_controller_config_default(bt_cfg.as_mut_ptr());
        let mut bt_cfg = bt_cfg.assume_init();
        check(esp_bt_controller_init(&mut bt_cfg), "esp_bt_controller_init");
        check(
            esp_bt_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE),
            "esp_bt_controller_enable",
        );
        check(esp_bluedroid_init(), "esp_bluedroid_init");
        check(esp_bluedroid_enable(), "esp_bluedroid_enable");

        check(
            esp_ble_gap_register_callback(Some(gap_cb)),
            "esp_ble_gap_register_callback",
        );
        check(
            esp_ble_gattc_register_callback(Some(gattc_cb)),
            "esp_ble_gattc_register_callback",
        );
        check(
            esp_ble_gattc_app_register(PROFILE_APP_ID),
            "esp_ble_gattc_app_register",
        );
        check(
            crate::ble::midi_fs::register_app(),
            "midi_fs::register_app",
        );
    }

    if !c305_queue_init(s) {
        return ESP_ERR_NO_MEM;
    }

    info!(target: TAG, "BLE stack ready (Nano + Chocolate MIDI)");
    ESP_OK
}
