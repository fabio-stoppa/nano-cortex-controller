# Nano Cortex — ESP32 firmware (bare metal Rust)

Firmware for the **Waveshare ESP32-S3-Touch-LCD-4.3B**. The protocol and the controller ideas come from the community projects in [Credits](#credits).

**Stack:** bare-metal Rust in `firmware/` — `esp-hal`, Embassy, `esp-radio`, and a TrouBLE central. Preset, effect, and tuner logic run in that image. The protocol crate stays host-testable.

## What it does

- Boots over USB serial (bare metal `main`)
- **Protocol in Rust** — C304 builders + C305/protobuf parsers (`crates/nano-wire`)
- **BLE in Rust** — TrouBLE central on `esp-radio` (`firmware/src/ble.rs`)
- **Preset, effects, and tuner** — the same behavior as the old ESP-IDF app, driven by Embassy deadlines (`firmware/src/state`, `firmware/src/fx`, `firmware/src/tuner`)
- **800×480 LCD** — Slint UI drawn into the RGB panel (`firmware/src/ui.rs`)
- **Bluetooth Tools page** — reads and edits per-preset expression sweeps and bypass assignments, calibrates the pedal, reads cab settings, shows live input/output/expression meters, and reads or changes the EXP/MIDI jack and USB playback level
- **MIDI learn page** (Tools → MIDI) — tap Learn on an action, then send a BLE-MIDI note, CC, or program change. Bindings cover FX block on/off, gate, tuner, tap tempo, preset/capture step, save, all five amp controls, capture volume, cab level/high-pass/low-pass, and the first four knobs of each FX block via CC. Saved in flash
- **Preset manager** — browse presets in 12-slot tabs and use the three-dot menu to rename (on-screen keyboard; 4–64 ASCII letters, digits, spaces, `-`, `_`, unique), delete, move, or copy to any slot. Overwriting an occupied slot asks for confirmation
- Open preset lists update in place when the refreshed catalog arrives after a rename, delete, move, or copy.
- Preset three-dot menus have a 64px-wide, full-row-height touch target separate from preset selection.
- Preset, capture, and cab popup names wrap to two lines; the rename keyboard has a blinking cursor.
- **Expression calibration** — guided reset, pedal sweep, and save flow; the screen shows the sampled range before saving
- **Protocol responses** — handles Nano preset-change requests and surfaces supported write acknowledgements

## Prerequisites

1. **Rust esp toolchain** — `espup install` (channel `esp` via `rust-toolchain.toml`)
2. **espflash** + **ldproxy** — `cargo install espflash ldproxy`
3. **Hardware:** Waveshare ESP32-S3-Touch-LCD-4.3B + Nano Cortex

## Build and flash

```bash
./flash.sh build           # or: (cd firmware && ./build.sh)
./flash.sh flash
```

Monitor (keep DTR/RTS off on Waveshare):

```bash
python -m serial.tools.miniterm /dev/ttyACM0 115200 --rts 0 --dtr 0
```

Expected early log:

```
INFO - panel up (800x480, GT911)
INFO - ui up
INFO - starting ble controller
```

## Project layout

```
├── firmware/                    # Bare-metal image (esp-hal, Embassy, TrouBLE, Slint)
└── crates/nano-wire/            # Protocol builders + parsers (host-testable)
```

Panel timings and pins live in `firmware/src/panel.rs`.

Host-test the protocol crate:

```bash
cd crates/nano-wire && cargo +stable test --target x86_64-unknown-linux-gnu
```

Host-test the embedded-font rename keyboard and partial framebuffer redraws:

```bash
cargo +stable test --manifest-path firmware/tests/ui-renderer/Cargo.toml --target x86_64-unknown-linux-gnu
```

Flipped landscape is rendered unrotated and its finished scan-line spans are
reversed into the framebuffer, avoiding Slint 1.13's mirrored glyph-sampling
panic while retaining partial redraws and the existing touch mapping.

## Before testing BLE

1. Power on the Nano Cortex with Bluetooth enabled
2. **Disconnect Cortex Cloud** (or close the browser editor) — only one BLE client at a time
3. Flash and watch serial for `Found Nano Cortex, connecting...`

## Notes

- Disconnect **Cortex Cloud** before connecting — one BLE client at a time.
- Back up presets in the official app before writing from custom firmware.

## Screen flicker and scan diagnostics

The RGB panel is scanned from six 10-line DRAM strips that an interrupt refills from the PSRAM framebuffer (`firmware/src/bounce.rs`). When that interrupt ran late, usually during BLE traffic, a strip was scanned with stale lines and the screen flickered.

Fixes:

- The refill refills up to 3 stale strips per interrupt (`MAX_REFILLS_PER_IRQ`) so it catches up after a delay.
- The scan interrupts (`DMA_OUT_CH0`, `LCD_CAM`) run on the second core. Slint, touch and BLE stay on the first core, because esp-radio pins the controller there.

Diagnostics are off by default. Build with `firmware/build.sh --features scan-diag` and about once a second the serial log prints `scan glitch: ...` when a counter moved.

- `stale_scan` — a strip was scanned with the wrong lines (the visible flicker)
- `stale_next` — the strip after it was already stale
- `multi_strip` / `max_gap` — interrupts that arrived after several strips had finished, and the largest gap
- `race_skip` — a refill was abandoned because the panel reached the buffer first

On a healthy build `stale_scan` stays near zero, including during the amp state download.

The selected screen layout is stored in the first sector of the `nvs` partition (`firmware/src/settings.rs`) and restored at boot.

## Credits

- [Builty](https://github.com/Builty/TonexOneController) — Tonex One controller. The Waveshare control surface this kind of board grew from.
- [choldy](https://github.com/choldy/nano-cortex-web-editor) — Nano Cortex web editor. The BLE protocol this firmware was ported from.
- [DrD85](https://github.com/DrD85/nano-cortex-editor) — Nano Cortex editor. Preset, library, cab, expression, and settings frames.
- [AJs-Creations](https://github.com/AJs-Creations/nano-cortex-editor) — those frames checked on a Nano running NanOS 2.2.1, including the cab-mic library index.

## License

[GNU GPLv3 only](LICENSE). Copyright (C) 2026 Fabio Stoppa.

Neural DSP and Nano Cortex are trademarks of Neural DSP Technologies Oy. This project is not affiliated with or endorsed by Neural DSP.
