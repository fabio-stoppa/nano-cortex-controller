#!/usr/bin/env bash
# Live desktop preview of the Slint UI (no flashing). Needs: cargo install slint-viewer --version 1.13.1 --locked
# Usage: ./firmware/preview.sh [data.json]   (default: tuner page sample data)
set -euo pipefail
ui="$(cd "$(dirname "$0")/ui" && pwd)"
exec slint-viewer "$ui/app.slint" --style fluent --auto-reload --load-data "${1:-$ui/preview-tuner.json}"
