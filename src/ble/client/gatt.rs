//! Characteristic discovery / notify registration and the Bluedroid GATT
//! client event handler.

use super::*;
use super::{ctx::*, worker::*};

pub(super) fn find_char_in_range(
    gattc_if: esp_gatt_if_t,
    conn_id: u16,
    start: u16,
    end: u16,
    char_uuid: u16,
) -> Option<u16> {
    if start == INVALID_HANDLE || end == INVALID_HANDLE {
        return None;
    }

    let mut count: u16 = 0;
    unsafe {
        if esp_ble_gattc_get_attr_count(
            gattc_if,
            conn_id,
            esp_gatt_db_attr_type_t_ESP_GATT_DB_CHARACTERISTIC,
            start,
            end,
            INVALID_HANDLE,
            &mut count,
        ) != esp_gatt_status_t_ESP_GATT_OK
            || count == 0
        {
            return None;
        }

        let mut elems =
            vec![core::mem::zeroed::<esp_gattc_char_elem_t>(); count as usize];
        let status = esp_ble_gattc_get_char_by_uuid(
            gattc_if,
            conn_id,
            start,
            end,
            uuid16(char_uuid),
            elems.as_mut_ptr(),
            &mut count,
        );
        if status == esp_gatt_status_t_ESP_GATT_OK && count > 0 {
            Some(elems[0].char_handle)
        } else {
            None
        }
    }
}

pub(super) fn add_sub(s: &mut Ctx, handle: u16, cccd_value: u16) {
    if handle == INVALID_HANDLE || cccd_value == 0 || s.sub_count >= GATT_NOTIFY_MAX {
        return;
    }
    for sub in &mut s.subs[..s.sub_count] {
        if sub.handle == handle {
            sub.cccd_value |= cccd_value;
            return;
        }
    }
    s.subs[s.sub_count] = GattSub { handle, cccd_value };
    s.sub_count += 1;
}

pub(super) fn cccd_for_handle(s: &Ctx, handle: u16) -> u16 {
    for sub in &s.subs[..s.sub_count] {
        if sub.handle == handle {
            return sub.cccd_value;
        }
    }
    0x0001 /* notify */
}

pub(super) fn handle_label(s: &Ctx, handle: u16) -> Option<&'static str> {
    if s.c305_handle != INVALID_HANDLE && handle == s.c305_handle {
        return Some("C305");
    }
    if s.c306_handle != INVALID_HANDLE && handle == s.c306_handle {
        return Some("C306");
    }
    if s.midi_handle != INVALID_HANDLE && handle == s.midi_handle {
        return Some("MIDI/C302");
    }
    None
}

pub(super) fn collect_chars(gattc_if: esp_gatt_if_t, conn_id: u16, start: u16, end: u16) -> Vec<esp_gattc_char_elem_t> {
    let mut count: u16 = 0;
    unsafe {
        if esp_ble_gattc_get_attr_count(
            gattc_if,
            conn_id,
            esp_gatt_db_attr_type_t_ESP_GATT_DB_CHARACTERISTIC,
            start,
            end,
            INVALID_HANDLE,
            &mut count,
        ) != esp_gatt_status_t_ESP_GATT_OK
            || count == 0
        {
            return Vec::new();
        }

        let mut elems =
            vec![core::mem::zeroed::<esp_gattc_char_elem_t>(); count as usize];
        if esp_ble_gattc_get_all_char(
            gattc_if,
            conn_id,
            start,
            end,
            elems.as_mut_ptr(),
            &mut count,
            0,
        ) != esp_gatt_status_t_ESP_GATT_OK
        {
            return Vec::new();
        }
        elems.truncate(count as usize);
        elems
    }
}

pub(super) fn log_and_collect_notify_chars(
    s: &mut Ctx,
    gattc_if: esp_gatt_if_t,
    conn_id: u16,
    start: u16,
    end: u16,
    svc_name: &str,
) {
    if start == INVALID_HANDLE || end == INVALID_HANDLE {
        return;
    }

    for elem in collect_chars(gattc_if, conn_id, start, end) {
        let uuid_s = format_uuid(elem.uuid);
        let can_notify = (elem.properties as u32 & ESP_GATT_CHAR_PROP_BIT_NOTIFY) != 0;
        let can_indicate = (elem.properties as u32 & ESP_GATT_CHAR_PROP_BIT_INDICATE) != 0;
        info!(
            target: TAG,
            "GATT {svc_name} char {uuid_s} handle={} props=0x{:02X}{}{}",
            elem.char_handle,
            elem.properties,
            if can_notify { " NOTIFY" } else { "" },
            if can_indicate { " INDICATE" } else { "" },
        );

        if uuid_as_16(elem.uuid) == Some(UUID_CHAR_C306) {
            s.c306_handle = elem.char_handle;
            // Skip the C306 indicate mirror — confirm traffic doubles pitch load.
            continue;
        }
        if uuid128_eq(elem.uuid, &UUID128_BLE_MIDI_IO) {
            s.midi_handle = elem.char_handle;
        }

        if let Some(u16_uuid) = uuid_as_16(elem.uuid) {
            if matches!(u16_uuid, UUID_CHAR_C300 | UUID_CHAR_C301 | UUID_CHAR_C307)
                && (elem.properties as u32 & ESP_GATT_CHAR_PROP_BIT_READ) != 0
                && s.bpm_read_count < BPM_READ_HANDLE_MAX
            {
                s.bpm_read_handles[s.bpm_read_count] = elem.char_handle;
                s.bpm_read_count += 1;
                info!(
                    target: TAG,
                    "BPM hunt: queued GATT read C{u16_uuid:03X} handle={}", elem.char_handle
                );
            }
        }

        if can_notify || can_indicate {
            let mut cccd = 0u16;
            if can_notify {
                cccd |= 0x0001;
            }
            if can_indicate {
                cccd |= 0x0002;
            }
            add_sub(s, elem.char_handle, cccd);
        }
    }
}

pub(super) fn discover_nano_chars(s: &mut Ctx, gattc_if: esp_gatt_if_t, conn_id: u16) -> bool {
    s.sub_count = 0;
    s.bpm_read_count = 0;
    s.bpm_read_next = 0;

    let c304 = find_char_in_range(gattc_if, conn_id, s.a002_start, s.a002_end, UUID_CHAR_C304)
        .or_else(|| find_char_in_range(gattc_if, conn_id, s.a003_start, s.a003_end, UUID_CHAR_C304));
    if let Some(handle) = c304 {
        s.c304_handle = handle;
    }

    let c305 = find_char_in_range(gattc_if, conn_id, s.a002_start, s.a002_end, UUID_CHAR_C305)
        .or_else(|| find_char_in_range(gattc_if, conn_id, s.a003_start, s.a003_end, UUID_CHAR_C305));
    if let Some(handle) = c305 {
        s.c305_handle = handle;
    }

    let c306 = find_char_in_range(gattc_if, conn_id, s.a002_start, s.a002_end, UUID_CHAR_C306)
        .or_else(|| find_char_in_range(gattc_if, conn_id, s.a003_start, s.a003_end, UUID_CHAR_C306));
    if let Some(handle) = c306 {
        s.c306_handle = handle;
    }

    // Prefer Nano C302 (raw MIDI write) over the standard BLE-MIDI UUID if present.
    let c302 = find_char_in_range(gattc_if, conn_id, s.a002_start, s.a002_end, UUID_CHAR_C302)
        .or_else(|| find_char_in_range(gattc_if, conn_id, s.a003_start, s.a003_end, UUID_CHAR_C302));

    let (a002_start, a002_end) = (s.a002_start, s.a002_end);
    let (a003_start, a003_end) = (s.a003_start, s.a003_end);
    log_and_collect_notify_chars(s, gattc_if, conn_id, a002_start, a002_end, "A002");
    log_and_collect_notify_chars(s, gattc_if, conn_id, a003_start, a003_end, "A003");
    for i in 0..s.extra_svc_count {
        let svc = &s.extra_svcs[i];
        let (start, end, name) = (svc.start, svc.end, svc.name.clone());
        log_and_collect_notify_chars(s, gattc_if, conn_id, start, end, &name);
    }

    // C302 wins over any BLE-MIDI IO char discovered above.
    if let Some(handle) = c302 {
        s.midi_handle = handle;
    }

    // Always include known streams even if the property scan missed them.
    if c305.is_some() {
        add_sub(s, s.c305_handle, 0x0001);
    }
    /*
     * Do NOT subscribe to C306. It INDICATEs a mirror of C305; each indicate
     * needs a confirm and doubles ATT traffic during pitch streaming.
     */
    if s.midi_handle != INVALID_HANDLE && Some(s.midi_handle) != c302 {
        add_sub(s, s.midi_handle, 0x0001);
    }
    // Always try C302 notify too — may carry MIDI clock / tap.
    if let Some(handle) = c302 {
        add_sub(s, handle, 0x0001);
    }

    let midi_kind = if s.midi_handle == INVALID_HANDLE {
        "none"
    } else if Some(s.midi_handle) == c302 {
        "C302"
    } else {
        "BLE-MIDI"
    };
    info!(
        target: TAG,
        "C304={} ({}) C305={} ({}) C306={} (skip sub) MIDI={} ({}) subs={}",
        s.c304_handle,
        if c304.is_some() { "ok" } else { "missing" },
        s.c305_handle,
        if c305.is_some() { "ok" } else { "missing" },
        s.c306_handle,
        s.midi_handle,
        midi_kind,
        s.sub_count,
    );
    c304.is_some() && c305.is_some()
}

pub(super) fn enable_all_notifies(s: &mut Ctx, gattc_if: esp_gatt_if_t) {
    for i in 0..s.sub_count {
        let sub = s.subs[i];
        let err = unsafe {
            esp_ble_gattc_register_for_notify(gattc_if, s.remote_bda.as_mut_ptr(), sub.handle)
        };
        if err != ESP_OK {
            error!(
                target: TAG,
                "register_for_notify handle={} failed: {}", sub.handle, err_name(err)
            );
        } else {
            info!(
                target: TAG,
                "Registered {} handle={} cccd=0x{:04X}",
                handle_label(s, sub.handle).unwrap_or("char"),
                sub.handle,
                sub.cccd_value
            );
        }
    }
}

// ESP-IDF bindgen names events `esp_gattc_cb_event_t_ESP_*` (type prefix +
// enum). Rust wants SCREAMING_SNAKE for consts-in-patterns; we keep the
// generated identifiers so arms match the C headers verbatim.
#[allow(non_upper_case_globals)]
pub(super) fn gattc_handler(
    s: &mut Ctx,
    event: esp_gattc_cb_event_t,
    gattc_if: esp_gatt_if_t,
    param: *mut esp_ble_gattc_cb_param_t,
) {
    match event {
        esp_gattc_cb_event_t_ESP_GATTC_REG_EVT => {
            info!(target: TAG, "GATT client registered");
            let mut params = scan_params();
            unsafe { esp_ble_gap_set_scan_params(&mut params) };
        }

        esp_gattc_cb_event_t_ESP_GATTC_CONNECT_EVT => {
            let connect = unsafe { (*param).connect };
            // Bluedroid reports CONNECT to every GATTC app — ignore peers we did not open.
            let Some(pending) = s.pending_bda else {
                return;
            };
            if !bda_eq(&pending, &connect.remote_bda) {
                return;
            }
            s.connected = true;
            s.scanning = false;
            s.conn_id = connect.conn_id;
            s.gattc_if = gattc_if;
            s.remote_bda = connect.remote_bda;
            s.pending_bda = None;
            reset_handles(s);
            publish_link(s);
            info!(target: TAG, "Connected to Nano Cortex");

            // Prefer a short interval so pitch notifies are not paced at ~50ms+.
            let mut conn = esp_ble_conn_update_params_t {
                bda: s.remote_bda,
                min_int: 0x06, /* 7.5 ms */
                max_int: 0x10, /* 20 ms */
                latency: 0,
                timeout: 400, /* 4 s */
            };
            unsafe {
                esp_ble_gap_update_conn_params(&mut conn);
                esp_ble_gattc_send_mtu_req(gattc_if, connect.conn_id);
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_OPEN_EVT => {
            let open = unsafe { (*param).open };
            if open.status != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "Nano open failed status={}", open.status);
                s.pending_bda = None;
                s.connected = false;
                reset_handles(s);
                crate::ble::client::resume_scan_if_needed();
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_CFG_MTU_EVT => {
            info!(target: TAG, "MTU {}", unsafe { (*param).cfg_mtu.mtu });
            unsafe { esp_ble_gattc_search_service(gattc_if, s.conn_id, ptr::null_mut()) };
        }

        esp_gattc_cb_event_t_ESP_GATTC_SEARCH_RES_EVT => {
            let res = unsafe { (*param).search_res };
            let uuid = res.srvc_id.uuid;
            let uuid_s = format_uuid(uuid);
            info!(
                target: TAG,
                "Found service {uuid_s} handles {}-{}", res.start_handle, res.end_handle
            );

            if let Some(id) = uuid_as_16(uuid) {
                if id == UUID_SVC_A002 {
                    s.a002_start = res.start_handle;
                    s.a002_end = res.end_handle;
                } else if id == UUID_SVC_A003 {
                    s.a003_start = res.start_handle;
                    s.a003_end = res.end_handle;
                } else if s.extra_svc_count < GATT_EXTRA_SVC_MAX {
                    s.extra_svcs[s.extra_svc_count] = ExtraSvc {
                        start: res.start_handle,
                        end: res.end_handle,
                        name: format!("0x{id:04X}"),
                    };
                    s.extra_svc_count += 1;
                }
            } else if s.extra_svc_count < GATT_EXTRA_SVC_MAX {
                let name = if uuid128_eq(uuid, &UUID128_BLE_MIDI_SVC) {
                    "BLE-MIDI".to_string()
                } else {
                    uuid_s.chars().take(20).collect()
                };
                s.extra_svcs[s.extra_svc_count] = ExtraSvc {
                    start: res.start_handle,
                    end: res.end_handle,
                    name,
                };
                s.extra_svc_count += 1;
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_SEARCH_CMPL_EVT => {
            if unsafe { (*param).search_cmpl.status } != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "Service discovery failed");
                return;
            }
            let conn_id = s.conn_id;
            if !discover_nano_chars(s, gattc_if, conn_id) {
                error!(target: TAG, "C304/C305 not found — disconnecting");
                unsafe { esp_ble_gattc_close(gattc_if, conn_id) };
                return;
            }
            publish_link(s);
            enable_all_notifies(s, gattc_if);
        }

        esp_gattc_cb_event_t_ESP_GATTC_REG_FOR_NOTIFY_EVT => {
            let reg = unsafe { (*param).reg_for_notify };
            if reg.status != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "Notify registration failed handle={}", reg.handle);
                return;
            }

            let mut count: u16 = 1;
            let mut descr = unsafe { core::mem::zeroed::<esp_gattc_descr_elem_t>() };
            let status = unsafe {
                esp_ble_gattc_get_descr_by_char_handle(
                    gattc_if,
                    s.conn_id,
                    reg.handle,
                    uuid16(ESP_GATT_UUID_CHAR_CLIENT_CONFIG as u16),
                    &mut descr,
                    &mut count,
                )
            };
            if status != esp_gatt_status_t_ESP_GATT_OK || count == 0 {
                error!(target: TAG, "CCCD not found handle={}", reg.handle);
                return;
            }

            // C306 is INDICATE — writing 0x0001 alone never enables it.
            let mut cccd_en = cccd_for_handle(s, reg.handle);
            info!(
                target: TAG,
                "CCCD write {} handle={} value=0x{cccd_en:04X}",
                handle_label(s, reg.handle).unwrap_or("char"),
                reg.handle
            );
            unsafe {
                esp_ble_gattc_write_char_descr(
                    gattc_if,
                    s.conn_id,
                    descr.handle,
                    size_of::<u16>() as u16,
                    (&raw mut cccd_en).cast::<u8>(),
                    esp_gatt_write_type_t_ESP_GATT_WRITE_TYPE_RSP,
                    esp_gatt_auth_req_t_ESP_GATT_AUTH_REQ_NONE,
                )
            };
        }

        esp_gattc_cb_event_t_ESP_GATTC_WRITE_DESCR_EVT => {
            if unsafe { (*param).write.status } == esp_gatt_status_t_ESP_GATT_OK
                && !s.notify_enabled
            {
                s.notify_enabled = true;
                publish_link(s);
                info!(target: TAG, "GATT subscriptions enabled (C305/C306/…) — syncing");
                crate::ui::screen::nano_ui_set_ble_connected(true);
                crate::state::nano_state_begin_sync();
                // Keep hunting for Chocolate after Nano is ready.
                crate::ble::client::resume_scan_if_needed();
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_NOTIFY_EVT => {
            let notify = unsafe { (*param).notify };
            on_gatt_notify(s, notify.handle, notify.value, notify.value_len);
        }

        esp_gattc_cb_event_t_ESP_GATTC_WRITE_CHAR_EVT => {
            let status = unsafe { (*param).write.status };
            if status != esp_gatt_status_t_ESP_GATT_OK {
                error!(target: TAG, "C304 write failed status=0x{status:x}");
            }
        }

        esp_gattc_cb_event_t_ESP_GATTC_READ_CHAR_EVT => {
            let read = unsafe { (*param).read };
            if read.status != esp_gatt_status_t_ESP_GATT_OK {
                warn!(
                    target: TAG,
                    "BPM hunt: GATT read status={} handle={}", read.status, read.handle
                );
            } else {
                let value = unsafe {
                    crate::memsafe::bytes_at_most(
                        read.value,
                        read.value_len as usize,
                        crate::memsafe::PKT_MAX,
                    )
                }
                .unwrap_or(&[]);
                let label =
                    CString::new(format!("GATT read h={}", read.handle)).unwrap_or_default();
                nano_wire::log_bytes_full(TAG, label.to_str().unwrap_or("GATT read"), value);
                if let Some(bpm) = parse_bpm(value) {
                    info!(target: TAG, "BPM {bpm} (GATT read)");
                    crate::state::nano_state_on_bpm_found(bpm);
                }
            }
            s.bpm_read_next += 1;
            bpm_gatt_read_next(s);
        }

        esp_gattc_cb_event_t_ESP_GATTC_DISCONNECT_EVT => {
            let reason = unsafe { (*param).disconnect.reason };
            warn!(target: TAG, "Disconnected reason=0x{reason:02x} — will rescan");
            s.connected = false;
            s.pending_bda = None;
            publish_link(s); // drop LINK_UP before tearing handles / sleeping
            // Keep gattc_if — required for the next esp_ble_gattc_open.
            reset_handles(s);
            drain_rx_queue(s);
            crate::ui::screen::nano_ui_set_ble_connected(false);
            crate::ui::screen::nano_ui_set_amp(None);
            crate::state::nano_state_reset();
            unsafe { vTaskDelay(ms_to_ticks(2000)) };
            start_scan(s);
        }

        _ => {}
    }
}

pub(super) extern "C" fn gattc_cb(
    event: esp_gattc_cb_event_t,
    gattc_if: esp_gatt_if_t,
    param: *mut esp_ble_gattc_cb_param_t,
) {
    // Dual GATTC apps share one callback — route by app_id on REG, else by if.
    if event == esp_gattc_cb_event_t_ESP_GATTC_REG_EVT {
        let app_id = unsafe { (*param).reg.app_id };
        if app_id == crate::ble::midi_fs::profile_app_id() {
            crate::ble::midi_fs::gattc_handler(event, gattc_if, param);
            return;
        }
        let s = ctx();
        if unsafe { (*param).reg.status } == esp_gatt_status_t_ESP_GATT_OK {
            s.gattc_if = gattc_if;
        }
        gattc_handler(s, event, gattc_if, param);
        return;
    }

    let midi_if = crate::ble::midi_fs::gattc_if();
    if midi_if != ESP_GATT_IF_NONE as esp_gatt_if_t && gattc_if == midi_if {
        crate::ble::midi_fs::gattc_handler(event, gattc_if, param);
        return;
    }

    let s = ctx();
    if gattc_if == ESP_GATT_IF_NONE as esp_gatt_if_t || gattc_if == s.gattc_if {
        gattc_handler(s, event, gattc_if, param);
    }
}
