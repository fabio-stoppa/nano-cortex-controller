//! `#[repr(C)]` mirrors of the structs and enums in `components/nano/nano_fx.h`.
//!
//! The C UI reads these by pointer, so field order and padding must match the
//! header exactly.

use core::ffi::c_char;
use core::ptr;

pub const NANO_FX_BLOCK_COUNT: usize = 5;
pub const NANO_FX_MAX_PARAMS: usize = 24;

/// `nano_fx_block_id_t` — a C enum, i.e. `int`-sized on Xtensa.
pub const NANO_FX_PRE1: u32 = 0;
pub const NANO_FX_PRE2: u32 = 1;
pub const NANO_FX_POST1: u32 = 2;
pub const NANO_FX_POST2: u32 = 3;
pub const NANO_FX_POST3: u32 = 4;

/// `nano_fx_category_t`.
pub const NANO_FX_CAT_NONE: u32 = 0;
pub const NANO_FX_CAT_GUITAR_OD: u32 = 1;
pub const NANO_FX_CAT_BASS_OD: u32 = 2;
pub const NANO_FX_CAT_COMPRESSOR: u32 = 3;
pub const NANO_FX_CAT_PITCH: u32 = 4;
pub const NANO_FX_CAT_FILTER: u32 = 5;
pub const NANO_FX_CAT_WAH: u32 = 6;
pub const NANO_FX_CAT_UTILITY: u32 = 7;
pub const NANO_FX_CAT_MODULATION: u32 = 8;
pub const NANO_FX_CAT_DELAY: u32 = 9;
pub const NANO_FX_CAT_REVERB: u32 = 10;

/// Mirror of `nano_fx_model_def_t`.
#[repr(C)]
pub struct NanoFxModelDef {
    pub name: *const c_char,
    pub id_len: u8,
    pub id: [u8; 3],
    pub param_count: u8,
    pub param_names: *const *const c_char,
}

// Catalog entries are immutable `const char *` tables shared with the C UI.
unsafe impl Sync for NanoFxModelDef {}

impl NanoFxModelDef {
    pub const fn empty() -> Self {
        Self {
            name: ptr::null(),
            id_len: 0,
            id: [0; 3],
            param_count: 0,
            param_names: ptr::null(),
        }
    }

    /// Catalog wire ID (`id[..id_len]`).
    pub fn id_bytes(&self) -> &[u8] {
        &self.id[..(self.id_len as usize).min(self.id.len())]
    }
}

/// Holder for the `const char *const []` param-name tables in the catalog.
#[repr(transparent)]
pub struct ParamNames<const N: usize>(pub [*const c_char; N]);

unsafe impl<const N: usize> Sync for ParamNames<N> {}

impl<const N: usize> ParamNames<N> {
    pub const fn as_ptr(&self) -> *const *const c_char {
        self.0.as_ptr()
    }
}

/// Mirror of `nano_fx_param_t`.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct NanoFxParam {
    pub normalized: f32,
    pub display: f32,
    pub valid: bool,
}

impl NanoFxParam {
    pub const fn zeroed() -> Self {
        Self {
            normalized: 0.0,
            display: 0.0,
            valid: false,
        }
    }
}

/// Mirror of `nano_gate_t`.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct NanoGate {
    pub on: bool,
    pub valid: bool,
    /// True when ON/OFF came from field 54 or a full dump (safe to apply).
    pub on_known: bool,
    /// True when `reduction_pct` came from preset field 53 or a local write.
    pub reduction_valid: bool,
    pub reduction_pct: u8,
}

impl NanoGate {
    pub const fn zeroed() -> Self {
        Self {
            on: false,
            valid: false,
            on_known: false,
            reduction_valid: false,
            reduction_pct: 0,
        }
    }
}

/// Mirror of `nano_fx_block_t`.
#[repr(C)]
pub struct NanoFxBlock {
    pub id: u32,
    pub on: bool,
    pub valid: bool,
    pub model: *const NanoFxModelDef,
    pub model_id_raw: [u8; 8],
    pub model_id_len: u8,
    pub params: [NanoFxParam; NANO_FX_MAX_PARAMS],
    pub param_count: u8,
    pub params_loaded: bool,
    pub has_model_id: bool,
}

impl NanoFxBlock {
    pub const fn zeroed() -> Self {
        Self {
            id: 0,
            on: false,
            valid: false,
            model: ptr::null(),
            model_id_raw: [0; 8],
            model_id_len: 0,
            params: [NanoFxParam::zeroed(); NANO_FX_MAX_PARAMS],
            param_count: 0,
            params_loaded: false,
            has_model_id: false,
        }
    }

    /// Model ID bytes captured from the preset (`model_id_raw[..model_id_len]`).
    pub fn model_id(&self) -> &[u8] {
        &self.model_id_raw[..(self.model_id_len as usize).min(self.model_id_raw.len())]
    }
}

/// Mirror of `nano_fx_state_t`.
#[repr(C)]
pub struct NanoFxState {
    pub gate: NanoGate,
    pub blocks: [NanoFxBlock; NANO_FX_BLOCK_COUNT],
    pub valid: bool,
}

impl NanoFxState {
    pub const fn zeroed() -> Self {
        Self {
            gate: NanoGate::zeroed(),
            blocks: [
                NanoFxBlock::zeroed(),
                NanoFxBlock::zeroed(),
                NanoFxBlock::zeroed(),
                NanoFxBlock::zeroed(),
                NanoFxBlock::zeroed(),
            ],
            valid: false,
        }
    }

    /// `memset(&state, 0, sizeof(state))` plus the block IDs the C port seeds.
    pub fn reset_with_ids() -> Self {
        let mut state = Self::zeroed();
        for (i, block) in state.blocks.iter_mut().enumerate() {
            block.id = i as u32;
        }
        state
    }
}

/// The C UI reads these by pointer, so hold them against the bindgen view of
/// `nano_fx.h`: any header drift has to break the build, not the display.
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    use esp_idf_svc::sys as sys;

    assert!(size_of::<NanoFxModelDef>() == size_of::<sys::nano_fx_model_def_t>());
    assert!(align_of::<NanoFxModelDef>() == align_of::<sys::nano_fx_model_def_t>());
    assert!(
        offset_of!(NanoFxModelDef, param_names)
            == offset_of!(sys::nano_fx_model_def_t, param_names)
    );

    assert!(size_of::<NanoFxParam>() == size_of::<sys::nano_fx_param_t>());
    assert!(size_of::<NanoGate>() == size_of::<sys::nano_gate_t>());

    assert!(size_of::<NanoFxBlock>() == size_of::<sys::nano_fx_block_t>());
    assert!(align_of::<NanoFxBlock>() == align_of::<sys::nano_fx_block_t>());
    assert!(offset_of!(NanoFxBlock, model) == offset_of!(sys::nano_fx_block_t, model));
    assert!(offset_of!(NanoFxBlock, params) == offset_of!(sys::nano_fx_block_t, params));
    assert!(
        offset_of!(NanoFxBlock, has_model_id) == offset_of!(sys::nano_fx_block_t, has_model_id)
    );

    assert!(size_of::<NanoFxState>() == size_of::<sys::nano_fx_state_t>());
    assert!(offset_of!(NanoFxState, blocks) == offset_of!(sys::nano_fx_state_t, blocks));
    assert!(offset_of!(NanoFxState, valid) == offset_of!(sys::nano_fx_state_t, valid));

    assert!(NANO_FX_BLOCK_COUNT == sys::NANO_FX_BLOCK_COUNT as usize);
    assert!(NANO_FX_MAX_PARAMS == sys::NANO_FX_MAX_PARAMS as usize);
};
