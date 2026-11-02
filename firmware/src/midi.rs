//! MIDI learn for the BLE-MIDI footswitch.
//!
//! The footswitch task feeds raw BLE-MIDI packets in. A message is either
//! turned into a learned binding (while the MIDI page is waiting for one) or
//! looked up in the binding table and run as a controller action.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;

use crate::link::Cmd;

#[path = "midi/backup.rs"]
mod backup;

pub const BLOCKS: usize = 5;
pub const KNOBS_PER_BLOCK: usize = 4;
const TRIGGERS: usize = 8;
const FX_KNOBS: usize = BLOCKS * KNOBS_PER_BLOCK;
const AMP_KNOBS: usize = 5;
const CAB_CONTROLS: usize = 3;
const LEGACY_ACTION_COUNT: usize = BLOCKS + TRIGGERS + FX_KNOBS;
const EXISTING_ACTION_COUNT: usize = LEGACY_ACTION_COUNT + AMP_KNOBS + 1 + CAB_CONTROLS;
const EXTRA_KNOBS: usize = crate::fx::types::NANO_FX_MAX_PARAMS - KNOBS_PER_BLOCK;
const EXTRA_KNOBS_START: usize = EXISTING_ACTION_COUNT;
const GATE_AMOUNT: usize = EXTRA_KNOBS_START + BLOCKS * EXTRA_KNOBS;
const USB_PLAYBACK: usize = GATE_AMOUNT + 1;
const MODEL_START: usize = USB_PLAYBACK + 1;
const PRESET_START: usize = MODEL_START + BLOCKS * 2;
const CAPTURE_START: usize = PRESET_START + 64;
const MUTE_START: usize = CAPTURE_START + 25;
const BANK_START: usize = MUTE_START + 6;
const SCENE_START: usize = BANK_START + 14;
const FX_PRESET_START: usize = SCENE_START + 4;
const GIG_START: usize = FX_PRESET_START + BLOCKS * crate::performance::data::FX_PRESETS;
pub const ACTION_COUNT: usize = GIG_START + 2;

const BLOCK_NAMES: [&str; BLOCKS] = [
    "Pre FX 1",
    "Pre FX 2",
    "Post FX 1",
    "Post FX 2",
    "Post FX 3",
];
const AMP_KNOB_NAMES: [&str; AMP_KNOBS] =
    ["Amp gain", "Amp level", "Amp bass", "Amp mid", "Amp treble"];
const CAB_CONTROL_NAMES: [&str; CAB_CONTROLS] = ["Cab level", "Cab high-pass", "Cab low-pass"];
const TRIGGER_NAMES: [&str; TRIGGERS] = [
    "Noise gate",
    "Tuner",
    "Tap tempo",
    "Next preset",
    "Previous preset",
    "Next capture",
    "Previous capture",
    "Save preset",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Note,
    Cc,
    Program,
}

/// One incoming message. `value` is the CC value, note velocity, or 0.
#[derive(Clone, Copy)]
struct Msg {
    kind: Kind,
    channel: u8,
    number: u8,
    value: u8,
}

impl Msg {
    /// Packs kind, channel and number; 0 stays free for "unassigned".
    fn key(self) -> u16 {
        let kind = match self.kind {
            Kind::Note => 0u16,
            Kind::Cc => 1,
            Kind::Program => 2,
        };
        0x8000 | kind << 12 | u16::from(self.channel & 0x0f) << 8 | u16::from(self.number & 0x7f)
    }
}

pub fn key_text(key: u16) -> String {
    if key == 0 {
        return String::new();
    }
    let channel = ((key >> 8) & 0x0f) + 1;
    let number = key & 0x7f;
    match (key >> 12) & 0x07 {
        0 => format!("Note {number} ch{channel}"),
        1 => format!("CC {number} ch{channel}"),
        _ => format!("PC {number} ch{channel}"),
    }
}

pub fn action_name(index: usize) -> String {
    if index < BLOCKS {
        format!("{} on/off", BLOCK_NAMES[index])
    } else if index < BLOCKS + TRIGGERS {
        String::from(TRIGGER_NAMES[index - BLOCKS])
    } else if index < LEGACY_ACTION_COUNT {
        let n = index - BLOCKS - TRIGGERS;
        format!(
            "{} knob {}",
            BLOCK_NAMES[n / KNOBS_PER_BLOCK],
            n % KNOBS_PER_BLOCK + 1
        )
    } else if index < LEGACY_ACTION_COUNT + AMP_KNOBS {
        String::from(AMP_KNOB_NAMES[index - LEGACY_ACTION_COUNT])
    } else if index == LEGACY_ACTION_COUNT + AMP_KNOBS {
        String::from("Capture volume")
    } else if index < EXISTING_ACTION_COUNT {
        String::from(CAB_CONTROL_NAMES[index - LEGACY_ACTION_COUNT - AMP_KNOBS - 1])
    } else if index < GATE_AMOUNT {
        let n = index - EXTRA_KNOBS_START;
        format!(
            "{} knob {}",
            BLOCK_NAMES[n / EXTRA_KNOBS],
            n % EXTRA_KNOBS + 5
        )
    } else if index == GATE_AMOUNT {
        String::from("Gate amount")
    } else if index == USB_PLAYBACK {
        String::from("USB playback level")
    } else if index < PRESET_START {
        let n = index - MODEL_START;
        format!(
            "{} {} model",
            BLOCK_NAMES[n / 2],
            if n % 2 == 0 { "next" } else { "previous" }
        )
    } else if index < CAPTURE_START {
        format!("Select preset {}", index - PRESET_START + 1)
    } else if index < MUTE_START {
        format!("Select capture {}", index - CAPTURE_START + 1)
    } else if index < BANK_START {
        String::from(
            [
                "Mute input",
                "Unmute input",
                "Mute output",
                "Unmute output",
                "Mute tuner output",
                "Unmute tuner output",
            ][index - MUTE_START],
        )
    } else if index < BANK_START + 2 {
        String::from(if index == BANK_START { "Next bank" } else { "Previous bank" })
    } else if index < BANK_START + 6 {
        format!("Recall bank slot {}", index - BANK_START - 1)
    } else if index < SCENE_START {
        format!("Select bank {}", index - BANK_START - 5)
    } else if index < FX_PRESET_START {
        format!("Recall scene {}", index - SCENE_START + 1)
    } else if index < GIG_START {
        let offset = index - FX_PRESET_START;
        format!("FX preset {} into {}", offset / BLOCKS + 1, BLOCK_NAMES[offset % BLOCKS])
    } else {
        String::from(if index == GIG_START { "Enter gig view" } else { "Exit gig view" })
    }
}

fn continuous_action(index: usize) -> bool {
    (BLOCKS + TRIGGERS..EXISTING_ACTION_COUNT).contains(&index)
        || (EXTRA_KNOBS_START..=USB_PLAYBACK).contains(&index)
}

pub fn is_continuous(index: usize) -> bool {
    continuous_action(index)
}

struct Table {
    keys: [u16; ACTION_COUNT],
    learning: Option<usize>,
    /// Bumped on every change the page has to repaint.
    version: u32,
    unsaved: bool,
    last: u16,
    notice: &'static str,
    retry_save_us: i64,
}

static TABLE: Mutex<CriticalSectionRawMutex, RefCell<Table>> = Mutex::new(RefCell::new(Table {
    keys: [0; ACTION_COUNT],
    learning: None,
    version: 1,
    unsaved: false,
    last: 0,
    notice: "",
    retry_save_us: 0,
}));

pub fn load() {
    let migrate = match crate::settings::load_midi_json() {
        Ok(Some(json)) => {
            match keys_from_json(&json) {
                Ok(keys) => TABLE.lock(|t| {
                    let mut t = t.borrow_mut();
                    t.keys = keys;
                    t.version += 1;
                }),
                Err(err) => report(err),
            }
            return;
        }
        Ok(None) => true,
        Err(err) => {
            report(err);
            false
        }
    };
    if let Some(keys) = crate::settings::load_midi(ACTION_COUNT.min(256)) {
        TABLE.lock(|t| {
            let mut t = t.borrow_mut();
            for (slot, key) in t.keys.iter_mut().zip(keys) {
                *slot = key;
            }
            t.version += 1;
            t.unsaved = migrate;
        });
    }
}

pub fn report(message: &'static str) {
    log::warn!("MIDI configuration: {message}");
    TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        t.notice = message;
        t.version += 1;
    });
}

pub struct Row {
    pub name: String,
    pub assigned: String,
    pub learning: bool,
}

pub fn version() -> u32 {
    TABLE.lock(|t| t.borrow().version)
}

pub fn rows() -> alloc::vec::Vec<Row> {
    TABLE.lock(|t| {
        let t = t.borrow();
        (0..ACTION_COUNT)
            .map(|i| Row {
                name: if continuous_action(i) {
                    format!("{} (CC)", action_name(i))
                } else {
                    action_name(i)
                },
                assigned: key_text(t.keys[i]),
                learning: t.learning == Some(i),
            })
            .collect()
    })
}

/// Last message seen, for the page's status line.
pub fn last_text() -> String {
    TABLE.lock(|t| {
        let t = t.borrow();
        if t.notice.is_empty() {
            key_text(t.last)
        } else {
            String::from(t.notice)
        }
    })
}

/// Starts waiting for a message for `index`; tapping the same row again cancels.
pub fn toggle_learn(index: usize) {
    if index >= ACTION_COUNT {
        return;
    }
    TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        t.learning = if t.learning == Some(index) {
            None
        } else {
            Some(index)
        };
        t.notice = "";
        t.version += 1;
    });
}

pub fn cancel_learn() {
    TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        if t.learning.take().is_some() {
            t.version += 1;
        }
    });
}

pub fn clear(index: usize) {
    if index >= ACTION_COUNT {
        return;
    }
    TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        if t.keys[index] != 0 {
            t.keys[index] = 0;
            t.unsaved = true;
            t.retry_save_us = 0;
        }
        t.learning = None;
        t.version += 1;
    });
}

pub fn clear_all() {
    TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        t.unsaved |= t.keys.iter().any(|key| *key != 0);
        t.keys.fill(0);
        t.learning = None;
        t.last = 0;
        t.notice = "";
        t.retry_save_us = 0;
        t.version += 1;
    });
    crate::link::clear_midi();
}

#[cfg(test)]
pub fn export_json() -> Result<String, &'static str> {
    let keys = TABLE.lock(|t| t.borrow().keys);
    backup::encode(&keys)
}

pub fn json_for_keys(keys: &[u16]) -> Result<String, &'static str> {
    let keys = keys.try_into().map_err(|_| "Invalid MIDI action count")?;
    backup::encode(keys)
}

pub fn keys_from_json(json: &str) -> Result<[u16; ACTION_COUNT], &'static str> {
    backup::decode(json)
}

#[cfg(test)]
pub fn import_json(json: &str) -> Result<(), &'static str> {
    let keys = keys_from_json(json)?;
    crate::settings::save_midi_json(json)?;
    TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        t.keys = keys;
        t.unsaved = false;
        t.learning = None;
        t.last = 0;
        t.notice = "";
        t.retry_save_us = 0;
        t.version += 1;
    });
    crate::link::clear_midi();
    Ok(())
}

/// Writes changed bindings to flash. Called from the UI task.
pub fn persist() {
    let now = crate::clock::now_us();
    let keys = TABLE.lock(|t| {
        let t = t.borrow();
        (t.unsaved && now >= t.retry_save_us).then_some(t.keys)
    });
    if let Some(keys) = keys {
        let saved = crate::settings::save_midi(&keys);
        TABLE.lock(|t| {
            let mut t = t.borrow_mut();
            if saved && t.keys == keys {
                t.unsaved = false;
                if t.notice == "Bindings not saved; retrying." {
                    t.notice = "";
                    t.version += 1;
                }
            } else if !saved {
                t.notice = "Bindings not saved; retrying.";
                t.retry_save_us = now + 5_000_000;
                t.version += 1;
            }
        });
    }
}

/// Parses one BLE-MIDI packet: a header byte, then timestamp-prefixed messages.
pub fn feed(packet: &[u8]) {
    let mut status = 0u8;
    let mut data = [0u8; 2];
    let mut have = 0usize;
    let mut want_ts = true;
    let mut in_sysex = false;
    for &b in packet.iter().skip(1) {
        if in_sysex {
            if b == 0xf7 {
                in_sysex = false;
                want_ts = false;
            }
            continue;
        }
        if b & 0x80 != 0 {
            if want_ts {
                want_ts = false;
                continue;
            }
            if b == 0xf0 {
                in_sysex = true;
                status = 0;
            } else if b < 0xf8 {
                status = b;
                have = 0;
            }
            continue;
        }
        if status == 0 {
            continue;
        }
        data[have] = b;
        have += 1;
        let need = if matches!(status & 0xf0, 0xc0 | 0xd0) {
            1
        } else {
            2
        };
        if have == need {
            have = 0;
            want_ts = true;
            dispatch(status, data);
        }
    }
}

fn dispatch(status: u8, data: [u8; 2]) {
    let channel = status & 0x0f;
    let msg = match status & 0xf0 {
        0x90 if data[1] > 0 => Msg {
            kind: Kind::Note,
            channel,
            number: data[0],
            value: data[1],
        },
        0xb0 => Msg {
            kind: Kind::Cc,
            channel,
            number: data[0],
            value: data[1],
        },
        0xc0 => Msg {
            kind: Kind::Program,
            channel,
            number: data[0],
            value: 0,
        },
        _ => return,
    };
    let key = msg.key();
    let keys = TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        if t.last != key {
            t.last = key;
            t.version += 1;
        }
        if let Some(index) = t.learning.take() {
            t.version += 1;
            if continuous_action(index) && msg.kind != Kind::Cc {
                t.learning = Some(index);
                t.notice = "This parameter needs a MIDI CC.";
                log::warn!("MIDI parameter learn requires CC");
                return None;
            }
            t.keys[index] = key;
            t.unsaved = true;
            t.retry_save_us = 0;
            t.notice = "";
            return None;
        }
        Some(t.keys)
    });
    if let Some(keys) = keys {
        let actions = keys.iter().enumerate().filter_map(|(index, assigned)| {
            (*assigned == key
                && (!continuous_action(index) || msg.kind == Kind::Cc)
                && (continuous_action(index) || msg.kind != Kind::Cc || msg.value > 0))
                .then_some(index)
        });
        if !crate::link::push_midi(actions, msg.value, msg.kind == Kind::Cc) {
            TABLE.lock(|t| {
                let mut t = t.borrow_mut();
                t.notice = "MIDI actions not queued; check connection/load.";
                t.version += 1;
            });
        }
    }
}

/// Called on the Nano link task, so snapshots and writes share its ordering.
pub fn command(index: usize, value: u8, continuous: bool) -> Option<Cmd> {
    if index >= ACTION_COUNT || (continuous_action(index) && !continuous) {
        log::warn!("Unsupported MIDI action {index}");
        return None;
    }
    if index < BLOCKS {
        let mut fx = crate::fx::types::NanoFxState::zeroed();
        if !crate::fx::nano_fx_snapshot(&mut fx) || !fx.blocks[index].valid {
            log::warn!("MIDI FX toggle needs current block state");
            return None;
        }
        Some(Cmd::Block {
            slot: index as u8,
            on: !fx.blocks[index].on,
        })
    } else if index < BLOCKS + TRIGGERS {
        match index - BLOCKS {
            0 => {
                let mut fx = crate::fx::types::NanoFxState::zeroed();
                if !crate::fx::nano_fx_snapshot(&mut fx) || !fx.gate.on_known {
                    log::warn!("MIDI gate toggle needs current gate state");
                    return None;
                }
                Some(Cmd::Gate(!fx.gate.on))
            }
            1 => Some(Cmd::TunerToggle),
            2 => Some(Cmd::Tempo),
            3 => Some(Cmd::Preset(1)),
            4 => Some(Cmd::Preset(-1)),
            5 => Some(Cmd::Capture(1)),
            6 => Some(Cmd::Capture(-1)),
            _ => Some(Cmd::Save),
        }
    } else if index < LEGACY_ACTION_COUNT {
        let n = index - BLOCKS - TRIGGERS;
        knob_command(n / KNOBS_PER_BLOCK, n % KNOBS_PER_BLOCK, value)
    } else if index < LEGACY_ACTION_COUNT + AMP_KNOBS {
        let knob = (index - LEGACY_ACTION_COUNT) as u8;
        Some(Cmd::AmpKnob {
            knob,
            raw: scale_midi_value(value),
        })
    } else if index == LEGACY_ACTION_COUNT + AMP_KNOBS {
        Some(Cmd::CaptureVolume(scale_midi_value(value)))
    } else if index < EXISTING_ACTION_COUNT {
        let which = match index - LEGACY_ACTION_COUNT - AMP_KNOBS - 1 {
            0 => nano_wire::editor::CabFloat::Level,
            1 => nano_wire::editor::CabFloat::HighPass,
            _ => nano_wire::editor::CabFloat::LowPass,
        };
        Some(Cmd::CabParam {
            which,
            normalized: f32::from(value) / 127.0,
        })
    } else if index < GATE_AMOUNT {
        let n = index - EXTRA_KNOBS_START;
        knob_command(n / EXTRA_KNOBS, n % EXTRA_KNOBS + KNOBS_PER_BLOCK, value)
    } else if index == GATE_AMOUNT {
        Some(Cmd::GateAmount(((u16::from(value) * 100 + 63) / 127) as u8))
    } else if index == USB_PLAYBACK {
        Some(Cmd::UsbPlayback(-40.0 + f32::from(value) * 40.0 / 127.0))
    } else if index < PRESET_START {
        let n = index - MODEL_START;
        Some(Cmd::Model {
            block: (n / 2) as u8,
            delta: if n % 2 == 0 { 1 } else { -1 },
        })
    } else if index < CAPTURE_START {
        Some(Cmd::PresetAt((index - PRESET_START + 1) as u8))
    } else if index < MUTE_START {
        Some(Cmd::CaptureAt((index - CAPTURE_START + 1) as u8))
    } else if index < BANK_START {
        let n = index - MUTE_START;
        if n < 4 {
            Some(Cmd::Mute {
                input: n < 2,
                muted: n % 2 == 0,
            })
        } else {
            Some(Cmd::TunerOutputMute(n == 4))
        }
    } else if index < BANK_START + 2 {
        Some(Cmd::BankStep(if index == BANK_START { 1 } else { -1 }))
    } else if index < BANK_START + 6 {
        Some(Cmd::BankSlot((index - BANK_START - 2) as u8))
    } else if index < SCENE_START {
        Some(Cmd::BankAt((index - BANK_START - 6) as u8))
    } else if index < FX_PRESET_START {
        Some(Cmd::SceneRecall((index - SCENE_START) as u8))
    } else if index < GIG_START {
        let offset = index - FX_PRESET_START;
        Some(Cmd::FxPresetRecall { index: (offset / BLOCKS) as u8, block: (offset % BLOCKS) as u8 })
    } else {
        Some(Cmd::Gig(index == GIG_START))
    }
}

fn scale_midi_value(value: u8) -> u8 {
    ((u16::from(value.min(127)) * u16::from(u8::MAX) + 63) / 127) as u8
}

fn knob_command(block: usize, cell: usize, value: u8) -> Option<Cmd> {
    let mut fx = crate::fx::types::NanoFxState::zeroed();
    if !crate::fx::nano_fx_snapshot(&mut fx) {
        log::warn!("MIDI FX parameter needs current FX state");
        return None;
    }
    let b = fx.blocks[block];
    let Some(model) = (unsafe { b.model.as_ref() }) else {
        log::warn!("MIDI FX parameter needs a known model");
        return None;
    };
    if cell >= b.param_count as usize {
        log::warn!("MIDI FX knob {} unavailable on block {block}", cell + 1);
        return None;
    }
    let wire = crate::fx::nano_fx_param_wire_index(model, cell);
    if wire >= b.param_count as usize {
        log::warn!("MIDI FX parameter wire index unavailable");
        return None;
    }
    let norm = crate::fx::params::normalize_for_model(model, wire, f32::from(value) / 127.0);
    Some(Cmd::Param(crate::link::ParamWrite {
        block: block as u8,
        wire: wire as u8,
        norm,
    }))
}
