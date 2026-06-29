idf_component_register(
    SRCS "main.c" "nano_protocol.c" "nano_proto.c" "nano_state.c" "nano_ble.c" "nano_ui.c"
         "nano_tuner.c" "nano_fx.c" "nano_fx_catalog.c"
    INCLUDE_DIRS "."
    REQUIRES nvs_flash bt esp_timer bobscott45__waveshare_esp32_s3_touch_lcd_4_3 lvgl__lvgl
)
