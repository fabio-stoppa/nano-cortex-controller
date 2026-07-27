.# Waveshare ESP32-S3 Touch LCD 4.3 Board Support Package (BSP)

This repository contains the ESP-IDF component driver for the **Waveshare ESP32-S3 Touch LCD 4.3 (A)** development board.
It provides complete initialization for the 4.3-inch 800x480 RGB display and the GT911 capacitive touch screen (interfaced via a CH422G I/O expander).

The hardware driver is fully decoupled from the UI framework, allowing you to use the screen independently or with the built-in, thread-safe LVGL porting layer.

---

## References

* **Official Wiki:** [Waveshare ESP32-S3-Touch-LCD-4.3 Wiki](https://www.waveshare.com/wiki/ESP32-S3-Touch-LCD-4.3)
* **Original Demo Code:** This code is based on the official demo code found in the [Waveshare ESP32-S3-Touch-LCD-4.3 Demo ZIP](https://files.waveshare.com/wiki/ESP32-S3-Touch-LCD-4.3/ESP32-S3-Touch-LCD-4.3-Demo.zip)

---

## Features

* **RGB LCD Panel:** Support for the high-resolution 800x480 screen via ESP-IDF's `esp_lcd` driver.
* **Touch Controller:** Integrated GT911 capacitive touchscreen.
* **Backlight Control:** Managed via the onboard CH422G I2C I/O expander.
* **LVGL Dual-Version Compatibility:** Full out-of-the-box compatibility with both **LVGL v8 (>=8.3.11)** and **LVGL v9 (>=9.0.0)**. The porting driver automatically handles differences in display drivers, input devices, tick timers, and rendering pipelines.
* **Hardware Tearing Avoidance:** VSYNC-synchronized double-buffering (Direct Mode) supported on both LVGL v8 and v9 for smooth, tearing-free animations.
* **Optimized Configs:** Full support for Octal PSRAM at 80 MHz to prevent LCD DMA underflow.
* **ESP-IDF Compatibility:** Fully compatible with ESP-IDF v5.1 and v6.0+. The driver complies with the modernized `esp_lcd` API (e.g., migration away from the legacy `.bits_per_pixel` configuration).

---

## Hardware Pinout Configuration

| Signal | GPIO / Interface | Description |
|---|---|---|
| **I2C SDA** | GPIO 8 | Shared I2C data bus (Touch controller / CH422G) |
| **I2C SCL** | GPIO 9 | Shared I2C clock bus |
| **RGB HSYNC** | GPIO 46 | Horizontal Synchronization |
| **RGB VSYNC** | GPIO 3 | Vertical Synchronization |
| **RGB DE** | GPIO 5 | Data Enable |
| **RGB PCLK** | GPIO 7 | Pixel Clock |
| **RGB Data (D0-D15)** | GPIO 14, 38, 18, 17, 10, 39, 0, 45, 48, 47, 21, 1, 2, 42, 41, 40 | 16-bit parallel RGB bus (565 color format) |

---

## How to Include in Your Project

### Option 1: Via the Espressif Component Registry (Recommended)

You can add this component to your project automatically by running this command inside your project directory:

```bash
idf.py add-dependency bobscott45/waveshare_esp32_s3_touch_lcd_4_3
```

### Option 2: Via Git Dependency

Alternatively, you can include it directly by adding the repository as a dependency in your project's `main/idf_component.yml` file:

```yaml
dependencies:
  waveshare_esp32_s3_touch_lcd_4_3:
    git: https://github.com/bobscott45/waveshare_esp32_s3_touch_lcd_4_3.git
```

---

## Required Project Configuration (sdkconfig.defaults)

Because this board features an 800x480 RGB LCD panel and Octal PSRAM, projects using this component **must** configure the ESP-IDF build system to enable and optimize these hardware features. 

It is highly recommended to copy the [`sdkconfig.defaults`](file:///home/robert/CLionProjects/esp/waveshare_esp32_s3_touch_lcd_4_3/examples/lvgl_widgets/sdkconfig.defaults) file from the example, or merge the following configurations into your project's `sdkconfig.defaults` file:

### Critical Settings
* **Octal PSRAM Enable:** The board requires `CONFIG_SPIRAM_MODE_OCT=y` to communicate with the onboard Octal PSRAM.
* **PSRAM Display Buffers:** An 800x480 RGB565 frame buffer requires 768 KB. Double-buffering requires over 1.5 MB. Since internal SRAM is limited to 512 KB, you must configure display buffers to be allocated in PSRAM (`CONFIG_LVGL_PORT_BUF_PSRAM=y`) to avoid out-of-memory crashes on boot.
* **80MHz Bus Speed:** Set Flash and PSRAM speeds to 80 MHz to prevent LCD DMA underflow and panel tearing.
* **16MB Flash Size:** Correctly configure the build system to target the board's 16MB Flash size.

Here is the minimum required configuration to append to your project's `sdkconfig.defaults`:

```config
# CPU Frequency (240MHz for maximum graphics processing speed)
CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ_240=y
CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ=240
CONFIG_ESP32S3_DEFAULT_CPU_FREQ_240=y
CONFIG_ESP32S3_DEFAULT_CPU_FREQ_MHZ=240

# Flash and PSRAM Speeds (80MHz - Stable & Supported for Mixed Modes)
CONFIG_ESPTOOLPY_FLASHFREQ_80M=y
CONFIG_ESPTOOLPY_FLASHFREQ="80m"
CONFIG_SPIRAM_SPEED_80M=y

# Flash Size Configuration (Match 16MB onboard hardware)
CONFIG_ESPTOOLPY_FLASHSIZE_16MB=y
CONFIG_ESPTOOLPY_FLASHSIZE="16MB"

# PSRAM (SPIRAM) Configuration
CONFIG_SPIRAM=y
CONFIG_SPIRAM_BOOT_INIT=y
CONFIG_SPIRAM_MODE_OCT=y
CONFIG_SPIRAM_USE_MALLOC=y
CONFIG_SPIRAM_MALLOC_ALWAYSINTERNAL=16384
CONFIG_SPIRAM_MALLOC_RESERVE_INTERNAL=32768
CONFIG_FREERTOS_TASK_CREATE_ALLOW_EXT_MEM=y

# Bootloader Optimization
CONFIG_BOOTLOADER_COMPILER_OPTIMIZATION_SIZE=y

# Use a larger partition table (1.5MB/2MB app partition) to prevent overflow
CONFIG_PARTITION_TABLE_SINGLE_APP_LARGE=y
CONFIG_PARTITION_TABLE_FILENAME="partitions_singleapp_large.csv"

# Fix RGB LCD Jitter / Tearing without VSYNC artifacts
CONFIG_LCD_RGB_BOUNCE_BUFFER_MODE=y
CONFIG_LCD_RGB_BOUNCE_BUFFER_HEIGHT=120
CONFIG_SPIRAM_FETCH_INSTRUCTIONS=y
CONFIG_SPIRAM_RODATA=y

# LVGL Configuration (Includes Custom Memory Allocation)
CONFIG_LV_USE_DEMO_WIDGETS=y
CONFIG_LVGL_PORT_TASK_STACK_SIZE_KB=16
CONFIG_LV_MEM_CUSTOM=y
CONFIG_LV_USE_CLIB_MALLOC=y
CONFIG_LV_USE_BUILTIN_MALLOC=n
CONFIG_LV_DISP_DEF_REFR_PERIOD=16
CONFIG_LV_DEF_REFR_PERIOD=15
CONFIG_LVGL_PORT_TASK_MIN_DELAY_MS=1

# Tearing Avoidance & Direct Mode (Saves drawing bandwidth on LVGL v8 & v9)
CONFIG_LVGL_PORT_AVOID_TEAR_ENABLE=y
CONFIG_LVGL_PORT_AVOID_TEAR_MODE_3=y
CONFIG_LVGL_PORT_AVOID_TEAR_MODE=3

# Allocate LVGL display buffer in PSRAM (SPIRAM) to avoid SRAM fragmentation crash
CONFIG_LVGL_PORT_BUF_PSRAM=y
CONFIG_LVGL_PORT_BUF_INTERNAL=n

# Compiler Optimization (Perf -O2)
CONFIG_COMPILER_OPTIMIZATION_PERF=y
```

---

## Quick Start (LVGL Usage)

The BSP abstraction layer allows the same initialization code to work seamlessly under both LVGL v8 and v9. In your application's `main.c`, initialize the board and start the LVGL engine using the following sequence:

```c
#include "bsp/board.h"
#include "bsp/lvgl_port.h"

void app_main(void)
{
    esp_lcd_panel_handle_t panel = NULL;
    esp_lcd_touch_handle_t touch = NULL;

    // 1. Initialize hardware drivers independently
    ESP_ERROR_CHECK(waveshare_esp32_s3_rgb_lcd_init(&panel, &touch));

    // 2. Initialize the LVGL port task loop with retrieved handles
    ESP_ERROR_CHECK(lvgl_port_init(panel, touch));

    // 3. Turn on the screen backlight
    ESP_ERROR_CHECK(waveshare_rgb_lcd_bl_on());

    // 4. Lock the port and build your UI (compatible with v8 and v9 widgets)
    if (lvgl_port_lock(-1)) {
        // Create your LVGL screens/widgets here
        lvgl_port_unlock();
    }
}
```

## Running the Example

An example demonstrating the official LVGL widgets benchmark is included in this repository.

1. Navigate to the example folder:
   ```bash
   cd examples/lvgl_widgets
   ```

2. Set the S3 build target:
   ```bash
   idf.py set-target esp32s3
   ```

3. Build, flash, and monitor:
   ```bash
   idf.py build flash monitor
   ```

---

## Advanced Performance Tuning

For applications requiring high frame rates or complex UI animations, you can optimize execution speed and display reliability by adding these configurations to your `sdkconfig.defaults`:

* **Pin LVGL to Core 1 (`CONFIG_LVGL_PORT_TASK_CORE=1`):** Isolates the rendering loop from network and radio overhead on Core 0.
* **Disable Assertions (`CONFIG_COMPILER_OPTIMIZATION_ASSERTIONS_DISABLE=y`):** Bypasses state/bounds checks in hot drawing paths.
* **Increase FreeRTOS Tick Rate (`CONFIG_FREERTOS_HZ=1000`):** Reduces scheduling delay from 10 ms to 1 ms, improving animation smoothness.
* **LVGL Fast Memory Placement (`CONFIG_LV_ATTRIBUTE_FAST_MEM=y`):** Forces graphics drawing and blending functions to load from IRAM instead of Flash.
* **Tune LCD Bounce Buffer (`CONFIG_LCD_RGB_BOUNCE_BUFFER_HEIGHT=20`):** Increases the internal SRAM cache, avoiding screen flickering or DMA underflow warnings under high system CPU/radio load.

---

## Troubleshooting / IDE Tips

### ESP-IDF v6.x CMake Validation Warnings
When building with ESP-IDF v6.0+, the core build system enforces strict dependency validation (`component_validation.cmake`). This often produces benign but visually overwhelming warnings in IDEs like CLion or VSCode, such as:

> `WARNING: Private include directory ... belongs to component ...`

Because these warnings are unconditionally fired by ESP-IDF and cannot be bypassed via standard flags, you can add this custom message filter to your project's top-level `CMakeLists.txt` (before calling `project()`) to keep your IDE logs perfectly clean:

```cmake
# Suppress the strict ESP-IDF v6 component validation warnings 
function(message)
    list(JOIN ARGV " " FULL_MSG)
    if(ARGV0 STREQUAL "WARNING" AND FULL_MSG MATCHES "belongs to component")
        return() # Silently drop the validation warning
    endif()
    _message(${ARGV}) # Pass everything else
endfunction()
```
