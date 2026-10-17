# Nano Cortex — ESP32 firmware (C / ESP-IDF)

Minimal firmware for the **Waveshare ESP32-S3-Touch-LCD-4.3B**, ported from [`this project`](https://github.com/choldy/nano-cortex-web-editor).

**Stack:** plain ESP-IDF C — no Rust toolchain.

## What it does today

- Boots and logs over USB serial
- **800×480 LCD** — LVGL amp panel (Gain / Level / Bass / Mid / Treble) with live BLE updates
- **Scans for "Neural DSP Nano Cortex"** and connects over BLE
- Discovers services A002/A003, C304 (write) and C305 (notify)
- On connect, sends metadata + preset state requests (same as the web editor)
- Parses amp knobs from preset state + hardware knob notifications

**Not yet:** touch control to write knobs, Amount knob, FX blocks, preset picker UI.

## Before testing BLE

1. Power on the Nano Cortex with Bluetooth enabled
2. **Disconnect Cortex Cloud** (or close the browser editor) — only one BLE client at a time
3. Flash this firmware and watch serial for `Found Nano Cortex, connecting...`

## Prerequisites

1. **ESP-IDF v5.3+** — [Install guide](https://docs.espressif.com/projects/esp-idf/en/latest/esp32s3/get-started/index.html)

   ```bash
   # After installing ESP-IDF:
   . ~/esp/esp-idf/export.sh
   ```

   If you already built the old Rust firmware, this repo may have ESP-IDF under `firmware/.embuild/` — `flash.sh` will use that automatically.

2. **Arch Linux:** `esp-clang` needs legacy libxml2:

   ```bash
   sudo pacman -S libxml2-legacy
   ```

3. **Hardware:** Waveshare ESP32-S3-Touch-LCD-4.3B + Nano Cortex (firmware 2.2.1 tested in browser)

## Build and flash

```bash
cd firmware
idf.py set-target esp32s3   # first time only
idf.py build
./flash.sh                  # or: idf.py -p /dev/ttyACM1 flash
```

Monitor (keep DTR/RTS off on Waveshare):

```bash
python -m serial.tools.miniterm /dev/ttyACM1 115200 --rts 0 --dtr 0
```

Expected log:

```
I (xxx) nano: Nano Cortex C MVP starting
I (xxx) nano: TX FE metadata: 06 C0 08 03 01 00 00 00
I (xxx) nano: TX preset state: 0C C0 08 03 18 01 ...
I (xxx) nano: TX amp gain=200: 0B C0 18 00 20 C8 01 ...
```

## Project layout

```
firmware/
├── CMakeLists.txt
├── sdkconfig.defaults      # PSRAM + USB serial console
├── main/
│   ├── main.c              # app entry
│   ├── nano_ui.c           # LVGL amp panel
│   ├── nano_protocol.c     # packets from index.html
│   └── ...
├── partitions.csv          # 2 MB app partition (LVGL + BLE)
└── flash.sh
```

## BLE constants (from index.html)

| Item | UUID |
|------|------|
| Services | `0000a002-...`, `0000a003-...` |
| C304 write | `0000c304-...` |
| C305 notify | `0000c305-...` |
| Device name | `Neural DSP Nano Cortex` |

## Notes

- Disconnect **Cortex Cloud** before connecting — one BLE client at a time.
- Back up presets in the official app before writing from custom firmware.
