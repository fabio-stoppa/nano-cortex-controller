#!/usr/bin/env bash
# Live desktop preview of every Slint screen and popup (no flashing). Needs: cargo install slint-viewer --version 1.13.1 --locked
# Usage: ./firmware/preview.sh [portrait]
# Navigate by tapping the real UI (chain tabs, tuner, settings, MIDI) or the dev chips at the bottom (screens + popups).
# Scenes and gig view use temporary sample data; no device or flash is accessed.
set -euo pipefail
ui="$(cd "$(dirname "$0")/ui" && pwd)"
component=Preview
if [ "${1:-}" = "portrait" ]; then component=PreviewPortrait; fi
exec slint-viewer "$ui/preview.slint" --component "$component" --style fluent --auto-reload
