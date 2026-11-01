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

pub const BLOCKS: usize = 5;
pub const KNOBS_PER_BLOCK: usize = 4;
const TRIGGERS: usize = 8;
const FX_KNOBS: usize = BLOCKS * KNOBS_PER_BLOCK;
const AMP_KNOBS: usize = 5;
const CAB_CONTROLS: usize = 3;
const LEGACY_ACTION_COUNT: usize = BLOCKS + TRIGGERS + FX_KNOBS;
pub const ACTION_COUNT: usize = LEGACY_ACTION_COUNT + AMP_KNOBS + 1 + CAB_CONTROLS;

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
    } else {
        String::from(CAB_CONTROL_NAMES[index - LEGACY_ACTION_COUNT - AMP_KNOBS - 1])
    }
}

struct Table {
    keys: [u16; ACTION_COUNT],
    learning: Option<usize>,
    /// Bumped on every change the page has to repaint.
    version: u32,
    unsaved: bool,
    last: u16,
}

static TABLE: Mutex<CriticalSectionRawMutex, RefCell<Table>> = Mutex::new(RefCell::new(Table {
    keys: [0; ACTION_COUNT],
    learning: None,
    version: 1,
    unsaved: false,
    last: 0,
}));

pub fn load() {
    if let Some(keys) = crate::settings::load_midi(ACTION_COUNT) {
        TABLE.lock(|t| {
            let mut t = t.borrow_mut();
            for (slot, key) in t.keys.iter_mut().zip(keys) {
                *slot = key;
            }
            t.version += 1;
        });
    }
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
                name: action_name(i),
                assigned: key_text(t.keys[i]),
                learning: t.learning == Some(i),
            })
            .collect()
    })
}

/// Last message seen, for the page's status line.
pub fn last_text() -> String {
    key_text(TABLE.lock(|t| t.borrow().last))
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
        }
        t.learning = None;
        t.version += 1;
    });
}

/// Writes changed bindings to flash. Called from the UI task.
pub fn persist() {
    let keys = TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        t.unsaved.then(|| {
            t.unsaved = false;
            t.keys
        })
    });
    if let Some(keys) = keys {
        crate::settings::save_midi(&keys);
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
        let need = if matches!(status & 0xf0, 0xc0 | 0xd0) { 1 } else { 2 };
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
    let action = TABLE.lock(|t| {
        let mut t = t.borrow_mut();
        t.last = key;
        t.version += 1;
        if let Some(index) = t.learning.take() {
            // A message drives one action, so move it off any previous owner.
            for slot in t.keys.iter_mut().filter(|k| **k == key) {
                *slot = 0;
            }
            t.keys[index] = key;
            t.unsaved = true;
            return None;
        }
        t.keys.iter().position(|k| *k == key)
    });
    if let Some(index) = action {
        run(index, msg);
    }
}

fn run(index: usize, msg: Msg) {
    let wanted = |current: bool| !current;
    if index < BLOCKS {
        let mut fx = crate::fx::types::NanoFxState::zeroed();
        let current = crate::fx::nano_fx_snapshot(&mut fx) && fx.blocks[index].on;
        crate::link::push_cmd(Cmd::Block {
            slot: index as u8,
            on: wanted(current),
        });
    } else if index < BLOCKS + TRIGGERS {
        match index - BLOCKS {
            0 => {
                let mut fx = crate::fx::types::NanoFxState::zeroed();
                let current = crate::fx::nano_fx_snapshot(&mut fx) && fx.gate.on;
                crate::link::push_cmd(Cmd::Gate(wanted(current)));
            }
            1 => crate::ui::midi_toggle_tuner(),
            2 => crate::link::push_cmd(Cmd::Tempo),
            3 => crate::link::push_cmd(Cmd::Preset(1)),
            4 => crate::link::push_cmd(Cmd::Preset(-1)),
            5 => crate::link::push_cmd(Cmd::Capture(1)),
            6 => crate::link::push_cmd(Cmd::Capture(-1)),
            _ => crate::link::push_cmd(Cmd::Save),
        }
    } else if index < LEGACY_ACTION_COUNT && msg.kind == Kind::Cc {
        let n = index - BLOCKS - TRIGGERS;
        set_knob(n / KNOBS_PER_BLOCK, n % KNOBS_PER_BLOCK, msg.value);
    } else if index < LEGACY_ACTION_COUNT + AMP_KNOBS && msg.kind == Kind::Cc {
        let knob = (index - LEGACY_ACTION_COUNT) as u8;
        crate::link::push_cmd(Cmd::AmpKnob {
            knob,
            raw: scale_midi_value(msg.value),
        });
    } else if index == LEGACY_ACTION_COUNT + AMP_KNOBS && msg.kind == Kind::Cc {
        crate::link::push_cmd(Cmd::CaptureVolume(scale_midi_value(msg.value)));
    } else if msg.kind == Kind::Cc {
        let which = match index - LEGACY_ACTION_COUNT - AMP_KNOBS - 1 {
            0 => nano_wire::editor::CabFloat::Level,
            1 => nano_wire::editor::CabFloat::HighPass,
            _ => nano_wire::editor::CabFloat::LowPass,
        };
        crate::link::push_cmd(Cmd::CabParam {
            which,
            normalized: f32::from(msg.value) / 127.0,
        });
    }
}

fn scale_midi_value(value: u8) -> u8 {
    ((u16::from(value.min(127)) * u16::from(u8::MAX) + 63) / 127) as u8
}

fn set_knob(block: usize, cell: usize, value: u8) {
    let mut fx = crate::fx::types::NanoFxState::zeroed();
    if !crate::fx::nano_fx_snapshot(&mut fx) {
        return;
    }
    let b = fx.blocks[block];
    let Some(model) = (unsafe { b.model.as_ref() }) else {
        return;
    };
    if cell >= b.param_count as usize {
        return;
    }
    let wire = crate::fx::nano_fx_param_wire_index(model, cell);
    if wire >= b.param_count as usize {
        return;
    }
    let norm = crate::fx::params::normalize_for_model(model, wire, f32::from(value) / 127.0);
    crate::link::set_param(block as u8, wire as u8, norm);
}
