
#include "esp_log.h"
#include "bsp/lvgl_port.h"
#include "bsp/board.h"
#include "widgets/lv_demo_widgets.h"

static const char *TAG = "main";

void app_main(void)
{

    esp_lcd_panel_handle_t panel_handle = NULL;
    esp_lcd_touch_handle_t touch_handle = NULL;

    // Initialize LCD and touch hardware
        esp_err_t ret = waveshare_esp32_s3_rgb_lcd_init(&panel_handle, &touch_handle);
    if (ret != ESP_OK) {
            ESP_LOGE(TAG, "Hardware driver initialization failed: %s", esp_err_to_name(ret));
            // In production, trigger a safe state / fallback mode here instead of crashing
            vTaskDelete(NULL);
            return;
        }

    // Initialize LVGL porting layer
    ret = lvgl_port_init(panel_handle, touch_handle);
    if (ret != ESP_OK) {
            ESP_LOGE(TAG, "LVGL port layer initialization failed: %s", esp_err_to_name(ret));
            vTaskDelete(NULL);
            return;
        }

    // Turn on screen backlight
    ret = waveshare_rgb_lcd_bl_on();
    if (ret != ESP_OK) {
            ESP_LOGW(TAG, "Backlight enabling failed: %s", esp_err_to_name(ret));
            // Proceed anyway as the display/touch driver is still running
        }


    ESP_LOGI(TAG, "Displaying LVGL widgets demo");

    if (lvgl_port_lock(-1)) {
        lv_demo_widgets();
        lvgl_port_unlock();
    } else {
        ESP_LOGE(TAG, "Failed to acquire LVGL lock, widgets demo skipped");
    }

    ESP_LOGI(TAG, "UI tasks running via background worker task. Deleting main task.");

    vTaskDelete(NULL);
}