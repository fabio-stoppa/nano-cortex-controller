/* ----------------------------------------------------- UI NVS prefs */

use core::ffi::c_char;

use esp_idf_svc::sys::{
    nvs_close, nvs_commit, nvs_flash_erase, nvs_flash_init, nvs_get_u8, nvs_handle_t, nvs_open,
    nvs_set_u8, nvs_open_mode_t_NVS_READONLY, nvs_open_mode_t_NVS_READWRITE, ESP_ERR_NVS_NOT_FOUND,
    ESP_ERR_NVS_NO_FREE_PAGES, ESP_ERR_NVS_NEW_VERSION_FOUND, ESP_OK,
};
use log::{info, warn};

use super::consts::LayoutMode;

const NVS_NS: &str = "nano_ui\0";
const NVS_KEY_ORIENT: &str = "orient\0";

fn nvs_ns() -> *const c_char {
    NVS_NS.as_ptr().cast()
}

fn nvs_key() -> *const c_char {
    NVS_KEY_ORIENT.as_ptr().cast()
}

/// Idempotent NVS init so UI prefs work before BLE starts.
pub(super) fn prefs_nvs_init() {
    unsafe {
        let mut ret = nvs_flash_init();
        if ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND {
            let _ = nvs_flash_erase();
            ret = nvs_flash_init();
        }
        if ret != ESP_OK {
            warn!("nvs_flash_init for UI prefs failed: {ret}");
        }
    }
}

pub(super) fn load_layout_mode() -> LayoutMode {
    prefs_nvs_init();
    unsafe {
        let mut handle: nvs_handle_t = 0;
        let err = nvs_open(nvs_ns(), nvs_open_mode_t_NVS_READONLY, &mut handle);
        if err != ESP_OK {
            return LayoutMode::Horizontal;
        }
        let mut val: u8 = 0;
        let err = nvs_get_u8(handle, nvs_key(), &mut val);
        nvs_close(handle);
        if err == ESP_OK {
            let mode = LayoutMode::from_u8(val);
            info!(
                "UI orientation from NVS: {}",
                match mode {
                    LayoutMode::Horizontal => "landscape",
                    LayoutMode::HorizontalFlip => "landscape-flip",
                    LayoutMode::Vertical => "portrait",
                    LayoutMode::VerticalFlip => "portrait-flip",
                }
            );
            mode
        } else {
            if err != ESP_ERR_NVS_NOT_FOUND {
                warn!("nvs_get_u8 orient failed: {err}");
            }
            LayoutMode::Horizontal
        }
    }
}

pub(super) fn save_layout_mode(mode: LayoutMode) {
    prefs_nvs_init();
    unsafe {
        let mut handle: nvs_handle_t = 0;
        let err = nvs_open(nvs_ns(), nvs_open_mode_t_NVS_READWRITE, &mut handle);
        if err != ESP_OK {
            warn!("nvs_open nano_ui failed: {err}");
            return;
        }
        let err = nvs_set_u8(handle, nvs_key(), mode as u8);
        if err != ESP_OK {
            warn!("nvs_set_u8 orient failed: {err}");
            nvs_close(handle);
            return;
        }
        let err = nvs_commit(handle);
        nvs_close(handle);
        if err != ESP_OK {
            warn!("nvs_commit orient failed: {err}");
            return;
        }
        info!(
            "UI orientation saved: {}",
            match mode {
                LayoutMode::Horizontal => "landscape",
                LayoutMode::HorizontalFlip => "landscape-flip",
                LayoutMode::Vertical => "portrait",
                LayoutMode::VerticalFlip => "portrait-flip",
            }
        );
    }
}
