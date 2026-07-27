//! Shared connection state (`Ctx`), link-snapshot atomics, constants, and the
//! small helpers used across the other BLE client submodules.

use super::*;

pub(super) const TAG: &str = "nano_ble";

pub(super) fn is_tuner_active() -> bool {
    crate::tuner::nano_tuner_is_active()
}

pub(super) fn parse_bpm(data: &[u8]) -> Option<u16> {
    nano_wire::try_parse_bpm_notify(data).or_else(|| nano_wire::parse_bpm(data))
}

pub(super) const NAME_PREFIX: &[u8] = b"Neural DSP Nano Cortex";

pub(super) const C305_QUEUE_LEN: u32 = 48;
pub(super) const C305_LIVE_QUEUE_LEN: u32 = 1;
/// Preset dumps often exceed one ATT MTU; 256 silently chopped the gate f53/f54 tail.
pub(super) const C305_PKT_MAX: usize = 512;
pub(super) const C305_TASK_STACK: u32 = 4096;
/// Keep below LVGL (prio 6) so continuous pitch cannot starve the display.
pub(super) const C305_TASK_PRIO: u32 = 1;
pub(super) const GATT_NOTIFY_MAX: usize = 16;
pub(super) const GATT_EXTRA_SVC_MAX: usize = 6;
pub(super) const BPM_READ_HANDLE_MAX: usize = 8;

pub(super) const PROFILE_APP_ID: u16 = 0;
pub(super) const INVALID_HANDLE: u16 = 0;

pub(super) const UUID_SVC_A002: u16 = 0xA002;
pub(super) const UUID_SVC_A003: u16 = 0xA003;
pub(super) const UUID_CHAR_C300: u16 = 0xC300;
pub(super) const UUID_CHAR_C301: u16 = 0xC301;
pub(super) const UUID_CHAR_C302: u16 = 0xC302;
pub(super) const UUID_CHAR_C304: u16 = 0xC304;
pub(super) const UUID_CHAR_C305: u16 = 0xC305;
pub(super) const UUID_CHAR_C306: u16 = 0xC306;
pub(super) const UUID_CHAR_C307: u16 = 0xC307;

/// BLE-MIDI service/char — Bluedroid little-endian 128-bit form.
pub(super) const UUID128_BLE_MIDI_SVC: [u8; 16] = [
    0x00, 0xC7, 0xC4, 0x4E, 0xE3, 0x6C, 0x51, 0xA7, 0x33, 0x4B, 0xE8, 0xED, 0x5A, 0x0E, 0xB8, 0x03,
];
pub(super) const UUID128_BLE_MIDI_IO: [u8; 16] = [
    0xF3, 0x6B, 0x10, 0x9D, 0x66, 0xF2, 0xA9, 0xA1, 0x12, 0x41, 0x68, 0x38, 0xDB, 0xE5, 0x72, 0x77,
];

pub(super) const PD_TRUE: BaseType_t = 1;
pub(super) const PD_FALSE: BaseType_t = 0;
pub(super) const PD_PASS: BaseType_t = 1;
pub(super) const QUEUE_SEND_TO_BACK: BaseType_t = 0;
pub(super) const QUEUE_OVERWRITE: BaseType_t = 2;
pub(super) const QUEUE_TYPE_BASE: u8 = 0;
pub(super) const NO_AFFINITY: BaseType_t = 0x7FFF_FFFF;
pub(super) const NOTIFY_INDEX: UBaseType_t = 0;

unsafe extern "C" {
    /// `BT_CONTROLLER_INIT_CONFIG_DEFAULT()` is a macro, so it needs the C shim in
    /// `components/nano/bt_cfg_helper.c`.
    pub(super) fn nano_bt_controller_config_default(cfg: *mut esp_bt_controller_config_t);
}

pub(super) fn ms_to_ticks(ms: u32) -> TickType_t {
    ms * configTICK_RATE_HZ / 1000
}

pub(super) fn err_name(err: esp_err_t) -> &'static str {
    unsafe { CStr::from_ptr(esp_err_to_name(err)) }
        .to_str()
        .unwrap_or("?")
}

pub(super) fn check(err: esp_err_t, what: &str) {
    if err != ESP_OK {
        panic!("{what} failed: {} ({err})", err_name(err));
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(super) struct C305Item {
    pub(super) handle: u16,
    pub(super) len: u16,
    pub(super) data: [u8; C305_PKT_MAX],
}

#[derive(Copy, Clone)]
pub(super) struct GattSub {
    pub(super) handle: u16,
    /// bit0=notify, bit1=indicate
    pub(super) cccd_value: u16,
}

pub(super) struct ExtraSvc {
    pub(super) start: u16,
    pub(super) end: u16,
    pub(super) name: String,
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

pub(super) struct Ctx {
    pub(super) gattc_if: esp_gatt_if_t,
    pub(super) conn_id: u16,
    pub(super) remote_bda: esp_bd_addr_t,
    pub(super) connected: bool,
    pub(super) scanning: bool,
    pub(super) a002_start: u16,
    pub(super) a002_end: u16,
    pub(super) a003_start: u16,
    pub(super) a003_end: u16,
    pub(super) c304_handle: u16,
    pub(super) c305_handle: u16,
    pub(super) c306_handle: u16,
    pub(super) midi_handle: u16,
    pub(super) notify_enabled: bool,

    pub(super) subs: [GattSub; GATT_NOTIFY_MAX],
    pub(super) sub_count: usize,

    pub(super) bpm_read_handles: [u16; BPM_READ_HANDLE_MAX],
    pub(super) bpm_read_count: usize,
    pub(super) bpm_read_next: usize,

    pub(super) extra_svcs: [ExtraSvc; GATT_EXTRA_SVC_MAX],
    pub(super) extra_svc_count: usize,

    pub(super) queue: QueueHandle_t,
    pub(super) live_queue: QueueHandle_t,
    pub(super) task: TaskHandle_t,
    pub(super) drop_count: u32,
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

use crate::syncutil::Shared;

static CTX: Shared<Ctx> = Shared::new(Ctx::new());

/// Cross-task link snapshot — UI / writers read these instead of racing `Ctx`.
/// Publish order on teardown: `LINK_UP` false first, then handles / if / conn.
pub(super) static LINK_UP: AtomicBool = AtomicBool::new(false);
pub(super) static SCANNING: AtomicBool = AtomicBool::new(false);
pub(super) static GATTC_IF: AtomicI32 = AtomicI32::new(-1);
pub(super) static CONN_ID: AtomicU16 = AtomicU16::new(0);
pub(super) static C304_HANDLE: AtomicU16 = AtomicU16::new(INVALID_HANDLE);
pub(super) static C305_HANDLE: AtomicU16 = AtomicU16::new(INVALID_HANDLE);

pub(super) fn ctx() -> &'static mut Ctx {
    CTX.get()
}

pub(super) fn publish_link(s: &Ctx) {
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
pub(super) struct LinkSnap {
    pub(super) gattc_if: esp_gatt_if_t,
    pub(super) conn_id: u16,
    pub(super) c304: u16,
}

pub(super) fn link_snap() -> Option<LinkSnap> {
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

pub(super) fn uuid16(id: u16) -> esp_bt_uuid_t {
    esp_bt_uuid_t {
        len: ESP_UUID_LEN_16 as u16,
        uuid: esp_bt_uuid_t__bindgen_ty_1 { uuid16: id },
    }
}

pub(super) fn scan_params() -> esp_ble_scan_params_t {
    esp_ble_scan_params_t {
        scan_type: esp_ble_scan_type_t_BLE_SCAN_TYPE_ACTIVE,
        own_addr_type: esp_ble_addr_type_t_BLE_ADDR_TYPE_PUBLIC,
        scan_filter_policy: esp_ble_scan_filter_t_BLE_SCAN_FILTER_ALLOW_ALL,
        scan_interval: 0x50,
        scan_window: 0x30,
        scan_duplicate: esp_ble_scan_duplicate_t_BLE_SCAN_DUPLICATE_DISABLE,
    }
}

pub(super) fn format_uuid(u: esp_bt_uuid_t) -> String {
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

pub(super) fn uuid128_eq(u: esp_bt_uuid_t, expect: &[u8; 16]) -> bool {
    u.len as u32 == ESP_UUID_LEN_128 && unsafe { u.uuid.uuid128 } == *expect
}

pub(super) fn uuid_as_16(u: esp_bt_uuid_t) -> Option<u16> {
    (u.len as u32 == ESP_UUID_LEN_16)
        .then(|| unsafe { core::ptr::addr_of!(u.uuid.uuid16).read_unaligned() })
}

pub(super) fn reset_handles(s: &mut Ctx) {
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

pub(super) fn is_connected(s: &Ctx) -> bool {
    s.connected && s.c304_handle != INVALID_HANDLE && s.notify_enabled
}

pub(super) fn start_scan(s: &mut Ctx) {
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
