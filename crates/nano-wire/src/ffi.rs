//! C ABI matching `nano_protocol.h` / `nano_proto.h` for the existing C call sites.

use core::slice;

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_fe_metadata_request as *const (),
        nano_request_current_state as *const (),
        nano_preset_change_ack as *const (),
        nano_post_save_refresh as *const (),
        nano_bpm_probe_payload as *const (),
        nano_amp_knob_payload as *const (),
        nano_capture_volume_payload as *const (),
        nano_capture_select_payload as *const (),
        nano_global_toggle_payload as *const (),
        nano_gate_reduction_payload as *const (),
        nano_fx_param_refresh_payload as *const (),
        nano_fx_param_payload as *const (),
        nano_bpm_tap_payload as *const (),
        nano_bpm_exit_payload as *const (),
        nano_fx_model_select_payload as *const (),
        nano_tuner_enable_payload as *const (),
        nano_tuner_enable_footswitch_payload as *const (),
        nano_tuner_disable_payload as *const (),
        nano_log_bytes as *const (),
        nano_log_bytes_full as *const (),
        nano_read_varint as *const (),
        nano_proto_first_string as *const (),
        nano_proto_field_varint as *const (),
        nano_parse_fe_preset_catalog as *const (),
        nano_parse_amp as *const (),
        nano_try_parse_capture_volume_notify as *const (),
        nano_parse_bpm as *const (),
        nano_log_bpm_hunt as *const (),
        nano_log_bpm_raw_scan as *const (),
        nano_parse_tuner_state as *const (),
        nano_parse_tuner_active as *const (),
        nano_try_parse_amp_knob_notify as *const (),
        nano_try_parse_amp_hw_notify as *const (),
        nano_try_parse_bpm_notify as *const (),
        nano_try_parse_bpm_notify_ex as *const (),
        nano_log_amp as *const (),
        nano_log_proto_fields as *const (),
        nano_log_tuner_state_fields as *const (),
    ];
    core::hint::black_box(fns);
}

use crate::builders;
use crate::parsers;
use crate::types::{Amp, Knob, TunerRoute, AMP_CAPTURE_NAME_MAX, PRESET_COUNT, PRESET_NAME_MAX};
use crate::varint;

#[repr(C)]
pub struct NanoAmpC {
    pub gain: u8,
    pub level: u8,
    pub bass: u8,
    pub mid: u8,
    pub treble: u8,
    pub capture_volume: u8,
    pub capture_slot: u8,
    pub capture_name: [u8; AMP_CAPTURE_NAME_MAX],
    pub have_capture_slot: bool,
    pub have_capture_name: bool,
    pub have_capture_volume: bool,
    pub valid: bool,
}

#[repr(C)]
pub struct NanoPresetCatalogC {
    pub names: [[u8; PRESET_NAME_MAX]; PRESET_COUNT],
    pub count: u8,
    pub valid: bool,
}

fn write_c_string(dst: &mut [u8], src: &str) {
    dst.fill(0);
    let bytes = src.as_bytes();
    let n = bytes.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&bytes[..n]);
}

fn amp_to_c(amp: &Amp, out: &mut NanoAmpC) {
    out.gain = amp.gain;
    out.level = amp.level;
    out.bass = amp.bass;
    out.mid = amp.mid;
    out.treble = amp.treble;
    out.capture_volume = amp.capture_volume;
    out.capture_slot = amp.capture_slot;
    write_c_string(&mut out.capture_name, &amp.capture_name);
    out.have_capture_slot = amp.have_capture_slot;
    out.have_capture_name = amp.have_capture_name;
    out.have_capture_volume = amp.have_capture_volume;
    out.valid = amp.valid;
}

unsafe fn buf_slice<'a>(buf: *mut u8, cap: usize) -> Option<&'a mut [u8]> {
    if buf.is_null() || cap == 0 {
        None
    } else {
        Some(unsafe { slice::from_raw_parts_mut(buf, cap) })
    }
}

unsafe fn data_slice<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() {
        None
    } else if len == 0 {
        Some(&[])
    } else {
        Some(unsafe { slice::from_raw_parts(data, len) })
    }
}

unsafe fn c_str<'a>(p: *const core::ffi::c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { core::ffi::CStr::from_ptr(p) }
        .to_str()
        .unwrap_or("")
}

#[no_mangle]
pub unsafe extern "C" fn nano_fe_metadata_request(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::fe_metadata_request(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_request_current_state(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::request_current_state(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_preset_change_ack(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::preset_change_ack(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_post_save_refresh(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::post_save_refresh(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_tuner_enable_payload(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::tuner_enable_payload(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_tuner_enable_footswitch_payload(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::tuner_enable_footswitch_payload(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_tuner_disable_payload(buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::tuner_disable_payload(buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_bpm_probe_payload(
    step: u32,
    buf: *mut u8,
    cap: usize,
    out_expects_state_dump: *mut bool,
) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        if !out_expects_state_dump.is_null() {
            unsafe { *out_expects_state_dump = false };
        }
        return 0;
    };
    let (n, expects) = builders::bpm_probe_payload(step, buf);
    if !out_expects_state_dump.is_null() {
        unsafe { *out_expects_state_dump = expects };
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn nano_amp_knob_payload(
    knob: u32,
    raw_value: u8,
    buf: *mut u8,
    cap: usize,
) -> usize {
    let Some(k) = Knob::from_u8(knob as u8) else {
        return 0;
    };
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::amp_knob_payload(k, raw_value, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_capture_volume_payload(
    raw_value: u8,
    buf: *mut u8,
    cap: usize,
) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::capture_volume_payload(raw_value, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_capture_select_payload(slot: u8, buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::capture_select_payload(slot, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_global_toggle_payload(
    id: u8,
    value: u8,
    buf: *mut u8,
    cap: usize,
) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::global_toggle_payload(id, value, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_gate_reduction_payload(pct: u8, buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::gate_reduction_payload(pct, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_refresh_payload(
    refresh_slot: u8,
    buf: *mut u8,
    cap: usize,
) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::fx_param_refresh_payload(refresh_slot, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_param_payload(
    model_slot: u8,
    param_index: u8,
    normalized: f32,
    buf: *mut u8,
    cap: usize,
) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::fx_param_payload(model_slot, param_index, normalized, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_bpm_tap_payload(bpm: f32, buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::bpm_tap_payload(bpm, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_bpm_exit_payload(bpm: f32, buf: *mut u8, cap: usize) -> usize {
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::bpm_exit_payload(bpm, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_fx_model_select_payload(
    model_slot: u8,
    id: *const u8,
    id_len: u8,
    buf: *mut u8,
    cap: usize,
) -> usize {
    if id.is_null() || id_len == 0 {
        return 0;
    }
    let id = unsafe { slice::from_raw_parts(id, id_len as usize) };
    let Some(buf) = (unsafe { buf_slice(buf, cap) }) else {
        return 0;
    };
    builders::fx_model_select_payload(model_slot, id, buf)
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_bytes(
    tag: *const core::ffi::c_char,
    label: *const core::ffi::c_char,
    data: *const u8,
    len: usize,
) {
    let tag = unsafe { c_str(tag) };
    let label = unsafe { c_str(label) };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return;
    };
    parsers::log_bytes(tag, label, data);
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_bytes_full(
    tag: *const core::ffi::c_char,
    label: *const core::ffi::c_char,
    data: *const u8,
    len: usize,
) {
    let tag = unsafe { c_str(tag) };
    let label = unsafe { c_str(label) };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return;
    };
    parsers::log_bytes_full(tag, label, data);
}

#[no_mangle]
pub unsafe extern "C" fn nano_read_varint(
    data: *const u8,
    len: usize,
    offset: usize,
    value: *mut u32,
    next: *mut usize,
) -> bool {
    if next.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match varint::read_varint(data, offset) {
        Some((v, n)) => {
            if !value.is_null() {
                unsafe { *value = v };
            }
            unsafe { *next = n };
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_proto_first_string(
    data: *const u8,
    len: usize,
    field_num: u32,
    out: *mut core::ffi::c_char,
    cap: usize,
) -> bool {
    if out.is_null() || cap == 0 {
        return false;
    }
    unsafe { *out = 0 };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    let Some(s) = parsers::first_string(data, field_num) else {
        return false;
    };
    let dst = unsafe { slice::from_raw_parts_mut(out as *mut u8, cap) };
    write_c_string(dst, &s);
    true
}

#[no_mangle]
pub unsafe extern "C" fn nano_proto_field_varint(
    data: *const u8,
    len: usize,
    field_num: u32,
    out_val: *mut u32,
) -> bool {
    if out_val.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::field_varint(data, field_num) {
        Some(v) => {
            unsafe { *out_val = v };
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_parse_fe_preset_catalog(
    data: *const u8,
    len: usize,
    out: *mut NanoPresetCatalogC,
) -> bool {
    if out.is_null() {
        return false;
    }
    let out = unsafe { &mut *out };
    *out = unsafe { core::mem::zeroed() };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    let Some(cat) = parsers::parse_fe_preset_catalog(data) else {
        return false;
    };
    out.count = cat.names.len().min(PRESET_COUNT) as u8;
    out.valid = cat.valid;
    for (i, name) in cat.names.iter().take(PRESET_COUNT).enumerate() {
        write_c_string(&mut out.names[i], name);
    }
    out.valid
}

#[no_mangle]
pub unsafe extern "C" fn nano_parse_amp(data: *const u8, len: usize, out: *mut NanoAmpC) -> bool {
    if out.is_null() {
        return false;
    }
    let out = unsafe { &mut *out };
    *out = unsafe { core::mem::zeroed() };
    out.capture_volume = 128;
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    let Some(amp) = parsers::parse_amp(data) else {
        return false;
    };
    amp_to_c(&amp, out);
    out.valid
}

#[no_mangle]
pub unsafe extern "C" fn nano_try_parse_capture_volume_notify(
    data: *const u8,
    len: usize,
    raw: *mut u8,
) -> bool {
    if raw.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::try_parse_capture_volume_notify(data) {
        Some(v) => {
            unsafe { *raw = v };
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_parse_bpm(data: *const u8, len: usize, out_bpm: *mut u16) -> bool {
    if out_bpm.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::parse_bpm(data) {
        Some(bpm) => {
            unsafe { *out_bpm = bpm };
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_bpm_hunt(
    tag: *const core::ffi::c_char,
    data: *const u8,
    len: usize,
) {
    let tag = unsafe { c_str(tag) };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return;
    };
    log::info!(target: tag, "BPM hunt: scan {}-byte state blob (rust)", data.len());
    if let Some(bpm) = parsers::parse_bpm(data) {
        log::info!(target: tag, "BPM hunt: parsed BPM={bpm}");
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_bpm_raw_scan(
    tag: *const core::ffi::c_char,
    label: *const core::ffi::c_char,
    data: *const u8,
    len: usize,
) {
    let tag = unsafe { c_str(tag) };
    let label = unsafe { c_str(label) };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return;
    };
    log::info!(target: tag, "BPM raw scan [{label}]: {} bytes (rust)", data.len());
}

#[no_mangle]
pub unsafe extern "C" fn nano_parse_tuner_state(
    data: *const u8,
    len: usize,
    out_active: *mut bool,
    out_route: *mut u32,
) -> bool {
    if out_active.is_null() || out_route.is_null() {
        return false;
    }
    unsafe {
        *out_active = false;
        *out_route = TunerRoute::Unknown as u32;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::parse_tuner_state(data) {
        Some((active, route)) => {
            unsafe {
                *out_active = active;
                *out_route = route as u32;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_parse_tuner_active(
    data: *const u8,
    len: usize,
    out_active: *mut bool,
) -> bool {
    if out_active.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::parse_tuner_active(data) {
        Some(active) => {
            unsafe { *out_active = active };
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_try_parse_amp_knob_notify(
    data: *const u8,
    len: usize,
    knob_id: *mut u8,
    raw: *mut u8,
) -> bool {
    if knob_id.is_null() || raw.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::try_parse_amp_knob_notify(data) {
        Some((k, v)) => {
            unsafe {
                *knob_id = k;
                *raw = v;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_try_parse_amp_hw_notify(
    data: *const u8,
    len: usize,
    knob_id: *mut u8,
    raw: *mut u8,
) -> bool {
    if knob_id.is_null() || raw.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::try_parse_amp_hw_notify(data) {
        Some((k, v)) => {
            unsafe {
                *knob_id = k;
                *raw = v;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_try_parse_bpm_notify_ex(
    data: *const u8,
    len: usize,
    out_bpm: *mut u16,
    out_tap_marker: *mut bool,
) -> bool {
    if out_bpm.is_null() {
        return false;
    }
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return false;
    };
    match parsers::try_parse_bpm_notify_ex(data) {
        Some((bpm, tap)) => {
            unsafe { *out_bpm = bpm };
            if !out_tap_marker.is_null() {
                unsafe { *out_tap_marker = tap };
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn nano_try_parse_bpm_notify(
    data: *const u8,
    len: usize,
    out_bpm: *mut u16,
) -> bool {
    unsafe { nano_try_parse_bpm_notify_ex(data, len, out_bpm, core::ptr::null_mut()) }
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_amp(tag: *const core::ffi::c_char, amp: *const NanoAmpC) {
    let tag = unsafe { c_str(tag) };
    if amp.is_null() {
        log::info!(target: tag, "Amp: (not parsed yet)");
        return;
    }
    let a = unsafe { &*amp };
    if !a.valid {
        log::info!(target: tag, "Amp: (not parsed yet)");
        return;
    }
    let rust_amp = Amp {
        gain: a.gain,
        level: a.level,
        bass: a.bass,
        mid: a.mid,
        treble: a.treble,
        capture_volume: a.capture_volume,
        capture_slot: a.capture_slot,
        capture_name: String::new(),
        have_capture_slot: a.have_capture_slot,
        have_capture_name: a.have_capture_name,
        have_capture_volume: a.have_capture_volume,
        valid: a.valid,
    };
    parsers::log_amp(tag, &rust_amp);
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_proto_fields(
    tag: *const core::ffi::c_char,
    label: *const core::ffi::c_char,
    data: *const u8,
    len: usize,
) {
    let tag = unsafe { c_str(tag) };
    let label = unsafe { c_str(label) };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return;
    };
    log::info!(
        target: tag,
        "STATE {label} — protobuf scan ({} bytes, rust)",
        data.len()
    );
    // Keep scan logging lighter than the C version; amp/BPM parsers are the source of truth.
    let mut i = 0usize;
    let mut entries = 0u32;
    while i < data.len() && entries < 128 {
        let Some((tag_val, next)) = varint::read_varint(data, i) else {
            break;
        };
        i = next;
        let field = tag_val >> 3;
        let wire = tag_val & 0x07;
        entries += 1;
        match wire {
            0 => {
                let Some((val, next)) = varint::read_varint(data, i) else {
                    break;
                };
                log::info!(target: tag, "STATE {label}   f{field} w0 varint {val}");
                i = next;
            }
            1 => {
                if i + 8 > data.len() {
                    break;
                }
                i += 8;
            }
            2 => {
                let Some((flen, next)) = varint::read_varint(data, i) else {
                    break;
                };
                i = next + flen as usize;
                if i > data.len() {
                    break;
                }
                log::info!(target: tag, "STATE {label}   f{field} w2 bytes len={flen}");
            }
            5 => {
                if i + 4 > data.len() {
                    break;
                }
                let f = f32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
                log::info!(target: tag, "STATE {label}   f{field} w5 float={f}");
                i += 4;
            }
            _ => break,
        }
    }
    log::info!(target: tag, "STATE {label} — scan done ({entries} entries)");
}

#[no_mangle]
pub unsafe extern "C" fn nano_log_tuner_state_fields(
    tag: *const core::ffi::c_char,
    data: *const u8,
    len: usize,
) {
    let tag = unsafe { c_str(tag) };
    let Some(data) = (unsafe { data_slice(data, len) }) else {
        return;
    };
    let f45 = parsers::field_varint(data, 45);
    let f48 = parsers::field_varint(data, 48);
    let f49 = parsers::field_varint(data, 49);
    let f50 = parsers::field_varint(data, 50);
    let f51 = parsers::field_varint(data, 51);
    log::info!(
        target: tag,
        "Tuner preset: f45={} f48={} f49={} f50={} f51={}",
        f45.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f48.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f49.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f50.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        f51.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
    );
}
