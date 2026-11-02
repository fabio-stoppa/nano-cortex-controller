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
- **Performance controls** — eight custom banks, four named scenes per bank slot, a 32-slot named FX shelf, and a large-button gig view. Touch and MIDI recalls share staged, transient restoration; controller data is saved in internal flash.
- Open preset lists update in place when the refreshed catalog arrives after a rename, delete, move, or copy.
- Refresh and Save live in Settings → General → Tools next to Tuner (Save is highlighted with unsaved changes); the header keeps Banks and the settings gear.
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
`midi` data partition at 8 MiB. Flash with this table when upgrading to persistent
MIDI configuration; do not erase the entire chip if you want to retain
existing MIDI assignments and screen layout.

Custom banks additionally use a checked 2 MiB `performance` partition at 10 MiB.
Reflash with the updated table when upgrading; existing partitions keep their offsets.

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

## Performance and MIDI configuration

### Custom banks

Tap **Banks** in the editor header or connecting screen. Create up to eight named
banks with four slots each, assign Nano preset numbers 1-64, and tap **Play**.
Selecting a bank only changes the controller's bank; it does not load a preset
until you play a slot. Bank stepping wraps. Unassigned slots and disconnected
recall are refused with a notice. Rename/delete affect controller data only;
deletion asks for confirmation and never deletes Nano presets.

MIDI Learn includes next/previous bank, four bank-slot recalls and direct bank
selection. Existing action IDs remain unchanged. Banks persist in controller flash.

### Scenes

Each assigned slot has four named **Scenes**: play the slot, adjust the sound and
tap **Store**. Replacing/deleting scenes, or changing an assignment that has scenes,
asks for confirmation. Scene recall also has four MIDI Learn actions.

Scenes restore amp controls, capture volume when available, active cab EQ/level,
gate, and all five FX models, parameters and bypass states. Library selections,
expression assignments, tempo and global USB/mute settings are not scene data.
Recall is transient: it never saves over the Nano preset. The controller waits
for model/parameter loading and fresh readback before reporting **Recall verified**.
Timeout/disconnect errors warn that sound may be partly updated; play the original
bank slot to recover. Actual sound recall still requires hardware validation.

### Named FX presets

**Banks → FX presets** opens a 32-slot shelf. Choose the source/target block using
the five buttons, then **Store** its current model, parameters and bypass with a
name. **Play** recalls into that compatible block only; amp/cab/gate and other FX
are unchanged. Incompatible models are labelled and refused. Replacing/deleting
asks for confirmation. Slots stay fixed when deleted, preserving the MIDI Learn
actions for each shelf-slot/target-block pair. FX presets share `performance.json`
and the scene restore/readback safeguards; they never save over Nano presets.

### Gig view

Tap **Banks → Gig** for large bank-slot and scene buttons. **Prev/Next** step banks
without loading a sound; play a slot before its scenes. Loading/verification
disables recall controls, and unassigned/empty/disconnected recalls are disabled.
Tuner **EXIT** returns to gig view; **Tap tempo** uses the existing tempo control.
**Editor** explicitly leaves gig view. MIDI Learn also includes enter/exit gig
actions; the bank/slot/scene actions work in either view. Layouts support both
landscape and portrait. Changing the Nano preset externally or disconnecting
clears stale slot/scene selection. Gig view itself is a temporary UI mode, not a
Nano preset or persisted sound setting.

### Internal configuration storage

MIDI assignments and performance data remain saved as checked JSON records in
internal flash. Configure them using the controller's touch UI or MIDI Learn.
The USB configuration drive is deferred: this firmware does not expose editable
files to a computer and has no import/export controls or serial backup commands.
USB serial monitoring/flashing and the Nano's USB playback-level control are unchanged.
The firmware uses the published Embassy runtime dependencies from the pre-USB build,
without the USB-specific Git patches.

Tools → MIDI → **Clear all bindings** asks for confirmation before removing all
learned assignments. Cancel leaves them unchanged. This does not change presets
on the Nano.

The firmware reads the JSON configuration at boot and updates it when MIDI Learn
or Clear changes assignments. It uses two checked flash records, commits to the
inactive copy, and verifies it before installing the new mappings. Interrupted
saves retain the previous committed copy. Existing compact MIDI records in NVS
are migrated without changing action IDs.
Action IDs are stable and append-only; the same message may appear on several
different actions.

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

Bluetooth HCI commands use a controller-local serialized adapter rather than
bt-hci's `ExternalController` command-slot `RefCell`. Command responses are copied
into owned buffers, command credits are respected, and a cancelled command's late
response is drained before sending the next command. ACL traffic remains independent.
This removes the command-slot borrow-panic path seen during initial connection;
the boot/connect behavior still needs verification on the 4.3B.

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
