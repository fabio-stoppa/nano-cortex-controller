#!/usr/bin/env bash
# Build and flash Nano Cortex firmware (Rust + ESP-IDF C component) to Waveshare ESP32-S3 4.3B
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

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
  echo "NOTE: '$MODE' is monitor-only — firmware is NOT updated."
  echo "      To flash first, run:  ./flash.sh $PORT"
  python3 -m serial.tools.miniterm "$PORT" 115200 --rts 0 --dtr 0
  exit 0
fi

echo "Building (release)…"
cargo build --release

echo "Flashing…"
espflash flash --port "$PORT" --monitor --baud 921600 \
  target/xtensa-esp32s3-espidf/release/nano-cortex-controller
