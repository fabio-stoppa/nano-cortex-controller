#pragma once

#include "lvgl.h"

LV_FONT_DECLARE(lv_font_montserrat_16);
LV_FONT_DECLARE(lv_font_montserrat_18);
LV_FONT_DECLARE(lv_font_montserrat_20);
LV_FONT_DECLARE(lv_font_montserrat_24);
LV_FONT_DECLARE(lv_font_montserrat_48);

#define UI_FONT_SM (&lv_font_montserrat_16)
#define UI_FONT_CHAIN (&lv_font_montserrat_18)
#define UI_FONT_MD (&lv_font_montserrat_20)
#define UI_FONT_LG (&lv_font_montserrat_24)
#define UI_FONT_SYMBOL (&lv_font_montserrat_24)
#define UI_FONT_TUNER_NOTE (&lv_font_montserrat_48)
