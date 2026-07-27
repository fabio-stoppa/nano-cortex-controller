#pragma once

#include "nano_ble.h"
#include "nano_state.h"
#include "nano_ui.h"

/* LVGL: the UI is a Rust port of the old nano_ui.c, so every widget entry
 * point it used has to reach bindgen.
 *
 * The Waveshare BSP headers (`bsp/board.h`, `bsp/lvgl_port.h`) are deliberately
 * left out: they pull in `esp_lcd_touch.h`, whose C23 `[[deprecated]]` attribute
 * clang rejects in the bindgen pass. Their five entry points are declared by
 * hand in `src/ui/c_deps.rs` with opaque handles instead. */
#include "lvgl.h"

#include "nano_fonts.h"
