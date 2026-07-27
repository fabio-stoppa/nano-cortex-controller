/* ----------------------------------------------------------------- state */

use super::*;
use super::consts::{
    CHAIN_AMP, CHAIN_BLOCK_COUNT, CHAIN_LABEL_MAX, KNOB_COUNT, NANO_AMP_CAPTURE_NAME_MAX,
    NANO_PRESET_NAME_MAX,
};
#[derive(Clone, Copy)]
pub(super) struct AmpKnobUi {
    pub(super) slider: *mut lv_obj_t,
    pub(super) value: *mut lv_obj_t,
}

#[derive(Clone, Copy)]
pub(super) struct GateCellUi {
    pub(super) col: *mut lv_obj_t,
    pub(super) name: *mut lv_obj_t,
    pub(super) slider: *mut lv_obj_t,
    pub(super) value: *mut lv_obj_t,
}

#[derive(Clone, Copy)]
pub(super) struct FxParamUi {
    pub(super) col: *mut lv_obj_t,
    pub(super) name: *mut lv_obj_t,
    pub(super) slider: *mut lv_obj_t,
    pub(super) value: *mut lv_obj_t,
}

#[derive(Clone, Copy)]
pub(super) struct ChainBlockUi {
    pub(super) col: *mut lv_obj_t,
    pub(super) btn: *mut lv_obj_t,
    pub(super) icon: *mut lv_obj_t,
    pub(super) icon_text: [u8; CHAIN_LABEL_MAX],
    pub(super) icon_color: lv_color_t,
    pub(super) last_active: bool,
    pub(super) last_selected: bool,
    pub(super) last_accent: lv_color_t,
}

#[derive(Clone, Copy)]
pub(super) struct ParamGesture {
    pub(super) active: bool,
    pub(super) mode: u8,
    pub(super) origin: lv_point_t,
    pub(super) press_us: i64,
}

pub(super) struct Ui {
    pub(super) ble_icon: *mut lv_obj_t,
    pub(super) bpm_lbl: *mut lv_obj_t,
    pub(super) ble_icon_timer: *mut lv_timer_t,
    pub(super) ble_blink_on: bool,

    pub(super) tuner_note: *mut lv_obj_t,
    pub(super) tuner_cents: *mut lv_obj_t,
    pub(super) tuner_state: *mut lv_obj_t,
    pub(super) tuner_btn: *mut lv_obj_t,
    pub(super) tuner_meter: *mut lv_obj_t,
    pub(super) tuner_meter_center: *mut lv_obj_t,
    pub(super) tuner_meter_needle: *mut lv_obj_t,

    pub(super) knobs: [AmpKnobUi; KNOB_COUNT],
    pub(super) ui_sync: bool,
    pub(super) ble_connected: bool,
    pub(super) drag_knob: i32,
    pub(super) last_sent: [u8; KNOB_COUNT],
    pub(super) last_send_us: [i64; KNOB_COUNT],

    pub(super) async_amp: NanoAmpC,
    pub(super) async_amp_have: bool,
    pub(super) async_tuner: NanoTunerC,
    pub(super) async_tuner_pending: bool,
    pub(super) async_ble_pending: bool,
    pub(super) async_ble_connected: bool,

    pub(super) gate_on: GateCellUi,
    pub(super) gate: GateCellUi,
    pub(super) cap_vol: GateCellUi,
    pub(super) gate_on_drag: i32,
    pub(super) gate_on_last_sent: i32,
    pub(super) gate_on_last_send_us: i64,
    pub(super) gate_drag: i32,
    pub(super) gate_last_sent: i32,
    pub(super) gate_last_send_us: i64,
    pub(super) cap_vol_drag: i32,
    pub(super) cap_vol_last_sent: i32,
    pub(super) cap_vol_last_send_us: i64,
    pub(super) chain_suppress_click: i32,

    pub(super) amp_chrome_valid: bool,
    pub(super) amp_chrome_have_slot: bool,
    pub(super) amp_chrome_slot: u8,
    pub(super) amp_chrome_name: [u8; NANO_AMP_CAPTURE_NAME_MAX],

    pub(super) header_bar: *mut lv_obj_t,
    pub(super) preset_lbl: *mut lv_obj_t,
    pub(super) chain_bar: *mut lv_obj_t,
    pub(super) content_root: *mut lv_obj_t,
    pub(super) content_w_normal: i32,
    pub(super) content_x_normal: i32,
    pub(super) body_h: i32,
    pub(super) amp_panel: *mut lv_obj_t,
    pub(super) fx_panel: *mut lv_obj_t,
    pub(super) tuner_panel: *mut lv_obj_t,
    pub(super) tuner_hdr_btn: *mut lv_obj_t,
    pub(super) tuner_view: bool,
    pub(super) tempo_panel: *mut lv_obj_t,
    pub(super) tempo_bpm_lbl: *mut lv_obj_t,
    pub(super) tempo_tap_btn: *mut lv_obj_t,
    pub(super) tempo_scope_sw: *mut lv_obj_t,
    pub(super) tempo_scope_lbl: *mut lv_obj_t,
    pub(super) tempo_view: bool,
    pub(super) tempo_scope_global: bool,

    pub(super) chain: [ChainBlockUi; CHAIN_BLOCK_COUNT],
    pub(super) chain_selected: usize,

    pub(super) fx_model_btn: *mut lv_obj_t,
    pub(super) fx_model_btn_lbl: *mut lv_obj_t,
    pub(super) fx_model_picker: *mut lv_obj_t,
    pub(super) fx_model_picker_grid: *mut lv_obj_t,
    pub(super) fx_model_picker_open: bool,
    pub(super) header_picker_mode: u8,
    pub(super) fx_model_btn_style: *mut lv_style_t,

    pub(super) amp_params_row: *mut lv_obj_t,
    pub(super) fx_params_row: *mut lv_obj_t,
    pub(super) fx_build_timer: *mut lv_timer_t,
    pub(super) param_cell_w: i32,
    pub(super) param_cell_h: i32,
    pub(super) fx_params: [FxParamUi; NANO_FX_MAX_PARAMS],
    pub(super) fx_selected: u32,
    pub(super) fx_scroll_anchor: u32,
    pub(super) fx_ui_sync: bool,
    pub(super) fx_drag_param: i32,
    pub(super) fx_last_sent: [i32; NANO_FX_MAX_PARAMS],
    pub(super) fx_last_send_us: [i64; NANO_FX_MAX_PARAMS],

    pub(super) async_fx: NanoFxState,
    pub(super) async_fx_pending: bool,
    pub(super) async_bpm: u16,
    pub(super) async_bpm_valid: bool,
    pub(super) async_bpm_pending: bool,
    pub(super) ui_bpm_cache: u16,
    pub(super) ui_bpm_cache_valid: bool,
    pub(super) async_tempo_mode: bool,
    pub(super) async_tempo_mode_pending: bool,
    pub(super) async_preset: [u8; NANO_PRESET_NAME_MAX],
    pub(super) ui_preset_cache: [u8; NANO_PRESET_NAME_MAX],
    pub(super) ui_preset_cache_valid: bool,
    pub(super) async_preset_pending: bool,
    pub(super) ui_amp_cache: NanoAmpC,
    pub(super) ui_amp_cache_valid: bool,
    pub(super) ui_fx_cache: NanoFxState,
    pub(super) ui_fx_cache_valid: bool,

    pub(super) pedal_params_scroll_row: *mut lv_obj_t,
    pub(super) pedal_params_scroll_on: bool,
    pub(super) pedal_params_scroll_inited: bool,
    pub(super) gate_style_valid: bool,
    pub(super) gate_style_on: bool,

    pub(super) param_gesture: ParamGesture,

    pub(super) dispatch_ready: bool,
    pub(super) dispatch_mask: u8,
    pub(super) dispatch_timer: *mut lv_timer_t,
}

impl Ui {
    const fn new() -> Ui {
        // Every field is a pointer, integer, bool or POD C struct, so the C
        // file's implicit zero-init for statics carries over verbatim.
        let mut u: Ui = unsafe { core::mem::zeroed() };
        u.drag_knob = -1;
        u.gate_on_last_sent = -1;
        u.gate_last_sent = -1;
        u.cap_vol_last_sent = -1;
        u.chain_suppress_click = -1;
        u.chain_selected = CHAIN_AMP;
        u.tempo_scope_global = true;
        u.fx_drag_param = -1;
        u
    }
}

pub(super) struct UiCell(UnsafeCell<Ui>);

// Widget tree: LVGL task only for paint. Cross-task setters use `with_inbox`
// (mutex) to stage async_* snapshots — see `INBOX_LOCK`.
unsafe impl Sync for UiCell {}

pub(super) static UI: UiCell = UiCell(UnsafeCell::new(Ui::new()));

/// Serializes staging fields (`async_*`, caches, `dispatch_mask`) between BLE /
/// state / FX tasks and the LVGL dispatch timer.
pub(super) static INBOX_LOCK: Mutex<()> = Mutex::new(());

/// FreeRTOS handle of the LVGL task (set on first UI timer tick).
pub(super) static LVGL_TASK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

#[inline]
pub(super) fn ui() -> &'static mut Ui {
    unsafe { &mut *UI.0.get() }
}

pub(super) fn note_lvgl_task() {
    let handle = unsafe { xTaskGetCurrentTaskHandle() };
    LVGL_TASK.store(handle.cast(), Ordering::Release);
}

pub(super) fn on_lvgl_task() -> bool {
    let expected = LVGL_TASK.load(Ordering::Acquire);
    if expected.is_null() {
        return true; // before first LVGL tick
    }
    unsafe { xTaskGetCurrentTaskHandle().cast() == expected }
}

pub(super) fn assert_lvgl_task(where_: &str) {
    debug_assert!(
        on_lvgl_task(),
        "UI paint must run on the LVGL task ({where_})"
    );
}

pub(super) fn with_inbox<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    let _guard = INBOX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    f(ui())
}

pub(super) static BUILD_DONE: AtomicBool = AtomicBool::new(false);

