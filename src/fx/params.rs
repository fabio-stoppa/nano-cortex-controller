//! Param name heuristics: ranges, enum labels, slider mapping and formatting
//! (port of the `fx_param_*` / `nano_fx_param_*` helpers in `nano_fx.c`).

use core::ffi::{c_char, c_int, CStr};

use super::c_deps::{c_str, write_c_buf};
use super::shared::Shared;
use super::types::NanoFxBlock;

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

fn name_has_enum_hint(name: &str) -> bool {
    name.contains("(On/Off)") || name.contains('/') || name.contains("(HB/Single)")
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
        return 4;
    }
    if name.contains("Tap Preset") {
        return 4;
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
            max: 500.0,
            step: 1.0,
            is_pct: false,
        };
    }

    if name.contains("Freq") && !name.contains("High Pass") && !name.contains("Low Pass") {
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
        Some(interval) if semi > 0 => format!("+{semi} ({interval})"),
        Some(interval) => format!("{semi} ({interval})"),
        None => format!("{semi}"),
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

/// `static char buf[8]` in the C port: the numeric enum fallback label.
static ENUM_LABEL_BUF: Shared<[u8; 8]> = Shared::new([0; 8]);

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
    let name = c_str(unsafe { *model.param_names.add(param_index as usize) });

    if name.contains("Semitones") {
        return format_semitone_param(p.display);
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

    let r = param_range(name);
    let decimals = if r.step >= 1.0 { 0 } else { 1 };
    if r.is_pct && r.min == 0.0 && r.max == 100.0 {
        format!("{:.*}%", decimals, p.display)
    } else {
        format!("{:.*}", decimals, p.display)
    }
}

/* ------------------------------------------------------------------ C ABI */

#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_is_enum(param_name: *const c_char) -> bool {
    param_is_enum(c_str(param_name))
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_enum_count(param_name: *const c_char) -> c_int {
    if param_name.is_null() {
        return 2;
    }
    param_enum_count(c_str(param_name))
}

/// Human-readable label for enum params (e.g. "Off", "On").
#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_enum_label(
    param_name: *const c_char,
    index: c_int,
) -> *const c_char {
    if param_name.is_null() {
        return c"?".as_ptr();
    }

    match param_enum_label(c_str(param_name), index) {
        Some(label) => label.as_ptr(),
        None => {
            let buf = ENUM_LABEL_BUF.get();
            let cap = buf.len();
            write_c_buf(buf.as_mut_ptr().cast::<c_char>(), cap, &format!("{index}"));
            buf.as_ptr().cast::<c_char>()
        }
    }
}

/// Param display range for UI sliders.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_range(
    param_name: *const c_char,
    min_out: *mut f32,
    max_out: *mut f32,
    step_out: *mut f32,
) {
    let r = param_range(c_str(param_name));
    unsafe {
        if !min_out.is_null() {
            *min_out = r.min;
        }
        if !max_out.is_null() {
            *max_out = r.max;
        }
        if !step_out.is_null() {
            *step_out = r.step;
        }
    }
}

/// Integer scale for `lv_slider` (1, 10, or 100 for fractional steps).
#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_scale(param_name: *const c_char) -> c_int {
    param_scale(c_str(param_name))
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_display_to_slider(
    param_name: *const c_char,
    display: f32,
) -> c_int {
    display_to_slider(c_str(param_name), display)
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_slider_to_display(
    param_name: *const c_char,
    slider_value: c_int,
) -> f32 {
    let name = c_str(param_name);
    let r = param_range(name);
    r.min + slider_value as f32 / param_scale(name) as f32
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_slider_min(_param_name: *const c_char) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_slider_max(param_name: *const c_char) -> c_int {
    let name = c_str(param_name);
    display_to_slider(name, param_range(name).max)
}

/// Map display value ↔ normalized using param name heuristics.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_display_to_normalized(
    param_name: *const c_char,
    display: f32,
) -> f32 {
    display_to_normalized(c_str(param_name), display)
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_normalized_to_display(
    param_name: *const c_char,
    normalized: f32,
) -> f32 {
    normalized_to_display(c_str(param_name), normalized)
}

/// Format param for UI (e.g. "35.0 %").
#[no_mangle]
pub unsafe extern "C" fn nano_fx_format_param(
    block: *const NanoFxBlock,
    param_index: u8,
    buf: *mut c_char,
    cap: usize,
) {
    if buf.is_null() || cap == 0 {
        return;
    }
    let text = match unsafe { block.as_ref() } {
        Some(block) => format_param(block, param_index),
        None => String::from("-"),
    };
    write_c_buf(buf, cap, &text);
}
