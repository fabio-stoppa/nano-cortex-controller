//! MIDI notify worker — drains BLE-MIDI packets off the Bluedroid callback.

use core::ffi::c_void;
use core::mem::{size_of, MaybeUninit};
use core::ptr;

use esp_idf_svc::sys::*;
use log::{error, info, warn};

use super::ctx::*;
use super::map;
use super::midi::{self, MidiMsg};

pub(super) extern "C" fn midi_worker_task(_arg: *mut c_void) {
    let s = ctx();
    let mut slot = MaybeUninit::<MidiItem>::uninit();
    let mut msgs = [MidiMsg::Pc {
        channel: 0,
        program: 0,
    }; 8];

    loop {
        if s.queue.is_null()
            || unsafe { xQueueReceive(s.queue, slot.as_mut_ptr().cast(), ms_to_ticks(50)) }
                != PD_TRUE
        {
            continue;
        }
        let item = unsafe { slot.assume_init_ref() };
        let kind = PeerKind::from_u8(item.kind);
        let len = (item.len as usize).min(MIDI_PKT_MAX);
        let n = midi::parse_ble_midi(&item.data[..len], &mut msgs);
        for msg in msgs.iter().take(n) {
            map::apply_msg(*msg, kind);
        }
    }
}

pub(super) fn queue_init(s: &mut Ctx) -> bool {
    if !s.queue.is_null() {
        return true;
    }

    s.queue = unsafe {
        xQueueGenericCreate(MIDI_QUEUE_LEN, size_of::<MidiItem>() as UBaseType_t, QUEUE_TYPE_BASE)
    };
    if s.queue.is_null() {
        error!(target: TAG, "MIDI queue create failed");
        return false;
    }

    let created = unsafe {
        xTaskCreatePinnedToCore(
            Some(midi_worker_task),
            c"midi_fs".as_ptr(),
            MIDI_TASK_STACK,
            ptr::null_mut(),
            MIDI_TASK_PRIO,
            &mut s.task,
            NO_AFFINITY,
        )
    };
    if created != PD_PASS {
        error!(target: TAG, "MIDI worker task create failed");
        unsafe { vQueueDelete(s.queue) };
        s.queue = ptr::null_mut();
        s.task = ptr::null_mut();
        return false;
    }

    info!(target: TAG, "MIDI FX worker ready (queue={MIDI_QUEUE_LEN})");
    true
}

pub(super) fn enqueue_notify(s: &mut Ctx, kind: PeerKind, data: &[u8]) {
    if s.queue.is_null() || data.is_empty() {
        return;
    }
    let copy_len = data.len().min(MIDI_PKT_MAX);
    let mut item = MidiItem {
        len: copy_len as u16,
        kind: kind.as_u8(),
        data: [0; MIDI_PKT_MAX],
    };
    item.data[..copy_len].copy_from_slice(&data[..copy_len]);
    if unsafe {
        xQueueGenericSend(
            s.queue,
            (&raw const item).cast::<c_void>(),
            0,
            QUEUE_SEND_TO_BACK,
        )
    } != PD_TRUE
    {
        warn!(target: TAG, "MIDI RX queue full — drop");
    }
}

pub(super) fn drain_queue(s: &mut Ctx) {
    if !s.queue.is_null() {
        unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
    }
}
