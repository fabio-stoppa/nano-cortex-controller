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
- **Performance controls** — eight named scenes per Nano preset, four FX presets per model shared across compatible effect slots, and a large-button scene gig view. Touch and MIDI recalls share staged, transient restoration; controller data is saved in internal flash. Sets/custom banks are deferred.
- Open preset lists update in place when the refreshed catalog arrives after a rename, delete, move, or copy.
- Refresh and Nano preset Save live in Settings → General → Tools next to Tuner (Save is highlighted with unsaved changes); the header has a scene selector and the settings gear.
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

Scenes and FX presets additionally use a checked 2 MiB `performance` partition at 10 MiB.
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

### Scenes

Each Nano preset (1-64) has eight named **Scenes**, independent of Sets. Tap the
header **Scene** indicator to open its card grid: two columns in portrait,
three in landscape. The title includes
the Nano preset number and name. Tap a populated card to recall it; the selector
stays open and highlights the selected scene. Cards match the Nano Presets
screen: 94px height, a separate number on the left, wrapped names and vertical
three dots with a 64px-wide full-height touch target on the right.
The dots open **Rename**, **Save** and **Delete**, with a separator before Delete,
separate from the card's recall touch target. Saving an empty card asks for a
name; saving an occupied card immediately replaces it using its existing name.
An asterisk on the selected card marks its unsaved controller edits, not edits
to other scenes. Renaming changes only the stored name, not the sound.
Saving, deleting and recalling scenes take effect without confirmation dialogs.
Eight MIDI Learn actions recall scenes of the current Nano preset.

New scene and FX preset names are limited to **16 ASCII characters** (letters,
numbers, spaces, `-`, `_`, no outer spaces), unique within their collection.

Scenes restore amp controls, capture volume when available, active cab EQ/level,
gate, and all five FX models, parameters and bypass states. Library selections,
expression assignments, tempo and global USB/mute settings are not scene data.
Recall is transient: it never saves over the Nano preset. The controller waits
for model/parameter loading and fresh readback before reporting **Recall verified**.
Timeout/disconnect errors warn that sound may be partly updated; reload the
original Nano preset to recover. Actual sound recall still requires hardware validation.

### Named FX presets

Each FX model has four named preset tabs immediately above its parameters.
The collection follows the model, not its Pre/Post FX slot, and is shared across
compatible slots and Nano presets. Switching models switches the displayed tabs.
An empty **+** tab stores the current model's parameters and bypass with a name;
tapping a populated tab recalls it into the current slot only.
The tabs stay visible while scrolling parameters, with an 8px gap below the row.
An asterisk on the selected tab marks unsaved changes. Press and hold a tab for
the **Save**, **Rename** and **Delete** dropdown (empty slots only enable Save),
with a separator below Delete.
Amp/cab/gate and other FX are unchanged. Replacing/deleting asks
for confirmation. Saving, renaming or deleting updates that model's shared collection;
deleting an entry does not move the other tabs. FX presets share `performance.json`
and the scene restore/readback safeguards; they never save over Nano presets.

Performance storage uses format version 3 with eight-slot `preset_scenes` and
`model_presets`, keyed by the model's wire ID. **The first boot of this firmware
permanently factory-resets all controller storage**: layout/settings, MIDI
bindings, scenes, FX presets and legacy Sets. The entire `nvs`, `midi` and
`performance` partitions are erased, including both journal generations.
No legacy Set, binary MIDI or four-scene migration paths remain.
The reset is verified and marked complete in the last NVS sector, so newly
saved data survives subsequent boots. An interrupted or failed reset is logged
and stops startup before settings are loaded; reboot retries the full reset.
Nano Cortex's own presets/settings and the app/PHY partitions are not erased.
The original 429 MIDI action IDs remain stable; four appended IDs recall scenes
5-8, for 433 total actions and 279 active actions. Set actions and old shelf
slots 5-32 are retired and do not execute. Old saved bindings are erased by the
factory reset.
The first four FX recall IDs per block now recall presets of the model currently
loaded in that block. Performance JSON is encoded compactly; all 64 eight-scene
collections plus four presets for every catalog model fit the storage limit.
The JSON limit uses the full 1 MiB journal slot minus its 32-byte header, within
the existing 2 MiB performance partition.

The desktop preview includes sample scenes, model-scoped FX preset collections
and gig view. Hold an FX chain tab to select a model and switch collections.
Run `./firmware/preview.sh` (or add `portrait`)
and use the **FX**, **Scenes**, **Gig** and **Edited** developer shortcuts.
Header/FX controls also work; saving and deleting update temporary preview data
only, without connecting to a Nano or writing flash.

### Gig view

Enter gig view through MIDI Learn's **Enter gig view** action for eight large
scene buttons for the current Nano preset (or use **Gig** in the desktop preview).
Loading/verification disables recall controls, and empty/disconnected recalls are disabled.
Tuner **EXIT** returns to gig view; **Tap tempo** uses the existing tempo control.
**Editor** explicitly leaves gig view. MIDI Learn also includes enter/exit gig
actions; scene actions work in either view. Layouts support both
landscape and portrait. Changing the Nano preset externally or disconnecting
clears stale scene selection. Gig view itself is a temporary UI mode, not a
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
saves retain the previous committed copy. Legacy compact MIDI records are erased
by the factory reset and are no longer read or migrated.
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

BLE link/session futures are heap-pinned to avoid accumulating large GATT
construction buffers in nested polling stack frames. The 16-packet notification
queues are unchanged; reducing their capacity is not the stack-overflow fix.

The MIDI page uses a virtualized list: only visible action rows are instantiated,
instead of constructing the entire action list when the page opens.

To isolate UI failures from Bluetooth, build with
`./firmware/build.sh --features ui-only`. This diagnostic retains the touch UI,
MIDI/performance configuration loading and display interrupts, but skips radio
initialization. It cannot connect to the Nano or footswitch. Build without this
feature to resume normal control; configuration partition offsets are unchanged.

For a separate HCI comparison, use `./firmware/build.sh --features original-hci`
without `ui-only`. This runs Bluetooth through the original `bt-hci`
`ExternalController` instead of the serialized adapter. It is a diagnostic, not
a verified fix for the previously observed command-slot borrow panic.

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
