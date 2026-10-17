//! GATTC profile for Chocolate / Chocolate Plus BLE-MIDI peers.

use core::ptr;

use esp_idf_svc::sys::*;
use log::{error, info, warn};

use super::api::maybe_resume_scan;
use super::ctx::*;
use super::worker;

#[allow(non_upper_case_globals)]
pub(crate) fn gattc_handler(
    event: esp_gattc_cb_event_t,
    gattc_if: esp_gatt_if_t,
    param: *mut esp_ble_gattc_cb_param_t,
) {
    let s = ctx();

    match event {
        esp_gattc_cb_event_t_ESP_GATTC_REG_EVT => {
            if unsafe { (*param).reg.status } == esp_gatt_status_t_ESP_GATT_OK {
                s.gattc_if = gattc_if;
                GATTC_IF.store(gattc_if as i32, core::sync::atomic::Ordering::Release);
                info!(target: TAG, "GATT client registered (footswitch)");
            } else {
                error!(
                    target: TAG,
                    "GATT register failed status={}",
                    unsafe { (*param).reg.status }
                );
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_CONNECT_EVT => {
            let connect = unsafe { (*param).connect };
            let Some(pending) = s.pending_bda else {
                return;
            };
            if pending != connect.remote_bda {
                return;
            }
            let Some(slot) = free_peer_slot(s) else {
                warn!(target: TAG, "No free MIDI peer slot — closing");
                s.pending_bda = None;
                unsafe { esp_ble_gattc_close(gattc_if, connect.conn_id) };
                maybe_resume_scan();
                return;
            };
            let kind = s.pending_kind;
            {
                let peer = &mut s.peers[slot];
                peer.clear();
                peer.kind = kind;
                peer.connected = true;
                peer.conn_id = connect.conn_id;
                peer.remote_bda = connect.remote_bda;
                peer.reset_handles();
            }
            s.pending_bda = None;
            publish_link(s);
            info!(target: TAG, "Connected to {}", kind.label());

            let mut conn = esp_ble_conn_update_params_t {
                bda: connect.remote_bda,
                min_int: 0x18, /* 30 ms — footswitch, not pitch stream */
                max_int: 0x30, /* 60 ms */
                latency: 0,
                timeout: 400,
            };
            unsafe {
                esp_ble_gap_update_conn_params(&mut conn);
                esp_ble_gattc_send_mtu_req(gattc_if, connect.conn_id);
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_OPEN_EVT => {
            let open = unsafe { (*param).open };
            if open.status != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "Open failed status={}", open.status);
                s.pending_bda = None;
                maybe_resume_scan();
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_CFG_MTU_EVT => {
            let cfg = unsafe { (*param).cfg_mtu };
            info!(target: TAG, "MTU {} (conn={})", cfg.mtu, cfg.conn_id);
            unsafe { esp_ble_gattc_search_service(gattc_if, cfg.conn_id, ptr::null_mut()) };
        }

        esp_gattc_cb_event_t_ESP_GATTC_SEARCH_RES_EVT => {
            let res = unsafe { (*param).search_res };
            let Some(peer) = peer_by_conn(s, res.conn_id) else {
                return;
            };
            if uuid128_eq(res.srvc_id.uuid, &UUID128_BLE_MIDI_SVC) {
                peer.midi_start = res.start_handle;
                peer.midi_end = res.end_handle;
                info!(
                    target: TAG,
                    "Found BLE-MIDI service handles {}-{} ({})",
                    res.start_handle,
                    res.end_handle,
                    peer.kind.label()
                );
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_SEARCH_CMPL_EVT => {
            let cmpl = unsafe { (*param).search_cmpl };
            if cmpl.status != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "Service discovery failed");
                return;
            }
            let Some(idx) = peer_idx_by_conn(s, cmpl.conn_id) else {
                return;
            };
            if !discover_midi_char(s, gattc_if, idx) {
                error!(target: TAG, "BLE-MIDI IO char missing — disconnecting");
                unsafe { esp_ble_gattc_close(gattc_if, cmpl.conn_id) };
                return;
            }
            publish_link(s);
            let peer = &mut s.peers[idx];
            let err = unsafe {
                esp_ble_gattc_register_for_notify(
                    gattc_if,
                    peer.remote_bda.as_mut_ptr(),
                    peer.midi_handle,
                )
            };
            if err != ESP_OK {
                error!(target: TAG, "register_for_notify failed: {}", err_name(err));
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_REG_FOR_NOTIFY_EVT => {
            let reg = unsafe { (*param).reg_for_notify };
            if reg.status != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "Notify registration failed");
                return;
            }

            let Some(idx) = peer_idx_by_midi_handle(s, reg.handle) else {
                error!(target: TAG, "Notify reg for unknown handle {}", reg.handle);
                return;
            };
            let peer = &s.peers[idx];
            let conn_id = peer.conn_id;

            let mut count: u16 = 1;
            let mut descr = unsafe { core::mem::zeroed::<esp_gattc_descr_elem_t>() };
            let status = unsafe {
                esp_ble_gattc_get_descr_by_char_handle(
                    gattc_if,
                    conn_id,
                    reg.handle,
                    uuid16(ESP_GATT_UUID_CHAR_CLIENT_CONFIG as u16),
                    &mut descr,
                    &mut count,
                )
            };
            if status != esp_gatt_status_t_ESP_GATT_OK || count == 0 {
                error!(target: TAG, "CCCD not found");
                return;
            }

            let mut cccd_en: u16 = 0x0001;
            unsafe {
                esp_ble_gattc_write_char_descr(
                    gattc_if,
                    conn_id,
                    descr.handle,
                    core::mem::size_of::<u16>() as u16,
                    (&raw mut cccd_en).cast::<u8>(),
                    esp_gatt_write_type_t_ESP_GATT_WRITE_TYPE_RSP,
                    esp_gatt_auth_req_t_ESP_GATT_AUTH_REQ_NONE,
                )
            };
        }

        esp_gattc_cb_event_t_ESP_GATTC_WRITE_DESCR_EVT => {
            let write = unsafe { (*param).write };
            if write.status != esp_gatt_status_t_ESP_GATT_OK {
                return;
            }
            let label = {
                let Some(peer) = peer_by_conn(s, write.conn_id) else {
                    return;
                };
                peer.notify_enabled = true;
                peer.kind.label()
            };
            publish_link(s);
            info!(
                target: TAG,
                "{label} MIDI subscribed — Chocolate: PC C banks 0-2 / Adv 102-108+112-116 / dual-FS 109; Plus bank0 CC0-3 = Post3/Gate/Tap/SoloBoost"
            );
            maybe_resume_scan();
        }

        esp_gattc_cb_event_t_ESP_GATTC_NOTIFY_EVT => {
            let notify = unsafe { (*param).notify };
            let Some(idx) = peer_idx_by_conn(s, notify.conn_id) else {
                return;
            };
            let peer = &s.peers[idx];
            if notify.handle != peer.midi_handle || notify.value_len == 0 {
                return;
            }
            let kind = peer.kind;
            let Some(bytes) = (unsafe {
                crate::memsafe::bytes_at_most(
                    notify.value,
                    notify.value_len as usize,
                    MIDI_PKT_MAX,
                )
            }) else {
                return;
            };
            worker::enqueue_notify(s, kind, bytes);
        }

        esp_gattc_cb_event_t_ESP_GATTC_DISCONNECT_EVT => {
            let disc = unsafe { (*param).disconnect };
            let reason = disc.reason;
            if let Some(idx) = peer_idx_by_conn(s, disc.conn_id) {
                let label = s.peers[idx].kind.label();
                warn!(target: TAG, "{label} disconnected reason=0x{reason:02x}");
                s.peers[idx].clear();
            } else {
                warn!(target: TAG, "Footswitch disconnected reason=0x{reason:02x}");
            }
            // Drop pending only if it was this BDA.
            if s.pending_bda == Some(disc.remote_bda) {
                s.pending_bda = None;
            }
            publish_link(s);
            worker::drain_queue(s);
            // Keep gattc_if — needed for the next open.
            maybe_resume_scan();
        }

        _ => {}
    }
}

fn discover_midi_char(s: &mut Ctx, gattc_if: esp_gatt_if_t, idx: usize) -> bool {
    let peer = &s.peers[idx];
    if peer.midi_start == INVALID_HANDLE || peer.midi_end == INVALID_HANDLE {
        return false;
    }
    let conn_id = peer.conn_id;
    let midi_start = peer.midi_start;
    let midi_end = peer.midi_end;

    let mut count: u16 = 0;
    unsafe {
        if esp_ble_gattc_get_attr_count(
            gattc_if,
            conn_id,
            esp_gatt_db_attr_type_t_ESP_GATT_DB_CHARACTERISTIC,
            midi_start,
            midi_end,
            INVALID_HANDLE,
            &mut count,
        ) != esp_gatt_status_t_ESP_GATT_OK
            || count == 0
        {
            return false;
        }

        let mut elems = vec![core::mem::zeroed::<esp_gattc_char_elem_t>(); count as usize];
        let uuid = esp_bt_uuid_t {
            len: ESP_UUID_LEN_128 as u16,
            uuid: esp_bt_uuid_t__bindgen_ty_1 {
                uuid128: UUID128_BLE_MIDI_IO,
            },
        };
        if esp_ble_gattc_get_char_by_uuid(
            gattc_if,
            conn_id,
            midi_start,
            midi_end,
            uuid,
            elems.as_mut_ptr(),
            &mut count,
        ) != esp_gatt_status_t_ESP_GATT_OK
            || count == 0
        {
            // Fall back: first notify-capable char in the service.
            count = elems.len() as u16;
            if esp_ble_gattc_get_all_char(
                gattc_if,
                conn_id,
                midi_start,
                midi_end,
                elems.as_mut_ptr(),
                &mut count,
                0,
            ) != esp_gatt_status_t_ESP_GATT_OK
                || count == 0
            {
                return false;
            }
            elems.truncate(count as usize);
            let Some(elem) = elems.iter().find(|e| {
                (e.properties as u32 & ESP_GATT_CHAR_PROP_BIT_NOTIFY) != 0
            }) else {
                return false;
            };
            s.peers[idx].midi_handle = elem.char_handle;
        } else {
            s.peers[idx].midi_handle = elems[0].char_handle;
        }
    }

    info!(
        target: TAG,
        "BLE-MIDI IO handle={} ({})",
        s.peers[idx].midi_handle,
        s.peers[idx].kind.label()
    );
    s.peers[idx].midi_handle != INVALID_HANDLE
}
