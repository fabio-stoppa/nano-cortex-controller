//! Connection state for BLE-MIDI footswitch peers (Chocolate + Chocolate Plus).

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, Ordering};

use esp_idf_svc::sys::*;

use crate::syncutil::Shared;

pub(super) const TAG: &str = "midi_fs";

/// Builty / M-Vave advertising names (matched case-insensitively).
pub(super) const NAME_FOOTCTRL: &[u8] = b"FootCtrl";
pub(super) const NAME_FOOTCTRL_PLUS: &[u8] = b"FootCtrlPlus";

pub(super) const PROFILE_APP_ID: u16 = 1;
pub(super) const INVALID_HANDLE: u16 = 0;
pub(super) const MAX_PEERS: usize = 2;

/// Same BLE-MIDI UUIDs as the Nano client (Bluedroid LE byte order).
pub(super) const UUID128_BLE_MIDI_SVC: [u8; 16] = [
    0x00, 0xC7, 0xC4, 0x4E, 0xE3, 0x6C, 0x51, 0xA7, 0x33, 0x4B, 0xE8, 0xED, 0x5A, 0x0E, 0xB8, 0x03,
];
pub(super) const UUID128_BLE_MIDI_IO: [u8; 16] = [
    0xF3, 0x6B, 0x10, 0x9D, 0x66, 0xF2, 0xA9, 0xA1, 0x12, 0x41, 0x68, 0x38, 0xDB, 0xE5, 0x72, 0x77,
];

pub(super) const MIDI_QUEUE_LEN: u32 = 16;
pub(super) const MIDI_PKT_MAX: usize = 64;
/// FX writes / tempo tap allocate format strings — 3K overflowed on first CC.
pub(super) const MIDI_TASK_STACK: u32 = 8192;
pub(super) const MIDI_TASK_PRIO: u32 = 1;

pub(super) const PD_TRUE: BaseType_t = 1;
pub(super) const PD_FALSE: BaseType_t = 0;
pub(super) const PD_PASS: BaseType_t = 1;
pub(super) const QUEUE_SEND_TO_BACK: BaseType_t = 0;
pub(super) const QUEUE_TYPE_BASE: u8 = 0;
pub(super) const NO_AFFINITY: BaseType_t = 0x7FFF_FFFF;

#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum PeerKind {
    Chocolate,
    ChocolatePlus,
}

impl PeerKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            PeerKind::Chocolate => "FootCtrl",
            PeerKind::ChocolatePlus => "FootCtrlPlus",
        }
    }

    pub(super) fn as_u8(self) -> u8 {
        match self {
            PeerKind::Chocolate => 0,
            PeerKind::ChocolatePlus => 1,
        }
    }

    pub(super) fn from_u8(v: u8) -> Self {
        if v == 1 {
            PeerKind::ChocolatePlus
        } else {
            PeerKind::Chocolate
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(super) struct MidiItem {
    pub(super) len: u16,
    pub(super) kind: u8,
    pub(super) data: [u8; MIDI_PKT_MAX],
}

pub(super) struct Peer {
    pub(super) kind: PeerKind,
    pub(super) remote_bda: esp_bd_addr_t,
    pub(super) conn_id: u16,
    pub(super) connected: bool,
    pub(super) notify_enabled: bool,
    pub(super) midi_start: u16,
    pub(super) midi_end: u16,
    pub(super) midi_handle: u16,
}

impl Peer {
    const fn empty() -> Self {
        Self {
            kind: PeerKind::Chocolate,
            remote_bda: [0; 6],
            conn_id: 0,
            connected: false,
            notify_enabled: false,
            midi_start: INVALID_HANDLE,
            midi_end: INVALID_HANDLE,
            midi_handle: INVALID_HANDLE,
        }
    }

    fn is_link_up(&self) -> bool {
        self.connected && self.notify_enabled && self.midi_handle != INVALID_HANDLE
    }

    pub(super) fn reset_handles(&mut self) {
        self.notify_enabled = false;
        self.midi_start = INVALID_HANDLE;
        self.midi_end = INVALID_HANDLE;
        self.midi_handle = INVALID_HANDLE;
    }

    pub(super) fn clear(&mut self) {
        *self = Self::empty();
    }
}

pub(super) struct Ctx {
    pub(super) gattc_if: esp_gatt_if_t,
    /// BDA we called `gattc_open` for — ignore CONNECT for the Nano peer.
    pub(super) pending_bda: Option<esp_bd_addr_t>,
    pub(super) pending_kind: PeerKind,
    pub(super) peers: [Peer; MAX_PEERS],
    pub(super) queue: QueueHandle_t,
    pub(super) task: TaskHandle_t,
}

impl Ctx {
    const fn new() -> Self {
        Self {
            gattc_if: ESP_GATT_IF_NONE as esp_gatt_if_t,
            pending_bda: None,
            pending_kind: PeerKind::Chocolate,
            peers: [Peer::empty(), Peer::empty()],
            queue: ptr::null_mut(),
            task: ptr::null_mut(),
        }
    }
}

static CTX: Shared<Ctx> = Shared::new(Ctx::new());

pub(super) static LINK_UP: AtomicBool = AtomicBool::new(false);
pub(super) static GATTC_IF: AtomicI32 = AtomicI32::new(-1);
pub(super) static CONN_ID: AtomicU16 = AtomicU16::new(0);

pub(super) fn ctx() -> &'static mut Ctx {
    CTX.get()
}

pub(super) fn publish_link(s: &Ctx) {
    let up = s.peers.iter().any(|p| p.is_link_up());
    if !up {
        LINK_UP.store(false, Ordering::Release);
    }
    GATTC_IF.store(s.gattc_if as i32, Ordering::Release);
    if let Some(p) = s.peers.iter().find(|p| p.is_link_up()) {
        CONN_ID.store(p.conn_id, Ordering::Release);
    } else {
        CONN_ID.store(0, Ordering::Release);
    }
    if up {
        LINK_UP.store(true, Ordering::Release);
    }
}

pub(super) fn is_link_up() -> bool {
    LINK_UP.load(Ordering::Acquire)
}

pub(super) fn have_kind(s: &Ctx, kind: PeerKind) -> bool {
    s.peers.iter().any(|p| p.connected && p.kind == kind)
        || (s.pending_bda.is_some() && s.pending_kind == kind)
}

pub(super) fn need_peer() -> bool {
    let s = ctx();
    !have_kind(s, PeerKind::Chocolate) || !have_kind(s, PeerKind::ChocolatePlus)
}

pub(super) fn free_peer_slot(s: &Ctx) -> Option<usize> {
    s.peers.iter().position(|p| !p.connected)
}

pub(super) fn peer_by_conn(s: &mut Ctx, conn_id: u16) -> Option<&mut Peer> {
    s.peers
        .iter_mut()
        .find(|p| p.connected && p.conn_id == conn_id)
}

pub(super) fn peer_idx_by_conn(s: &Ctx, conn_id: u16) -> Option<usize> {
    s.peers
        .iter()
        .position(|p| p.connected && p.conn_id == conn_id)
}

pub(super) fn peer_idx_by_midi_handle(s: &Ctx, handle: u16) -> Option<usize> {
    s.peers
        .iter()
        .position(|p| p.connected && p.midi_handle == handle)
}

fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

/// Classify a complete local name as Chocolate / Chocolate Plus.
pub(super) fn classify_name(adv_name: &[u8]) -> Option<PeerKind> {
    // Plus first — exact length match, but keep order explicit.
    if eq_ignore_ascii_case(adv_name, NAME_FOOTCTRL_PLUS) {
        Some(PeerKind::ChocolatePlus)
    } else if eq_ignore_ascii_case(adv_name, NAME_FOOTCTRL) {
        Some(PeerKind::Chocolate)
    } else {
        None
    }
}

pub(super) fn name_matches(adv_name: &[u8]) -> bool {
    classify_name(adv_name).is_some()
}

pub(super) fn uuid16(id: u16) -> esp_bt_uuid_t {
    esp_bt_uuid_t {
        len: ESP_UUID_LEN_16 as u16,
        uuid: esp_bt_uuid_t__bindgen_ty_1 { uuid16: id },
    }
}

pub(super) fn uuid128_eq(u: esp_bt_uuid_t, expect: &[u8; 16]) -> bool {
    u.len as u32 == ESP_UUID_LEN_128 && unsafe { u.uuid.uuid128 } == *expect
}

pub(super) fn err_name(err: esp_err_t) -> &'static str {
    unsafe { core::ffi::CStr::from_ptr(esp_err_to_name(err)) }
        .to_str()
        .unwrap_or("?")
}

pub(super) fn ms_to_ticks(ms: u32) -> TickType_t {
    ms * configTICK_RATE_HZ / 1000
}
