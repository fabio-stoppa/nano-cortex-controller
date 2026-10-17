#pragma once

/* LVGL only — app types live in Rust (`crate::{fx,state,tuner,amp,…}` /
 * `nano_wire`). Waveshare BSP headers stay out: they pull in
 * `esp_lcd_touch.h`, whose C23 `[[deprecated]]` attribute clang rejects in the
 * bindgen pass. BSP entry points are declared by hand in `src/ui/lvgl.rs`. */

#include "lvgl.h"

LV_FONT_DECLARE(lv_font_montserrat_16);
LV_FONT_DECLARE(lv_font_montserrat_18);
LV_FONT_DECLARE(lv_font_montserrat_20);
LV_FONT_DECLARE(lv_font_montserrat_24);
LV_FONT_DECLARE(lv_font_montserrat_48);
