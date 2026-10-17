# Nano Cortex — ESP32 firmware (Rust + ESP-IDF)

Firmware for the **Waveshare ESP32-S3-Touch-LCD-4.3B**. The protocol and the controller ideas come from the community projects in [Credits](#credits).

**Stack:** Rust firmware on ESP-IDF — protocol, BLE, state, FX, tuner, amp catalog, and LVGL UI. Only `bt_cfg_helper.c` remains C (BT init macro shim).

## What it does

- Boots over USB serial (Rust `app_main`)
- **Protocol in Rust** — C304 builders + C305/protobuf parsers (`crates/nano-wire`)
- **BLE in Rust** — Bluedroid GATT client (`src/ble/`)
- **State / FX / tuner / amp / UI in Rust** — `src/state/`, `src/fx/`, `src/tuner/`, `src/amp/`, `src/ui/`
- **800×480 LCD** — LVGL amp / FX / tuner / tempo UI
- Syncs amp knobs, gate, FX blocks, BPM, tuner

## Prerequisites

1. **Rust esp toolchain** — `espup install` (channel `esp` via `rust-toolchain.toml`)
2. **espflash** + **ldproxy** — `cargo install espflash ldproxy`
3. **Hardware:** Waveshare ESP32-S3-Touch-LCD-4.3B + Nano Cortex

## Build and flash

```bash
cargo build --release
./flash.sh                 # or: espflash flash --port /dev/ttyACM0 --partition-table partitions.csv \
                           #        target/xtensa-esp32s3-espidf/release/nano-cortex-controller
```

Monitor (keep DTR/RTS off on Waveshare):

```bash
python -m serial.tools.miniterm /dev/ttyACM0 115200 --rts 0 --dtr 0
```

Expected early log:

```
I (xxx) nano_cortex_controller: firmware build 2026-07-26-rust7 (...)
I (xxx) nano_ui: UI ready (800x480, left chain rail + panels)
I (xxx) nano_ble: Connected to Nano Cortex
I (xxx) nano_state: STATE INITIAL — protobuf scan (... bytes, rust)
```

## Project layout

```
├── Cargo.toml / src/main.rs     # Rust entry
├── src/ble/                     # GATT client
├── src/state/ src/fx/           # Preset/amp state machine + FX engine
├── src/tuner/ src/amp/          # Tuner + factory capture catalog
├── src/ui/                      # LVGL UI
├── crates/nano-wire/            # Protocol builders + parsers (host-testable)
├── components/nano/             # bt_cfg_helper.c shim only
├── components/waveshare_.../    # Vendored BSP (IDF 5.3 LCD callback patch)
├── sdkconfig.defaults
└── partitions.csv               # 2 MB app partition
```

The Waveshare BSP stays vendored: registry `1.0.7` uses `on_frame_buf_complete`, which ESP-IDF 5.3.2 does not provide. The local tree keeps `on_bounce_frame_finish`.

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
- Only `components/nano/bt_cfg_helper.c` is still C (`BT_CONTROLLER_INIT_CONFIG_DEFAULT` macro shim).

## Credits

- [Builty](https://github.com/Builty/TonexOneController) — Tonex One controller. The Waveshare control surface this kind of board grew from.
- [choldy](https://github.com/choldy/nano-cortex-web-editor) — Nano Cortex web editor. The BLE protocol this firmware was ported from.
- [DrD85](https://github.com/DrD85/nano-cortex-editor) — Nano Cortex editor. Preset, library, cab, expression, and settings frames.
- [AJs-Creations](https://github.com/AJs-Creations/nano-cortex-editor) — those frames checked on a Nano running NanOS 2.2.1, including the cab-mic library index.

## License

[MIT](LICENSE). The vendored Waveshare board support keeps its own MIT license in `components/waveshare_esp32_s3_touch_lcd_4_3/LICENSE`.

Neural DSP and Nano Cortex are trademarks of Neural DSP Technologies Oy. This project is not affiliated with or endorsed by Neural DSP.
