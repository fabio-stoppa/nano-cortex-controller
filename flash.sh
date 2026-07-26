#!/usr/bin/env bash
# Build and flash Nano Cortex firmware (ESP-IDF C) to Waveshare ESP32-S3 4.3B
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

source_idf() {
  if [[ -n "${IDF_PATH:-}" ]]; then
    return 0
  fi
  LOCAL_IDF="$ROOT/.embuild/espressif/esp-idf/v5.3.2"
  if [[ -f "$LOCAL_IDF/export.sh" ]]; then
    export IDF_TOOLS_PATH="$ROOT/.embuild/espressif"
    # shellcheck disable=SC1091
    source "$LOCAL_IDF/export.sh"
  elif [[ -f "$HOME/esp/esp-idf/export.sh" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/esp/esp-idf/export.sh"
  else
    echo "ERROR: ESP-IDF not found. Install ESP-IDF v5.3+ and run export.sh, or build once with Rust embuild."
    exit 1
  fi
}

PORT="${1:-}"
MODE="${2:-}"
if [[ -z "$PORT" ]]; then
  PORT="$(ls /dev/ttyACM* 2>/dev/null | head -1 || true)"
fi
if [[ -z "$PORT" ]]; then
  echo "ERROR: No /dev/ttyACM* port found. Plug in the board and retry."
  exit 1
fi
echo "Using port: $PORT"

if [[ "$MODE" == "tuner" || "$MODE" == "hunt" || "$MODE" == "monitor" ]]; then
  source_idf
  echo "NOTE: '$MODE' is monitor-only — firmware is NOT updated."
  echo "      To flash first, run:  ./flash.sh $PORT"
  echo ""
fi

if [[ "$MODE" == "tuner" ]]; then
  echo "==> Monitor (nano_tuner INFO+, others WARN+ only)..."
  idf.py -p "$PORT" monitor --print-filter="nano_tuner:I *:W"
  exit 0
fi

if [[ "$MODE" == "hunt" ]]; then
  echo "==> Monitor (BPM/BLE hunt: nano_state + nano_ble + nano_tuner INFO)"
  echo "    Look for: 'BPM hunt:', field 56, C1 dump chunks, suspicious amp-hw / unknown C305"
  idf.py -p "$PORT" monitor --print-filter="nano_ble:I nano_tuner:I nano_state:I nano:I *:W"
  exit 0
fi

if [[ "$MODE" == "monitor" ]]; then
  echo "==> Monitor (full INFO logs)..."
  idf.py -p "$PORT" monitor
  exit 0
fi

source_idf

if [[ ! -f sdkconfig ]]; then
  echo "==> Setting target esp32s3..."
  idf.py set-target esp32s3
fi

WS_LVGL="$ROOT/managed_components/bobscott45__waveshare_esp32_s3_touch_lcd_4_3/src/lvgl_port.c"
if [[ -f "$WS_LVGL" ]] && grep -q 'on_frame_buf_complete' "$WS_LVGL"; then
  echo "==> Patching Waveshare BSP for ESP-IDF 5.3..."
  sed -i 's/on_frame_buf_complete = rgb_lcd_on_vsync_event/on_bounce_frame_finish = rgb_lcd_on_vsync_event/' "$WS_LVGL"
fi

echo "==> Building..."
idf.py build

echo "==> Flashing to $PORT..."
idf.py -p "$PORT" flash

echo ""
echo "Flash complete. Boot should log:  nano: firmware build ..."
echo "Monitor (full logs):"
echo "  ./flash.sh $PORT monitor"
echo "Tuner-only monitor:"
echo "  ./flash.sh $PORT tuner"
echo "BLE hunt (all GATT notifies + diverted C305):"
echo "  ./flash.sh $PORT hunt"
echo "Or:"
echo "  idf.py -p $PORT monitor"
echo "Or miniterm:"
echo "  python -m serial.tools.miniterm $PORT 115200 --rts 0 --dtr 0"
