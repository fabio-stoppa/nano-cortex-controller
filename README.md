# Nano Cortex — ESP32 firmware (bare metal Rust)

Firmware for the **Waveshare ESP32-S3-Touch-LCD-4.3B**. The protocol and the controller ideas come from the community projects in [Credits](#credits).

**Stack:** bare-metal Rust in `firmware/` — `esp-hal`, Embassy, `esp-radio`, and a TrouBLE central. Preset, effect, and tuner logic run in that image. The protocol crate stays host-testable.

## What it does

- Boots over USB serial (bare metal `main`)
- **Protocol in Rust** — C304 builders + C305/protobuf parsers (`crates/nano-wire`)
- **BLE in Rust** — TrouBLE central on `esp-radio` (`firmware/src/ble.rs`)
- Nano setup reports Searching, Connecting, Discovering, Subscribing, Loading,
  and Ready separately on the welcome screen and serial log.
- Ready requires both Nano services, writable C304, a successful C305 notification
  subscription, a complete initial state snapshot, preset names, and a parsed library.
  The loading screen never proceeds merely because a timer expired.
- Nano connect attempts have a 12-second deadline, individual setup operations
  8 seconds, C304 writes 3 seconds, and initial loading 45 seconds. A stalled or
  failed write ends the session rather than continuing uncertain queued edits.
- **Preset, effects, and tuner** — the same behavior as the old ESP-IDF app, driven by Embassy deadlines (`firmware/src/state`, `firmware/src/fx`, `firmware/src/tuner`)
- **800×480 LCD** — Slint UI drawn into the RGB panel (`firmware/src/ui.rs`)
- **Category-colored FX tabs** — guitar OD orange, bass OD amber, compressor yellow, pitch magenta, filter lime, wah green, utility pink, modulation purple, delay cyan, and reverb blue. Colors match the saturation and brightness of the blue and amp-red accents in both orientations; bypassed FX retain black backgrounds with category-colored labels.
- Popups (Settings, preset/model/capture/cab lists, rename keyboard) fill the screen with the main gray background, and the screen behind them is hidden instead of redrawn. Close them with their close button.
- Delete/overwrite confirmations are centered cards over a dimmed preset list; answer with Cancel or the action button (outside taps are ignored).
- **Bluetooth devices** live in Settings → Bluetooth: tap a device to connect, or use its button to connect or forget it.
- Parameter cells (amp and FX) use the settings-row gray to stand out from the main background.
- **Pedals page** (header three-dot menu → Pedals) — General controls the EXP/MIDI jack and shows expression status; Sweeps edits per-preset heel/toe assignments; Bypass edits expression switch assignments; Calibration guides pedal calibration. Audio meters and USB playback level remain in Settings → Audio.
- **MIDI learn page** (header three-dot menu → MIDI) — 14 horizontally scrollable tabs: Controls; Presets 1–16, 17–32, 33–48, 49–64; Captures; Scenes/Gig; Amp/Cab; FX switches/models; and a parameter tab for each of Pre FX 1, Pre FX 2, Post FX 1, Post FX 2, Post FX 3. Categories contain at most 27 actions; each FX parameter tab has 24. Tap Learn on an action, then send a BLE-MIDI note, CC, or program change. Learn the same message on several rows to control multiple targets; Clear removes only that row's binding. Changing tabs or closing the page cancels learning. Bulk clearing lives in Settings → General. Buttons cover FX block on/off, gate, tuner, tap tempo, preset/capture step, direct preset 1–64 and capture 1–25 selection, FX model step, save, and explicit input/output/tuner mute/unmute. CC-only parameters cover all five amp controls, capture volume, cab level/high-pass/low-pass, gate amount, USB playback level, and up to 24 displayed knobs per FX block (only knobs present on the current model apply). CC 0 does not trigger button actions; it still sets continuous parameters to their minimum. Repeated CC values for a pending parameter are coalesced; different targets are retained. Saved in flash with readback verification.
- **Preset manager** — browse presets in 12-slot tabs and use the three-dot menu to rename (on-screen keyboard; 4–64 ASCII letters, digits, spaces, `-`, `_`, unique), delete, move, or copy to any slot. Overwriting an occupied slot asks for confirmation
- **Scenes** — eight named scenes per Nano preset, available from the scene selector and large-button Gig view. Touch and MIDI recalls share staged, transient restoration; scene data is saved in internal flash. Sets/custom banks are deferred.
- Open preset lists update in place when the refreshed catalog arrives after a rename, delete, move, or copy.
- Refresh and Nano preset Save live in Settings → General → Tools next to Tuner (Save is highlighted with unsaved changes); the header has a background-free scene selector (hold a selected scene for Rename, Save, or Delete) and a vertical three-dot menu with Gig mode, a separator, then MIDI, Pedals, and Settings. BPM text and icon are white. An asterisk after the current Nano preset title marks unsaved Nano edits.
- In portrait the FX chain sits at the bottom of the screen, below the page content.
- Tap an FX chain tab to select it, hold it to toggle bypass, or double-tap it to open model selection. The Amp tab remains selection-only.
- Preset three-dot menus have a 64px-wide, full-row-height touch target separate from preset selection.
- Selecting a preset keeps the preset list open and moves its highlight, so presets can be auditioned back to back.
- All full-screen pages, including Welcome, share 12px edge padding. The tuner keeps the editor header above its tuning display and controls, including when opened from Gig mode. The Amp page's capture and cab cards have no outline, center their icon, slot and name, and wrap names to two lines before truncating with "…".
- Preset popup names wrap to three lines on taller cards, and capture and cab names wrap to two; the rename keyboard has a blinking cursor, keys light up while pressed, holding backspace repeats (400ms delay, then every 60ms), and long names keep their end visible.
- While the rename keyboard is open, touch is polled every 10ms instead of 50ms; duplicate-name validation scans the stored catalog without allocating copies.
- The software renderer has no occlusion culling, so screens covered by the keyboard or a full-screen panel are hidden rather than redrawn underneath it. The header title elides long preset names.
- Long item names and popup titles truncate with an ellipsis within their available space; cards that support multiple lines keep wrapping before truncation. Full names remain stored.
- Flash commits pause RGB scan-out at vertical blank and restart with freshly filled bounce buffers, avoiding repeated strips while the refill core is parked. Journal saves erase only sectors needed for the new record.
- Touch and MIDI sound edits mark the Nano preset dirty locally as well as accepting Nano dirty notifications, so the header asterisk does not depend on receiving an edit notification.
- The header reads the Nano's initial unsaved flag from current-state field 41, so already-dirty presets show an asterisk on connection without another edit.
- FX parameter labels use 19px text; amp labels remain 17px and parameter values remain 24px.
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

Scenes additionally use a checked 2 MiB `scenes` partition at 10 MiB.
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

## Scenes and MIDI configuration

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

New scene names are limited to **16 ASCII characters** (letters,
numbers, spaces, `-`, `_`, no outer spaces), unique within their collection.

Scenes restore amp controls, capture volume when available, active cab EQ/level,
gate, and all five FX models, parameters and bypass states. Library selections,
expression assignments, tempo and global USB/mute settings are not scene data.
Recall is transient: it never saves over the Nano preset. The controller waits
for model/parameter loading and fresh readback before reporting **Recall verified**.
Timeout/disconnect errors warn that sound may be partly updated; reload the
original Nano preset to recover. Actual sound recall still requires hardware validation.

Scene storage uses format version 3 (`nano-scenes`) with eight-slot
`preset_scenes`.
**The first boot of this firmware permanently factory-resets all controller
storage**: layout/settings, MIDI
bindings, scenes and legacy Sets. The entire `nvs`, `midi` and `scenes`
partitions are erased, including both journal generations.
No legacy Set, binary MIDI or four-scene migration paths remain.
The reset is verified and marked complete in the last NVS sector, so newly
saved data survives subsequent boots. An interrupted or failed reset is logged
and stops startup before settings are loaded; reboot retries the full reset.
Nano Cortex's own presets/settings and the app/PHY partitions are not erased.
MIDI action IDs are compacted after removing the FX preset slots; the action
table now contains 273 entries and 259 active actions. Set actions and old shelf
slots 5-32 remain unavailable. Scene JSON is encoded
compactly; all 64 eight-scene collections fit the storage limit.
The JSON limit uses the full 1 MiB journal slot minus its 32-byte header, within
the existing 2 MiB scenes partition.

The desktop preview includes sample scenes and gig view.
Run `./firmware/preview.sh` (or add `portrait`)
and use the **FX**, **Scenes**, **Gig** and **Edited** developer shortcuts.
Header/FX controls also work; scene changes update temporary preview data only,
without connecting to a Nano or writing flash.

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

### USB configuration drive and internal storage

The desktop preview's **USB** shortcut opens a simulated USB screen.
**Cancel USB edits** shows the result screen and its recovery controls;
**Restart controller** returns to the preview. No USB drive or flash writes occur.

MIDI assignments and Scenes data remain saved as checked JSON records in
internal flash. Configure them using the controller's touch UI or MIDI Learn.
To back up or load JSON files, connect the board's native USB data port to a
computer and tap **USB drive** on the connection screen, or
**Settings → General → Configuration → USB drive** when connected.
Bluetooth control and MIDI editing pause until the controller is restarted.
Use a data cable; a separate USB-to-serial port does not expose the drive.
Close any serial monitor (including the firmware monitor) before entering USB
mode: keeping it open can prevent the drive from appearing. If **NANO CONFIG**
does not appear, close the monitor, restart the controller and re-enter USB mode.
The USB screen shows this warning; it does not detect whether a monitor is open.

The computer mounts a disposable 3 MiB FAT12 volume named **NANO CONFIG** containing:

- `midi.json`: learned MIDI assignments (maximum 48 KiB).
- `scenes.json`: all controller scenes, using the current version-3 `nano-scenes`
  format (maximum 1,048,544 bytes). Older banks/`performance.json` are not imported.
- `preferences.json`: controller screen orientation and Autosave (maximum 4 KiB).
  Does not include Nano settings or Bluetooth pairings.
- `README.TXT`: editing instructions and active MIDI action IDs.

Copy the JSON files to your computer for backup, or edit/replace them on the
drive, keeping all three filenames in its root directory. **Safely eject the drive**
to import: all three files must pass filesystem, UTF-8 and configuration validation
before any are written to flash. Check the controller screen for the result,
then tap **Restart controller** to resume normal operation.
Saving a file or flushing the drive alone does not import it. **Cancel USB edits**
discards the disposable drive without saving computer changes; restart afterward.
Unplugging without eject does not commit changes.

Invalid, missing, oversized or reformatted files are rejected. MIDI and scenes use
the existing checked, two-generation flash journals with readback verification.
Preferences use the existing shared layout/Autosave record in NVS, also verified
by readback; it is not a two-generation journal and an interrupted preferences
write can require setting the preferences again.
The files are **not one atomic transaction**: scenes, MIDI, then preferences
are saved in that order. Errors explicitly identify partial saves; a preferences
save failure does not roll back MIDI or scenes. Back up all three files before editing.
USB mode refuses to export over unreadable saved configuration rather than
silently replacing it with defaults. Do not format the volume; additional files
are temporary and are not imported.

Native USB switches to mass storage during this mode, so its serial connection
may disappear until restart; normal flashing/serial operation returns afterward.
The Nano's USB playback-level control is unrelated and unchanged.
The USB stack, driver and USB-internal synchronization are pinned to Git for MSC
support. Controller/Bluetooth synchronization and the shared Embassy
time/futures runtime remain on their published dependencies.

`preferences.json` example:

```json
{
  "format": "nano-controller-preferences",
  "version": 1,
  "orientation": "landscape",
  "autosave": false
}
```

`orientation` accepts `landscape`, `portrait`, `portrait_flipped`, or
`landscape_flipped`. `autosave` must be a JSON boolean. Missing/unknown fields,
unsupported versions and invalid values are rejected rather than defaulted.
Imported preferences take effect on **Restart controller**, so the USB screen
does not rotate mid-import. Existing touch settings and boot behavior are unchanged;
unchanged preference records are not rewritten.

**Settings → General → Clear** provides independent, confirmed recovery actions
for every editable USB file:

- **MIDI** replaces saved assignments with a valid empty `midi.json`, even if
  the old file was invalid and no bindings could be loaded.
- **Scenes** replaces all controller scenes across all Nano presets with a
  valid empty `scenes.json`, including when the saved scenes cannot be loaded.
- **Preferences** resets `preferences.json` values to landscape and Autosave
  off, immediately updating the controller screen after a verified save.

The same three actions are available on the **USB result/error screen**, so
invalid data can be reset even without a Nano connection. Confirm the affected
file, then **Restart controller** and re-enter USB mode. Recovery is never automatic;
Cancel leaves data unchanged. MIDI/scene clears preserve their live state if saving
fails. Success confirmations and errors appear in a dismissible result modal,
not below the Clear row. Preference writes use the NVS record described
above; a failed write may require retrying the reset.
Each action leaves the other two configurations alone. Nano presets, Nano settings,
pedal calibration and Bluetooth pairings are not cleared.

**Settings → General → Autosave** is off by default and persisted with the screen
layout. When enabled, edits made through touch or MIDI restart a five-second idle
timer. It overwrites the selected dirty scene using its existing name, then saves
the current dirty Nano preset. No empty controller
slots are created. Autosave waits for state transfers/recalls to finish and for
tuner exit; changing Nano presets, disconnecting or recalling a controller preset
cancels the pending timer. Enabling it applies to subsequent edits.
Errors are displayed in General; a failed autosave requires another edit to retry.
The Nano save status reports the request being sent, not an unverified success.

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

Renamed Nanos are discovered through their advertised A002/A003 service UUIDs,
independently of the local name. Factory-name matching remains a fallback.
Service-only reports use "Nano Cortex" in the picker until a named report arrives;
advertisements and scan responses are associated by Bluetooth address.
If a Nano does not advertise either service, its custom name alone cannot identify
it automatically; restore the factory name or capture its advertising data for diagnosis.

If scanning continues with the Nano's GAIN ring spinning, serial logs distinguish
scan-start failures/timeouts from scans that receive advertisements but no matching
device. The first eight named reports are logged as `BLE scan heard ...`; unsuccessful
scans report advertisement/name/malformed-report counts for the first four scan
windows and every tenth window afterward. These diagnostics do not change device
matching or connect to unsupported devices. `Scenes: Invalid scenes.json` is a
separate saved-configuration error and does not prevent Bluetooth scanning.

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
MIDI/scene configuration loading and display interrupts, but skips radio
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
