idf_component_register(SRCS "src/board.c"
                            "src/lvgl_port.c"
                    INCLUDE_DIRS "include"
                    REQUIRES  "esp_timer"
                              "esp_lcd" "esp_lcd_touch" "esp_lcd_touch_gt911"
                              "lvgl"
)