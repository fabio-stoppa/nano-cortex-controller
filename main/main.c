/*
 * Nano Cortex editor — ESP-IDF C firmware for Waveshare ESP32-S3-Touch-LCD-4.3B.
 */

#include "nano_ble.h"
#include "nano_state.h"
#include "nano_ui.h"

#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "sdkconfig.h"

static const char *TAG = "nano";

static void configure_log_filter(void)
{
#if CONFIG_NANO_LOG_TUNER_ONLY
    esp_log_level_set("*", ESP_LOG_WARN);
    esp_log_level_set("nano_tuner", ESP_LOG_INFO);
#endif
}

void app_main(void)
{
    configure_log_filter();

    /* Bump this string whenever diagnosing "did flash land?" — check serial at boot. */
    ESP_LOGI(TAG, "firmware build 2026-07-26ao (no beat dots; plain BPM header)");
    ESP_LOGI(TAG, "Nano Cortex firmware starting");

    ESP_ERROR_CHECK(nano_ui_start());
    ESP_ERROR_CHECK(nano_ble_start());

    while (true) {
        vTaskDelay(pdMS_TO_TICKS(60000));
        if (!nano_ble_connected()) {
            ESP_LOGI(TAG, "Waiting for Nano Cortex (power on, BT enabled, not paired elsewhere)");
        } else if (!nano_state_get_amp()) {
            ESP_LOGI(TAG, "BLE connected — waiting for preset state parse");
        }
    }
}
