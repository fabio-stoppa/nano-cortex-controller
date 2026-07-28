//! Shared FX state, per-block slot config and small helpers.

use core::ffi::c_char;
use core::ptr;
use std::ffi::CString;

use esp_idf_svc::sys::esp_timer_handle_t;

use crate::fx::types::*;
use crate::syncutil::{SeqLock, Shared};

pub(super) const TAG: &str = "nano_fx";
pub(super) const FX_REFRESH_TIMEOUT_US: u64 = 900 * 1000;
pub(super) const FX_REFRESH_GAP_US: u64 = 80 * 1000;
/// Coalesce Amount LED→poll off the BLE notify path.
pub(super) const FX_AMOUNT_UI_GAP_US: u64 = 45 * 1000;
/// Shorter inter-request gap when re-polling the same Amount-focused block.
pub(super) const FX_AMOUNT_REFRESH_GAP_US: u64 = 30 * 1000;
/// Skip Amount poll if a live param echo for that block landed this recently.
pub(super) const FX_AMOUNT_LIVE_SKIP_US: i64 = 100 * 1000;
/// After a refresh timeout, drop late replies before starting the next request
/// (responses are untagged — a late packet would corrupt the next block).
pub(super) const FX_REFRESH_STALE_GUARD_US: u64 = 150 * 1000;
pub(super) const FX_ENABLE_ASSERT_US: i64 = 3000 * 1000;
pub(super) const FX_MODEL_WRITE_ASSERT_US: i64 = 30 * 1000 * 1000;

/// Per-block write/refresh slots and the preset field carrying the model ID.
pub(super) struct SlotCfg {
    pub(super) model_slot: u8,
    pub(super) refresh_slot: u8,
    pub(super) enable_slot: u8,
    pub(super) model_field: u32,
}

pub(super) const SLOT_CFG: [SlotCfg; NANO_FX_BLOCK_COUNT] = [
    SlotCfg { model_slot: 0x00, refresh_slot: 0x00, enable_slot: 0x04, model_field: 48 },
    SlotCfg { model_slot: 0x01, refresh_slot: 0x01, enable_slot: 0x05, model_field: 49 },
    SlotCfg { model_slot: 0x02, refresh_slot: 0x02, enable_slot: 0x06, model_field: 50 },
    SlotCfg { model_slot: 0x03, refresh_slot: 0x03, enable_slot: 0x07, model_field: 51 },
    SlotCfg { model_slot: 0x04, refresh_slot: 0x04, enable_slot: 0x08, model_field: 52 },
];

/// Device firmware also exposes 1-byte model IDs in these varint fields.
pub(super) const MODEL_FIELD_LEGACY: [u32; NANO_FX_BLOCK_COUNT] = [26, 9, 13, 14, 15];
/// Alternate field numbers seen on some firmware builds.
pub(super) const MODEL_FIELD_LEGACY_ALT: [u32; NANO_FX_BLOCK_COUNT] = [8, 10, 0, 0, 0];

pub(super) struct Fx {
    pub(super) fx: NanoFxState,
    pub(super) refresh_queue: [u32; NANO_FX_BLOCK_COUNT],
    pub(super) refresh_q_head: usize,
    pub(super) refresh_q_len: usize,
    pub(super) pending_block: u32,
    pub(super) refresh_active: bool,
    pub(super) refresh_timeout_timer: esp_timer_handle_t,
    pub(super) refresh_gap_timer: esp_timer_handle_t,
    pub(super) begin_refresh_timer: esp_timer_handle_t,
    pub(super) amount_ui_timer: esp_timer_handle_t,
    pub(super) block_on_asserted_us: [i64; NANO_FX_BLOCK_COUNT],
    pub(super) block_model_write_us: [i64; NANO_FX_BLOCK_COUNT],
    /// Protects local Gate ON/OFF against stale live-poll until assert window ends.
    pub(super) gate_assert_us: i64,
    /// -1 idle, 0 off, 1 on.
    pub(super) gate_assert_on: i32,
    /// Protects local Gate amount against stale field-53 until assert window ends.
    pub(super) gate_amount_assert_us: i64,
    /// -1 idle, else 0..100.
    pub(super) gate_amount_assert_pct: i32,
    /// Model last selected via BLE write; preset/inference must not override.
    pub(super) block_trusted_model: [*const NanoFxModelDef; NANO_FX_BLOCK_COUNT],
    pub(super) generic_model: NanoFxModelDef,
    pub(super) generic_param_ptrs: [*const c_char; NANO_FX_MAX_PARAMS],
    pub(super) generic_param_names: [[u8; 12]; NANO_FX_MAX_PARAMS],
    /// Last AMOUNT knob raw (0..255); -1 = none yet.
    pub(super) last_amount_raw: i16,
    /// Nano-selected FX block (model slot 0..=4); -1 = unknown.
    pub(super) pedal_fx_focus: i8,
    /// Block id snapshotted for the pending Amount poll log.
    pub(super) amount_log_block: i8,
    /// True when AMOUNT changed and a poll is due on the UI timer.
    pub(super) amount_log_pending: bool,
    /// Last live FX param notify time / block (skip redundant Amount polls).
    pub(super) live_param_us: i64,
    pub(super) live_param_block: i8,
    /// Pending refresh was queued by Amount (shorter gap after response).
    pub(super) amount_refresh: bool,
    /// Drop param-refresh replies until this time (post-timeout stale guard).
    pub(super) refresh_stale_until_us: i64,
    /// After the next preset sync, re-push UI FX focus to the Nano.
    pub(super) reassert_focus_after_sync: bool,
    /// Hold FX param paints until the visible block's refresh finishes.
    pub(super) suppress_fx_param_ui: bool,
}

impl Fx {
    const fn new() -> Self {
        Self {
            fx: NanoFxState::zeroed(),
            refresh_queue: [0; NANO_FX_BLOCK_COUNT],
            refresh_q_head: 0,
            refresh_q_len: 0,
            pending_block: NANO_FX_PRE1,
            refresh_active: false,
            refresh_timeout_timer: ptr::null_mut(),
            refresh_gap_timer: ptr::null_mut(),
            begin_refresh_timer: ptr::null_mut(),
            amount_ui_timer: ptr::null_mut(),
            block_on_asserted_us: [0; NANO_FX_BLOCK_COUNT],
            block_model_write_us: [0; NANO_FX_BLOCK_COUNT],
            gate_assert_us: 0,
            gate_assert_on: -1,
            gate_amount_assert_us: 0,
            gate_amount_assert_pct: -1,
            block_trusted_model: [ptr::null(); NANO_FX_BLOCK_COUNT],
            generic_model: NanoFxModelDef::empty(),
            generic_param_ptrs: [ptr::null(); NANO_FX_MAX_PARAMS],
            generic_param_names: [[0; 12]; NANO_FX_MAX_PARAMS],
            last_amount_raw: -1,
            pedal_fx_focus: -1,
            amount_log_block: -1,
            amount_log_pending: false,
            live_param_us: 0,
            live_param_block: -1,
            amount_refresh: false,
            refresh_stale_until_us: 0,
            reassert_focus_after_sync: false,
            suppress_fx_param_ui: false,
        }
    }
}

static FX: Shared<Fx> = Shared::new(Fx::new());
/// Odd/even seqlock for consistent UI snapshots of `NanoFxState`.
pub(super) static FX_SEQ: SeqLock = SeqLock::new();

pub(super) fn fx() -> &'static mut Fx {
    FX.get()
}

pub(super) fn fx_publish() {
    FX_SEQ.publish();
}

pub(super) fn push_fx_ui() {
    fx_publish();
    crate::ui::screen::nano_ui_set_fx(&fx().fx);
}

pub(super) fn now_us() -> i64 {
    unsafe { esp_idf_svc::sys::esp_timer_get_time() }
}

pub(super) fn c_label(label: &str) -> CString {
    CString::new(label).unwrap_or_default()
}

pub(super) fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
