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
  Footswitch connection/setup operations use the same deadlines, so a stalled
  footswitch cannot hold the shared scan/connect lock indefinitely.
- C305 receive processing runs alongside the serialized C304 writer, including
  during setup writes and touch/MIDI bursts. Command batches yield cooperatively;
  scene commands keep their existing ordering and verified readback.
- Disconnect clears command/write queues, slider signals, deferred library reads,
  reassembly, timers, and connection snapshots. Offline device edits are refused,
  so they cannot replay against a later connection.
- Services are enumerated once per Nano connection; each protocol service's
  characteristics and descriptors are discovered once and reused for all writes.
  Handles and the unsubscribed C306 mirror never survive a disconnected session.
- Nano connect/discovery/subscription/loading preempts new footswitch scans. Once
  Nano is Ready, footswitch scanning uses a 500 ms window every 2.5 seconds,
  reducing discovery latency while preserving Nano setup priority.
- Touch slider updates coalesce independently by control in a bounded queue;
  sweeping one slider cannot evict the latest value of another control.
- The C304 writer serves queued live-value controls before background requests,
  keeps FIFO ordering within each class, and switches to strict FIFO during scene
  recall so its staged write order is preserved.
- Save/copy/rename/delete/preset-change operations report **sent** only after
  their GATT write completes and **acknowledged** only on a Nano response;
  acknowledgements explicitly remain unverified unless a matching readback
  proves the result. Scene recall retains its existing verified-readback result.
- Failed Nano connections retry with 1, 2, 4, 8, 16, then 30-second backoff.
  The welcome screen's Retry now action cancels the wait and starts immediately.
- Reconnects rerun idempotent initial state/catalog/library reads. Failed C304
  writes and queued Save/Copy/Move/Delete actions are discarded, not replayed;
  inspect the Nano before manually retrying an operation with uncertain outcome.
- The Bluetooth picker remembers the selected Nano across reboot and displays
  device addresses to distinguish nearby units. Choose another Nano clears the
  preference and lets the picker discover alternatives.
- BLE Ready and periodic traffic summaries report setup duration, negotiated
  MTU/connection interval, write/command queue peaks, and failed writes.
  Dropped-notification counts are marked unavailable because TrouBLE's listener
  discards its lag count before firmware can observe it.
- ATT authentication/encryption failures are called out with stale-pairing
  cleanup steps. This firmware does not enable TrouBLE security or persist bonds,
  so it cannot automatically remove/recreate a bond; recovery is explicitly
  manual rather than claiming unsupported pairing behavior.
- Raw BLE packet dumps and per-write/slider logs are debug-only. Info logging
  retains setup stages and ten-second/per-disconnect RX, TX, and write-failure
  totals; failures remain errors. TX counts transport sends, not verified edits.
- The welcome screen retains the last connection failure through reconnect
  attempts until Ready. Errors identify scan, discovery, subscription, write, or
  loading, preserve the underlying host/ATT/controller debug code, and suggest
  recovery. A write error warns that a destructive operation may already have
  applied; the controller does not automatically replay it.
- **Preset, effects, and tuner** — the same behavior as the old ESP-IDF app, driven by Embassy deadlines (`firmware/src/state`, `firmware/src/fx`, `firmware/src/tuner`)
- **800×480 LCD** — Slint UI drawn into the RGB panel (`firmware/src/ui.rs`)
- **Category-colored FX tabs** — guitar OD orange, bass OD amber, compressor yellow, pitch magenta, filter lime, wah green, utility pink, modulation purple, delay cyan, and reverb blue. Colors match the saturation and brightness of the blue and amp-red accents in both orientations; bypassed FX retain black backgrounds with category-colored labels.
- Popups (Settings, preset/model/capture/cab lists, rename keyboard) fill the screen with the main gray background, and the screen behind them is hidden instead of redrawn. Close them with their close button.
- Delete/overwrite confirmations are centered cards over a dimmed preset list; answer with Cancel or the action button (outside taps are ignored).
- **Bluetooth devices** live in Settings → Bluetooth: tap a device to connect, or use its button to connect or forget it.
- Parameter cells (amp and FX) use the settings-row gray to stand out from the main background.
- **Pedals page** (header three-dot menu → Pedals) — General controls the EXP/MIDI jack and shows expression status; Sweeps edits per-preset heel/toe assignments; Bypass edits expression switch assignments; Calibration guides pedal calibration. Audio meters and USB playback level remain in Settings → Audio.
- **MIDI learn page** (header three-dot menu → MIDI) — 14 horizontally scrollable tabs: Controls; Presets 1–16, 17–32, 33–48, 49–64; Captures; Scenes/Gig; Amp/Cab; FX switches/models; and a parameter tab for each of Pre FX 1, Pre FX 2, Post FX 1, Post FX 2, Post FX 3. Categories contain at most 27 actions; each FX parameter tab has 24. Tap Learn on an action, then send a BLE-MIDI note, CC, or program change. Learn the same message on several rows to control multiple targets; Clear removes only that row's binding. Changing tabs or closing the page cancels learning. Bulk clearing lives in Settings → General. Buttons cover FX block on/off, gate, tuner, tap tempo, preset/capture step, direct preset 1–64 and capture 1–25 selection, FX model step, save, and explicit input/output/tuner mute/unmute. CC-only parameters cover all five amp controls, capture volume, cab level/high-pass/low-pass, gate amount, USB playback level, and up to 24 displayed knobs per FX block (only knobs present on the current model apply). For discrete CC actions, learn records whether 0 or a nonzero value is the press; the opposite value is treated as release. CC values still set continuous parameters across their full range. Repeated CC values for a pending parameter are coalesced; different targets are retained. Saved in flash with readback verification.
- **Preset manager** — browse presets in 12-slot tabs and use the three-dot menu to rename (on-screen keyboard; 4–64 ASCII letters, digits, spaces, `-`, `_`, unique), delete, move, or copy to any slot. Overwriting an occupied slot asks for confirmation
- **Scenes** — six named scenes per Nano preset, available from the scene selector and large-button Gig view. Touch and MIDI recalls share staged, transient restoration; scene data is saved in internal flash. Sets/custom banks are deferred.
- Gig scene buttons remain available during recall. Rapid consecutive touch/MIDI
  scene requests use the latest selection, replacing an unfinished recall without
  waiting for its verification. Other queued actions retain their ordering.
  Verification checks the requested preset, FX parameter, and cab readbacks
  without waiting for an additional periodic preset poll.
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

The root `Makefile` consolidates the common project commands:

```bash
make                      # show help (does not build or flash)
make build                # build release firmware, including BLE dependency patches
make flash                # flash the existing build without rebuilding
make run                  # build, flash, and monitor
make monitor              # serial monitor only; close it before flashing
make preview              # live landscape desktop preview
make preview-portrait     # live portrait desktop preview
make test                 # host protocol + MIDI/scene/connection tests; no UI renderer
make test-ui              # explicitly run UI renderer regression tests
make test-all             # all host tests, including UI renderer
make flash PORT=/dev/ttyACM0 BAUD=115200
```

`PORT` defaults to `NANO_PORT` or the flash helper's automatic detection.
`BAUD` controls flashing only and defaults to `NANO_BAUD` or 921600.
Host tests default to `HOST_TARGET=x86_64-unknown-linux-gnu`; override it for
another host architecture. `make test-protocol` and `make test-midi` run the
individual host suites. Desktop preview requires
`cargo +stable install slint-viewer --version 1.13.1 --locked` and a graphical
desktop session.

The existing scripts remain available:

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

### Flash a release from your browser

No Rust toolchain is needed. Download `bootloader.bin`, `partitions.bin`, and
`nano-cortex-firmware.bin` from the same
[release](https://github.com/fabio-stoppa/nano-cortex-controller/releases/latest).
Open [ESPWebTool](https://esptool.spacehuhn.com/) in a desktop Chrome or Edge
browser with Web Serial support, connect the board over USB, and close any
serial monitor before clicking **Connect** and selecting its serial port.
Add the three binary files with these addresses:

| File | Flash address |
| --- | --- |
| `bootloader.bin` | `0x0` |
| `partitions.bin` | `0x8000` |
| `nano-cortex-firmware.bin` | `0x10000` |

Click **Program**, then reset the board when flashing finishes. Do not select
whole-chip erase: these separate images leave the NVS, MIDI, and scene partitions
untouched. Back up your configuration before upgrading. The application binary
is not a merged image and must not be written at address `0x0`.

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

Host-test MIDI learn, multi-target queues, binding records, and connection reliability:

```bash
cargo +stable test --manifest-path firmware/tests/midi/Cargo.toml --target x86_64-unknown-linux-gnu
```

The connection tests use a deterministic Embassy clock and the production
deadline/duplex helpers. They cover exact write deadlines without retries,
underlying Bluetooth codes, cancellation at every connection stage, a 261,120-byte
transfer through a 16-notification queue while writes wait for replies, and
10,000 touch/MIDI edits followed by disconnect. Protocol tests also check the
exact 256 KiB reassembly limit, overflow recovery, and reset between sessions.
These are simulated transport tests, not radio or Nano firmware validation.

## Scenes and MIDI configuration

### Scenes

Each Nano preset (1-64) has six named **Scenes**, independent of Sets. Tap the
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
Six MIDI Learn actions recall scenes of the current Nano preset.

New scene names are limited to **16 ASCII characters** (letters,
numbers, spaces, `-`, `_`, no outer spaces), unique within their collection.

Scenes restore the capture slot, CAB/IR selection, amp controls, capture volume
when available, active cab EQ/level, gate, and all five FX models, parameters and
bypass states. Capture and CAB/IR selections load before their saved controls.
The saved CAB/IR name must identify a unique entry in the connected Nano library.
Expression assignments, tempo and global USB/mute settings are not scene data.
**Settings → General → Keep master/EQ** (saved with the scenes) makes recall leave
the amp Level, Bass, Mid and Treble knobs untouched; Gain, capture volume and cab
settings still follow the scene.
Older scenes remain loadable but must be re-saved to include capture/CAB selections;
missing selection fields leave the current selections unchanged.
Bypassed sources have no saved slot selection and retain this same behavior.
Recall is transient: it never saves over the Nano preset. The controller waits
for model/parameter loading and fresh readback before reporting **Recall verified**.
Recall sends only changed values for already-loaded models; changing a model
still waits for loading and restores all its parameters. Commands wake the BLE
writer immediately, and scene writes run in bounded cooperative bursts rather
than pausing between every parameter. Serial logs distinguish time until writes
are sent from time until recall is verified; neither measures audible latency.
There is no guaranteed 10 ms recall: BLE uses a requested 7.5 ms connection
interval, and multiple writes, model loading and readback can span many intervals.
Timeout/disconnect errors warn that sound may be partly updated; reload the
original Nano preset to recover. Actual sound recall still requires hardware validation.

Scene storage uses format version 4 (`nano-scenes`) with six-slot
`preset_scenes`. Version-3 eight-slot records are migrated on load or import:
scenes 1-6 are retained, and scenes 7-8 are permanently removed. Loading saves
the migrated record before making it available; failed writes are reported and
leave the original record intact for retry. Back up old exports before upgrading.
MIDI bindings for scenes 7-8 are removed on load/import; surviving action IDs
are unchanged.
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
table now contains 271 entries and 257 active actions. Set actions and old shelf
slots 5-32 remain unavailable. Scene JSON is encoded
compactly; all 64 six-scene collections fit the storage limit.
The JSON limit uses the full 1 MiB journal slot minus its 32-byte header, within
the existing 2 MiB scenes partition.

The desktop preview includes sample scenes and gig view.
Run `./firmware/preview.sh` (or add `portrait`)
and use the **FX**, **Scenes**, **Gig** and **Edited** developer shortcuts.
Header/FX controls also work; scene changes update temporary preview data only,
without connecting to a Nano or writing flash.

### Gig view

Enter gig view through MIDI Learn's **Enter gig view** action for six large
scene buttons for the current Nano preset (or use **Gig** in the desktop preview).
Loading/verification disables recall controls, and empty/disconnected recalls are disabled.
An accepted recall highlights its scene immediately, before readback completes;
Gig view omits routine recall status text. Failures remain in the serial log.
Gig view polls touch and
redraws at a 16 ms interval without rebuilding unchanged scene models.
The standard page header shows the preset name, an **X** back to the editor,
and tappable BPM on the right with a high-contrast, 160ms beat flash
(no continuous animation). In portrait, the BPM label sits below the value
to leave more room for the preset title.
Scene cards have a dimmed number at the top left and a large, left-aligned
name immediately below. Six cards use two columns in portrait and three in
landscape. The preview includes a maximum-length, 16-character scene name.
Compact amp/capture and cab/IR names share one line directly beneath the preset title
inside the header, aligned with the title and without labels.
Both lines fit within the standard 58px header height.
The portrait editor uses two rows of three FX-chain buttons with the parameter cells'
8px gaps and 2px inner side insets; portrait Gig view uses the same
chain dimensions. Portrait tuner controls use
a narrower reference slider between MUTE and EXIT without overlapping them.
The portrait editor header stacks the left-aligned preset title above its
smaller scene selector, with BPM and the menu on the right.
FX parameters use three columns in portrait and four in landscape, with
vertical scrolling for additional rows.
Portrait amp controls group Input Gate and Gate Amount on the first row;
Capture Level, Master Level, and Gain on the second; Bass, Mid, and Treble
on the third; and Cab Output, High Pass, and Low Pass on the fourth.
Below the scenes, the live category-colored FX chain replaces the tuner and
tap-tempo buttons. Tap or hold an FX block to toggle it using the editor's
bypass action; double-tap opens its model picker in the editor. Tap AMP to return
to the amp editor. Chain controls are disabled during recall or disconnection.
The FX chain uses the same 8px horizontal gaps and screen margins as the scene
cards. Tuning from Gig view retains the Gig header; tuner **EXIT** or turning
the tuner off returns to the scenes, while the header **X** exits to the editor.
MIDI Learn also includes enter/exit gig
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
- `scenes.json`: all controller scenes, using the current version-4 `nano-scenes`
  format (maximum 1,048,544 bytes). Older banks/`performance.json` are not imported.
- `preferences.json`: controller screen orientation, Autosave, and Remember last
  mode (maximum 4 KiB).
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
  "autosave": false,
  "remember_mode": false
}
```

`orientation` accepts `landscape`, `portrait`, `portrait_flipped`, or
`landscape_flipped`. `autosave` and `remember_mode` must be JSON booleans. Missing
or unknown fields (except the optional `remember_mode` in older files),
unsupported versions and invalid values are rejected rather than defaulted.
Imported preferences take effect on **Restart controller**, so the USB screen
does not rotate mid-import. Unchanged preference records are not rewritten.

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

**Settings → General → Autosave scenes** is off by default and persisted with the screen
layout. When enabled, edits made through touch or MIDI restart a five-second idle
timer. It overwrites the selected dirty scene using its existing name; it never
saves the Nano preset. No empty controller slots are created. Autosave waits for state transfers/recalls to finish and for
tuner exit; changing Nano presets, disconnecting or recalling a controller preset
cancels the pending timer. Enabling it applies to subsequent edits.
Errors are displayed in General; a failed autosave requires another edit to retry.
The Nano save status reports the request being sent, not an unverified success.

**Settings → General → Remember last mode** is off by default. When enabled, the
controller saves whether Gig mode or the editor was last open and restores that
screen on the next boot. It applies to mode changes from the menu, Gig screen, or
MIDI; disabling it boots into the editor.

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
3. Flash and watch serial for `Nano connection: Searching` through `Ready`.

For hardware acceptance, interrupt Nano power or leave range at Connecting,
Discovering, Subscribing, Loading, and Ready; also restart the controller while
Searching. Each attempt must end with the correct stage/code, leave no edits
to replay, and reconnect with fresh state. Exercise fast touch/MIDI sweeps,
scene recalls, and library transfers together; inspect traffic summaries for
write failures and confirm replies continue during write bursts. Force a missing
or incompatible service and a rejected subscription where a BLE test peripheral
is available; neither may reach Ready. Do not retry destructive operations
without checking the Nano's resulting state. Capture setup timings and negotiated
MTU/interval before proposing any radio interval/window/buffer changes.

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
