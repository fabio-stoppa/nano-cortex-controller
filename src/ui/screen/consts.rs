/* ------------------------------------------------------------- constants */

pub(super) const KNOB_COUNT: usize = 5;
pub(super) const DRAG_WRITE_INTERVAL_US: i64 = 60 * 1000;

pub(super) const FX_MODEL_PICKER_COLS: i32 = 3;
pub(super) const FX_MODEL_PICKER_GAP: i32 = 8;
pub(super) const FX_MODEL_PICKER_PAD: i32 = 12;
pub(super) const FX_MODEL_PICKER_TILE_H: i32 = 64;
pub(super) const FX_MODEL_PICKER_SCROLL_PAD: i32 = 16;

pub(super) const UI_DISP_AMP: u8 = 1 << 0;
pub(super) const UI_DISP_BLE: u8 = 1 << 1;
pub(super) const UI_DISP_TUNER: u8 = 1 << 2;
pub(super) const UI_DISP_FX: u8 = 1 << 3;
pub(super) const UI_DISP_BPM: u8 = 1 << 4;
pub(super) const UI_DISP_PRESET: u8 = 1 << 5;
pub(super) const UI_DISP_TEMPO: u8 = 1 << 6;
pub(super) const UI_DISP_FX_FOCUS: u8 = 1 << 7;

pub(super) const CHAIN_BLOCK_COUNT: usize = 6;
pub(super) const UI_EDGE_PAD: i32 = 1;
pub(super) const HEADER_H: i32 = 48;
pub(super) const HEADER_DD_H: i32 = HEADER_H - 4;
pub(super) const HEADER_RIGHT_PAD: i32 = 4;
pub(super) const CHAIN_RAIL_PAD: i32 = 4;
pub(super) const CHAIN_TAB_GAP: i32 = 8;
pub(super) const CHAIN_RAIL_BOTTOM_PAD: i32 = 4;
pub(super) const CONTENT_CHAIN_GAP: i32 = 4;
pub(super) const CHAIN_TAB_BORDER_W: i32 = 1;
pub(super) const CHAIN_TAB_BORDER_SEL: i32 = 2;

pub(super) const CHAIN_RAIL_W: i32 = 112;
pub(super) const CHAIN_TAB_W: i32 = 104;
pub(super) const CHAIN_TAB_H: i32 = 64;
pub(super) const CHAIN_ICON_W: i32 = 94;

pub(super) const CHAIN_PRE1: usize = 0;
pub(super) const CHAIN_PRE2: usize = 1;
pub(super) const CHAIN_AMP: usize = 2;
pub(super) const CHAIN_PFX1: usize = 3;
pub(super) const CHAIN_PFX2: usize = 4;
pub(super) const CHAIN_PFX3: usize = 5;

pub(super) const CHAIN_LABEL_MAX: usize = 12;

pub(super) const PARAM_GRID_COLS: i32 = 4;
pub(super) const PARAM_GRID_ROWS: i32 = 4;
pub(super) const PEDAL_PARAM_GRID_VISIBLE: u8 = (PARAM_GRID_COLS * PARAM_GRID_ROWS) as u8;
pub(super) const AMP_PEDAL_PARAM_COUNT: u8 = 8;
pub(super) const PARAM_GRID_GAP: i32 = 4;
pub(super) const PARAM_GRID_PAD: i32 = 2;
pub(super) const PARAM_CELL_PAD: i32 = 4;
pub(super) const PARAM_CELL_ROW_GAP: i32 = 7;
pub(super) const PARAM_LABEL_MAX_LEN: usize = 11;
pub(super) const PARAM_SLIDER_INSET_H: i32 = 14;
pub(super) const PARAM_GESTURE_SETTLE_MS: i64 = 150;
pub(super) const PARAM_GESTURE_MOVE_THRESH: i32 = 12;
pub(super) const PARAM_GESTURE_SCROLL_MIN_DY: i32 = 8;
pub(super) const PARAM_GESTURE_TAP_THRESH: i32 = 10;
pub(super) const PARAM_GRID_SCROLLBAR_W: i32 = 8;

pub(super) const PARAM_CELL_AMP_TAG: usize = 0x100;
pub(super) const PARAM_CELL_GATE_TAG: usize = 0x200;
pub(super) const PARAM_CELL_GATE_ON_TAG: usize = 0x400;
pub(super) const PARAM_CELL_CAP_VOL_TAG: usize = 0x800;

pub(super) const TUNER_METER_W: i32 = 560;
pub(super) const TUNER_METER_H: i32 = 36;
pub(super) const TUNER_NEEDLE_W: i32 = 8;
pub(super) const TUNER_CENTS_SPAN: f32 = 50.0;
pub(super) const TUNER_IN_TUNE_CENTS: f32 = 2.0;
pub(super) const TUNER_COLOR_OK: u32 = 0x3DCC7A;
pub(super) const TUNER_COLOR_IDLE: u32 = 0x606060;
pub(super) const TUNER_COLOR_NEEDLE: u32 = 0xFFFFFF;

pub(super) const NANO_PRESET_NAME_MAX: usize = 48;
pub(super) const NANO_AMP_CAPTURE_NAME_MAX: usize = 40;
pub(super) const NANO_AMP_CAPTURE_COUNT: u8 = 25;
pub(super) const NANO_BPM_MIN: u16 = 40;
pub(super) const NANO_BPM_MAX: u16 = 240;

/// Keep LVGL awake for live tuner (~12.5 Hz pitch → paint a bit faster).
pub(super) const UI_DISPATCH_PERIOD_MS: u32 = 8;

pub(super) const HEADER_PICKER_FX: u8 = 0;
pub(super) const HEADER_PICKER_AMP: u8 = 1;

pub(super) const PARAM_GESTURE_NONE: u8 = 0;
pub(super) const PARAM_GESTURE_PENDING: u8 = 1;
pub(super) const PARAM_GESTURE_SCROLL: u8 = 2;
pub(super) const PARAM_GESTURE_ADJUST: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum IconKind {
    Gate,
    Amp,
    GuitarOd,
    BassOd,
    Compressor,
    Pitch,
    Filter,
    Wah,
    Utility,
}

