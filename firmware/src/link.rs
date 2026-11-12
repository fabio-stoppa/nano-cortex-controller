//! Outbound C304 packets and screen commands for the Bluetooth task.
//!
//! The preset machine and the touch task both enqueue. Only the link task
//! sends, so the controller writes stay on one task.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ffi::CStr;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

use nano_wire::editor::CabFloat;
use nano_wire::editor::ExpressionAssignments;
use nano_wire::types::JackMode;

static UP: AtomicBool = AtomicBool::new(false);

struct QueuedWrite {
    data: Vec<u8>,
    live: bool,
    label: Option<String>,
}

static OUT: Mutex<CriticalSectionRawMutex, RefCell<VecDeque<QueuedWrite>>> =
    Mutex::new(RefCell::new(VecDeque::new()));

static CMDS: Mutex<CriticalSectionRawMutex, RefCell<VecDeque<Cmd>>> =
    Mutex::new(RefCell::new(VecDeque::new()));

pub static COMMAND_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

static WRITE_QUEUE_PEAK: AtomicU32 = AtomicU32::new(0);
static COMMAND_QUEUE_PEAK: AtomicU32 = AtomicU32::new(0);

pub(crate) const COMMAND_CAPACITY: usize = 512;

fn record_peak(peak: &AtomicU32, len: usize) {
    let len = len.min(u32::MAX as usize) as u32;
    let mut current = peak.load(Ordering::Relaxed);
    while len > current {
        match peak.compare_exchange_weak(current, len, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

pub fn reset_queue_peaks() {
    WRITE_QUEUE_PEAK.store(0, Ordering::Relaxed);
    COMMAND_QUEUE_PEAK.store(0, Ordering::Relaxed);
}

pub fn queue_peaks() -> (u32, u32) {
    (
        WRITE_QUEUE_PEAK.load(Ordering::Relaxed),
        COMMAND_QUEUE_PEAK.load(Ordering::Relaxed),
    )
}

pub enum Cmd {
    Gig(bool),
    SceneRecallAt {
        preset: u8,
        scene: u8,
    },
    SceneRecall(u8),
    SceneCapture {
        preset: u8,
        scene: u8,
        name: alloc::string::String,
    },
    SceneRefresh,
    SceneFxRefresh,
    SceneCab {
        slot: u8,
        name: String,
    },
    FxModelId {
        block: u8,
        id: Vec<u8>,
    },
    Gate(bool),
    GateAmount(u8),
    Tuner(bool),
    TunerToggle,
    MidiAction {
        index: usize,
        value: u8,
        continuous: bool,
    },
    Param(ParamWrite),
    TunerReference(u16),
    TunerOutputMute(bool),
    /// TAP from the header; also enters Tempo Mode.
    Tempo,
    Focus(u8),
    Block {
        slot: u8,
        on: bool,
    },
    /// Step the catalog model loaded in an effect block.
    Model {
        block: u8,
        delta: i8,
    },
    /// Load the Nth allowed catalog model into an effect block.
    ModelAt {
        block: u8,
        index: u8,
    },
    Save,
    Copy,
    Library,
    #[allow(dead_code)] // reserved for MIDI-controller stepping
    Preset(i8),
    Capture(i8),
    /// Select factory capture slot 1..=25.
    CaptureAt(u8),
    PresetAt(u8),
    PresetRename {
        slot: u8,
        name: alloc::string::String,
    },
    PresetDelete(u8),
    /// Copy (or move, when `remove_src`) a preset to another slot.
    PresetTransfer {
        src: u8,
        dst: u8,
        remove_src: bool,
    },
    /// Load the Nth library IR into the active cab slot.
    CabAt(u16),
    CabParam {
        which: CabFloat,
        normalized: f32,
    },
    AmpKnob {
        knob: u8,
        raw: u8,
    },
    CaptureVolume(u8),
    ExpressionSave {
        slot: u8,
        assignments: ExpressionAssignments,
    },
    ToolsOpen,
    JackMode(JackMode),
    UsbPlayback(f32),
    /// Mute guitar input (true) or output (false).
    Mute {
        input: bool,
        muted: bool,
    },
    ExpressionCalibrationStart,
    ExpressionCalibrationSave,
}

#[derive(Clone, Copy)]
pub struct ParamWrite {
    pub block: u8,
    pub wire: u8,
    pub norm: f32,
}

static PARAM: Mutex<CriticalSectionRawMutex, RefCell<Option<ParamWrite>>> =
    Mutex::new(RefCell::new(None));

pub fn set_up(up: bool) {
    UP.store(up, Ordering::Release);
    if !up {
        crate::scenes::disconnected();
        OUT.lock(|q| q.borrow_mut().clear());
        CMDS.lock(|q| q.borrow_mut().clear());
        PARAM.lock(|p| *p.borrow_mut() = None);
        COMMAND_READY.reset();
    }
}

pub fn connected() -> bool {
    UP.load(Ordering::Acquire)
}

#[allow(dead_code)]
pub fn write(data: &[u8]) -> bool {
    write_inner(None, data)
}

fn write_inner(label: Option<&CStr>, data: &[u8]) -> bool {
    if data.is_empty() || !connected() {
        return false;
    }
    OUT.lock(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 8 {
            log::warn!("C304 queue full; write refused");
            return false;
        }
        let live = nano_wire::editor::is_live_value_write(data);
        queue.push_back(QueuedWrite {
            data: data.to_vec(),
            live,
            label: label
                .and_then(|label| label.to_str().ok())
                .map(String::from),
        });
        record_peak(&WRITE_QUEUE_PEAK, queue.len());
        true
    })
}

pub fn write_labeled(label: &CStr, data: &[u8]) -> bool {
    write_inner(Some(label), data)
}

#[allow(dead_code)]
pub fn pop_write() -> Option<Vec<u8>> {
    pop_write_entry(crate::scenes::busy()).map(|(data, _)| data)
}

#[allow(dead_code)]
pub fn pop_write_with_scene_busy(scene_busy: bool) -> Option<Vec<u8>> {
    pop_write_entry(scene_busy).map(|(data, _)| data)
}

pub fn pop_write_entry(scene_busy: bool) -> Option<(Vec<u8>, Option<String>)> {
    OUT.lock(|queue| {
        let mut queue = queue.borrow_mut();
        let selected = if scene_busy {
            0
        } else {
            queue.iter().position(|write| write.live).unwrap_or(0)
        };
        queue
            .remove(selected)
            .map(|write| (write.data, write.label))
    })
}

pub fn push_cmd(cmd: Cmd) {
    if !connected() && !matches!(&cmd, Cmd::Gig(_)) {
        log::warn!("Controller command refused while Nano disconnected");
        return;
    }
    CMDS.lock(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= COMMAND_CAPACITY {
            log::warn!("Controller command queue full; command refused");
            return;
        }
        queue.push_back(cmd);
        record_peak(&COMMAND_QUEUE_PEAK, queue.len());
        COMMAND_READY.signal(());
    });
}

/// Enqueue a whole learned message, or refuse it without evicting older actions.
pub fn push_midi(actions: impl Iterator<Item = usize>, value: u8, continuous: bool) -> bool {
    let actions: Vec<_> = actions.collect();
    if actions.is_empty() {
        return true;
    }
    if !connected() {
        log::warn!("MIDI actions refused while Nano disconnected");
        return false;
    }
    CMDS.lock(|queue| {
        let mut queue = queue.borrow_mut();
        let start = queue
            .iter()
            .rposition(|cmd| {
                !matches!(cmd,
            Cmd::MidiAction { index, continuous: true, .. } if crate::midi::is_continuous(*index))
            })
            .map_or(0, |index| index + 1);
        let replaceable = |index: usize, cmd: &Cmd| {
            matches!(cmd,
            Cmd::MidiAction { index: pending, continuous: true, .. }
                if continuous && crate::midi::is_continuous(index) && *pending == index)
        };
        let additional = actions
            .iter()
            .filter(|&&index| !queue.iter().skip(start).any(|cmd| replaceable(index, cmd)))
            .count();
        if queue.len() + additional > COMMAND_CAPACITY {
            log::warn!("MIDI group exceeds command queue; entire message refused");
            return false;
        }
        for index in actions {
            let cmd = Cmd::MidiAction {
                index,
                value,
                continuous,
            };
            if let Some(pending) = queue
                .iter_mut()
                .skip(start)
                .find(|cmd| replaceable(index, cmd))
            {
                *pending = cmd;
            } else {
                queue.push_back(cmd);
            }
        }
        record_peak(&COMMAND_QUEUE_PEAK, queue.len());
        COMMAND_READY.signal(());
        true
    })
}

#[allow(dead_code)]
pub fn pop_cmd() -> Option<Cmd> {
    CMDS.lock(|queue| queue.borrow_mut().pop_front())
}

fn is_scene_recall(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::SceneRecall(_) | Cmd::SceneRecallAt { .. } => true,
        Cmd::MidiAction { index, .. } => crate::midi::scene_index(*index).is_some(),
        _ => false,
    }
}

pub fn pop_cmd_for_scene_busy(scene_busy: bool) -> Option<Cmd> {
    CMDS.lock(|queue| {
        let mut queue = queue.borrow_mut();
        if scene_busy && !queue.front().is_some_and(is_scene_recall) {
            return None;
        }
        let mut command = queue.pop_front()?;
        if is_scene_recall(&command) {
            // Never coalesce across another action (especially a preset change or save).
            while queue.front().is_some_and(is_scene_recall) {
                command = queue.pop_front()?;
            }
        }
        Some(command)
    })
}

pub fn clear_midi() {
    CMDS.lock(|queue| {
        queue
            .borrow_mut()
            .retain(|cmd| !matches!(cmd, Cmd::MidiAction { .. }))
    });
}

/// Latest effect-cell drag. Moves overwrite each other so a sweep does not fill the command queue.
pub fn set_param(block: u8, wire: u8, norm: f32) {
    if !connected() {
        log::warn!("Parameter edit refused while Nano disconnected");
        return;
    }
    PARAM.lock(|slot| {
        *slot.borrow_mut() = Some(ParamWrite { block, wire, norm });
    });
    COMMAND_READY.signal(());
}

pub fn take_param() -> Option<ParamWrite> {
    PARAM.lock(|slot| slot.borrow_mut().take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_peak_only_tracks_the_high_water_mark() {
        let peak = AtomicU32::new(0);
        record_peak(&peak, 3);
        record_peak(&peak, 1);
        record_peak(&peak, 5);
        assert_eq!(peak.load(Ordering::Relaxed), 5);
    }
}
