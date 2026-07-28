//! C305 RX worker task: queue init, pitch/tuner-hot classifiers, notify
//! ingestion, and the BPM GATT-read hunt sequencer.

use super::*;
use super::{ctx::*, gatt::*};

pub(super) unsafe extern "C" fn c305_worker_task(_arg: *mut c_void) {
    let s = ctx();
    let mut slot = MaybeUninit::<C305Item>::uninit();

    loop {
        let tuner_live = is_tuner_active();

        // Prefer the live pitch/mode mailbox whenever it has a frame.
        let from_live = !s.live_queue.is_null()
            && unsafe { xQueueReceive(s.live_queue, slot.as_mut_ptr().cast(), 0) } == PD_TRUE;

        if from_live {
            if tuner_live && !s.queue.is_null() && unsafe { uxQueueMessagesWaiting(s.queue) } > 0 {
                unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
            }
        } else if tuner_live {
            /*
             * While tuning, NEVER drain the normal FIFO — it holds ~4s of stale
             * traffic and replays behind live pitch.
             */
            if !s.queue.is_null() && unsafe { uxQueueMessagesWaiting(s.queue) } > 0 {
                unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
            }
            unsafe { ulTaskGenericNotifyTake(NOTIFY_INDEX, PD_TRUE, ms_to_ticks(20)) };
            continue;
        } else if s.queue.is_null()
            || unsafe { xQueueReceive(s.queue, slot.as_mut_ptr().cast(), 0) } != PD_TRUE
        {
            unsafe { ulTaskGenericNotifyTake(NOTIFY_INDEX, PD_TRUE, ms_to_ticks(20)) };
            continue;
        }

        let item = unsafe { slot.assume_init_ref() };
        let c305 = C305_HANDLE.load(Ordering::Acquire);
        if c305 == INVALID_HANDLE || item.handle != c305 {
            debug!(
                target: TAG,
                "GATT RX {} h={} len={}",
                handle_label(s, item.handle).unwrap_or("other"),
                item.handle,
                item.len
            );
        }

        let len = (item.len as usize).min(C305_PKT_MAX);
        crate::state::feed_c305_bytes(&item.data[..len]);
    }
}

pub(super) fn c305_queue_init(s: &mut Ctx) -> bool {
    if !s.queue.is_null() {
        return true;
    }

    let item_size = size_of::<C305Item>() as UBaseType_t;
    s.queue = unsafe { xQueueGenericCreate(C305_QUEUE_LEN, item_size, QUEUE_TYPE_BASE) };
    if s.queue.is_null() {
        error!(target: TAG, "C305 queue create failed");
        return false;
    }

    s.live_queue = unsafe { xQueueGenericCreate(C305_LIVE_QUEUE_LEN, item_size, QUEUE_TYPE_BASE) };
    if s.live_queue.is_null() {
        error!(target: TAG, "C305 live queue create failed");
        unsafe { vQueueDelete(s.queue) };
        s.queue = ptr::null_mut();
        return false;
    }

    let created = unsafe {
        xTaskCreatePinnedToCore(
            Some(c305_worker_task),
            c"c305_rx".as_ptr(),
            C305_TASK_STACK,
            ptr::null_mut(),
            C305_TASK_PRIO,
            &mut s.task,
            NO_AFFINITY,
        )
    };
    if created != PD_PASS {
        error!(target: TAG, "C305 worker task create failed");
        unsafe {
            vQueueDelete(s.live_queue);
            vQueueDelete(s.queue);
        }
        s.live_queue = ptr::null_mut();
        s.queue = ptr::null_mut();
        s.task = ptr::null_mut();
        return false;
    }

    info!(
        target: TAG,
        "C305 RX worker ready (queue={C305_QUEUE_LEN} live={C305_LIVE_QUEUE_LEN}) \
         build=2026-07-26-rust-ble"
    );
    true
}

pub(super) fn c305_is_pitch(data: &[u8]) -> bool {
    if data.len() < 5 {
        return false;
    }
    data.windows(5).any(|w| {
        w[0] == 0xC0
            && w[1] == 0x08
            && w[2] == 0x01
            && w[3] == 0x22
            && (w[4] == 0x01 || w[4] == 0x02)
    })
}

/// Pitch (22/01|02) or tuner mode (2D / gain+2D) — keep these on the live path.
/// Bulk FX param readback (`C0 08 06 22 …`) — fast-path off the C305 FIFO so
/// short 1–2 param replies are not dropped behind preset dump chunks.
pub(super) fn c305_is_fx_param_refresh(data: &[u8]) -> bool {
    if data.len() < 10 {
        return false;
    }
    data.windows(5).any(|w| {
        w[0] == 0xC0 && w[1] == 0x08 && w[2] == 0x06 && w[3] == 0x22
    })
}

pub(super) fn c305_is_tuner_hot(data: &[u8]) -> bool {
    if data.len() < 5 {
        return false;
    }
    data.windows(5).any(|w| {
        if w[0] != 0xC0 || w[1] != 0x08 || w[2] != 0x01 {
            return false;
        }
        match w[3] {
            0x22 => w[4] == 0x01 || w[4] == 0x02,
            0x2D | 0x20 => true,
            _ => false,
        }
    })
}

pub(super) fn notify_worker(s: &Ctx) {
    if !s.task.is_null() {
        unsafe {
            xTaskGenericNotify(
                s.task,
                NOTIFY_INDEX,
                0,
                eNotifyAction_eIncrement,
                ptr::null_mut(),
            )
        };
    }
}

pub(super) fn on_gatt_notify(s: &mut Ctx, handle: u16, data: *const u8, len: u16) {
    if len == 0 || s.queue.is_null() {
        return;
    }
    let Some(bytes) = (unsafe {
        crate::memsafe::bytes_truncated(data, len as usize, C305_PKT_MAX)
    }) else {
        return;
    };

    let tuner_live = is_tuner_active();
    let c305 = C305_HANDLE.load(Ordering::Acquire);
    let is_c305 = c305 != INVALID_HANDLE && handle == c305;
    let is_c306 = s.c306_handle != INVALID_HANDLE && handle == s.c306_handle;

    if !is_c305 && !is_c306 {
        debug!(
            target: TAG,
            "GATT notify h={handle} ({}) len={len}",
            handle_label(s, handle).unwrap_or("?")
        );
    }

    // C306 mirrors C305 — skip the queue, but still apply FX param readbacks
    // (short replies sometimes only show up on the indicate mirror).
    if is_c306 {
        if !tuner_live && c305_is_fx_param_refresh(bytes) {
            crate::state::feed_c305_bytes(bytes);
        }
        return;
    }

    /*
     * Pitch: apply in the notify callback with no logging and never enqueue.
     * Falling through to the 48-deep FIFO replayed ~4s of stale pitch
     * (48 × ~80 ms). Logging in this path also stalls Bluedroid the same way.
     */
    if is_c305 && c305_is_pitch(bytes) {
        crate::tuner::feed_pitch_fast_bytes(bytes);
        return;
    }

    /*
     * Amp knobs / capture volume: same problem — dump traffic fills the FIFO and
     * LEVELHUNT-style INFO logging stalled the worker, so the UI froze then caught
     * up seconds later. Apply live, skip the queue.
     */
    if is_c305 && !tuner_live && crate::state::try_amp_notify_fast_bytes(bytes) {
        return;
    }

    if is_c305 && !tuner_live && c305_is_fx_param_refresh(bytes) {
        crate::state::feed_c305_bytes(bytes);
        return;
    }

    if (len as usize) > C305_PKT_MAX {
        warn!(
            target: TAG,
            "C305 notify truncated {} → {C305_PKT_MAX} bytes",
            len
        );
    }
    let copy_len = bytes.len();
    let mut item = C305Item {
        handle,
        len: copy_len as u16,
        data: [0; C305_PKT_MAX],
    };
    item.data[..copy_len].copy_from_slice(bytes);
    let item_ptr = (&raw const item).cast::<c_void>();

    /*
     * Mode frames use the length-1 overwrite mailbox.
     * While the tuner is live, drop everything else on C305.
     */
    if is_c305 && c305_is_tuner_hot(bytes) && !s.live_queue.is_null() {
        if unsafe { xQueueGenericSend(s.live_queue, item_ptr, 0, QUEUE_OVERWRITE) } == PD_PASS {
            notify_worker(s);
        } else {
            s.drop_count += 1;
        }
        return;
    }

    if tuner_live && is_c305 {
        return;
    }

    if unsafe { xQueueGenericSend(s.queue, item_ptr, 0, QUEUE_SEND_TO_BACK) } != PD_TRUE {
        let mut dropped = MaybeUninit::<C305Item>::uninit();
        if unsafe { xQueueReceive(s.queue, dropped.as_mut_ptr().cast(), 0) } == PD_TRUE {
            s.drop_count += 1;
            unsafe { xQueueGenericSend(s.queue, item_ptr, 0, QUEUE_SEND_TO_BACK) };
        }
        if s.drop_count % 8 == 1 {
            warn!(target: TAG, "C305 RX queue overflow (drops={})", s.drop_count);
        }
    } else {
        notify_worker(s);
    }
}

pub(super) fn drain_rx_queue(s: &mut Ctx) {
    if !s.queue.is_null() {
        unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
    }
    if !s.live_queue.is_null() {
        unsafe { xQueueGenericReset(s.live_queue, PD_FALSE) };
    }
    s.drop_count = 0;
}

pub(super) fn bpm_gatt_read_next(s: &mut Ctx) {
    while is_connected(s) && s.bpm_read_next < s.bpm_read_count {
        let handle = s.bpm_read_handles[s.bpm_read_next];
        info!(
            target: TAG,
            "BPM hunt: GATT read handle={handle} ({}/{})",
            s.bpm_read_next + 1,
            s.bpm_read_count
        );
        let err = unsafe {
            esp_ble_gattc_read_char(
                s.gattc_if,
                s.conn_id,
                handle,
                esp_gatt_auth_req_t_ESP_GATT_AUTH_REQ_NONE,
            )
        };
        if err == ESP_OK {
            return;
        }
        warn!(target: TAG, "BPM hunt: read_char failed: {}", err_name(err));
        s.bpm_read_next += 1;
    }
}
