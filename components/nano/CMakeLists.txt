idf_component_register(
    SRCS "bt_cfg_helper.c"
    INCLUDE_DIRS "."
    REQUIRES nvs_flash bt esp_timer waveshare_esp32_s3_touch_lcd_4_3
)
