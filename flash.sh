#!/usr/bin/env bash
# Nano Cortex build / flash / monitor helper
#
# Usage:
#   ./flash.sh build                          — compile the bare-metal image
#   ./flash.sh flash   [port]                — flash only (no monitor)
#   ./flash.sh monitor [port]                — open serial monitor only
#   ./flash.sh run     [port]                — build → flash → monitor
#   ./flash.sh         [port]                — alias for 'run'
#
# Port resolution order: explicit arg > $NANO_PORT env var > /dev/ttyACM0 > /dev/ttyUSB0
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

find_port() {
    if [[ -n "${NANO_PORT:-}" ]]; then
        echo "$NANO_PORT"
        return
    fi
    for p in /dev/ttyACM0 /dev/ttyUSB0 /dev/ttyACM1 /dev/ttyUSB1; do
        if [[ -e "$p" ]]; then
            echo "$p"
            return
        fi
    done
    echo ""
}

require_port() {
    local port="$1"
    if [[ -z "$port" ]]; then
        echo "ERROR: No device port found." >&2
        echo "  Plug in the board, or set NANO_PORT=/dev/ttyACMx" >&2
        exit 1
    fi
}

elf_path() {
    echo "$ROOT/firmware/target/xtensa-esp32s3-none-elf/release/nano-cortex-firmware"
}

do_build() {
    echo "--- build (bare metal, release) ---"
    (cd "$ROOT/firmware" && env -u CARGO_TARGET_DIR ./build.sh)
}

do_flash() {
    local profile="${1:-dev}"
    local port="${2:-}"
    [[ -z "$port" ]] && port="$(find_port)"
    require_port "$port"
    local elf
    elf="$(elf_path "$profile")"
    if [[ ! -f "$elf" ]]; then
        echo "ERROR: ELF not found at $elf — run build first." >&2
        exit 1
    fi
    echo "--- flash ($profile) → $port ---"
    espflash flash --port "$port" --baud 921600 --flash-freq 80mhz "$elf"
}

do_monitor() {
    local port="${1:-}"
    [[ -z "$port" ]] && port="$(find_port)"
    require_port "$port"
    echo "--- monitor → $port ---"
    espflash monitor --port "$port"
}

do_run() {
    local profile="${1:-dev}"
    local port="${2:-}"
    [[ -z "$port" ]] && port="$(find_port)"
    require_port "$port"
    do_build "$profile"
    local elf
    elf="$(elf_path "$profile")"
    echo "--- flash + monitor ($profile) → $port ---"
    espflash flash --port "$port" --baud 921600 --flash-freq 80mhz --monitor "$elf"
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

CMD="${1:-run}"
shift || true

case "$CMD" in
    build)
        PROFILE="${1:-dev}"; shift || true
        do_build "$PROFILE"
        ;;
    flash)
        PROFILE="${1:-dev}"; shift || true
        PORT="${1:-}";       shift || true
        do_flash "$PROFILE" "$PORT"
        ;;
    monitor)
        PORT="${1:-}"; shift || true
        do_monitor "$PORT"
        ;;
    run)
        PROFILE="${1:-dev}"; shift || true
        PORT="${1:-}";       shift || true
        do_run "$PROFILE" "$PORT"
        ;;
    dev|release)
        # Bare ./flash.sh dev [port] or ./flash.sh release [port]
        PROFILE="$CMD"
        PORT="${1:-}"; shift || true
        do_run "$PROFILE" "$PORT"
        ;;
    *)
        echo "Unknown command: $CMD" >&2
        echo "Usage: ./flash.sh {build|flash|monitor|run} [dev|release] [port]" >&2
        exit 1
        ;;
esac
