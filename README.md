# Nano Cortex — ESP32 firmware (bare metal Rust)

Firmware for the **Waveshare ESP32-S3-Touch-LCD-4.3B**. The protocol and the controller ideas come from the community projects in [Credits](#credits).

**Stack:** bare-metal Rust in `firmware/` — `esp-hal`, Embassy, `esp-radio`, and a TrouBLE central. Preset, effect, and tuner logic run in that image. The protocol crate stays host-testable.

## What it does

- Boots over USB serial (bare metal `main`)
- **Protocol in Rust** — C304 builders + C305/protobuf parsers (`crates/nano-wire`)
- **BLE in Rust** — TrouBLE central on `esp-radio` (`firmware/src/ble.rs`)
- **Preset, effects, and tuner** — the same behavior as the old ESP-IDF app, driven by Embassy deadlines (`firmware/src/state`, `firmware/src/fx`, `firmware/src/tuner`)
- **800×480 LCD** — Slint UI drawn into the RGB panel (`firmware/src/ui.rs`)
- **Category-colored FX tabs** — guitar OD orange, bass OD amber, compressor yellow, pitch magenta, filter lime, wah green, utility pink, modulation purple, delay cyan, and reverb blue. Colors match the saturation and brightness of the blue and amp-red accents in both orientations; bypassed FX retain black backgrounds with category-colored labels.
- Popups (Settings, preset/model/capture/cab lists, rename keyboard) fill the screen with the main gray background, and the screen behind them is hidden instead of redrawn. Close them with their close button.
- Delete/overwrite confirmations are centered cards over a dimmed preset list; answer with Cancel or the action button (outside taps are ignored).
- **Bluetooth devices** live in Settings → Bluetooth: tap a device to connect, or use its button to connect or forget it.
- Parameter cells (amp and FX) use the settings-row gray to stand out from the main background.
- **Bluetooth Tools page** — reads and edits per-preset expression sweeps and bypass assignments, calibrates the pedal, reads cab settings, shows live input/output/expression meters, and reads or changes the EXP/MIDI jack and USB playback level
- **MIDI learn page** (Tools → MIDI) — tap Learn on an action, then send a BLE-MIDI note, CC, or program change. Learn the same message on several rows to control multiple targets; Clear removes only that row's binding. Buttons cover FX block on/off, gate, tuner, tap tempo, preset/capture step, direct preset 1–64 and capture 1–25 selection, FX model step, save, and explicit input/output/tuner mute/unmute. CC-only parameters cover all five amp controls, capture volume, cab level/high-pass/low-pass, gate amount, USB playback level, and up to 24 displayed knobs per FX block (only knobs present on the current model apply). CC 0 does not trigger button actions; it still sets continuous parameters to their minimum. Repeated CC values for a pending parameter are coalesced; different targets are retained. Saved in flash with readback verification; old MIDI bindings keep their existing action assignments.
- **Preset manager** — browse presets in 12-slot tabs and use the three-dot menu to rename (on-screen keyboard; 4–64 ASCII letters, digits, spaces, `-`, `_`, unique), delete, move, or copy to any slot. Overwriting an occupied slot asks for confirmation
- Open preset lists update in place when the refreshed catalog arrives after a rename, delete, move, or copy.
- Refresh and Save live in Settings → General → Tools next to Tuner (Save is highlighted with unsaved changes); the header only keeps the settings gear.
- In portrait the FX chain sits at the bottom of the screen, below the page content.
- Preset three-dot menus have a 64px-wide, full-row-height touch target separate from preset selection.
- Selecting a preset keeps the preset list open and moves its highlight, so presets can be auditioned back to back.
- The main screen has the same 14px edge padding as Settings; the Amp page's capture and cab cards have no outline, center their icon, slot and name, and wrap names to two lines before truncating with "…".
- Preset popup names wrap to three lines on taller cards, and capture and cab names wrap to two; the rename keyboard has a blinking cursor, keys light up while pressed, holding backspace repeats (400ms delay, then every 60ms), and long names keep their end visible.
- While the rename keyboard is open, touch is polled every 10ms instead of 50ms; duplicate-name validation scans the stored catalog without allocating copies.
- The software renderer has no occlusion culling, so screens covered by the keyboard or a full-screen panel are hidden rather than redrawn underneath it. The header title elides long preset names.
- **Expression calibration** — guided reset, pedal sweep, and save flow; the screen shows the sampled range before saving
- **Protocol responses** — handles Nano preset-change requests and surfaces supported write acknowledgements

## Prerequisites

1. **Rust esp toolchain** — `espup install` (channel `esp` via `rust-toolchain.toml`)
2. **espflash** + **ldproxy** — `cargo install espflash ldproxy`
3. **Hardware:** Waveshare ESP32-S3-Touch-LCD-4.3B + Nano Cortex

## Build and flash

```bash
./flash.sh build           # or: (cd firmware && ./build.sh)
./flash.sh flash           # 921600 baud; NANO_BAUD=115200 ./flash.sh flash for flaky cables
```

The flash helper installs `firmware/partitions.csv` for the 16 MiB 4.3B board.
It retains the default NVS and application start addresses and adds a 128 KiB
`midi` data partition at 8 MiB. Flash with this table when upgrading to USB
configuration support; do not erase the entire chip if you want to retain
existing MIDI assignments and screen layout.

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

Host-test MIDI learn, multi-target queues, and binding-record compatibility:

```bash
cargo +stable test --manifest-path firmware/tests/midi/Cargo.toml --target x86_64-unknown-linux-gnu
```

## MIDI configuration USB drive

Tools → MIDI → **Clear all bindings** asks for confirmation before removing all
learned assignments. Cancel leaves them unchanged. This does not change presets
on the Nano.

1. Connect the 4.3B's USB-C port to a computer with a data cable.
2. Open Settings → MIDI → **USB drive: midi.json**, or tap **USB drive** on the
   connecting screen (no Nano Cortex connection required).
3. The computer sees a small **NANO MIDI** FAT12 drive. Open, edit, copy, or
   replace `midi.json` with your file manager/text editor. `README.TXT` on the
   drive lists every action ID and whether it requires CC.
4. **Safely eject** the drive using the computer's eject command. The controller
   validates the file and reports whether it was saved. On Linux, use Eject /
   Safely Remove (not only Unmount); the USB medium-eject command finishes the
   session.
5. Tap **Restart controller** to return to Bluetooth control and USB serial.

MIDI Learn and Bluetooth control pause while the computer owns the drive.
Serial monitoring/flashing is unavailable during USB storage mode; no serial
backup commands or computer scripts are required. **Cancel USB edits** discards
computer changes. Restarting/powering off before eject also discards them.
Other files on the drive are temporary; only `midi.json` is retained. Do not
format the drive.

Replacing the file **replaces all bindings**, including clearing actions omitted
from it, so copy it to your computer as a backup first. Invalid, missing,
oversized (over 48 KiB), or damaged files are rejected, retaining previous bindings.
Format/version, action IDs, message types, channel (1–16), number (0–127), and
duplicate action IDs are checked before writing; parameter actions require CC.
The firmware reads the JSON configuration at boot and updates it when MIDI Learn
or Clear changes assignments. It uses two checked flash records, commits to the
inactive copy, and verifies it before installing the new mappings. Interrupted
saves retain the previous committed copy. Existing compact MIDI records in NVS
are migrated without changing action IDs.
Action IDs are stable and append-only; the same message may appear on several
different actions.

```json
{
  "format": "nano-midi",
  "version": 1,
  "bindings": [
    {"action": 0, "kind": "cc", "channel": 1, "number": 7},
    {"action": 33, "kind": "cc", "channel": 1, "number": 7}
  ]
}
```

This example maps CC 7 to Pre FX 1 on/off and amp gain. JSON text is stored in
internal flash; the USB drive is a temporary FAT view of that configuration,
not unrestricted access to the chip. Backups cover MIDI bindings only, not
Nano presets. The USB MSC implementation uses a pinned Embassy revision because
MSC device support is not included in the published 0.6 release.

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
