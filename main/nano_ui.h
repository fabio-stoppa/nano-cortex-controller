#pragma once

#include "esp_err.h"

#include "nano_proto.h"
#include "nano_tuner.h"
#include "nano_fx.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Init Waveshare LCD + LVGL and show amp panel. */
esp_err_t nano_ui_start(void);

/** Refresh on-screen amp values (safe from any task). */
void nano_ui_set_amp(const nano_amp_t *amp);

/** Update BLE status line. */
void nano_ui_set_ble_connected(bool connected);

/** Update header tempo readout (safe from any task). */
void nano_ui_set_bpm(uint16_t bpm, bool valid);

/**
 * Live tap-tempo notify: update BPM and rephase the beat blinker to this instant.
 * Safe from any task.
 */
void nano_ui_bpm_tap(uint16_t bpm);

/** Update header preset name (safe from any task). */
void nano_ui_set_preset_name(const char *name);

/** Refresh tuner tab (note/cents/status). */
void nano_ui_set_tuner(const nano_tuner_t *tuner);

/** Refresh gate + FX tabs. */
void nano_ui_set_fx(const nano_fx_state_t *fx);

/** Drain pending UI updates; call from the LVGL task only. */
void nano_ui_dispatch_if_pending(void);

#ifdef __cplusplus
}
#endif
