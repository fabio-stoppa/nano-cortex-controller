# Nano Cortex — ESP32 firmware (bare metal Rust)

Firmware for the **Waveshare ESP32-S3-Touch-LCD-4.3B**. The protocol and the controller ideas come from the community projects in [Credits](#credits).

**Stack:** bare-metal Rust in `firmware/` — `esp-hal`, Embassy, `esp-radio`, and a TrouBLE central. Preset, effect, and tuner logic run in that image. The protocol crate stays host-testable.

## What it does

- Boots over USB serial (bare metal `main`)
- **Protocol in Rust** — C304 builders + C305/protobuf parsers (`crates/nano-wire`)
- **BLE in Rust** — TrouBLE central on `esp-radio` (`firmware/src/ble.rs`)
- **Preset, effects, and tuner** — the same behavior as the old ESP-IDF app, driven by Embassy deadlines (`firmware/src/state`, `firmware/src/fx`, `firmware/src/tuner`)
- **800×480 LCD** — Slint UI drawn into the RGB panel (`firmware/src/ui.rs`)

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

## Before testing BLE

1. Power on the Nano Cortex with Bluetooth enabled
2. **Disconnect Cortex Cloud** (or close the browser editor) — only one BLE client at a time
3. Flash and watch serial for `Found Nano Cortex, connecting...`

## Notes

- Disconnect **Cortex Cloud** before connecting — one BLE client at a time.
- Back up presets in the official app before writing from custom firmware.

## Credits

- [Builty](https://github.com/Builty/TonexOneController) — Tonex One controller. The Waveshare control surface this kind of board grew from.
- [choldy](https://github.com/choldy/nano-cortex-web-editor) — Nano Cortex web editor. The BLE protocol this firmware was ported from.
- [DrD85](https://github.com/DrD85/nano-cortex-editor) — Nano Cortex editor. Preset, library, cab, expression, and settings frames.
- [AJs-Creations](https://github.com/AJs-Creations/nano-cortex-editor) — those frames checked on a Nano running NanOS 2.2.1, including the cab-mic library index.

## License

[GNU GPLv3 only](LICENSE). Copyright (C) 2026 Fabio Stoppa.

Neural DSP and Nano Cortex are trademarks of Neural DSP Technologies Oy. This project is not affiliated with or endorsed by Neural DSP.
