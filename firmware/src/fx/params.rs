//! FX parameter ranges, enum labels, slider mapping, and formatting.

use alloc::format;
use alloc::string::String;

use core::ffi::CStr;

use super::models::{model_name, param_name};
use super::types::{NanoFxBlock, NanoFxModelDef};

/// Display range for a param: `(min, max, step, is_pct)`.
pub struct Range {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub is_pct: bool,
}

pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

const ON_OFF: &[&str] = &["Off", "On"];
const DYN_MODE: &[&str] = &["Off", "Duck", "Gate"];
const PICKUP: &[&str] = &["HB", "Single"];
const LOW_HIGH_PASS: &[&str] = &["LP", "HP"];
const FILTER_SLOPE: &[&str] = &[
    "Flat", "-6", "-12", "-18", "-24", "-30", "-36", "-42", "-48",
];
const PARAMETRIC_TYPE: &[&str] = &["Peak", "Hi Pass", "Lo Pass", "Hi Shelf", "Lo Shelf"];
const LINEAR_LOG: &[&str] = &["Linear", "Log"];
const TRIG_DETECTION: &[&str] = &["Down", "Up"];
const TRIGGER_MODE: &[&str] = &["Off", "Full", "1/2"];
const FILTER_CUTOFF: &[&str] = &["Low", "A", "B", "High"];
const FILTER_TYPE: &[&str] = &["Lowpass", "Bandpass", "Highpass"];
const RATIO_5: &[&str] = &["4", "8", "12", "20", "All"];
const RATIO_3: &[&str] = &["2", "4", "10"];
const CE2_TYPE: &[&str] = &["CE2-C", "CE1-C", "CE1-V"];
const DC2_MODE: &[&str] = &["1", "2", "3", "4", "1+2", "1+3", "1+4", "2+3", "2+4", "3+4"];
const MODE_TYPE: &[&str] = &["S", "SDD-320"];
const DREAM_CHORUS_MODE: &[&str] = &["CHO1", "CHO2"];
const PHASE_TYPE: &[&str] = &["90", "45"];
const PHASE_MODE: &[&str] = &["Block", "Script"];
const WAVEFORM: &[&str] = &["Sine", "Triangle", "Square", "Saw Up", "Saw Dn"];
const TAP_PRESET: &[&str] = &["1", "2", "3", "4", "4 Alt", "5", "5 Alt", "6", "6 Alt"];
const FEEDBACK_MODE: &[&str] = &["Reverse", "Flip"];
const AMBIENCE_SIZE: &[&str] = &["Small", "Medium", "Large"];
const SYNC_NOTE_11: &[&str] = &[
    "1/8", "1/4T", "1/8D", "1/4", "1/2T", "1/4D", "1/2", "1/1T", "1/2D", "1/1", "1/1D",
];
const SYNC_NOTE_14: &[&str] = &[
    "1/16", "1/8T", "1/16D", "1/8", "1/4T", "1/8D", "1/4", "1/2T", "1/4D", "1/2", "1/1T", "1/2D",
    "1/1", "1/1D",
];
const SYNC_NOTE_17: &[&str] = &[
    "1/32", "1/16T", "1/32D", "1/16", "1/8T", "1/16D", "1/8", "1/4T", "1/8D", "1/4", "1/2T",
    "1/4D", "1/2", "1/1T", "1/2D", "1/1", "1/1D",
];
const SYNC_NOTE_21: &[&str] = &[
    "1/64T", "1/64", "1/32T", "1/64D", "1/32", "1/16T", "1/32D", "1/16", "1/8T", "1/16D", "1/8",
    "1/4T", "1/8D", "1/4", "1/2T", "1/4D", "1/2", "1/1T", "1/2D", "1/1", "1/1D",
];

fn enum_base_name(name: &str) -> &str {
    name.split_once(" (").map_or(name, |(base, _)| base)
}

fn model_param_enum_options(model: &str, name: &str) -> Option<&'static [&'static str]> {
    let name = enum_base_name(name);
    let options = match (model, name) {
        ("Facial Fuzz", "Pickup") => PICKUP,
        ("Obsessive Drive", "Peak") => LOW_HIGH_PASS,
        ("Low-High Cut", "HPF Slope" | "LPF Slope") => FILTER_SLOPE,
        ("Parametric 3", "1 Type" | "2 Type" | "3 Type") => PARAMETRIC_TYPE,
        ("Volume", "Curve") => LINEAR_LOG,
        ("Love Meat", "Trig Detection") => TRIG_DETECTION,
        ("Love Meat", "Trigger Mode") => TRIGGER_MODE,
        ("Love Meat", "Filter Cutoff") => FILTER_CUTOFF,
        ("Love Meat", "Filter Type") => FILTER_TYPE,
        ("Legendary 87 (M)" | "Legendary 87 (ST)", "Ratio") => RATIO_5,
        ("Solid State Comp (M)" | "Solid State Comp (ST)", "Ratio") => RATIO_3,
        ("Chief CE2W (ST)", "Type") => CE2_TYPE,
        ("Chief DC2W (ST)", "Mode") => DC2_MODE,
        ("Chief DC2W (ST)", "Mode Type") => MODE_TYPE,
        ("Dream Chorus", "Mode") => DREAM_CHORUS_MODE,
        ("MX Phase 95", "Type") => PHASE_TYPE,
        ("MX Phase 95", "Mode") => PHASE_MODE,
        ("MX Phase 95" | "Tremolo", "Sync Note") => SYNC_NOTE_17,
        ("Chorus 229T", "Sync Note") => SYNC_NOTE_11,
        (
            "Chief CE2W (ST)" | "Dream Chorus" | "MX Flanger" | "MX Vibe" | "Analog Delay",
            "Sync Note",
        ) => SYNC_NOTE_14,
        (
            "Circular Delay" | "Digital Delay (ST)" | "Dual Delay" | "Dual Reverse Delay"
            | "Tape Delay",
            "Sync Note" | "Sync Note L" | "Sync Note R",
        ) => SYNC_NOTE_21,
        ("Tremolo", "Waveform") => WAVEFORM,
        ("Circular Delay", "Tap Preset") => TAP_PRESET,
        ("Dual Reverse Delay", "Feedback Mode") => FEEDBACK_MODE,
        ("Ambience", "Size") => AMBIENCE_SIZE,
        ("Blossom (ST)", "Extra E.R.") => ON_OFF,
        (
            _,
            "Growl" | "Midboost" | "1 Active" | "2 Active" | "3 Active" | "LFO Active" | "Sync"
            | "Sync L" | "Sync R" | "Ping Pong" | "Vintage Mode" | "Link FBack" | "Vibe",
        ) => ON_OFF,
        ("Digital Delay (ST)" | "Dual Delay" | "Dual Reverse Delay", "Dyn Mode") => DYN_MODE,
        _ => return None,
    };
    Some(options)
}

fn name_has_enum_hint(name: &str) -> bool {
    name != "LP/BP/HP"
        && (name.contains("(On/Off)") || name.contains('/') || name.contains("(HB/Single)"))
}

pub fn param_is_enum(name: &str) -> bool {
    name_has_enum_hint(name)
}

pub fn param_enum_count(name: &str) -> i32 {
    if name.contains("Sync Note") {
        return 21;
    }
    if name.contains("Dyn Mode") {
        return 3;
    }
    if name.contains("Type (Peak") {
        return 5;
    }
    if name.contains("Waveform") {
        return 5;
    }
    if name.contains("Tap Preset") {
        return 9;
    }
    2
}

pub fn param_range(name: &str) -> Range {
    let mut r = Range {
        min: 0.0,
        max: 100.0,
        step: 1.0,
        is_pct: true,
    };

    if name.is_empty() {
        return r;
    }

    if param_is_enum(name) {
        r.max = (param_enum_count(name) - 1) as f32;
        r.step = 1.0;
        r.is_pct = false;
        return r;
    }

    if name.contains("Threshold") || (name.contains("Range") && !name.contains("Freq")) {
        return Range {
            min: -90.0,
            max: 0.0,
            step: 0.1,
            is_pct: false,
        };
    }

    if name.contains("Dry Level") || name.contains("FX Level") {
        return Range {
            min: -40.0,
            max: 12.0,
            step: 0.1,
            is_pct: false,
        };
    }

    if name.contains("Spread") {
        return Range {
            min: -100.0,
            max: 100.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Semitones") {
        return Range {
            min: -12.0,
            max: 12.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Delay Time") || name.contains("Time L") || name.contains("Time R") {
        return Range {
            min: 50.0,
            max: 2000.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Attack") || name.contains("Release") || name.contains("Hold") {
        return Range {
            min: 1.0,
            max: 2000.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Pre Delay") {
        return Range {
            min: 0.0,
            max: 200.0,
            step: 0.1,
            is_pct: false,
        };
    }

    if name.contains("Mod Speed") {
        return Range {
            min: 0.1,
            max: 5.0,
            step: 0.01,
            is_pct: false,
        };
    }

    // Must be before the generic "Freq" / default %-age branches — High/Low Pass
    // were incorrectly shown as 0–100% (same as Mix), which made Amount look
    // like it was driving Low Pass on reverbs.
    if name.contains("High Pass") {
        return Range {
            min: 20.0,
            max: 800.0,
            step: 1.0,
            is_pct: false,
        };
    }
    if name.contains("Low Pass") {
        return Range {
            min: 800.0,
            max: 12000.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Freq") {
        return Range {
            min: 20.0,
            max: 20000.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Hz") {
        r.min = -12.0;
        r.max = 12.0;
        r.step = 0.5;
        r.is_pct = false;
    }

    r
}

fn param_range_for_model(model: &str, name: &str) -> Range {
    if let Some(options) = model_param_enum_options(model, name) {
        return Range {
            min: 0.0,
            max: options.len().saturating_sub(1) as f32,
            step: 1.0,
            is_pct: false,
        };
    }

    let make = |min, max, step, unit: &str| Range {
        min,
        max,
        step,
        is_pct: unit == "%",
    };

    match (model, name) {
        ("Poly Octaver", "Dry" | "Oct" | "Sub")
        | ("Chief OC2", "Dry" | "Oct 1" | "Oct 2")
        | ("Wham", "Pedal")
        | ("Blossom (ST)", "Modulation")
        | ("Shimmer", "Brightness") => return make(0.0, 100.0, 0.1, "%"),
        ("Wham", "Blend") | ("Blossom (ST)", "E.R. Length" | "Diffusion" | "Ducking") => {
            return make(0.0, 100.0, 1.0, "%");
        }
        ("Wham", "Heel Pitch 1" | "Toe Pitch 1" | "Heel Pitch 2" | "Toe Pitch 2")
        | ("Shimmer", "Pitch") => return make(-24.0, 24.0, 1.0, "Sem"),
        ("Blossom (ST)", "Decay") => return make(0.5, 20.0, 0.01, "s"),
        ("Blossom (ST)", "Pre Delay") => return make(0.0, 500.0, 1.0, "ms"),
        ("Blossom (ST)", "High Pass") => return make(30.0, 800.0, 1.0, "Hz"),
        ("Blossom (ST)", "Low Pass") => return make(1000.0, 20000.0, 1.0, "Hz"),
        ("Blossom (ST)", "Bass" | "Treble") => return make(-18.0, 6.0, 0.1, "dB"),
        ("Shimmer", "Decay") => return make(4.0, 20.0, 0.1, "s"),
        ("Shimmer", "Pre Delay") => return make(1.0, 200.0, 1.0, "ms"),
        ("Shimmer", "Rand Pitch") => return make(2.0, 33.0, 1.0, "Cent"),
        ("Shimmer", "Mod Speed") => return make(0.1, 5.0, 0.01, "Hz"),
        ("Utility Gate" | "Envelope Filter" | "Love Meat", "Attack") => {
            return make(1.0, 1000.0, 1.0, "ms");
        }
        ("Legendary 87 (M)" | "Legendary 87 (ST)", "Attack") => {
            return make(0.02, 0.8, 0.01, "ms");
        }
        ("Opto Comp (M)" | "Opto Comp (ST)" | "VCA Comp (M)" | "VCA Comp (ST)", "Attack") => {
            return make(1.0, 250.0, 0.01, "ms");
        }
        ("Solid State Comp (M)" | "Solid State Comp (ST)", "Attack") => {
            return make(0.1, 30.0, 0.01, "ms");
        }
        ("Digital Delay (ST)" | "Dual Delay" | "Dual Reverse Delay", "Attack") => {
            return make(1.0, 2000.0, 1.0, "ms");
        }
        ("Envelope Filter" | "Love Meat", "Decay") => return make(0.0, 10.0, 0.1, ""),
        ("Cave", "Decay") => return make(5.0, 20.0, 0.01, "s"),
        ("Hall" | "Mind Hall" | "Modulated", "Decay") => {
            return make(1.0, 10.0, 0.01, "s");
        }
        ("Room", "Decay") => return make(0.1, 2.0, 0.01, "s"),
        ("Analog Delay", "Delay Time") => return make(200.0, 1100.0, 0.1, "ms"),
        ("Circular Delay", "Delay Time") => return make(27.0, 2000.0, 0.01, "ms"),
        ("Digital Delay (ST)" | "Tape Delay", "Delay Time") => {
            return make(7.0, 6000.0, 1.0, "ms");
        }
        ("Dual Delay", "Delay Time L" | "Delay Time R") => {
            return make(7.0, 6000.0, 1.0, "ms");
        }
        ("Dual Reverse Delay", "Delay Time L" | "Delay Time R") => {
            return make(20.0, 4000.0, 1.0, "ms");
        }
        ("Transpose", "High Pass") => return make(20.0, 2000.0, 1.0, "Hz"),
        (
            "Analog Delay" | "Circular Delay" | "Digital Delay (ST)" | "Dual Delay"
            | "Dual Reverse Delay" | "Ambience" | "Cave" | "Hall" | "Mind Hall" | "Modulated"
            | "Room",
            "High Pass",
        ) => return make(20.0, 800.0, 1.0, "Hz"),
        ("Tape Delay", "High Pass") => return make(80.0, 800.0, 1.0, "Hz"),
        ("Graphic 9", "HPF") => return make(20.0, 800.0, 1.0, "Hz"),
        ("Transpose", "Low Pass") => return make(200.0, 19900.0, 1.0, "Hz"),
        (
            "Analog Delay" | "Circular Delay" | "Digital Delay (ST)" | "Dual Delay"
            | "Dual Reverse Delay",
            "Low Pass",
        ) => return make(1000.0, 16000.0, 1.0, "Hz"),
        ("Tape Delay", "Low Pass") => return make(800.0, 6000.0, 1.0, "Hz"),
        ("Ambience" | "Hall" | "Mind Hall" | "Modulated" | "Room", "Low Pass") => {
            return make(800.0, 12000.0, 1.0, "Hz");
        }
        ("Cave", "Low Pass") => return make(800.0, 8000.0, 1.0, "Hz"),
        ("Graphic 9", "LPF") => return make(1000.0, 16000.0, 1.0, "Hz"),
        ("Ambience", "Pre Delay") => return make(0.0, 100.0, 0.1, "ms"),
        ("Cave" | "Modulated", "Pre Delay") => return make(1.0, 200.0, 0.1, "ms"),
        ("Hall" | "Mind Hall" | "Room", "Pre Delay") => {
            return make(1.0, 100.0, 0.1, "ms");
        }
        ("Chief CE2W (ST)", "Rate") => return make(0.0, 100.0, 1.0, "%"),
        ("Chorus 229T", "Rate") => return make(0.2, 3.0, 0.01, "Hz"),
        ("Tremolo", "Rate") => return make(0.1, 20.0, 0.1, "Hz"),
        ("Utility Gate", "Release") => return make(2.0, 5000.0, 1.0, "ms"),
        ("Legendary 87 (M)" | "Legendary 87 (ST)", "Release") => {
            return make(0.06, 1.1, 0.01, "s");
        }
        ("Opto Comp (M)" | "Opto Comp (ST)" | "VCA Comp (M)" | "VCA Comp (ST)", "Release") => {
            return make(50.0, 1200.0, 1.0, "ms");
        }
        ("Solid State Comp (M)" | "Solid State Comp (ST)", "Release") => {
            return make(0.01, 1.2, 0.01, "s");
        }
        ("Digital Delay (ST)" | "Dual Delay" | "Dual Reverse Delay", "Release") => {
            return make(1.0, 2000.0, 1.0, "ms");
        }
        ("Utility Gate", "Threshold") => return make(-90.0, 0.0, 0.1, "dB"),
        (
            "Opto Comp (M)"
            | "Opto Comp (ST)"
            | "Solid State Comp (M)"
            | "Solid State Comp (ST)"
            | "VCA Comp (M)"
            | "VCA Comp (ST)",
            "Threshold",
        ) => {
            return make(-60.0, 12.0, 0.1, "dB");
        }
        ("Digital Delay (ST)" | "Dual Delay" | "Dual Reverse Delay", "Threshold") => {
            return make(-65.0, -10.0, 0.1, "dB");
        }
        ("Chief DC2W (ST)", "Drive") => return make(0.0, 100.0, 1.0, "%"),
        ("Tape Delay", "Drive") => return make(0.0, 100.0, 1.0, "%"),
        ("Chief CE2W (ST)", "Speed") => return make(0.0, 100.0, 1.0, "%"),
        (
            "Graphic 9",
            "65Hz" | "125Hz" | "250Hz" | "500hz" | "1kHz" | "2kHz" | "4kHz" | "8kHz" | "16kHz"
            | "Output",
        ) => return make(-12.0, 12.0, 0.1, "dB"),
        ("Low-High Cut" | "Parametric 3", "Output") => return make(-12.0, 12.0, 0.1, "dB"),
        ("Envelope Filter", "LP/BP/HP") => return make(-1.0, 1.0, 0.01, ""),
        _ => {}
    }

    match name {
        "1 Gain" | "2 Gain" | "3 Gain" | "65Hz" | "125Hz" | "250Hz" | "500hz" | "1kHz" | "2kHz"
        | "4kHz" | "8kHz" | "16kHz" | "Output" => make(-12.0, 12.0, 0.1, "dB"),
        "1 Freq" | "2 Freq" | "3 Freq" | "1 Q" | "2 Q" | "3 Q" => make(0.1, 10.0, 0.1, ""),
        "Boost" | "Damping" | "Depth" | "Diffusion" | "Duty Cycle" | "Dyn Depth" | "Feedback"
        | "Feedback Depth" | "Feedback L" | "Feedback R" | "Freq Depth" | "Level" | "Mix"
        | "Mod Depth" | "Noise Reduction" | "Overlap" | "Smoothing" | "Width" | "X-Feedback" => {
            make(0.0, 100.0, 0.1, "%")
        }
        "Bass" | "Blend" | "Color" | "Distortion" | "Filter" | "Flutter" | "Fuzz" | "Gain"
        | "Intensity" | "Manual" | "Overdrive" | "Pickup Level" | "Regen" | "Sens"
        | "Sensitivity" | "Tone" | "Treble" | "Volume" | "Wow" => make(0.0, 10.0, 0.1, ""),
        "Dry Level" | "FX Level" => make(-40.0, 12.0, 0.1, "dB"),
        "Fade In" | "Fade Out" => make(1.0, 5000.0, 1.0, "ms"),
        "Freq" | "HPF Freq" | "LPF Freq" => make(20.0, 20000.0, 1.0, "Hz"),
        "HPF" => make(20.0, 800.0, 1.0, "Hz"),
        "LPF" => make(1000.0, 16000.0, 1.0, "Hz"),
        "Hold" => make(1.0, 2000.0, 1.0, "ms"),
        "Input" => make(-48.0, 0.0, 0.1, "dB"),
        "Knee" => make(1.0, 20.0, 0.01, "dB"),
        "Makeup" => make(-48.0, 48.0, 0.01, "dB"),
        "Mod Rate" => make(0.1, 10.0, 0.1, "Hz"),
        "Mod Speed" => make(0.1, 5.0, 0.01, "Hz"),
        "Pitch Fine" => make(-100.0, 100.0, 1.0, "Cent"),
        "Range" => make(-90.0, -6.0, 0.1, "dB"),
        "Reso" => make(1.0, 10.0, 0.01, ""),
        "Semitones" => make(-12.0, 12.0, 1.0, "Sem"),
        "Spread" => make(3.0, 60.0, 0.01, "ms"),
        "Speed" => make(0.0, 100.0, 1.0, "%"),
        "Trig Threshold" => make(-65.0, -10.0, 0.1, "dB"),
        "Wah" => make(0.0, 1.0, 0.01, ""),
        "Attack" | "Release" => make(1.0, 2000.0, 1.0, "ms"),
        "Delay Time" | "Delay Time L" | "Delay Time R" => make(50.0, 2000.0, 1.0, "ms"),
        "Decay" => make(0.0, 10.0, 0.1, ""),
        "Drive" => make(0.0, 10.0, 0.1, ""),
        "High Pass" => make(20.0, 800.0, 1.0, "Hz"),
        "Low Pass" => make(800.0, 12000.0, 1.0, "Hz"),
        "Pre Delay" => make(0.0, 200.0, 0.1, "ms"),
        "Rate" => make(0.1, 20.0, 0.1, "Hz"),
        "Threshold" => make(-90.0, 0.0, 0.1, "dB"),
        _ => param_range(name),
    }
}

fn param_unit_for_model(model: &str, name: &str) -> &'static str {
    match (model, name) {
        ("Wham", "Heel Pitch 1" | "Toe Pitch 1" | "Heel Pitch 2" | "Toe Pitch 2")
        | ("Shimmer", "Pitch") => "Sem",
        ("Blossom (ST)" | "Shimmer", "Decay") => "s",
        ("Blossom (ST)", "Bass" | "Treble") => "dB",
        ("Shimmer", "Rand Pitch") => "Cent",
        ("Poly Octaver", "Dry" | "Oct" | "Sub")
        | ("Chief OC2", "Dry" | "Oct 1" | "Oct 2")
        | ("Wham", "Pedal" | "Blend")
        | ("Blossom (ST)", "E.R. Length" | "Ducking" | "Modulation")
        | ("Shimmer", "Brightness") => "%",
        (
            "Legendary 87 (M)"
            | "Legendary 87 (ST)"
            | "Solid State Comp (M)"
            | "Solid State Comp (ST)",
            "Release",
        ) => "s",
        ("Cave" | "Hall" | "Mind Hall" | "Modulated" | "Room", "Decay") => "s",
        (
            "Graphic 9",
            "65Hz" | "125Hz" | "250Hz" | "500hz" | "1kHz" | "2kHz" | "4kHz" | "8kHz" | "16kHz"
            | "Output",
        ) => "dB",
        ("Graphic 9", "HPF" | "LPF") => "Hz",
        ("Low-High Cut" | "Parametric 3", "Output") => "dB",
        ("Transpose", "High Pass" | "Low Pass") => "Hz",
        (
            "Analog Delay" | "Circular Delay" | "Digital Delay (ST)" | "Dual Delay"
            | "Dual Reverse Delay" | "Tape Delay" | "Ambience" | "Cave" | "Hall" | "Mind Hall"
            | "Modulated" | "Room",
            "High Pass" | "Low Pass",
        ) => "Hz",
        ("Chief CE2W (ST)", "Rate" | "Speed") | ("Chief DC2W (ST)" | "Tape Delay", "Drive") => "%",
        ("Chorus 229T", "Rate") | ("Tremolo", "Rate") => "Hz",
        ("Utility Gate" | "Envelope Filter" | "Love Meat", "Attack") => "ms",
        ("Digital Delay (ST)" | "Dual Delay" | "Dual Reverse Delay", "Attack" | "Release") => "ms",
        ("Utility Gate", "Release") => "ms",
        ("Opto Comp (M)" | "Opto Comp (ST)" | "VCA Comp (M)" | "VCA Comp (ST)", "Release") => "ms",
        _ => match name {
            "1 Gain" | "2 Gain" | "3 Gain" | "65Hz" | "125Hz" | "250Hz" | "500hz" | "1kHz"
            | "2kHz" | "4kHz" | "8kHz" | "16kHz" | "Output" | "Dry Level" | "FX Level"
            | "Input" | "Knee" | "Makeup" | "Range" | "Threshold" | "Trig Threshold" => "dB",
            "Fade In" | "Fade Out" | "Hold" | "Pre Delay" | "Spread" | "Attack" | "Release"
            | "Delay Time" | "Delay Time L" | "Delay Time R" => "ms",
            "Freq" | "HPF" | "LPF" | "HPF Freq" | "LPF Freq" | "High Pass" | "Low Pass"
            | "Mod Rate" | "Mod Speed" => "Hz",
            "Pitch Fine" => "Cent",
            "Semitones" => "Sem",
            "Boost" | "Damping" | "Depth" | "Diffusion" | "Duty Cycle" | "Dyn Depth"
            | "Feedback" | "Feedback Depth" | "Feedback L" | "Feedback R" | "Freq Depth"
            | "Level" | "Mix" | "Mod Depth" | "Noise Reduction" | "Overlap" | "Smoothing"
            | "Speed" | "Width" | "X-Feedback" => "%",
            _ => "",
        },
    }
}

fn param_skew_for_model(model: &str, name: &str) -> f32 {
    match (model, name) {
        ("Blossom (ST)", "Decay" | "Pre Delay" | "E.R. Length") => 0.4,
        ("Blossom (ST)", "High Pass" | "Low Pass") => 0.5,
        ("Blossom (ST)", "Bass" | "Treble") => 2.375,
        ("Shimmer", "Decay" | "Pre Delay" | "High Pass" | "Mod Speed") => 0.6,
        _ => 1.0,
    }
}

fn unskew(normalized: f32, skew: f32) -> f32 {
    if skew == 1.0 {
        normalized
    } else {
        libm::powf(normalized, 1.0 / skew)
    }
}

fn skew(normalized: f32, skew: f32) -> f32 {
    if skew == 1.0 {
        normalized
    } else {
        libm::powf(normalized, skew)
    }
}

pub fn normalized_to_display_for_model(
    model: &NanoFxModelDef,
    param_index: usize,
    normalized: f32,
) -> f32 {
    let name = param_name(model, param_index);
    let range = param_range_for_model(model_name(model), name);
    let snapped = normalize_for_model(model, param_index, normalized);
    let skew = param_skew_for_model(model_name(model), name);
    range.min + unskew(snapped, skew) * (range.max - range.min)
}

pub fn normalize_for_model(model: &NanoFxModelDef, param_index: usize, normalized: f32) -> f32 {
    let normalized = clampf(normalized, 0.0, 1.0);
    let name = param_name(model, param_index);
    let model_name = model_name(model);
    if let Some(options) = model_param_enum_options(model_name, name) {
        let last_index = options.len().saturating_sub(1);
        if last_index == 0 {
            return 0.0;
        }
        let index = (normalized * last_index as f32 + 0.5) as usize;
        return index as f32 / last_index as f32;
    }

    let range = param_range_for_model(model_name, name);
    if range.step <= 0.0 || range.max <= range.min {
        return normalized;
    }
    let curve = param_skew_for_model(model_name, name);
    let display = range.min + unskew(normalized, curve) * (range.max - range.min);
    let steps = ((display - range.min) / range.step + 0.5) as i32;
    let snapped = range.min + steps as f32 * range.step;
    skew(
        clampf((snapped - range.min) / (range.max - range.min), 0.0, 1.0),
        curve,
    )
}

pub fn display_to_normalized_for_model(
    model: &NanoFxModelDef,
    param_index: usize,
    display: f32,
) -> f32 {
    let name = param_name(model, param_index);
    let range = param_range_for_model(model_name(model), name);
    if range.max <= range.min {
        return 0.0;
    }
    let curve = param_skew_for_model(model_name(model), name);
    let normalized = skew(
        clampf((display - range.min) / (range.max - range.min), 0.0, 1.0),
        curve,
    );
    normalize_for_model(model, param_index, normalized)
}

pub fn param_scale(name: &str) -> i32 {
    if param_is_enum(name) {
        return 1;
    }

    let step = param_range(name).step;
    if step >= 1.0 {
        1
    } else if step >= 0.1 {
        10
    } else {
        100
    }
}

pub fn display_to_slider(name: &str, display: f32) -> i32 {
    let r = param_range(name);
    let scale = param_scale(name) as f32;
    let raw = ((display - r.min) * scale + 0.5) as i32;
    let max_slider = ((r.max - r.min) * scale + 0.5) as i32;
    if raw < 0 {
        0
    } else if raw > max_slider {
        max_slider
    } else {
        raw
    }
}

pub fn display_to_normalized(name: &str, display: f32) -> f32 {
    let r = param_range(name);
    if r.max <= r.min {
        return 0.0;
    }
    clampf((display - r.min) / (r.max - r.min), 0.0, 1.0)
}

pub fn normalized_to_display(name: &str, normalized: f32) -> f32 {
    let r = param_range(name);
    r.min + clampf(normalized, 0.0, 1.0) * (r.max - r.min)
}

fn semitone_interval_abbr(semitones: i32) -> Option<&'static str> {
    match semitones.abs() {
        0 => Some("Unison"),
        1 => Some("m2nd"),
        2 => Some("M2nd"),
        3 => Some("m3rd"),
        4 => Some("M3rd"),
        5 => Some("P4th"),
        6 => Some("TT"),
        7 => Some("P5th"),
        8 => Some("m6th"),
        9 => Some("M6th"),
        10 => Some("m7th"),
        11 => Some("M7th"),
        12 => Some("Oct"),
        _ => None,
    }
}

fn format_semitone_param(display: f32) -> String {
    let semi = (display + if display >= 0.0 { 0.5 } else { -0.5 }) as i32;
    match semitone_interval_abbr(semi) {
        Some(interval) if semi > 0 => format!("+{semi} ({interval}) Sem"),
        Some(interval) => format!("{semi} ({interval}) Sem"),
        None => format!("{semi} Sem"),
    }
}

/// Enum label, or `None` when the C port falls back to the numeric index.
fn param_enum_label(name: &str, index: i32) -> Option<&'static CStr> {
    if name.contains("Dyn Mode") {
        return Some(match index {
            0 => c"Off",
            1 => c"Duck",
            2 => c"Gate",
            _ => c"?",
        });
    }

    if name.contains("Curve (Linear/Log)") {
        return Some(if index == 0 { c"Linear" } else { c"Log" });
    }

    if name.contains("Peak (LP/HP)") {
        return Some(if index == 0 { c"LP" } else { c"HP" });
    }

    if name.contains("Pickup (HB/Single)") {
        return Some(if index == 0 { c"HB" } else { c"Single" });
    }

    if name.contains("On/Off")
        || name.contains("Sync (")
        || name.contains("Sync L")
        || name.contains("Sync R")
        || name.contains("Ping Pong")
        || name.contains("Vintage Mode")
        || name.contains("Link FBack")
        || name.contains("Growl")
        || name.contains("Midboost")
        || name.contains("Active (On/Off)")
    {
        return Some(if index == 0 { c"Off" } else { c"On" });
    }

    None
}

/// Formatted param value for the UI (port of `nano_fx_format_param`).
pub fn format_param(block: &NanoFxBlock, param_index: u8) -> String {
    if block.model.is_null() || param_index >= block.param_count {
        return String::from("-");
    }

    let p = &block.params[param_index as usize];
    if !p.valid {
        return String::from("-");
    }

    let model = unsafe { &*block.model };
    let name = param_name(model, param_index as usize);
    let model_name = model_name(model);

    if name.contains("Semitones") {
        return format_semitone_param(p.display);
    }

    if let Some(options) = model_param_enum_options(model_name, name) {
        let mut idx = (p.display + 0.5) as i32;
        idx = idx.clamp(0, options.len() as i32 - 1);
        return String::from(options[idx as usize]);
    }

    if param_is_enum(name) {
        let count = param_enum_count(name);
        let mut idx = (p.display + 0.5) as i32;
        if idx < 0 {
            idx = 0;
        }
        if idx >= count {
            idx = count - 1;
        }
        return match param_enum_label(name, idx) {
            Some(label) => label.to_string_lossy().into_owned(),
            None => format!("{idx}"),
        };
    }

    let range = param_range_for_model(model_name, name);
    let decimals = if range.step >= 1.0 {
        0
    } else if range.step >= 0.1 {
        1
    } else if range.step >= 0.01 {
        2
    } else {
        3
    };
    let unit = param_unit_for_model(model_name, name);
    if unit == "%" {
        format!("{:.*}%", decimals, p.display)
    } else if unit.is_empty() {
        format!("{:.*}", decimals, p.display)
    } else {
        format!("{:.*} {unit}", decimals, p.display)
    }
}

pub fn slider_to_display(name: &str, slider_value: i32) -> f32 {
    let r = param_range(name);
    r.min + slider_value as f32 / param_scale(name) as f32
}

pub fn slider_min(_name: &str) -> i32 {
    0
}

pub fn slider_max(name: &str) -> i32 {
    display_to_slider(name, param_range(name).max)
}

/* -------------------------------------------------------------- public API */

pub fn nano_fx_param_is_enum(param_name: &str) -> bool {
    param_is_enum(param_name)
}

pub fn nano_fx_param_enum_count(param_name: &str) -> i32 {
    param_enum_count(param_name)
}

/// Human-readable label for enum params (e.g. "Off", "On").
pub fn nano_fx_param_enum_label(param_name: &str, index: i32) -> String {
    match param_enum_label(param_name, index) {
        Some(label) => label.to_string_lossy().into_owned(),
        None => format!("{index}"),
    }
}

/// Param display range for UI sliders.
pub fn nano_fx_param_range(param_name: &str) -> Range {
    param_range(param_name)
}

/// Integer scale for `lv_slider` (1, 10, or 100 for fractional steps).
pub fn nano_fx_param_scale(param_name: &str) -> i32 {
    param_scale(param_name)
}

pub fn nano_fx_display_to_slider(param_name: &str, display: f32) -> i32 {
    display_to_slider(param_name, display)
}

pub fn nano_fx_slider_to_display(param_name: &str, slider_value: i32) -> f32 {
    slider_to_display(param_name, slider_value)
}

pub fn nano_fx_slider_min(param_name: &str) -> i32 {
    slider_min(param_name)
}

pub fn nano_fx_slider_max(param_name: &str) -> i32 {
    slider_max(param_name)
}

/// Map display value ↔ normalized using param name heuristics.
pub fn nano_fx_display_to_normalized(param_name: &str, display: f32) -> f32 {
    display_to_normalized(param_name, display)
}

pub fn nano_fx_normalized_to_display(param_name: &str, normalized: f32) -> f32 {
    normalized_to_display(param_name, normalized)
}

/// Format param for UI (e.g. "35.0 %").
pub fn nano_fx_format_param(block: &NanoFxBlock, param_index: u8) -> String {
    format_param(block, param_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    static SYNC_NOTE_PARAMS: super::super::types::ParamNames<1> =
        super::super::types::ParamNames([c"Sync Note".as_ptr()]);
    static DREAM_CHORUS_PARAMS: super::super::types::ParamNames<7> =
        super::super::types::ParamNames([
            c"Mix".as_ptr(),
            c"Speed".as_ptr(),
            c"Depth".as_ptr(),
            c"Mode".as_ptr(),
            c"Output".as_ptr(),
            c"Sync".as_ptr(),
            c"Sync Note".as_ptr(),
        ]);
    static DREAM_CHORUS_MODEL: NanoFxModelDef = NanoFxModelDef {
        name: c"Dream Chorus".as_ptr(),
        id_len: 0,
        id: [0; 3],
        param_count: 7,
        param_names: DREAM_CHORUS_PARAMS.as_ptr(),
    };
    static GRAPHIC_HPF_PARAMS: super::super::types::ParamNames<1> =
        super::super::types::ParamNames([c"HPF".as_ptr()]);
    static GRAPHIC_HPF_MODEL: NanoFxModelDef = NanoFxModelDef {
        name: c"Graphic 9".as_ptr(),
        id_len: 0,
        id: [0; 3],
        param_count: 1,
        param_names: GRAPHIC_HPF_PARAMS.as_ptr(),
    };
    static MX_PHASE_MODEL: NanoFxModelDef = NanoFxModelDef {
        name: c"MX Phase 95".as_ptr(),
        id_len: 0,
        id: [0; 3],
        param_count: 1,
        param_names: SYNC_NOTE_PARAMS.as_ptr(),
    };

    #[test]
    fn enum_counts_and_options_match_model_specific_choices() {
        assert_eq!(
            model_param_enum_options("Chorus 229T", "Sync Note")
                .unwrap()
                .len(),
            11
        );
        assert_eq!(
            model_param_enum_options("Chief CE2W (ST)", "Sync Note")
                .unwrap()
                .len(),
            14
        );
        assert_eq!(
            model_param_enum_options("Dream Chorus", "Sync Note").unwrap(),
            SYNC_NOTE_14
        );
        assert_eq!(
            model_param_enum_options("MX Phase 95", "Sync Note")
                .unwrap()
                .len(),
            17
        );
        assert_eq!(
            model_param_enum_options("Digital Delay (ST)", "Sync Note")
                .unwrap()
                .len(),
            21
        );
        assert_eq!(
            model_param_enum_options("Chief DC2W (ST)", "Mode").unwrap(),
            DC2_MODE
        );
        assert_eq!(
            model_param_enum_options("Chief DC2W (ST)", "Mode Type").unwrap(),
            MODE_TYPE
        );
        assert_eq!(
            model_param_enum_options("Tremolo", "Waveform").unwrap(),
            WAVEFORM
        );
        assert_eq!(
            model_param_enum_options("Circular Delay", "Tap Preset").unwrap(),
            TAP_PRESET
        );
    }

    #[test]
    fn enum_display_and_label_use_the_model_option_count() {
        assert_eq!(
            normalized_to_display_for_model(&MX_PHASE_MODEL, 0, 1.0),
            16.0
        );
        assert_eq!(
            display_to_normalized_for_model(&MX_PHASE_MODEL, 0, 16.0),
            1.0
        );
        assert_eq!(normalize_for_model(&MX_PHASE_MODEL, 0, 0.61), 10.0 / 16.0);

        let mut block = NanoFxBlock::zeroed();
        block.model = &MX_PHASE_MODEL;
        block.param_count = 1;
        block.params[0].display = 16.0;
        block.params[0].valid = true;
        assert_eq!(format_param(&block, 0), "1/1D");
    }

    #[test]
    fn eq_ranges_and_units_match_the_editor_metadata() {
        let hpf = param_range_for_model("Graphic 9", "HPF");
        assert_eq!((hpf.min, hpf.max, hpf.step), (20.0, 800.0, 1.0));
        assert_eq!(param_unit_for_model("Graphic 9", "HPF"), "Hz");

        let band = param_range_for_model("Graphic 9", "65Hz");
        assert_eq!((band.min, band.max, band.step), (-12.0, 12.0, 0.1));
        assert_eq!(param_unit_for_model("Graphic 9", "65Hz"), "dB");

        let parametric = param_range_for_model("Parametric 3", "1 Freq");
        assert_eq!(
            (parametric.min, parametric.max, parametric.step),
            (0.1, 10.0, 0.1)
        );

        let cutoff = param_range_for_model("Low-High Cut", "HPF Freq");
        assert_eq!((cutoff.min, cutoff.max, cutoff.step), (20.0, 20000.0, 1.0));
        assert_eq!(param_unit_for_model("Low-High Cut", "HPF Freq"), "Hz");
    }

    #[test]
    fn dream_chorus_sync_note_uses_source_display_position_and_labels() {
        assert_eq!(
            super::super::models::param_wire_index(&DREAM_CHORUS_MODEL, 3),
            6
        );
        assert_eq!(
            super::super::models::param_wire_index(&DREAM_CHORUS_MODEL, 2),
            5
        );

        let mut block = NanoFxBlock::zeroed();
        block.model = &DREAM_CHORUS_MODEL;
        block.param_count = 7;
        block.params[6].display = 13.0;
        block.params[6].valid = true;
        assert_eq!(format_param(&block, 6), "1/1D");
    }

    #[test]
    fn fractional_values_keep_precision_and_units() {
        assert_eq!(param_unit_for_model("Tremolo", "Rate"), "Hz");
        assert_eq!(param_unit_for_model("Utility Gate", "Threshold"), "dB");
        assert_eq!(param_unit_for_model("Room", "Decay"), "s");
        assert!(!param_is_enum("LP/BP/HP"));
        let mode = param_range_for_model("Envelope Filter", "LP/BP/HP");
        assert_eq!((mode.min, mode.max, mode.step), (-1.0, 1.0, 0.01));

        let mut block = NanoFxBlock::zeroed();
        block.model = &GRAPHIC_HPF_MODEL;
        block.param_count = 1;
        block.params[0].display = 800.0;
        block.params[0].valid = true;
        assert_eq!(format_param(&block, 0), "800 Hz");
    }

    #[test]
    fn nanos_230_parameter_ranges_units_and_scaling_round_trip() {
        use super::super::models::find_model_by_name;

        for name in [
            "Poly Octaver",
            "Wham",
            "Chief OC2",
            "Blossom (ST)",
            "Shimmer",
        ] {
            let model = find_model_by_name(name).unwrap();
            for wire in 0..usize::from(model.param_count) {
                for step in 0..=100 {
                    let normalized = step as f32 / 100.0;
                    let snapped = normalize_for_model(model, wire, normalized);
                    let display = normalized_to_display_for_model(model, wire, normalized);
                    let round_trip = display_to_normalized_for_model(model, wire, display);
                    assert!(
                        (snapped - round_trip).abs() < 0.00001,
                        "{name}, wire {wire}, {normalized}: {snapped} != {round_trip}"
                    );
                    assert!((0.0..=1.0).contains(&snapped));
                }
            }
        }

        let blossom = find_model_by_name("Blossom (ST)").unwrap();
        let high_pass = display_to_normalized_for_model(blossom, 3, 200.0);
        assert!((high_pass - 0.46987).abs() < 0.0001);
        assert!((normalized_to_display_for_model(blossom, 3, high_pass) - 200.0).abs() < 0.001);
        assert_eq!(normalized_to_display_for_model(blossom, 8, 0.0), -18.0);
        assert_eq!(normalized_to_display_for_model(blossom, 8, 1.0), 6.0);
        assert_eq!(
            model_param_enum_options("Blossom (ST)", "Extra E.R. (Off/On)"),
            Some(ON_OFF)
        );
        assert_eq!(normalize_for_model(blossom, 5, 0.6), 1.0);
        assert_eq!(normalize_for_model(blossom, 5, 0.4), 0.0);

        let wham = find_model_by_name("Wham").unwrap();
        for wire in 2..6 {
            assert_eq!(normalized_to_display_for_model(wham, wire, 0.0), -24.0);
            assert_eq!(normalized_to_display_for_model(wham, wire, 0.5), 0.0);
            assert_eq!(normalized_to_display_for_model(wham, wire, 1.0), 24.0);
        }
        for (model, parameter, unit) in [
            ("Poly Octaver", "Dry", "%"),
            ("Wham", "Pedal", "%"),
            ("Wham", "Heel Pitch 1", "Sem"),
            ("Chief OC2", "Output", "dB"),
            ("Blossom (ST)", "Decay", "s"),
            ("Blossom (ST)", "Bass", "dB"),
            ("Blossom (ST)", "Ducking", "%"),
            ("Shimmer", "Rand Pitch", "Cent"),
            ("Shimmer", "Pitch", "Sem"),
            ("Shimmer", "Mod Speed", "Hz"),
        ] {
            assert_eq!(param_unit_for_model(model, parameter), unit);
        }
    }
}
