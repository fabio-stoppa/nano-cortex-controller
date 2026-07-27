//! BLE GATT client for the Neural DSP Nano Cortex (port of `components/nano/nano_ble.c`).
//!
//! A002/A003: C304 write, C305 notify, C306 indicate (mirrors C305 and is deliberately
//! left unsubscribed). Other NOTIFY/INDICATE chars (incl. BLE-MIDI) are subscribed too.

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void, CStr};
use core::mem::{size_of, MaybeUninit};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, Ordering};
use std::ffi::CString;

use esp_idf_svc::sys::*;
use log::{debug, error, info, warn};

const TAG: &str = "nano_ble";

fn is_tuner_active() -> bool {
    crate::tuner::nano_tuner_is_active()
}

fn parse_bpm(data: &[u8]) -> Option<u16> {
    nano_wire::try_parse_bpm_notify(data).or_else(|| nano_wire::parse_bpm(data))
}

const NAME_PREFIX: &[u8] = b"Neural DSP Nano Cortex";

const C305_QUEUE_LEN: u32 = 48;
const C305_LIVE_QUEUE_LEN: u32 = 1;
/// Preset dumps often exceed one ATT MTU; 256 silently chopped the gate f53/f54 tail.
const C305_PKT_MAX: usize = 512;
const C305_TASK_STACK: u32 = 4096;
/// Keep below LVGL (prio 6) so continuous pitch cannot starve the display.
const C305_TASK_PRIO: u32 = 1;
const GATT_NOTIFY_MAX: usize = 16;
const GATT_EXTRA_SVC_MAX: usize = 6;
const BPM_READ_HANDLE_MAX: usize = 8;

const PROFILE_APP_ID: u16 = 0;
const INVALID_HANDLE: u16 = 0;

const UUID_SVC_A002: u16 = 0xA002;
const UUID_SVC_A003: u16 = 0xA003;
const UUID_CHAR_C300: u16 = 0xC300;
const UUID_CHAR_C301: u16 = 0xC301;
const UUID_CHAR_C302: u16 = 0xC302;
const UUID_CHAR_C304: u16 = 0xC304;
const UUID_CHAR_C305: u16 = 0xC305;
const UUID_CHAR_C306: u16 = 0xC306;
const UUID_CHAR_C307: u16 = 0xC307;

/// BLE-MIDI service/char — Bluedroid little-endian 128-bit form.
const UUID128_BLE_MIDI_SVC: [u8; 16] = [
    0x00, 0xC7, 0xC4, 0x4E, 0xE3, 0x6C, 0x51, 0xA7, 0x33, 0x4B, 0xE8, 0xED, 0x5A, 0x0E, 0xB8, 0x03,
];
const UUID128_BLE_MIDI_IO: [u8; 16] = [
    0xF3, 0x6B, 0x10, 0x9D, 0x66, 0xF2, 0xA9, 0xA1, 0x12, 0x41, 0x68, 0x38, 0xDB, 0xE5, 0x72, 0x77,
];

const PD_TRUE: BaseType_t = 1;
const PD_FALSE: BaseType_t = 0;
const PD_PASS: BaseType_t = 1;
const QUEUE_SEND_TO_BACK: BaseType_t = 0;
const QUEUE_OVERWRITE: BaseType_t = 2;
const QUEUE_TYPE_BASE: u8 = 0;
const NO_AFFINITY: BaseType_t = 0x7FFF_FFFF;
const NOTIFY_INDEX: UBaseType_t = 0;

unsafe extern "C" {
    /// `BT_CONTROLLER_INIT_CONFIG_DEFAULT()` is a macro, so it needs the C shim in
    /// `components/nano/bt_cfg_helper.c`.
    fn nano_bt_controller_config_default(cfg: *mut esp_bt_controller_config_t);
}

fn ms_to_ticks(ms: u32) -> TickType_t {
    ms * configTICK_RATE_HZ / 1000
}

fn err_name(err: esp_err_t) -> &'static str {
    unsafe { CStr::from_ptr(esp_err_to_name(err)) }
        .to_str()
        .unwrap_or("?")
}

fn check(err: esp_err_t, what: &str) {
    if err != ESP_OK {
        panic!("{what} failed: {} ({err})", err_name(err));
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct C305Item {
    handle: u16,
    len: u16,
    data: [u8; C305_PKT_MAX],
}

#[derive(Copy, Clone)]
struct GattSub {
    handle: u16,
    /// bit0=notify, bit1=indicate
    cccd_value: u16,
}

struct ExtraSvc {
    start: u16,
    end: u16,
    name: String,
}

impl ExtraSvc {
    const fn new() -> Self {
        Self {
            start: 0,
            end: 0,
            name: String::new(),
        }
    }
}

struct Ctx {
    gattc_if: esp_gatt_if_t,
    conn_id: u16,
    remote_bda: esp_bd_addr_t,
    connected: bool,
    scanning: bool,
    a002_start: u16,
    a002_end: u16,
    a003_start: u16,
    a003_end: u16,
    c304_handle: u16,
    c305_handle: u16,
    c306_handle: u16,
    midi_handle: u16,
    notify_enabled: bool,

    subs: [GattSub; GATT_NOTIFY_MAX],
    sub_count: usize,

    bpm_read_handles: [u16; BPM_READ_HANDLE_MAX],
    bpm_read_count: usize,
    bpm_read_next: usize,

    extra_svcs: [ExtraSvc; GATT_EXTRA_SVC_MAX],
    extra_svc_count: usize,

    queue: QueueHandle_t,
    live_queue: QueueHandle_t,
    task: TaskHandle_t,
    drop_count: u32,
}

impl Ctx {
    const fn new() -> Self {
        Self {
            gattc_if: ESP_GATT_IF_NONE as esp_gatt_if_t,
            conn_id: 0,
            remote_bda: [0; 6],
            connected: false,
            scanning: false,
            a002_start: INVALID_HANDLE,
            a002_end: INVALID_HANDLE,
            a003_start: INVALID_HANDLE,
            a003_end: INVALID_HANDLE,
            c304_handle: INVALID_HANDLE,
            c305_handle: INVALID_HANDLE,
            c306_handle: INVALID_HANDLE,
            midi_handle: INVALID_HANDLE,
            notify_enabled: false,
            subs: [GattSub {
                handle: 0,
                cccd_value: 0,
            }; GATT_NOTIFY_MAX],
            sub_count: 0,
            bpm_read_handles: [0; BPM_READ_HANDLE_MAX],
            bpm_read_count: 0,
            bpm_read_next: 0,
            extra_svcs: [const { ExtraSvc::new() }; GATT_EXTRA_SVC_MAX],
            extra_svc_count: 0,
            queue: ptr::null_mut(),
            live_queue: ptr::null_mut(),
            task: ptr::null_mut(),
            drop_count: 0,
        }
    }
}

struct Shared<T>(UnsafeCell<T>);

// Task contract: Bluedroid GATT/GAP callbacks + C305 worker + UI queries.
// Ordering is the BLE event flow / FreeRTOS queues — not a Rust Mutex.
// See `crate::fx::shared`.
unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}

static CTX: Shared<Ctx> = Shared::new(Ctx::new());

/// Cross-task link snapshot — UI / writers read these instead of racing `Ctx`.
/// Publish order on teardown: `LINK_UP` false first, then handles / if / conn.
static LINK_UP: AtomicBool = AtomicBool::new(false);
static SCANNING: AtomicBool = AtomicBool::new(false);
static GATTC_IF: AtomicI32 = AtomicI32::new(-1);
static CONN_ID: AtomicU16 = AtomicU16::new(0);
static C304_HANDLE: AtomicU16 = AtomicU16::new(INVALID_HANDLE);
static C305_HANDLE: AtomicU16 = AtomicU16::new(INVALID_HANDLE);

fn ctx() -> &'static mut Ctx {
    CTX.get()
}

fn publish_link(s: &Ctx) {
    let up = is_connected(s);
    if !up {
        // Tear down: drop readiness before invalidating handles used by writers.
        LINK_UP.store(false, Ordering::Release);
    }
    SCANNING.store(s.scanning, Ordering::Release);
    GATTC_IF.store(s.gattc_if as i32, Ordering::Release);
    CONN_ID.store(s.conn_id, Ordering::Release);
    C304_HANDLE.store(s.c304_handle, Ordering::Release);
    C305_HANDLE.store(s.c305_handle, Ordering::Release);
    if up {
        LINK_UP.store(true, Ordering::Release);
    }
}

#[derive(Copy, Clone)]
struct LinkSnap {
    gattc_if: esp_gatt_if_t,
    conn_id: u16,
    c304: u16,
}

fn link_snap() -> Option<LinkSnap> {
    if !LINK_UP.load(Ordering::Acquire) {
        return None;
    }
    let snap = LinkSnap {
        gattc_if: GATTC_IF.load(Ordering::Acquire) as esp_gatt_if_t,
        conn_id: CONN_ID.load(Ordering::Acquire),
        c304: C304_HANDLE.load(Ordering::Acquire),
    };
    if !LINK_UP.load(Ordering::Acquire) || snap.c304 == INVALID_HANDLE {
        None
    } else {
        Some(snap)
    }
}

fn uuid16(id: u16) -> esp_bt_uuid_t {
    esp_bt_uuid_t {
        len: ESP_UUID_LEN_16 as u16,
        uuid: esp_bt_uuid_t__bindgen_ty_1 { uuid16: id },
    }
}

fn scan_params() -> esp_ble_scan_params_t {
    esp_ble_scan_params_t {
        scan_type: esp_ble_scan_type_t_BLE_SCAN_TYPE_ACTIVE,
        own_addr_type: esp_ble_addr_type_t_BLE_ADDR_TYPE_PUBLIC,
        scan_filter_policy: esp_ble_scan_filter_t_BLE_SCAN_FILTER_ALLOW_ALL,
        scan_interval: 0x50,
        scan_window: 0x30,
        scan_duplicate: esp_ble_scan_duplicate_t_BLE_SCAN_DUPLICATE_DISABLE,
    }
}

fn format_uuid(u: esp_bt_uuid_t) -> String {
    let len = u.len as u32;
    unsafe {
        if len == ESP_UUID_LEN_16 {
            let id = core::ptr::addr_of!(u.uuid.uuid16).read_unaligned();
            format!("0x{id:04X}")
        } else if len == ESP_UUID_LEN_32 {
            let id = core::ptr::addr_of!(u.uuid.uuid32).read_unaligned();
            format!("0x{id:08X}")
        } else if len == ESP_UUID_LEN_128 {
            let b = u.uuid.uuid128;
            format!(
                "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-\
                 {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                b[15], b[14], b[13], b[12], b[11], b[10], b[9], b[8], b[7], b[6], b[5], b[4], b[3],
                b[2], b[1], b[0]
            )
        } else {
            "?".to_string()
        }
    }
}

fn uuid128_eq(u: esp_bt_uuid_t, expect: &[u8; 16]) -> bool {
    u.len as u32 == ESP_UUID_LEN_128 && unsafe { u.uuid.uuid128 } == *expect
}

fn uuid_as_16(u: esp_bt_uuid_t) -> Option<u16> {
    (u.len as u32 == ESP_UUID_LEN_16)
        .then(|| unsafe { core::ptr::addr_of!(u.uuid.uuid16).read_unaligned() })
}

fn reset_handles(s: &mut Ctx) {
    s.a002_start = INVALID_HANDLE;
    s.a002_end = INVALID_HANDLE;
    s.a003_start = INVALID_HANDLE;
    s.a003_end = INVALID_HANDLE;
    s.c304_handle = INVALID_HANDLE;
    s.c305_handle = INVALID_HANDLE;
    s.c306_handle = INVALID_HANDLE;
    s.midi_handle = INVALID_HANDLE;
    s.notify_enabled = false;
    s.sub_count = 0;
    s.extra_svc_count = 0;
    s.bpm_read_count = 0;
    s.bpm_read_next = 0;
    publish_link(s);
}

fn is_connected(s: &Ctx) -> bool {
    s.connected && s.c304_handle != INVALID_HANDLE && s.notify_enabled
}

fn start_scan(s: &mut Ctx) {
    if s.scanning || s.connected {
        return;
    }
    let err = unsafe { esp_ble_gap_start_scanning(0) };
    if err == ESP_OK {
        s.scanning = true;
        publish_link(s);
        info!(target: TAG, "Scanning for \"{}\"...", String::from_utf8_lossy(NAME_PREFIX));
    } else {
        error!(target: TAG, "Scan start failed: {}", err_name(err));
    }
}

fn find_char_in_range(
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

        let mut elems = vec![unsafe { core::mem::zeroed::<esp_gattc_char_elem_t>() }; count as usize];
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

fn add_sub(s: &mut Ctx, handle: u16, cccd_value: u16) {
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

fn cccd_for_handle(s: &Ctx, handle: u16) -> u16 {
    for sub in &s.subs[..s.sub_count] {
        if sub.handle == handle {
            return sub.cccd_value;
        }
    }
    0x0001 /* notify */
}

fn handle_label(s: &Ctx, handle: u16) -> Option<&'static str> {
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

fn collect_chars(gattc_if: esp_gatt_if_t, conn_id: u16, start: u16, end: u16) -> Vec<esp_gattc_char_elem_t> {
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

        let mut elems = vec![unsafe { core::mem::zeroed::<esp_gattc_char_elem_t>() }; count as usize];
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

fn log_and_collect_notify_chars(
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

fn discover_nano_chars(s: &mut Ctx, gattc_if: esp_gatt_if_t, conn_id: u16) -> bool {
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

fn enable_all_notifies(s: &mut Ctx, gattc_if: esp_gatt_if_t) {
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

unsafe extern "C" fn c305_worker_task(_arg: *mut c_void) {
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

fn c305_queue_init(s: &mut Ctx) -> bool {
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

fn c305_is_pitch(data: &[u8]) -> bool {
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
fn c305_is_tuner_hot(data: &[u8]) -> bool {
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

fn notify_worker(s: &Ctx) {
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

fn on_gatt_notify(s: &mut Ctx, handle: u16, data: *const u8, len: u16) {
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

    // C306 mirrors C305 — never queue it (doubles work; stomps the live pitch mailbox).
    if is_c306 {
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

fn drain_rx_queue(s: &mut Ctx) {
    if !s.queue.is_null() {
        unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
    }
    if !s.live_queue.is_null() {
        unsafe { xQueueGenericReset(s.live_queue, PD_FALSE) };
    }
    s.drop_count = 0;
}

fn bpm_gatt_read_next(s: &mut Ctx) {
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

fn gattc_handler(
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
            s.connected = true;
            s.scanning = false;
            s.conn_id = connect.conn_id;
            s.gattc_if = gattc_if;
            s.remote_bda = connect.remote_bda;
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
                unsafe { crate::ui::screen::nano_ui_set_ble_connected(true) };
                crate::state::nano_state_begin_sync();
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
            publish_link(s); // drop LINK_UP before tearing handles / sleeping
            s.gattc_if = ESP_GATT_IF_NONE as esp_gatt_if_t;
            reset_handles(s);
            drain_rx_queue(s);
            unsafe { crate::ui::screen::nano_ui_set_ble_connected(false) };
            unsafe { crate::ui::screen::nano_ui_set_amp(core::ptr::null()) };
            crate::state::nano_state_reset();
            unsafe { vTaskDelay(ms_to_ticks(2000)) };
            start_scan(s);
        }

        _ => {}
    }
}

unsafe extern "C" fn gattc_cb(
    event: esp_gattc_cb_event_t,
    gattc_if: esp_gatt_if_t,
    param: *mut esp_ble_gattc_cb_param_t,
) {
    let s = ctx();
    if event == esp_gattc_cb_event_t_ESP_GATTC_REG_EVT
        && unsafe { (*param).reg.status } == esp_gatt_status_t_ESP_GATT_OK
    {
        s.gattc_if = gattc_if;
    }
    if gattc_if == ESP_GATT_IF_NONE as esp_gatt_if_t || gattc_if == s.gattc_if {
        gattc_handler(s, event, gattc_if, param);
    }
}

unsafe extern "C" fn gap_cb(event: esp_gap_ble_cb_event_t, param: *mut esp_ble_gap_cb_param_t) {
    let s = ctx();

    match event {
        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_PARAM_SET_COMPLETE_EVT => start_scan(s),

        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_START_COMPLETE_EVT => {
            if unsafe { (*param).scan_start_cmpl.status } != esp_bt_status_t_ESP_BT_STATUS_SUCCESS {
                error!(target: TAG, "Scan start complete failed");
                s.scanning = false;
                publish_link(s);
            }
        }

        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_RESULT_EVT => {
            let scan_rst = unsafe { &mut (*param).scan_rst };
            if scan_rst.search_evt != esp_gap_search_evt_t_ESP_GAP_SEARCH_INQ_RES_EVT || s.connected
            {
                return;
            }

            let mut adv_name_len: u8 = 0;
            let adv_name = unsafe {
                esp_ble_resolve_adv_data_by_type(
                    scan_rst.ble_adv.as_mut_ptr(),
                    scan_rst.adv_data_len as u16 + scan_rst.scan_rsp_len as u16,
                    esp_ble_adv_data_type_ESP_BLE_AD_TYPE_NAME_CMPL,
                    &mut adv_name_len,
                )
            };
            if adv_name.is_null() || (adv_name_len as usize) < NAME_PREFIX.len() {
                return;
            }
            let Some(adv_name) = (unsafe {
                crate::memsafe::bytes_at_most(adv_name, adv_name_len as usize, 64)
            }) else {
                return;
            };
            if !adv_name.starts_with(NAME_PREFIX) {
                return;
            }

            info!(target: TAG, "Found Nano Cortex, connecting...");
            unsafe { esp_ble_gap_stop_scanning() };
            s.scanning = false;
            publish_link(s);
            unsafe {
                esp_ble_gattc_open(
                    s.gattc_if,
                    scan_rst.bda.as_mut_ptr(),
                    scan_rst.ble_addr_type,
                    true,
                )
            };
        }

        esp_gap_ble_cb_event_t_ESP_GAP_BLE_SCAN_STOP_COMPLETE_EVT => {
            s.scanning = false;
            publish_link(s);
        }

        _ => {}
    }
}

/* ------------------------------------------------------------------ C ABI */

#[no_mangle]
pub extern "C" fn nano_ble_connected() -> bool {
    LINK_UP.load(Ordering::Acquire)
}

#[no_mangle]
pub extern "C" fn nano_ble_scanning() -> bool {
    SCANNING.load(Ordering::Acquire)
}

/// Drop pending C305 packets (call on disconnect).
#[no_mangle]
pub extern "C" fn nano_ble_drain_rx_queue() {
    drain_rx_queue(ctx());
}

/// Drop the FIFO backlog only — keeps the live pitch mailbox. Call when the tuner goes live.
#[no_mangle]
pub extern "C" fn nano_ble_flush_c305_backlog() {
    let s = ctx();
    if !s.queue.is_null() {
        unsafe { xQueueGenericReset(s.queue, PD_FALSE) };
    }
}

/// Write payload to C304 (no-op if not connected).
#[no_mangle]
pub extern "C" fn nano_ble_write_c304(
    label: *const c_char,
    data: *const u8,
    len: usize,
) -> esp_err_t {
    let label = if label.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(label) })
    };

    let Some(link) = link_snap() else {
        warn!(
            target: TAG,
            "Skip write ({}): not ready",
            label.map_or("?", |l| l.to_str().unwrap_or("?"))
        );
        return ESP_ERR_INVALID_STATE;
    };
    if len == 0 || len > crate::memsafe::PKT_MAX || data.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    {
        let Some(bytes) = (unsafe { crate::memsafe::bytes(data, len) }) else {
            return ESP_ERR_INVALID_ARG;
        };
        let lbl = label.map_or("C304", |l| l.to_str().unwrap_or("C304"));
        nano_wire::log_bytes(TAG, lbl, bytes);
    }

    let err = unsafe {
        esp_ble_gattc_write_char(
            link.gattc_if,
            link.conn_id,
            link.c304,
            len as u16,
            data.cast_mut(),
            esp_gatt_write_type_t_ESP_GATT_WRITE_TYPE_RSP,
            esp_gatt_auth_req_t_ESP_GATT_AUTH_REQ_NONE,
        )
    };
    if err != ESP_OK {
        error!(target: TAG, "write_char failed: {}", err_name(err));
    }
    err
}

/// Read C300/C301/C307 (and log hex) — boot BPM hunt.
#[no_mangle]
pub extern "C" fn nano_ble_read_bpm_probe_chars() {
    let s = ctx();
    if !LINK_UP.load(Ordering::Acquire) {
        warn!(target: TAG, "BPM hunt: GATT read skipped (not connected)");
        return;
    }
    if s.bpm_read_count == 0 {
        warn!(target: TAG, "BPM hunt: no readable C300/C301/C307 handles");
        return;
    }
    s.bpm_read_next = 0;
    bpm_gatt_read_next(s);
}

/// Init NVS + BT controller, register the GATT client, start scanning.
#[no_mangle]
pub extern "C" fn nano_ble_start() -> esp_err_t {
    let s = ctx();
    s.gattc_if = ESP_GATT_IF_NONE as esp_gatt_if_t;
    s.conn_id = 0;
    s.remote_bda = [0; 6];
    s.connected = false;
    s.scanning = false;
    s.drop_count = 0;
    reset_handles(s);

    unsafe {
        let mut ret = nvs_flash_init();
        if ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND {
            check(nvs_flash_erase(), "nvs_flash_erase");
            ret = nvs_flash_init();
        }
        check(ret, "nvs_flash_init");

        check(
            esp_bt_controller_mem_release(esp_bt_mode_t_ESP_BT_MODE_CLASSIC_BT),
            "esp_bt_controller_mem_release",
        );

        let mut bt_cfg = MaybeUninit::<esp_bt_controller_config_t>::uninit();
        nano_bt_controller_config_default(bt_cfg.as_mut_ptr());
        let mut bt_cfg = bt_cfg.assume_init();
        check(esp_bt_controller_init(&mut bt_cfg), "esp_bt_controller_init");
        check(
            esp_bt_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE),
            "esp_bt_controller_enable",
        );
        check(esp_bluedroid_init(), "esp_bluedroid_init");
        check(esp_bluedroid_enable(), "esp_bluedroid_enable");

        check(
            esp_ble_gap_register_callback(Some(gap_cb)),
            "esp_ble_gap_register_callback",
        );
        check(
            esp_ble_gattc_register_callback(Some(gattc_cb)),
            "esp_ble_gattc_register_callback",
        );
        check(
            esp_ble_gattc_app_register(PROFILE_APP_ID),
            "esp_ble_gattc_app_register",
        );
    }

    if !c305_queue_init(s) {
        return ESP_ERR_NO_MEM;
    }

    info!(target: TAG, "BLE stack ready");
    ESP_OK
}
