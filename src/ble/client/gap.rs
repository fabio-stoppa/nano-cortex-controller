//! GAP scan lifecycle: params-set, scan start/stop, and advertisement
//! filtering for the Nano Cortex name prefix.

use super::*;
use super::ctx::*;

// ESP-IDF bindgen names events `esp_gap_ble_cb_event_t_ESP_*` (type prefix +
// enum). Rust wants SCREAMING_SNAKE for consts-in-patterns; we keep the
// generated identifiers so arms match the C headers verbatim.
#[allow(non_upper_case_globals)]
pub(super) unsafe extern "C" fn gap_cb(event: esp_gap_ble_cb_event_t, param: *mut esp_ble_gap_cb_param_t) {
    let s = ctx();

    match event {
        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_PARAM_SET_COMPLETE_EVT => start_scan(s),

        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_START_COMPLETE_EVT => {
            if unsafe { (*param).scan_start_cmpl.status } != esp_bt_status_t_ESP_BT_STATUS_SUCCESS {
                error!(target: TAG, "Scan start complete failed");
                s.scanning = false;
                publish_link(s);
            }
        }

        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_RESULT_EVT => {
            let scan_rst = unsafe { &mut (*param).scan_rst };
            if scan_rst.search_evt != esp_gap_search_evt_t_ESP_GAP_SEARCH_INQ_RES_EVT || s.connected
            {
                return;
            }

            let mut adv_name_len: u8 = 0;
            let adv_name = unsafe {
                esp_ble_resolve_adv_data_by_type(
                    scan_rst.ble_adv.as_mut_ptr(),
                    scan_rst.adv_data_len as u16 + scan_rst.scan_rsp_len as u16,
                    esp_ble_adv_data_type_ESP_BLE_AD_TYPE_NAME_CMPL,
                    &mut adv_name_len,
                )
            };
            if adv_name.is_null() || (adv_name_len as usize) < NAME_PREFIX.len() {
                return;
            }
            let Some(adv_name) = (unsafe {
                crate::memsafe::bytes_at_most(adv_name, adv_name_len as usize, 64)
            }) else {
                return;
            };
            if !adv_name.starts_with(NAME_PREFIX) {
                return;
            }

            info!(target: TAG, "Found Nano Cortex, connecting...");
            unsafe { esp_ble_gap_stop_scanning() };
            s.scanning = false;
            publish_link(s);
            unsafe {
                esp_ble_gattc_open(
                    s.gattc_if,
                    scan_rst.bda.as_mut_ptr(),
                    scan_rst.ble_addr_type,
                    true,
                )
            };
        }

        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_STOP_COMPLETE_EVT => {
            s.scanning = false;
            publish_link(s);
        }

        _ => {}
    }
}
