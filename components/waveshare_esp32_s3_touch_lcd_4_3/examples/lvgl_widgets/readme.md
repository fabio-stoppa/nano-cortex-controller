# Waveshare ESP32-S3 Touch LCD 4.3 - LVGL Widgets Example

This example demonstrates how to compile and run the official LVGL Widgets demo on the Waveshare ESP32-S3 Touch LCD 4.3 board.

It showcases the decoupled board drivers (`bsp/board.h`) initializing the display and touch controllers separately and feeding them into the LVGL port wrapper (`bsp/lvgl_port.h`).

---

## References

* **Official Wiki:** [Waveshare ESP32-S3-Touch-LCD-4.3 Wiki](https://www.waveshare.com/wiki/ESP32-S3-Touch-LCD-4.3)
* **Original Demo Code:** This example and driver are based on the official demo code found in the [Waveshare ESP32-S3-Touch-LCD-4.3 Demo ZIP](https://files.waveshare.com/wiki/ESP32-S3-Touch-LCD-4.3/ESP32-S3-Touch-LCD-4.3-Demo.zip)

---

## Requirements

* **Target Hardware:** Waveshare ESP32-S3-Touch-LCD-4.3 (A) board.
* **ESP-IDF Version:** v5.3 or higher.
* **LVGL Version:** Works with **LVGL v8 (>=8.3.11)** or **LVGL v9 (>=9.0.0)**.

## Switching Between LVGL v8 and v9

The example dynamically compiles against either version based on the dependency defined in `main/idf_component.yml`.     

To switch between versions:
1. Open `main/idf_component.yml` and adjust the `lvgl` version rule (e.g., `^8.3.11` or `^9.0.0`).
2. Run `idf.py reconfigure` to update components.
3. Clean the configuration and rebuild:
   ```bash
   rm -f sdkconfig
   rm -rf build
   idf.py build
   ```

## Building and Flashing

Before building or setting the target, set up your ESP-IDF environment in your active terminal by sourcing the export script:

```bash
# Replace ~/esp/esp-idf with the path to your actual ESP-IDF installation directory
. ~/esp/esp-idf/export.sh
```

Because this project uses board-specific RGB display timings and Octal PSRAM configurations, the build target is locked to the `esp32s3`.

1. Set the build target:
   ```bash
   idf.py set-target esp32s3
   ```

2. Build the project:
   ```bash
   idf.py build
   ```

3. Flash the executable and open the serial monitor:
   ```bash
   idf.py flash monitor
   ```

---

## Performance Tuning

To optimize display performance and eliminate rendering stutter or system-load glitches, the following configurations can be added to the project's `sdkconfig.defaults`:

* **Isolate UI Task to Core 1:**
  ```config
  CONFIG_LVGL_PORT_TASK_CORE=1
  ```
  Moves the main UI thread off Core 0 (where Wi-Fi/Bluetooth stacks run).
* **Enable High Frequency Tick Timer:**
  ```config
  CONFIG_FREERTOS_HZ=1000
  ```
  Improves scheduling resolution for more responsive animations.
* **Bypass Assertion Verification:**
  ```config
  CONFIG_COMPILER_OPTIMIZATION_ASSERTIONS_DISABLE=y
  ```
  Disables runtime assertion checks within hot graphics routines.
* **Load Graphics Routines from IRAM:**
  ```config
  CONFIG_LV_ATTRIBUTE_FAST_MEM=y
  ```
  Keeps critical code path functions in fast internal SRAM instead of external flash.
* **Increase LCD DMA Bounce Buffer Height:**
  ```config
  CONFIG_LCD_RGB_BOUNCE_BUFFER_HEIGHT=120
  ```
  Prevents display flicker/corruptions due to PSRAM bus congestion.