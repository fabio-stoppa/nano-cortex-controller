//! FX model catalog (port of `components/nano/nano_fx_catalog.c`).
//!
//! Transcribed 1:1 from the C table (archived in `recovered/fx-c/`); the
//! catalog index also drives category and per-slot allow rules in `models.rs`.

#![allow(non_upper_case_globals)]

use super::types::{NanoFxModelDef, ParamNames};

static PARAMS_0: ParamNames<3> = ParamNames([
    c"Gain".as_ptr(),
    c"Tone".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_1: ParamNames<2> = ParamNames([
    c"Gain".as_ptr(),
    c"Level".as_ptr(),
]);

static PARAMS_2: ParamNames<4> = ParamNames([
    c"Gain".as_ptr(),
    c"Bass".as_ptr(),
    c"Treble".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_3: ParamNames<4> = ParamNames([
    c"Gain".as_ptr(),
    c"Bass".as_ptr(),
    c"Treble".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_4: ParamNames<4> = ParamNames([
    c"Gain".as_ptr(),
    c"Bass".as_ptr(),
    c"Treble".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_5: ParamNames<4> = ParamNames([
    c"Fuzz".as_ptr(),
    c"Volume".as_ptr(),
    c"Pickup (HB/Single)".as_ptr(),
    c"Pickup Level".as_ptr(),
]);

static PARAMS_6: ParamNames<3> = ParamNames([
    c"Overdrive".as_ptr(),
    c"Tone".as_ptr(),
    c"Level".as_ptr(),
]);

static PARAMS_7: ParamNames<6> = ParamNames([
    c"Drive".as_ptr(),
    c"Growl (On/Off)".as_ptr(),
    c"Midboost (On/Off)".as_ptr(),
    c"Tone".as_ptr(),
    c"Level".as_ptr(),
    c"Blend".as_ptr(),
]);

static PARAMS_8: ParamNames<2> = ParamNames([
    c"Gain".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_9: ParamNames<4> = ParamNames([
    c"Drive".as_ptr(),
    c"Peak (LP/HP)".as_ptr(),
    c"Tone".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_10: ParamNames<3> = ParamNames([
    c"Distortion".as_ptr(),
    c"Filter".as_ptr(),
    c"Volume".as_ptr(),
]);

static PARAMS_11: ParamNames<1> = ParamNames([
    c"Noise Reduction".as_ptr(),
]);

static PARAMS_12: ParamNames<12> = ParamNames([
    c"65Hz".as_ptr(),
    c"125Hz".as_ptr(),
    c"250Hz".as_ptr(),
    c"500hz".as_ptr(),
    c"1kHz".as_ptr(),
    c"2kHz".as_ptr(),
    c"4kHz".as_ptr(),
    c"8kHz".as_ptr(),
    c"16kHz".as_ptr(),
    c"HPF".as_ptr(),
    c"LPF".as_ptr(),
    c"Output".as_ptr(),
]);

static PARAMS_13: ParamNames<5> = ParamNames([
    c"HPF Slope".as_ptr(),
    c"HPF Freq".as_ptr(),
    c"LPF Slope".as_ptr(),
    c"LPF Freq".as_ptr(),
    c"Output".as_ptr(),
]);

static PARAMS_14: ParamNames<16> = ParamNames([
    c"1 Gain".as_ptr(),
    c"1 Freq".as_ptr(),
    c"1 Q".as_ptr(),
    c"1 Type (Peak/Hi Pass/Lo Pass/Hi Shelf/Lo Shelf)".as_ptr(),
    c"1 Active (On/Off)".as_ptr(),
    c"2 Gain".as_ptr(),
    c"2 Freq".as_ptr(),
    c"2 Q".as_ptr(),
    c"2 Type (Peak/Hi Pass/Lo Pass/Hi Shelf/Lo Shelf)".as_ptr(),
    c"2 Active (On/Off)".as_ptr(),
    c"3 Gain".as_ptr(),
    c"3 Freq".as_ptr(),
    c"3 Q".as_ptr(),
    c"3 Type (Peak/Hi Pass/Lo Pass/Hi Shelf/Lo Shelf)".as_ptr(),
    c"3 Active (On/Off)".as_ptr(),
    c"Output".as_ptr(),
]);

static PARAMS_15: ParamNames<5> = ParamNames([
    c"Threshold".as_ptr(),
    c"Attack".as_ptr(),
    c"Hold".as_ptr(),
    c"Release".as_ptr(),
    c"Range".as_ptr(),
]);

static PARAMS_16: ParamNames<2> = ParamNames([
    c"Level".as_ptr(),
    c"Curve (Linear/Log)".as_ptr(),
]);

static PARAMS_17: ParamNames<1> = ParamNames([
    c"Wah".as_ptr(),
]);

static PARAMS_18: ParamNames<1> = ParamNames([
    c"Wah".as_ptr(),
]);

static PARAMS_19: ParamNames<1> = ParamNames([
    c"Wah".as_ptr(),
]);

static PARAMS_20: ParamNames<1> = ParamNames([
    c"Wah".as_ptr(),
]);

static PARAMS_21: ParamNames<9> = ParamNames([
    c"Sens".as_ptr(),
    c"Attack".as_ptr(),
    c"Decay".as_ptr(),
    c"LP/BP/HP".as_ptr(),
    c"Level".as_ptr(),
    c"Freq".as_ptr(),
    c"Freq Depth".as_ptr(),
    c"Reso".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_22: ParamNames<11> = ParamNames([
    c"Sensitivity".as_ptr(),
    c"Attack".as_ptr(),
    c"Decay".as_ptr(),
    c"Color".as_ptr(),
    c"Intensity".as_ptr(),
    c"Blend".as_ptr(),
    c"Trig Detection (Down/Up)".as_ptr(),
    c"Trigger Mode".as_ptr(),
    c"Filter Cutoff".as_ptr(),
    c"Filter Type".as_ptr(),
    c"Level".as_ptr(),
]);

static PARAMS_23: ParamNames<6> = ParamNames([
    c"Input".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_24: ParamNames<6> = ParamNames([
    c"Threshold".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_25: ParamNames<6> = ParamNames([
    c"Threshold".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_26: ParamNames<6> = ParamNames([
    c"Threshold".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_27: ParamNames<5> = ParamNames([
    c"Mix".as_ptr(),
    c"Semitones".as_ptr(),
    c"Pitch Fine".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
]);

static PARAMS_28: ParamNames<3> = ParamNames([
    c"Spread".as_ptr(),
    c"Dry Level".as_ptr(),
    c"FX Level".as_ptr(),
]);

static PARAMS_29: ParamNames<6> = ParamNames([
    c"Input".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_30: ParamNames<6> = ParamNames([
    c"Threshold".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_31: ParamNames<6> = ParamNames([
    c"Threshold".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_32: ParamNames<6> = ParamNames([
    c"Threshold".as_ptr(),
    c"Ratio".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Makeup".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_33: ParamNames<8> = ParamNames([
    c"Mix".as_ptr(),
    c"Rate".as_ptr(),
    c"Depth".as_ptr(),
    c"Type".as_ptr(),
    c"Width".as_ptr(),
    c"Output".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_34: ParamNames<5> = ParamNames([
    c"Mix".as_ptr(),
    c"Mode".as_ptr(),
    c"Mode Type".as_ptr(),
    c"Drive".as_ptr(),
    c"Output".as_ptr(),
]);

static PARAMS_35: ParamNames<7> = ParamNames([
    c"Mix".as_ptr(),
    c"Rate".as_ptr(),
    c"Depth".as_ptr(),
    c"Width".as_ptr(),
    c"Output".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_36: ParamNames<7> = ParamNames([
    c"Mix".as_ptr(),
    c"Speed".as_ptr(),
    c"Depth".as_ptr(),
    c"Mode".as_ptr(),
    c"Output".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_37: ParamNames<8> = ParamNames([
    c"Mix".as_ptr(),
    c"Manual".as_ptr(),
    c"Width".as_ptr(),
    c"Speed".as_ptr(),
    c"Regen".as_ptr(),
    c"Output".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_38: ParamNames<7> = ParamNames([
    c"Mix".as_ptr(),
    c"Speed".as_ptr(),
    c"Type".as_ptr(),
    c"Mode".as_ptr(),
    c"Output".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_39: ParamNames<8> = ParamNames([
    c"Mix".as_ptr(),
    c"Vibe".as_ptr(),
    c"Speed".as_ptr(),
    c"Level".as_ptr(),
    c"Depth".as_ptr(),
    c"Output".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_40: ParamNames<12> = ParamNames([
    c"Rate".as_ptr(),
    c"Depth".as_ptr(),
    c"Waveform".as_ptr(),
    c"Duty Cycle".as_ptr(),
    c"Width".as_ptr(),
    c"Smoothing".as_ptr(),
    c"LFO Active".as_ptr(),
    c"Fade In".as_ptr(),
    c"Fade Out".as_ptr(),
    c"Boost".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_41: ParamNames<12> = ParamNames([
    c"Mix".as_ptr(),
    c"Feedback".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Ping Pong (On/Off)".as_ptr(),
    c"Delay Time".as_ptr(),
    c"Mod Rate".as_ptr(),
    c"Mod Depth".as_ptr(),
    c"Width".as_ptr(),
    c"Drive".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_42: ParamNames<12> = ParamNames([
    c"Mix".as_ptr(),
    c"Tap Preset".as_ptr(),
    c"Delay Time".as_ptr(),
    c"Feedback".as_ptr(),
    c"Diffusion".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mod Rate".as_ptr(),
    c"Mod Depth".as_ptr(),
    c"Vintage Mode (On/Off)".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_43: ParamNames<18> = ParamNames([
    c"Mix".as_ptr(),
    c"Feedback".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Ping Pong (On/Off)".as_ptr(),
    c"Delay Time".as_ptr(),
    c"Mod Rate".as_ptr(),
    c"Mod Depth".as_ptr(),
    c"Width".as_ptr(),
    c"Dyn Depth".as_ptr(),
    c"Dyn Mode".as_ptr(),
    c"Threshold".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Knee".as_ptr(),
    c"Feedback Depth".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_44: ParamNames<22> = ParamNames([
    c"Mix".as_ptr(),
    c"Delay Time L".as_ptr(),
    c"Feedback L".as_ptr(),
    c"X-Feedback".as_ptr(),
    c"Delay Time R".as_ptr(),
    c"Feedback R".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mod Rate".as_ptr(),
    c"Mod Depth".as_ptr(),
    c"Link FBack (On/Off)".as_ptr(),
    c"Dyn Depth".as_ptr(),
    c"Dyn Mode".as_ptr(),
    c"Threshold".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Knee".as_ptr(),
    c"Feedback Depth".as_ptr(),
    c"Sync L (On/Off)".as_ptr(),
    c"Sync Note L".as_ptr(),
    c"Sync R (On/Off)".as_ptr(),
    c"Sync Note R".as_ptr(),
]);

static PARAMS_45: ParamNames<22> = ParamNames([
    c"Mix".as_ptr(),
    c"Delay Time L".as_ptr(),
    c"Feedback L".as_ptr(),
    c"X-Feedback".as_ptr(),
    c"Feedback Mode".as_ptr(),
    c"Delay Time R".as_ptr(),
    c"Feedback R".as_ptr(),
    c"Overlap".as_ptr(),
    c"Trig Threshold".as_ptr(),
    c"Dyn Depth".as_ptr(),
    c"Dyn Mode".as_ptr(),
    c"Threshold".as_ptr(),
    c"Attack".as_ptr(),
    c"Release".as_ptr(),
    c"Knee".as_ptr(),
    c"Feedback Depth".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Link FBack (On/Off)".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note L".as_ptr(),
    c"Sync Note R".as_ptr(),
]);

static PARAMS_46: ParamNames<11> = ParamNames([
    c"Mix".as_ptr(),
    c"Feedback".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Drive".as_ptr(),
    c"Delay Time".as_ptr(),
    c"Wow".as_ptr(),
    c"Flutter".as_ptr(),
    c"Ping Pong (On/Off)".as_ptr(),
    c"Sync (On/Off)".as_ptr(),
    c"Sync Note".as_ptr(),
]);

static PARAMS_47: ParamNames<5> = ParamNames([
    // Wire order: Mix is last (Amount). Catalog UI lists often put Mix first.
    c"Size".as_ptr(),
    c"Pre Delay".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_48: ParamNames<6> = ParamNames([
    c"Decay".as_ptr(),
    c"Pre Delay".as_ptr(),
    c"Damping".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_49: ParamNames<5> = ParamNames([
    c"Decay".as_ptr(),
    c"Pre Delay".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_50: ParamNames<6> = ParamNames([
    // Mind Hall: Amount/Mix is first (matches device + Cortex Cloud).
    c"Mix".as_ptr(),
    c"Decay".as_ptr(),
    c"Pre Delay".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Damping".as_ptr(),
]);

static PARAMS_51: ParamNames<7> = ParamNames([
    c"Decay".as_ptr(),
    c"Pre Delay".as_ptr(),
    c"Mod Speed".as_ptr(),
    c"Mod Depth".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mix".as_ptr(),
]);

static PARAMS_52: ParamNames<5> = ParamNames([
    c"Decay".as_ptr(),
    c"Pre Delay".as_ptr(),
    c"High Pass".as_ptr(),
    c"Low Pass".as_ptr(),
    c"Mix".as_ptr(),
]);

pub const NANO_FX_CATALOG_COUNT: usize = 53;

pub static nano_fx_catalog: [NanoFxModelDef; NANO_FX_CATALOG_COUNT] = [
    NanoFxModelDef {
        name: c"Chief BD2".as_ptr(),
        id_len: 1,
        id: [0x12, 0x00, 0x00],
        param_count: 3,
        param_names: PARAMS_0.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Chief OD1".as_ptr(),
        id_len: 1,
        id: [0x0D, 0x00, 0x00],
        param_count: 2,
        param_names: PARAMS_1.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Exotic".as_ptr(),
        id_len: 1,
        id: [0x06, 0x00, 0x00],
        param_count: 4,
        param_names: PARAMS_2.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Exotic Bass Z Boost".as_ptr(),
        id_len: 2,
        id: [0xBF, 0x17, 0x00],
        param_count: 4,
        param_names: PARAMS_3.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Exotic Z Boost".as_ptr(),
        id_len: 1,
        id: [0x17, 0x00, 0x00],
        param_count: 4,
        param_names: PARAMS_4.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Facial Fuzz".as_ptr(),
        id_len: 1,
        id: [0x16, 0x00, 0x00],
        param_count: 4,
        param_names: PARAMS_5.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Green 808".as_ptr(),
        id_len: 1,
        id: [0x1B, 0x00, 0x00],
        param_count: 3,
        param_names: PARAMS_6.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Microtubes B3K".as_ptr(),
        id_len: 2,
        id: [0xB8, 0x17, 0x00],
        param_count: 6,
        param_names: PARAMS_7.as_ptr(),
    },
    NanoFxModelDef {
        name: c"OD250".as_ptr(),
        id_len: 1,
        id: [0x03, 0x00, 0x00],
        param_count: 2,
        param_names: PARAMS_8.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Obsessive Drive".as_ptr(),
        id_len: 1,
        id: [0x02, 0x00, 0x00],
        param_count: 4,
        param_names: PARAMS_9.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Rodent Drive".as_ptr(),
        id_len: 1,
        id: [0x04, 0x00, 0x00],
        param_count: 3,
        param_names: PARAMS_10.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Adaptive Gate".as_ptr(),
        id_len: 2,
        id: [0x81, 0x7D, 0x00],
        param_count: 1,
        param_names: PARAMS_11.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Graphic 9".as_ptr(),
        id_len: 2,
        id: [0xA5, 0x1F, 0x00],
        param_count: 12,
        param_names: PARAMS_12.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Low-High Cut".as_ptr(),
        id_len: 2,
        id: [0xA3, 0x1F, 0x00],
        param_count: 5,
        param_names: PARAMS_13.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Parametric 3".as_ptr(),
        id_len: 2,
        id: [0xA1, 0x1F, 0x00],
        param_count: 16,
        param_names: PARAMS_14.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Utility Gate".as_ptr(),
        id_len: 2,
        id: [0x82, 0x7D, 0x00],
        param_count: 5,
        param_names: PARAMS_15.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Volume".as_ptr(),
        id_len: 2,
        id: [0x86, 0x7D, 0x00],
        param_count: 2,
        param_names: PARAMS_16.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Bass Wah".as_ptr(),
        id_len: 2,
        id: [0xB4, 0x46, 0x00],
        param_count: 1,
        param_names: PARAMS_17.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Bubba Wah".as_ptr(),
        id_len: 2,
        id: [0xB2, 0x46, 0x00],
        param_count: 1,
        param_names: PARAMS_18.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Crying Clyde Wah".as_ptr(),
        id_len: 2,
        id: [0xB6, 0x46, 0x00],
        param_count: 1,
        param_names: PARAMS_19.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Crying Wah".as_ptr(),
        id_len: 2,
        id: [0xB5, 0x46, 0x00],
        param_count: 1,
        param_names: PARAMS_20.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Envelope Filter".as_ptr(),
        id_len: 3,
        id: [0xC6, 0xBB, 0x01],
        param_count: 9,
        param_names: PARAMS_21.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Love Meat".as_ptr(),
        id_len: 3,
        id: [0xC1, 0xBB, 0x01],
        param_count: 11,
        param_names: PARAMS_22.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Legendary 87 (M)".as_ptr(),
        id_len: 2,
        id: [0x89, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_23.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Opto Comp (M)".as_ptr(),
        id_len: 2,
        id: [0x8F, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_24.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Solid State Comp (M)".as_ptr(),
        id_len: 2,
        id: [0x8C, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_25.as_ptr(),
    },
    NanoFxModelDef {
        name: c"VCA Comp (M)".as_ptr(),
        id_len: 2,
        id: [0x8D, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_26.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Transpose".as_ptr(),
        id_len: 3,
        id: [0xD1, 0x8C, 0x01],
        param_count: 5,
        param_names: PARAMS_27.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Doubler".as_ptr(),
        id_len: 2,
        id: [0x8B, 0x7D, 0x00],
        param_count: 3,
        param_names: PARAMS_28.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Legendary 87 (ST)".as_ptr(),
        id_len: 2,
        id: [0x94, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_29.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Opto Comp (ST)".as_ptr(),
        id_len: 2,
        id: [0x97, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_30.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Solid State Comp (ST)".as_ptr(),
        id_len: 2,
        id: [0x95, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_31.as_ptr(),
    },
    NanoFxModelDef {
        name: c"VCA Comp (ST)".as_ptr(),
        id_len: 2,
        id: [0x96, 0x27, 0x00],
        param_count: 6,
        param_names: PARAMS_32.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Chief CE2W (ST)".as_ptr(),
        id_len: 2,
        id: [0xF0, 0x36, 0x00],
        param_count: 8,
        param_names: PARAMS_33.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Chief DC2W (ST)".as_ptr(),
        id_len: 2,
        id: [0xF3, 0x36, 0x00],
        param_count: 5,
        param_names: PARAMS_34.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Chorus 229T".as_ptr(),
        id_len: 2,
        id: [0xEF, 0x36, 0x00],
        param_count: 7,
        param_names: PARAMS_35.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Dream Chorus".as_ptr(),
        id_len: 2,
        id: [0xEE, 0x36, 0x00],
        param_count: 7,
        param_names: PARAMS_36.as_ptr(),
    },
    NanoFxModelDef {
        name: c"MX Flanger".as_ptr(),
        id_len: 2,
        id: [0xED, 0x36, 0x00],
        param_count: 8,
        param_names: PARAMS_37.as_ptr(),
    },
    NanoFxModelDef {
        name: c"MX Phase 95".as_ptr(),
        id_len: 2,
        id: [0xF4, 0x36, 0x00],
        param_count: 7,
        param_names: PARAMS_38.as_ptr(),
    },
    NanoFxModelDef {
        name: c"MX Vibe".as_ptr(),
        id_len: 2,
        id: [0xF5, 0x36, 0x00],
        param_count: 8,
        param_names: PARAMS_39.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Tremolo".as_ptr(),
        id_len: 2,
        id: [0xDC, 0x36, 0x00],
        param_count: 12,
        param_names: PARAMS_40.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Analog Delay".as_ptr(),
        id_len: 2,
        id: [0xFA, 0x2E, 0x00],
        param_count: 12,
        param_names: PARAMS_41.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Circular Delay".as_ptr(),
        id_len: 2,
        id: [0xFF, 0x2E, 0x00],
        param_count: 12,
        param_names: PARAMS_42.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Digital Delay (ST)".as_ptr(),
        id_len: 2,
        id: [0xFB, 0x2E, 0x00],
        param_count: 18,
        param_names: PARAMS_43.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Dual Delay".as_ptr(),
        id_len: 2,
        id: [0xFC, 0x2E, 0x00],
        param_count: 22,
        param_names: PARAMS_44.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Dual Reverse Delay".as_ptr(),
        id_len: 2,
        id: [0xFE, 0x2E, 0x00],
        param_count: 22,
        param_names: PARAMS_45.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Tape Delay".as_ptr(),
        id_len: 2,
        id: [0xF4, 0x2E, 0x00],
        param_count: 11,
        param_names: PARAMS_46.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Ambience".as_ptr(),
        id_len: 2,
        id: [0xC8, 0x3E, 0x00],
        param_count: 5,
        param_names: PARAMS_47.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Cave".as_ptr(),
        id_len: 2,
        id: [0xC9, 0x3E, 0x00],
        param_count: 6,
        param_names: PARAMS_48.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Hall".as_ptr(),
        id_len: 2,
        id: [0xC3, 0x3E, 0x00],
        param_count: 5,
        param_names: PARAMS_49.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Mind Hall".as_ptr(),
        id_len: 2,
        id: [0xCB, 0x3E, 0x00],
        param_count: 6,
        param_names: PARAMS_50.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Modulated".as_ptr(),
        id_len: 2,
        id: [0xC7, 0x3E, 0x00],
        param_count: 7,
        param_names: PARAMS_51.as_ptr(),
    },
    NanoFxModelDef {
        name: c"Room".as_ptr(),
        id_len: 2,
        id: [0xC0, 0x3E, 0x00],
        param_count: 5,
        param_names: PARAMS_52.as_ptr(),
    },
];

pub static nano_fx_catalog_count: usize = NANO_FX_CATALOG_COUNT;

/// The catalog as a slice; every lookup goes through here.
pub fn catalog() -> &'static [NanoFxModelDef] {
    &nano_fx_catalog
}
