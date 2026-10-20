//! Outbound C304 packets and screen commands for the Bluetooth task.
//!
//! The preset machine and the touch task both enqueue. Only the link task
//! sends, so the controller writes stay on one task.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ffi::CStr;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;

static UP: AtomicBool = AtomicBool::new(false);

static OUT: Mutex<CriticalSectionRawMutex, RefCell<VecDeque<Vec<u8>>>> =
    Mutex::new(RefCell::new(VecDeque::new()));

static CMDS: Mutex<CriticalSectionRawMutex, RefCell<VecDeque<Cmd>>> =
    Mutex::new(RefCell::new(VecDeque::new()));

pub enum Cmd {
    Gate(bool),
    Tuner(bool),
    /// TAP while Tempo Mode is already open.
    Tempo,
    TempoEnter,
    TempoExit,
    Focus(u8),
    Block { slot: u8, on: bool },
    /// Step the catalog model loaded in an effect block.
    Model { block: u8, delta: i8 },
    Save,
    Copy,
    Library,
    Preset(i8),
    Capture(i8),
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
}

pub fn connected() -> bool {
    UP.load(Ordering::Acquire)
}

pub fn write(data: &[u8]) -> bool {
    if data.is_empty() || !connected() {
        return false;
    }
    OUT.lock(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 8 {
            queue.pop_front();
        }
        queue.push_back(data.to_vec());
    });
    true
}

pub fn write_labeled(_label: &CStr, data: &[u8]) -> bool {
    write(data)
}

pub fn pop_write() -> Option<Vec<u8>> {
    OUT.lock(|queue| queue.borrow_mut().pop_front())
}

pub fn push_cmd(cmd: Cmd) {
    CMDS.lock(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 8 {
            queue.pop_front();
        }
        queue.push_back(cmd);
    });
}

pub fn pop_cmd() -> Option<Cmd> {
    CMDS.lock(|queue| queue.borrow_mut().pop_front())
}

/// Latest effect-cell drag. Moves overwrite each other so a sweep does not fill the command queue.
pub fn set_param(block: u8, wire: u8, norm: f32) {
    PARAM.lock(|slot| {
        *slot.borrow_mut() = Some(ParamWrite { block, wire, norm });
    });
}

pub fn take_param() -> Option<ParamWrite> {
    PARAM.lock(|slot| slot.borrow_mut().take())
}
