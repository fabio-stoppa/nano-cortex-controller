//! CC → FX mapping for M-Vave Chocolate / Chocolate Plus.
//!
//! Supports Chocolate setups plus a dedicated Plus bank-0 layout:
//!
//! ## Chocolate Plus — Program Change C bank 0 (CC 0–3)
//! Remapped so the Plus’s four switches cover what bank 0 on ChocolateCtrl leaves out:
//!
//! | CC | Action                                      |
//! |----|---------------------------------------------|
//! | 0  | Toggle Post3                                |
//! | 1  | Toggle gate                                 |
//! | 2  | Tap tempo                                   |
//! | 3  | Solo boost (Post3 + ±15% capture LV)        |
//!
//! ## Program Change C (built-in, no CubeSuite programming)
//! Despite the name, this mode sends **Control Change**, not Program Change.
//! Banks of 4 (A–D); A+B / C+D change bank. Messages are typically `CC(n, 0)`.
//!
//! | Bank | A     | B     | C      | D        |
//! |------|-------|-------|--------|----------|
//! | 0    | Pre1  | Pre2  | Post1  | Post2    |  (CC 0–3 toggle)
//! | 1    | Post3 | Gate  | Focus1 | Focus2   |  (CC 4–7)
//! | 2    | Focus3| Focus4| Focus5 | TapTempo |  (CC 8–11)
//!
//! ## Advanced Custom (CubeSuite)
//! Momentary buttons send 127 on press / 0 on release — we act on value ≥ 64.
//!
//! | CC  | Action        |
//! |-----|----------------|
//! | 102–106 | Toggle Pre1…Post3 |
//! | 107 | Toggle gate    |
//! | 108 | Tap tempo      |
//! | 110 | Solo boost (Post3 + LV) |
//! | 112–116 | Focus Pre1…Post3 |
//!
//! ## Dual footswitch on EXP jack (CubeSuite)
//! One CC, tip/ring distinguished by value (not press/release).
//! Momentary ring often sends `127` then `0` on release — that trailing `0`
//! is ignored so it does not also toggle reverb.
//!
//! | CC  | Value | Action              |
//! |-----|-------|---------------------|
//! | 109 | 0     | Solo boost (Post3 + ±15% capture LV) |
//! | 109 | 127   | Tap tempo           |

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use log::info;
use nano_wire::types::{NanoAmpC, AMP_CAPTURE_NAME_MAX};

use crate::fx::types::{NanoFxState, NANO_FX_BLOCK_COUNT, NANO_FX_POST3};
use crate::fx::{
    nano_fx_focus_from_ui, nano_fx_snapshot, nano_fx_write_block_enabled, nano_fx_write_gate_enabled,
};
use crate::state::{
    nano_state_snapshot_amp, nano_state_tempo_tap, nano_state_write_capture_volume,
};

use super::ctx::{PeerKind, TAG};
use super::midi::MidiMsg;

const PRESS_THRESHOLD: u8 = 64;

/// Advanced Custom CCs (value ≥ 64).
const CC_ADV_TOGGLE_PRE1: u8 = 102;
const CC_ADV_TOGGLE_PRE2: u8 = 103;
const CC_ADV_TOGGLE_POST1: u8 = 104;
const CC_ADV_TOGGLE_POST2: u8 = 105;
const CC_ADV_TOGGLE_POST3: u8 = 106;
const CC_ADV_TOGGLE_GATE: u8 = 107;
const CC_ADV_TAP_TEMPO: u8 = 108;
const CC_ADV_SOLO_BOOST: u8 = 110;
const CC_ADV_FOCUS_PRE1: u8 = 112;
const CC_ADV_FOCUS_PRE2: u8 = 113;
const CC_ADV_FOCUS_POST1: u8 = 114;
const CC_ADV_FOCUS_POST2: u8 = 115;
const CC_ADV_FOCUS_POST3: u8 = 116;

/// Dual FS on the Chocolate EXP jack: same CC, value selects switch.
const CC_DUAL_FS: u8 = 109;

/// CAPTURE LV raw delta for ±15% (`pct` scale uses 0..255 → 0..100).
const REVERB_CAPTURE_BOOST_RAW: u8 = ((255u16 * 15 + 50) / 100) as u8;

/// Program Change C fixed layout (any value, including 0).
const CC_PC_C_MAX: u8 = 11;

/// Ignore repeat of the same CC+value within this window (PC C often double-fires).
const DEBOUNCE_MS: u32 = 80;

/// After a dual-FS 127 (tap), ignore value 0 for this long (momentary release).
const DUAL_FS_RELEASE_SUPPRESS_MS: u32 = 750;

static LAST_CC: AtomicU8 = AtomicU8::new(0xFF);
static LAST_VAL: AtomicU8 = AtomicU8::new(0xFF);
static LAST_MS: AtomicU32 = AtomicU32::new(0);
static DUAL_FS_SAW_127: AtomicBool = AtomicBool::new(false);
static DUAL_FS_LAST_127_MS: AtomicU32 = AtomicU32::new(0);
static REVERB_CAPTURE_BOOSTED: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u32 {
    (unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1000) as u32
}

fn debounced(cc: u8, value: u8) -> bool {
    let t = now_ms();
    let prev_cc = LAST_CC.load(Ordering::Relaxed);
    let prev_val = LAST_VAL.load(Ordering::Relaxed);
    let prev_ms = LAST_MS.load(Ordering::Relaxed);
    if cc == prev_cc && value == prev_val && t.wrapping_sub(prev_ms) < DEBOUNCE_MS {
        return false;
    }
    LAST_CC.store(cc, Ordering::Relaxed);
    LAST_VAL.store(value, Ordering::Relaxed);
    LAST_MS.store(t, Ordering::Relaxed);
    true
}

pub(super) fn apply_msg(msg: MidiMsg, kind: PeerKind) {
    match msg {
        MidiMsg::Pc { channel, program } => {
            info!(
                target: TAG,
                "MIDI PC ch={} prog={} (ignored — use Program Change C or Advanced CC)",
                channel + 1,
                program
            );
        }
        MidiMsg::Cc {
            channel,
            cc,
            value,
        } => {
            let action = match classify(cc, value, kind) {
                Some(a) => a,
                None => return,
            };
            if !debounced(cc, value) {
                return;
            }
            info!(
                target: TAG,
                "MIDI CC ch={} cc={} val={} ({})",
                channel + 1,
                cc,
                value,
                kind.label()
            );
            match action {
                Action::ToggleBlock(b) => toggle_block(b),
                Action::ToggleGate => toggle_gate(),
                Action::SoloBoost => solo_boost(),
                Action::Focus(b) => focus_block(b),
                Action::TapTempo => {
                    let _ = nano_state_tempo_tap();
                }
            }
        }
    }
}

#[derive(Copy, Clone)]
enum Action {
    ToggleBlock(u32),
    ToggleGate,
    SoloBoost,
    Focus(u8),
    TapTempo,
}

fn classify(cc: u8, value: u8, kind: PeerKind) -> Option<Action> {
    // Chocolate Plus bank 0: remaining FX + tap + solo boost on the four switches.
    if kind == PeerKind::ChocolatePlus && cc <= 3 {
        return Some(match cc {
            0 => Action::ToggleBlock(NANO_FX_POST3),
            1 => Action::ToggleGate,
            2 => Action::TapTempo,
            3 => Action::SoloBoost,
            _ => return None,
        });
    }

    // Program Change C: CC 0–11, any value (usually 0).
    if cc <= CC_PC_C_MAX {
        return Some(match cc {
            0 => Action::ToggleBlock(0),
            1 => Action::ToggleBlock(1),
            2 => Action::ToggleBlock(2),
            3 => Action::ToggleBlock(3),
            4 => Action::ToggleBlock(4),
            5 => Action::ToggleGate,
            6 => Action::Focus(0),
            7 => Action::Focus(1),
            8 => Action::Focus(2),
            9 => Action::Focus(3),
            10 => Action::Focus(4),
            11 => Action::TapTempo,
            _ => return None,
        });
    }

    // Dual FS on EXP: value selects which switch (0 must fire — not a release).
    if cc == CC_DUAL_FS {
        return match value {
            127 => {
                DUAL_FS_SAW_127.store(true, Ordering::Relaxed);
                DUAL_FS_LAST_127_MS.store(now_ms(), Ordering::Relaxed);
                Some(Action::TapTempo)
            }
            0 => {
                // Momentary tap side: press=127, release=0 — don't treat release as reverb.
                if DUAL_FS_SAW_127.load(Ordering::Relaxed) {
                    let since = now_ms()
                        .wrapping_sub(DUAL_FS_LAST_127_MS.load(Ordering::Relaxed));
                    if since < DUAL_FS_RELEASE_SUPPRESS_MS {
                        return None;
                    }
                }
                Some(Action::SoloBoost)
            }
            _ => None,
        };
    }

    // Advanced Custom: press only.
    if value < PRESS_THRESHOLD {
        return None;
    }
    Some(match cc {
        CC_ADV_TOGGLE_PRE1 => Action::ToggleBlock(0),
        CC_ADV_TOGGLE_PRE2 => Action::ToggleBlock(1),
        CC_ADV_TOGGLE_POST1 => Action::ToggleBlock(2),
        CC_ADV_TOGGLE_POST2 => Action::ToggleBlock(3),
        CC_ADV_TOGGLE_POST3 => Action::ToggleBlock(4),
        CC_ADV_TOGGLE_GATE => Action::ToggleGate,
        CC_ADV_TAP_TEMPO => Action::TapTempo,
        CC_ADV_SOLO_BOOST => Action::SoloBoost,
        CC_ADV_FOCUS_PRE1 => Action::Focus(0),
        CC_ADV_FOCUS_PRE2 => Action::Focus(1),
        CC_ADV_FOCUS_POST1 => Action::Focus(2),
        CC_ADV_FOCUS_POST2 => Action::Focus(3),
        CC_ADV_FOCUS_POST3 => Action::Focus(4),
        _ => return None,
    })
}

fn toggle_block(block: u32) {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }
    let mut snap = NanoFxState::zeroed();
    let on = if nano_fx_snapshot(&mut snap) {
        snap.blocks[block as usize].on
    } else {
        false
    };
    let _ = nano_fx_write_block_enabled(block, !on);
}

fn solo_boost() {
    let mut snap = NanoFxState::zeroed();
    let on = if nano_fx_snapshot(&mut snap) {
        snap.blocks[NANO_FX_POST3 as usize].on
    } else {
        false
    };
    let new_on = !on;
    let _ = nano_fx_write_block_enabled(NANO_FX_POST3, new_on);

    let boosted = REVERB_CAPTURE_BOOSTED.load(Ordering::Relaxed);
    if new_on == boosted {
        return;
    }
    let mut amp = NanoAmpC {
        gain: 0,
        level: 0,
        bass: 0,
        mid: 0,
        treble: 0,
        capture_volume: 0,
        capture_slot: 0,
        capture_name: [0; AMP_CAPTURE_NAME_MAX],
        have_capture_slot: false,
        have_capture_name: false,
        have_capture_volume: false,
        valid: false,
    };
    if !nano_state_snapshot_amp(&mut amp) {
        return;
    }
    let vol = amp.capture_volume as i16;
    let delta = REVERB_CAPTURE_BOOST_RAW as i16;
    let new_vol = if new_on {
        (vol + delta).min(255)
    } else {
        (vol - delta).max(0)
    } as u8;
    if new_vol != amp.capture_volume {
        let _ = nano_state_write_capture_volume(new_vol);
    }
    REVERB_CAPTURE_BOOSTED.store(new_on, Ordering::Relaxed);
    info!(
        target: TAG,
        "solo boost {} — capture LV {} -> {} (±{} raw)",
        if new_on { "on" } else { "off" },
        amp.capture_volume,
        new_vol,
        REVERB_CAPTURE_BOOST_RAW
    );
}

fn toggle_gate() {
    let mut snap = NanoFxState::zeroed();
    let on = if nano_fx_snapshot(&mut snap) {
        snap.gate.on
    } else {
        false
    };
    let _ = nano_fx_write_gate_enabled(!on);
}

fn focus_block(block: u8) {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }
    nano_fx_focus_from_ui(block);
}
