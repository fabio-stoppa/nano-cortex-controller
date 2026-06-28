#pragma once

#include "esp_err.h"

#include "nano_proto.h"
#include "nano_tuner.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Init Waveshare LCD + LVGL and show amp panel. */
esp_err_t nano_ui_start(void);

/** Refresh on-screen amp values (safe from any task). */
void nano_ui_set_amp(const nano_amp_t *amp);

/** Update BLE status line. */
void nano_ui_set_ble_connected(bool connected);

/** Refresh tuner tab (note/cents/status). */
void nano_ui_set_tuner(const nano_tuner_t *tuner);

#ifdef __cplusplus
}
#endif
