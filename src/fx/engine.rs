//! FX block state: preset parsing, the param-refresh queue and the C304 writes
//! (port of `components/nano/nano_fx.c`).

use core::ffi::{c_char, c_void, CStr};
use core::ptr;
use core::slice;
use std::ffi::CString;

// Explicit imports: `sys` also re-exports the `NANO_FX_*` defines from the header.
use esp_idf_svc::sys::{
    esp_timer_create, esp_timer_create_args_t, esp_timer_get_time, esp_timer_handle_t,
    esp_timer_start_once, esp_timer_stop, ESP_OK,
};
use log::{debug, info, warn};

use nano_wire::builders;
use nano_wire::varint::read_varint;

use super::catalog::catalog;
use super::models::*;
use super::params::{clampf, format_param, normalized_to_display};
use super::shared::Shared;
use super::types::*;
use super::util::c_str;

const TAG: &str = "nano_fx";
const FX_REFRESH_TIMEOUT_US: u64 = 900 * 1000;
const FX_REFRESH_GAP_US: u64 = 80 * 1000;
const FX_ENABLE_ASSERT_US: i64 = 3000 * 1000;
const FX_MODEL_WRITE_ASSERT_US: i64 = 30 * 1000 * 1000;

/// Per-block write/refresh slots and the preset field carrying the model ID.
struct SlotCfg {
    model_slot: u8,
    refresh_slot: u8,
    enable_slot: u8,
    model_field: u32,
}

const SLOT_CFG: [SlotCfg; NANO_FX_BLOCK_COUNT] = [
    SlotCfg { model_slot: 0x00, refresh_slot: 0x00, enable_slot: 0x04, model_field: 48 },
    SlotCfg { model_slot: 0x01, refresh_slot: 0x01, enable_slot: 0x05, model_field: 49 },
    SlotCfg { model_slot: 0x02, refresh_slot: 0x02, enable_slot: 0x06, model_field: 50 },
    SlotCfg { model_slot: 0x03, refresh_slot: 0x03, enable_slot: 0x07, model_field: 51 },
    SlotCfg { model_slot: 0x04, refresh_slot: 0x04, enable_slot: 0x08, model_field: 52 },
];

/// Device firmware also exposes 1-byte model IDs in these varint fields.
const MODEL_FIELD_LEGACY: [u32; NANO_FX_BLOCK_COUNT] = [26, 9, 13, 14, 15];
/// Alternate field numbers seen on some firmware builds.
const MODEL_FIELD_LEGACY_ALT: [u32; NANO_FX_BLOCK_COUNT] = [8, 10, 0, 0, 0];

struct Fx {
    fx: NanoFxState,
    refresh_queue: [u32; NANO_FX_BLOCK_COUNT],
    refresh_q_head: usize,
    refresh_q_len: usize,
    pending_block: u32,
    refresh_active: bool,
    refresh_timeout_timer: esp_timer_handle_t,
    refresh_gap_timer: esp_timer_handle_t,
    begin_refresh_timer: esp_timer_handle_t,
    block_on_asserted_us: [i64; NANO_FX_BLOCK_COUNT],
    block_model_write_us: [i64; NANO_FX_BLOCK_COUNT],
    /// Protects local Gate ON/OFF against stale live-poll until assert window ends.
    gate_assert_us: i64,
    /// -1 idle, 0 off, 1 on.
    gate_assert_on: i32,
    /// Protects local Gate amount against stale field-53 until assert window ends.
    gate_amount_assert_us: i64,
    /// -1 idle, else 0..100.
    gate_amount_assert_pct: i32,
    /// Model last selected via BLE write; preset/inference must not override.
    block_trusted_model: [*const NanoFxModelDef; NANO_FX_BLOCK_COUNT],
    generic_model: NanoFxModelDef,
    generic_param_ptrs: [*const c_char; NANO_FX_MAX_PARAMS],
    generic_param_names: [[u8; 12]; NANO_FX_MAX_PARAMS],
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
        }
    }
}

static FX: Shared<Fx> = Shared::new(Fx::new());

fn fx() -> &'static mut Fx {
    FX.get()
}

fn now_us() -> i64 {
    unsafe { esp_timer_get_time() }
}

fn c_label(label: &str) -> CString {
    CString::new(label).unwrap_or_default()
}

fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/* -------------------------------------------------------------- protobuf */

/// Advance past one field body; `None` when the blob is truncated.
fn skip_wire(data: &[u8], i: usize, wire: u32) -> Option<usize> {
    let len = data.len();
    match wire {
        0 => read_varint(data, i).map(|(_, next)| next),
        1 => (i + 8 <= len).then_some(i + 8),
        2 => {
            let (flen, next) = read_varint(data, i)?;
            let end = next + flen as usize;
            (end <= len).then_some(end)
        }
        5 => (i + 4 <= len).then_some(i + 4),
        _ => None,
    }
}

fn le_f32(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Extract FX model ID from preset field (wire-2 bytes and 1–3 byte varint raw).
fn extract_model_field(data: &[u8], field_num: u32) -> Option<&[u8]> {
    let len = data.len();
    let mut first: Option<&[u8]> = None;
    let mut best_wire2: Option<&[u8]> = None;
    let mut catalog_hit: Option<&[u8]> = None;

    let mut i = 0usize;
    while i < len {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;

        let field = tag >> 3;
        let wire = tag & 0x07;

        if field == field_num && wire == 0 {
            let raw_start = i;
            let Some((_, next)) = read_varint(data, i) else {
                break;
            };
            let raw = &data[raw_start..next];

            if first.is_none() && !raw.is_empty() {
                first = Some(raw);
            }
            // Field 51 wire-0 is the tuner-off marker, not a model ID.
            if field_num != 51 && !raw.is_empty() && raw.len() <= 3 && find_model_in_blob(raw).is_some()
            {
                catalog_hit = Some(raw);
            }
            i = next;
            continue;
        }

        if field == field_num && wire == 2 {
            let Some((flen, next)) = read_varint(data, i) else {
                break;
            };
            let flen = flen as usize;
            if next + flen > len {
                break;
            }
            let raw = &data[next..next + flen];

            if first.is_none() && flen > 0 {
                first = Some(raw);
            }
            if find_model_in_blob(raw).is_some() {
                catalog_hit = Some(raw);
            }
            if flen > 0 {
                best_wire2 = Some(raw);
            }
            i = next + flen;
            continue;
        }

        match skip_wire(data, i, wire) {
            Some(next) => i = next,
            None => break,
        }
    }

    catalog_hit
        .or(best_wire2)
        .or(if field_num != 51 { first } else { None })
}

fn extract_field_bytes(data: &[u8], field_num: u32) -> Option<&[u8]> {
    let len = data.len();
    let mut best: Option<&[u8]> = None;

    let mut i = 0usize;
    while i < len {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;

        let field = tag >> 3;
        let wire = tag & 0x07;

        if field == field_num && wire == 2 {
            let Some((flen, next)) = read_varint(data, i) else {
                break;
            };
            let flen = flen as usize;
            if next + flen > len {
                break;
            }
            let raw = &data[next..next + flen];
            if find_model_in_blob(raw).is_some() {
                return Some(raw);
            }
            if flen > 0 {
                best = Some(raw);
            }
            i = next + flen;
            continue;
        }

        match skip_wire(data, i, wire) {
            Some(next) => i = next,
            None => break,
        }
    }

    best
}

fn extract_field_varint(data: &[u8], field_num: u32) -> Option<u32> {
    let len = data.len();
    let mut i = 0usize;
    while i < len {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;

        let field = tag >> 3;
        let wire = tag & 0x07;

        if field == field_num && wire == 0 {
            return read_varint(data, i).map(|(val, _)| val);
        }

        match skip_wire(data, i, wire) {
            Some(next) => i = next,
            None => break,
        }
    }
    None
}

/// Last fixed32 value for the field (the C port keeps scanning to the end).
fn extract_field_fixed32_float(data: &[u8], field_num: u32) -> Option<f32> {
    let len = data.len();
    let mut found: Option<f32> = None;

    let mut i = 0usize;
    while i < len {
        let Some((tag, next)) = read_varint(data, i) else {
            break;
        };
        i = next;

        let field = tag >> 3;
        let wire = tag & 0x07;

        if field == field_num && wire == 5 {
            if i + 4 > len {
                break;
            }
            found = Some(le_f32(&data[i..i + 4]));
            i += 4;
            continue;
        }

        match skip_wire(data, i, wire) {
            Some(next) => i = next,
            None => break,
        }
    }

    found
}

/// Scan raw dump for gate amount (field 53 fixed32) even if earlier fields are truncated.
fn scan_gate_amount_field53(data: &[u8]) -> Option<f32> {
    // Tag for field 53 wire5 = (53<<3)|5 = 429 = AD 03
    if data.len() < 6 {
        return None;
    }
    for i in 0..=(data.len() - 6) {
        if data[i] != 0xAD || data[i + 1] != 0x03 {
            continue;
        }
        let value = le_f32(&data[i + 2..i + 6]);
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            continue;
        }
        return Some(value);
    }
    None
}

/// Scan raw dump for gate bypass (field 54 varint 0/1) even if earlier fields are truncated.
fn scan_gate_bypass_field54(data: &[u8]) -> Option<u32> {
    // Tag for field 54 wire0 = (54<<3)|0 = 432 = B0 03
    if data.len() < 3 {
        return None;
    }
    for i in 0..=(data.len() - 3) {
        if data[i] != 0xB0 || data[i + 1] != 0x03 {
            continue;
        }
        let Some((val, _)) = read_varint(data, i + 2) else {
            continue;
        };
        if val > 1 {
            continue;
        }
        return Some(val);
    }
    None
}

/* ------------------------------------------------------------ model apply */

fn log_model_id_hex(label: &str, raw: &[u8]) {
    let mut hex = String::new();
    for b in raw.iter().take(11) {
        hex.push_str(&format!("{b:02X}"));
    }
    info!(target: TAG, "{label} model field raw: {hex}");
}

fn store_catalog_model_id(block: &mut NanoFxBlock, model: &NanoFxModelDef) {
    let id = model.id_bytes();
    if id.is_empty() {
        return;
    }
    let n = id.len().min(block.model_id_raw.len());
    block.model_id_raw[..n].copy_from_slice(&id[..n]);
    block.model_id_len = n as u8;
    block.has_model_id = true;
}

fn reset_block_params(block: &mut NanoFxBlock) {
    block.params_loaded = false;
    block.param_count = 0;
    block.params = [NanoFxParam::zeroed(); NANO_FX_MAX_PARAMS];
}

fn apply_model(block: &mut NanoFxBlock, model: Option<&'static NanoFxModelDef>) {
    let model_ptr_new = model_ptr(model);
    if ptr::eq(block.model, model_ptr_new) && !block.model.is_null() && block.param_count > 0 {
        return;
    }

    block.model = model_ptr_new;
    reset_block_params(block);

    if let Some(model) = model {
        block.param_count = model.param_count;
        for i in 0..(model.param_count as usize).min(NANO_FX_MAX_PARAMS) {
            let name = c_str(unsafe { *model.param_names.add(i) });
            block.params[i].display = normalized_to_display(name, 0.5);
            block.params[i].normalized = 0.5;
            block.params[i].valid = false;
        }
    }
}

/// Params with no catalog model: "Param 1".."Param N" backed by module storage.
fn set_generic_model(block: &mut NanoFxBlock, count: u8) {
    if count == 0 {
        return;
    }
    let count = count.min(NANO_FX_MAX_PARAMS as u8);
    let s = fx();

    for i in 0..count as usize {
        let name = format!("Param {}", i + 1);
        let dst = &mut s.generic_param_names[i];
        dst.fill(0);
        let bytes = name.as_bytes();
        let n = bytes.len().min(dst.len() - 1);
        dst[..n].copy_from_slice(&bytes[..n]);
        s.generic_param_ptrs[i] = dst.as_ptr().cast::<c_char>();
        block.params[i].display = 50.0;
        block.params[i].normalized = 0.5;
        block.params[i].valid = false;
    }

    s.generic_model.name = c"FX".as_ptr();
    s.generic_model.id_len = 0;
    s.generic_model.param_count = count;
    s.generic_model.param_names = s.generic_param_ptrs.as_ptr();
    block.model = &s.generic_model as *const NanoFxModelDef;
    block.param_count = count;
}

fn try_apply_catalog_model(block: &mut NanoFxBlock) {
    if !block.has_model_id || block.model_id_len == 0 {
        return;
    }

    let Some(model) = find_model_in_blob(block.model_id()) else {
        return;
    };
    if ptr::eq(block.model, model as *const NanoFxModelDef) {
        return;
    }

    apply_model(block, Some(model));
}

fn upgrade_block_model(block: &mut NanoFxBlock, model: &'static NanoFxModelDef, floats: &[f32]) {
    if ptr::eq(block.model, model as *const NanoFxModelDef) {
        return;
    }

    block.model = model as *const NanoFxModelDef;
    block.param_count = model.param_count;

    let n = (model.param_count as usize)
        .min(floats.len())
        .min(NANO_FX_MAX_PARAMS);

    for i in 0..n {
        let name = c_str(unsafe { *model.param_names.add(i) });
        block.params[i].normalized = clampf(floats[i], 0.0, 1.0);
        block.params[i].display = normalized_to_display(name, block.params[i].normalized);
        block.params[i].valid = true;
    }

    for i in n..(model.param_count as usize).min(NANO_FX_MAX_PARAMS) {
        let name = c_str(unsafe { *model.param_names.add(i) });
        block.params[i].display = normalized_to_display(name, 0.5);
        block.params[i].normalized = 0.5;
        block.params[i].valid = false;
    }
}

/* ---------------------------------------------------------- model guesses */

/// Post FX 2 is delay-only (NanOS 2.0); scan preset for delay wire IDs.
fn find_post2_delay_in_preset(data: &[u8]) -> Option<&'static NanoFxModelDef> {
    const DELAYS: [([u8; 2], &str); 6] = [
        ([0xFA, 0x2E], "Analog Delay"),
        ([0xFF, 0x2E], "Circular Delay"),
        ([0xF4, 0x2E], "Tape Delay"),
        ([0xFB, 0x2E], "Digital Delay (ST)"),
        ([0xFC, 0x2E], "Dual Delay"),
        ([0xFE, 0x2E], "Dual Reverse Delay"),
    ];

    for (id, name) in DELAYS {
        if find_model_id_in_preset(data, &id).is_some() {
            return find_model_by_name(name);
        }
    }

    None
}

fn infer_post2_delay(param_count: u8) -> Option<&'static NanoFxModelDef> {
    match param_count {
        11 => find_model_by_name("Tape Delay"),
        12 => find_model_by_name("Analog Delay"),
        18 => find_model_by_name("Digital Delay (ST)"),
        22 => find_model_by_name("Dual Delay"),
        _ => None,
    }
}

fn infer_model_by_params(block: u32, param_count: u8) -> Option<&'static NanoFxModelDef> {
    if block == NANO_FX_PRE1 && param_count == 5 {
        return find_model_by_name("Transpose");
    }

    if block == NANO_FX_POST1 {
        if param_count == 3 || param_count == 4 {
            return find_model_by_name("Doubler");
        }
        if param_count == 5 {
            return find_model_by_name("Chief DC2W (ST)");
        }
    }

    if block == NANO_FX_POST2 {
        return infer_post2_delay(param_count);
    }

    let mut found: Option<&'static NanoFxModelDef> = None;
    let mut matches = 0usize;

    for model in catalog() {
        if model.param_count != param_count {
            continue;
        }

        let ok = match block {
            NANO_FX_POST3 => is_reverb_model(model),
            NANO_FX_POST2 => is_delay_model(model),
            NANO_FX_POST1 => is_modulation_model(model) || param_count <= 8,
            NANO_FX_PRE1 | NANO_FX_PRE2 => {
                !is_reverb_model(model) && !is_delay_model(model) && param_count <= 12
            }
            _ => false,
        };

        if !ok {
            continue;
        }

        found = Some(model);
        matches += 1;
    }

    if matches == 1 {
        return found;
    }

    if block == NANO_FX_POST3 && found.is_some_and(is_reverb_model) {
        return found;
    }

    None
}

fn try_parse_legacy_model_field(data: &[u8], block_idx: usize, block: &mut NanoFxBlock) -> bool {
    if block_idx >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let alt = MODEL_FIELD_LEGACY_ALT[block_idx];
    let fields: &[u32] = if alt != 0 {
        &[MODEL_FIELD_LEGACY[block_idx], alt]
    } else {
        &[MODEL_FIELD_LEGACY[block_idx]]
    };
    let label = BLOCK_LABELS[block_idx];

    for &field in fields {
        if field == 0 {
            continue;
        }

        let Some(val) = extract_field_varint(data, field) else {
            continue;
        };
        if val == 0 || val > 0xFF {
            continue;
        }

        block.model_id_raw[0] = val as u8;
        block.model_id_len = 1;
        block.has_model_id = true;

        log_model_id_hex(label, block.model_id());
        info!(target: TAG, "{label} legacy model field {field} = {val}");

        match resolve_legacy_model(block_idx as u32, val as u8) {
            Some(model) => {
                store_catalog_model_id(block, model);
                apply_model(block, Some(model));
                info!(target: TAG, "{label} model: {}", model_name(model));
            }
            None => {
                warn!(target: TAG, "{label} legacy model ID {val} not in catalog");
                block.model = ptr::null();
                block.param_count = 0;
                block.params_loaded = false;
            }
        }

        return true;
    }

    false
}

fn try_apply_slot_exclusive_model(data: &[u8], block_idx: usize, block: &mut NanoFxBlock) -> bool {
    const EXCLUSIVE: [(u32, &[u8], &str); 2] = [
        (NANO_FX_PRE1, &[0xD1, 0x8C, 0x01], "Transpose"),
        (NANO_FX_POST1, &[0x8B, 0x7D], "Doubler"),
    ];

    for (slot, id, name) in EXCLUSIVE {
        if block_idx as u32 != slot {
            continue;
        }

        let model = find_model_id_in_preset(data, id).or_else(|| find_model_by_name(name));
        let Some(model) = model else {
            continue;
        };

        block.id = block_idx as u32;
        block.valid = true;
        store_catalog_model_id(block, model);
        apply_model(block, Some(model));
        info!(
            target: TAG,
            "{} model: {} (slot ID)", BLOCK_LABELS[block_idx], model_name(model)
        );
        return true;
    }

    false
}

/* --------------------------------------------------------- refresh queue */

fn block_needs_refresh(block: &NanoFxBlock) -> bool {
    block.on && !block.params_loaded
}

fn block_model_write_recent(block: u32) -> bool {
    let s = fx();
    let idx = block as usize;
    if idx >= NANO_FX_BLOCK_COUNT || s.block_model_write_us[idx] == 0 {
        return false;
    }
    now_us() - s.block_model_write_us[idx] < FX_MODEL_WRITE_ASSERT_US
}

/// Preset 1-byte firmware enums often lag behind a BLE model select.
fn preset_model_stale_legacy(local: &NanoFxBlock, preset: &NanoFxBlock) -> bool {
    if local.model.is_null() || local.model_id_len < 2 || preset.model_id_len != 1 {
        return false;
    }
    local.params_loaded || block_model_write_recent(local.id)
}

fn create_timer(
    handle: &mut esp_timer_handle_t,
    cb: unsafe extern "C" fn(*mut c_void),
    name: &CStr,
) {
    if !handle.is_null() {
        return;
    }
    // Zeroed rest mirrors the C designated initializer (dispatch = ESP_TIMER_TASK).
    let mut args: esp_timer_create_args_t = unsafe { core::mem::zeroed() };
    args.callback = Some(cb);
    args.name = name.as_ptr();
    let err = unsafe { esp_timer_create(&args, handle) };
    if err != ESP_OK {
        panic!("esp_timer_create failed: {err}");
    }
}

fn ensure_refresh_timers() {
    let s = fx();
    create_timer(&mut s.refresh_timeout_timer, refresh_timeout_cb, c"nano_fx_to");
    create_timer(&mut s.refresh_gap_timer, refresh_gap_cb, c"nano_fx_gap");
    create_timer(&mut s.begin_refresh_timer, begin_refresh_cb, c"nano_fx_begin");
}

unsafe extern "C" fn refresh_timeout_cb(_arg: *mut c_void) {
    let s = fx();
    warn!(
        target: TAG,
        "FX param refresh timed out for {}", block_label(s.pending_block)
    );
    s.refresh_active = false;
    send_next_refresh();
}

unsafe extern "C" fn refresh_gap_cb(_arg: *mut c_void) {
    send_next_refresh();
}

unsafe extern "C" fn begin_refresh_cb(_arg: *mut c_void) {
    run_begin_param_refresh();
}

fn block_in_refresh_queue(block: u32) -> bool {
    let s = fx();
    s.refresh_queue[s.refresh_q_head..s.refresh_q_len]
        .iter()
        .any(|&queued| queued == block)
}

fn merge_refresh_queue_from_blocks() {
    let s = fx();
    for i in 0..NANO_FX_BLOCK_COUNT {
        if !block_needs_refresh(&s.fx.blocks[i]) {
            continue;
        }
        if block_in_refresh_queue(i as u32) {
            continue;
        }
        if s.refresh_q_len < NANO_FX_BLOCK_COUNT {
            s.refresh_queue[s.refresh_q_len] = i as u32;
            s.refresh_q_len += 1;
        }
    }
}

fn enqueue_param_refresh(block: u32) {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }

    ensure_refresh_timers();
    let s = fx();
    s.fx.blocks[block as usize].params_loaded = false;

    if block_in_refresh_queue(block) {
        if !s.refresh_active {
            send_next_refresh();
        }
        return;
    }

    if s.refresh_q_len < NANO_FX_BLOCK_COUNT {
        s.refresh_queue[s.refresh_q_len] = block;
        s.refresh_q_len += 1;
    }

    if !s.refresh_active {
        send_next_refresh();
    }
}

fn send_next_refresh() {
    ensure_refresh_timers();
    let s = fx();

    while s.refresh_q_head < s.refresh_q_len {
        let block_id = s.refresh_queue[s.refresh_q_head];
        let idx = block_id as usize;
        s.refresh_q_head += 1;

        if !s.fx.blocks[idx].on || !block_needs_refresh(&s.fx.blocks[idx]) {
            continue;
        }

        s.pending_block = block_id;
        s.refresh_active = true;

        let mut buf = [0u8; 16];
        let n = builders::fx_param_refresh_payload(SLOT_CFG[idx].refresh_slot, &mut buf);
        if n == 0 || !crate::ble::nano_ble_connected() {
            s.refresh_active = false;
            continue;
        }

        let label = BLOCK_LABELS[idx];
        let model_text = clip(&block_model_text(&s.fx.blocks[idx]), 31);
        info!(target: TAG, "Refreshing params: {label} ({model_text})");
        let _ = crate::ble::nano_ble_write_c304(
            (c_label(&clip(&format!("{label} param refresh"), 31))).as_ptr(),
            buf[..n].as_ptr(),
            buf[..n].len(),
        );

        unsafe {
            esp_timer_stop(s.refresh_timeout_timer);
            esp_timer_start_once(s.refresh_timeout_timer, FX_REFRESH_TIMEOUT_US);
        }
        return;
    }

    s.refresh_active = false;
    s.refresh_q_len = 0;
    s.refresh_q_head = 0;
    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
}

fn run_begin_param_refresh() {
    let s = fx();
    if s.refresh_active {
        merge_refresh_queue_from_blocks();
        return;
    }

    s.refresh_q_len = 0;
    s.refresh_q_head = 0;

    for i in 0..NANO_FX_BLOCK_COUNT {
        if block_needs_refresh(&s.fx.blocks[i]) {
            s.refresh_queue[s.refresh_q_len] = i as u32;
            s.refresh_q_len += 1;
        }
    }

    if s.refresh_q_len == 0 {
        info!(target: TAG, "No active FX blocks to refresh");
        unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
        return;
    }

    info!(
        target: TAG,
        "Queueing FX param refresh for {} block(s)", s.refresh_q_len
    );
    send_next_refresh();
}

/* ------------------------------------------------------------------ gate */

fn apply_gate_from_parsed(parsed: &NanoFxState) {
    if !parsed.gate.valid {
        return;
    }
    let s = fx();

    if parsed.gate.on_known {
        let remote_on = parsed.gate.on;
        let mut accept_on = true;
        if s.gate_assert_on >= 0 && s.gate_assert_us != 0 {
            if now_us() - s.gate_assert_us <= FX_ENABLE_ASSERT_US {
                let want_on = s.gate_assert_on != 0;
                if remote_on != want_on {
                    accept_on = false;
                } else {
                    s.gate_assert_on = -1;
                    s.gate_assert_us = 0;
                }
            } else {
                s.gate_assert_on = -1;
                s.gate_assert_us = 0;
            }
        }
        if accept_on {
            s.fx.gate.on = remote_on;
            s.fx.gate.on_known = true;
        }
    }

    if parsed.gate.reduction_valid {
        let mut accept_amt = true;
        if s.gate_amount_assert_pct >= 0 && s.gate_amount_assert_us != 0 {
            if now_us() - s.gate_amount_assert_us <= FX_ENABLE_ASSERT_US {
                if parsed.gate.reduction_pct as i32 != s.gate_amount_assert_pct {
                    accept_amt = false;
                } else {
                    s.gate_amount_assert_pct = -1;
                    s.gate_amount_assert_us = 0;
                }
            } else {
                s.gate_amount_assert_pct = -1;
                s.gate_amount_assert_us = 0;
            }
        }
        if accept_amt {
            s.fx.gate.reduction_pct = parsed.gate.reduction_pct;
            s.fx.gate.reduction_valid = true;
        }
    } else if !s.fx.gate.reduction_valid && s.fx.gate.reduction_pct == 0 {
        s.fx.gate.reduction_pct = 50;
    }

    s.fx.gate.valid = true;
    s.fx.valid = true;
    let state = if s.fx.gate.on_known {
        if s.fx.gate.on {
            "ON"
        } else {
            "OFF"
        }
    } else {
        "?"
    };
    info!(
        target: TAG,
        "Gate sync: {state}{} amount={}%{}",
        if s.fx.gate.on_known { "" } else { " (bypass unknown)" },
        s.fx.gate.reduction_pct,
        if parsed.gate.reduction_valid { " (from dump)" } else { "" }
    );
}

/* ------------------------------------------------------------ C305 params */

fn decode_float32_array(data: &[u8], out: &mut [f32]) -> usize {
    let mut count = 0;
    let mut i = 0usize;
    while i + 3 < data.len() && count < out.len() {
        out[count] = le_f32(&data[i..i + 4]);
        count += 1;
        i += 4;
    }
    count
}

fn find_fx_param_response(data: &[u8]) -> usize {
    let len = data.len();
    if len < 6 {
        return len;
    }

    for i in 0..=(len - 5) {
        if data[i] == 0xC0 && data[i + 1] == 0x08 && data[i + 2] == 0x06 && data[i + 3] == 0x22 {
            return i;
        }
    }

    len
}

/* ------------------------------------------------------------------ C ABI */

#[no_mangle]
pub extern "C" fn nano_fx_reset() {
    let s = fx();
    s.fx = NanoFxState::zeroed();
    s.block_on_asserted_us = [0; NANO_FX_BLOCK_COUNT];
    s.block_model_write_us = [0; NANO_FX_BLOCK_COUNT];
    s.block_trusted_model = [ptr::null(); NANO_FX_BLOCK_COUNT];
    s.gate_assert_us = 0;
    s.gate_assert_on = -1;
    s.gate_amount_assert_us = 0;
    s.gate_amount_assert_pct = -1;
    s.refresh_q_len = 0;
    s.refresh_q_head = 0;
    s.refresh_active = false;
    for timer in [
        s.refresh_timeout_timer,
        s.refresh_gap_timer,
        s.begin_refresh_timer,
    ] {
        if !timer.is_null() {
            unsafe { esp_timer_stop(timer) };
        }
    }
}

#[no_mangle]
pub extern "C" fn nano_fx_get() -> *const NanoFxState {
    let s = fx();
    if s.fx.valid {
        &s.fx as *const NanoFxState
    } else {
        ptr::null()
    }
}

/// Parse gate + FX block models/bypass from preset state blob.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_parse_preset(
    data: *const u8,
    len: usize,
    out: *mut NanoFxState,
) -> bool {
    if data.is_null() || len == 0 || out.is_null() {
        return false;
    }
    parse_preset(unsafe { slice::from_raw_parts(data, len) }, unsafe {
        &mut *out
    })
}

fn parse_preset(data: &[u8], out: &mut NanoFxState) -> bool {
    let mut any = false;

    if let Some(bypass) = extract_field_bytes(data, 31) {
        if bypass.len() >= NANO_FX_BLOCK_COUNT {
            for i in 0..NANO_FX_BLOCK_COUNT {
                out.blocks[i].id = i as u32;
                out.blocks[i].on = bypass[i] == 0;
                out.blocks[i].valid = true;
                any = true;
            }
        }
    }

    for i in 0..NANO_FX_BLOCK_COUNT {
        let label = BLOCK_LABELS[i];
        let raw = extract_model_field(data, SLOT_CFG[i].model_field).filter(|raw| !raw.is_empty());

        if let Some(raw) = raw {
            let block = &mut out.blocks[i];
            block.id = i as u32;
            block.valid = true;
            any = true;

            let n = raw.len().min(block.model_id_raw.len());
            block.model_id_raw[..n].copy_from_slice(&raw[..n]);
            block.model_id_len = n as u8;
            block.has_model_id = true;

            log_model_id_hex(label, block.model_id());

            let mut model = find_model_in_blob(block.model_id());
            if let Some(model) = model {
                apply_model(block, Some(model));
                info!(target: TAG, "{label} model: {}", model_name(model));
                continue;
            }

            if i as u32 == NANO_FX_POST2 {
                model = find_post2_delay_in_preset(data);
                if model.is_none() && block.model_id_len == 1 {
                    model = resolve_legacy_model(NANO_FX_POST2, block.model_id_raw[0]);
                }
                if let Some(model) = model {
                    store_catalog_model_id(block, model);
                    apply_model(block, Some(model));
                    info!(target: TAG, "{label} model: {}", model_name(model));
                    continue;
                }
            }

            warn!(
                target: TAG,
                "{label} model ID not in catalog — will refresh params anyway"
            );
            block.model = ptr::null();
            block.param_count = 0;
            block.params_loaded = false;
        } else if try_parse_legacy_model_field(data, i, &mut out.blocks[i]) {
            any = true;
        } else if try_apply_slot_exclusive_model(data, i, &mut out.blocks[i]) {
            any = true;
        }
    }

    {
        let delay = find_post2_delay_in_preset(data);
        let p2 = &mut out.blocks[NANO_FX_POST2 as usize];
        let keep = model_ref(p2.model).is_some_and(is_delay_model);
        if let Some(delay) = delay {
            if !keep {
                p2.valid = true;
                store_catalog_model_id(p2, delay);
                apply_model(p2, Some(delay));
                info!(
                    target: TAG,
                    "{} model: {} (preset scan)",
                    BLOCK_LABELS[NANO_FX_POST2 as usize],
                    model_name(delay)
                );
                any = true;
            }
        }
    }

    /*
     * Gate always salvage-scanned — not gated on amp/FX models. Truncated dumps
     * often stop mid-f34 before a sequential walk reaches f53/f54; short C305
     * tails after a mid-dump gap may contain only those fields.
     * Field 54 = inverted bypass (0=ON, 1=OFF). Proto3 omits default 0 (ON): if
     * we found amount (f53) in the gate region but no f54, treat as ON.
     */
    {
        let mut gate_field = extract_field_varint(data, 54);
        let mut f54_src = if gate_field.is_some() { "walk" } else { "none" };
        if gate_field.is_none() {
            gate_field = scan_gate_bypass_field54(data);
            if gate_field.is_some() {
                f54_src = "scan";
            }
        }

        // Amount: field 53 fixed32 in [0,1] → raw 0..255 → percent.
        let mut amt_src = "none";
        let mut gate_norm = -1.0f32;
        match extract_field_fixed32_float(data, 53) {
            Some(value) if value.is_finite() && (0.0..=1.0).contains(&value) => {
                gate_norm = value;
                amt_src = "walk";
            }
            _ => {
                if let Some(value) = scan_gate_amount_field53(data) {
                    gate_norm = value;
                    amt_src = "scan";
                }
            }
        }

        let mut have_amt = false;
        let mut amt_raw = 0u32;
        if (0.0..=1.0).contains(&gate_norm) {
            let raw = (gate_norm * 255.0).round().clamp(0.0, 255.0) as u32;
            // Placeholder ~0.1 in some dumps is not a real amount.
            if raw > 32 || gate_norm >= 0.15 {
                amt_raw = raw;
                out.gate.reduction_pct = ((amt_raw * 100 + 127) / 255) as u8;
                have_amt = true;
            } else {
                amt_src = "none";
            }
        }
        if have_amt {
            if out.gate.reduction_pct > 100 {
                out.gate.reduction_pct = 100;
            }
            out.gate.reduction_valid = true;
            out.gate.valid = true;
            any = true;
        }

        if let Some(field) = gate_field {
            out.gate.on = field == 0;
            out.gate.on_known = true;
            out.gate.valid = true;
            any = true;
        } else if have_amt {
            // Reached gate region (f53 present); omitted f54 = enabled.
            out.gate.on = true;
            out.gate.on_known = true;
            out.gate.valid = true;
            f54_src = "omit";
            any = true;
        }

        info!(
            target: TAG,
            "Gate parse: dump={} f54={}/{} on_known={} amt={}/{}% (src={amt_src} raw={})",
            data.len(),
            if gate_field.is_some() || have_amt { f54_src } else { "no" },
            gate_field.unwrap_or(0),
            out.gate.on_known as i32,
            if out.gate.reduction_valid { "yes" } else { "no" },
            if out.gate.reduction_valid { out.gate.reduction_pct } else { 0 },
            if have_amt { amt_raw } else { 0 }
        );
    }

    out.valid = any;
    any
}

/// True when raw dump bytes contain gate amount (f53) and/or bypass (f54) tags.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_dump_has_gate_fields(data: *const u8, len: usize) -> bool {
    if data.is_null() || len == 0 {
        return false;
    }
    let data = unsafe { slice::from_raw_parts(data, len) };
    scan_gate_amount_field53(data).is_some() || scan_gate_bypass_field54(data).is_some()
}

/// Salvage gate ON/OFF + amount from any blob that contains f53/f54 tags
/// (full preset dump or a late high-bit C305 tail after a mid-dump gap).
#[no_mangle]
pub unsafe extern "C" fn nano_fx_try_apply_gate_bytes(data: *const u8, len: usize) -> bool {
    if data.is_null() || len < 6 {
        return false;
    }
    let data = unsafe { slice::from_raw_parts(data, len) };
    if scan_gate_amount_field53(data).is_none() && scan_gate_bypass_field54(data).is_none() {
        return false;
    }

    let mut parsed = NanoFxState::reset_with_ids();
    if !parse_preset(data, &mut parsed) || !parsed.gate.valid {
        return false;
    }

    apply_gate_from_parsed(&parsed);
    unsafe { crate::ui::screen::nano_ui_set_fx(&fx().fx) };
    true
}

/// After preset parse, queue param refresh for active blocks.
#[no_mangle]
pub extern "C" fn nano_fx_begin_param_refresh() {
    ensure_refresh_timers();
    let s = fx();
    unsafe {
        esp_timer_stop(s.begin_refresh_timer);
        esp_timer_start_once(s.begin_refresh_timer, 1);
    }
}

/// Parse preset blob into internal FX state and start param refresh.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_sync_from_preset(data: *const u8, len: usize) {
    if data.is_null() || len == 0 {
        return;
    }
    sync_from_preset(unsafe { slice::from_raw_parts(data, len) });
}

fn sync_from_preset(data: &[u8]) {
    let mut parsed = NanoFxState::reset_with_ids();

    if !parse_preset(data, &mut parsed) {
        return;
    }

    if parsed.gate.valid {
        apply_gate_from_parsed(&parsed);
    }

    let s = fx();
    for i in 0..NANO_FX_BLOCK_COUNT {
        let src = &parsed.blocks[i];
        let dst = &mut s.fx.blocks[i];

        if src.valid {
            if src.on {
                dst.on = true;
                s.block_on_asserted_us[i] = 0;
            } else if !dst.on {
                dst.on = false;
            } else if s.block_on_asserted_us[i] != 0
                && now_us() - s.block_on_asserted_us[i] > FX_ENABLE_ASSERT_US
            {
                dst.on = false;
                s.block_on_asserted_us[i] = 0;
            }
            dst.valid = true;
        }

        if src.model_id_len == 0 {
            continue;
        }

        if !s.block_trusted_model[i].is_null()
            || block_model_write_recent(i as u32)
            || preset_model_stale_legacy(dst, src)
        {
            continue;
        }

        dst.has_model_id = true;

        let id_changed = dst.model_id_len != src.model_id_len || dst.model_id() != src.model_id();

        if id_changed {
            let n = src.model_id_len as usize;
            dst.model_id_len = src.model_id_len;
            dst.model_id_raw[..n].copy_from_slice(&src.model_id_raw[..n]);

            let model = model_ref(src.model).or_else(|| find_model_in_blob(dst.model_id()));
            match model {
                Some(model) => apply_model(dst, Some(model)),
                None => {
                    dst.model = ptr::null();
                    dst.param_count = 0;
                    dst.params_loaded = false;
                }
            }
        } else if dst.model.is_null() {
            let model = model_ref(src.model).or_else(|| find_model_in_blob(dst.model_id()));
            if let Some(model) = model {
                apply_model(dst, Some(model));
            }
        }
    }

    s.fx.valid = true;
    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
    nano_fx_begin_param_refresh();
}

/// Feed C305; returns true if consumed (FX param refresh response).
#[no_mangle]
pub unsafe extern "C" fn nano_fx_feed_c305(data: *const u8, len: usize) -> bool {
    if data.is_null() || len < 10 {
        return false;
    }
    feed_c305(unsafe { slice::from_raw_parts(data, len) })
}

fn feed_c305(data: &[u8]) -> bool {
    let len = data.len();
    let hdr = find_fx_param_response(data);
    if hdr >= len || hdr + 6 > len {
        return false;
    }

    let s = fx();
    if !s.refresh_active {
        debug!(target: TAG, "FX param response with no pending refresh");
        return true;
    }

    let pending = s.pending_block;
    let float_len = data[hdr + 4] as usize;
    let start = hdr + 5;
    let end = start + float_len;

    if end > len || float_len == 0 {
        warn!(
            target: TAG,
            "Invalid FX param response for {}", block_label(pending)
        );
        s.refresh_active = false;
        unsafe {
            esp_timer_stop(s.refresh_timeout_timer);
            esp_timer_start_once(s.refresh_gap_timer, FX_REFRESH_GAP_US);
        }
        return true;
    }

    let mut floats = [0f32; NANO_FX_MAX_PARAMS];
    let float_count = decode_float32_array(&data[start..end], &mut floats);
    let floats = &floats[..float_count];

    let label = BLOCK_LABELS[pending as usize];
    let trusted = model_ref(s.block_trusted_model[pending as usize]);
    let block = &mut s.fx.blocks[pending as usize];

    try_apply_catalog_model(block);

    if let Some(trusted) = trusted {
        if !ptr::eq(block.model, trusted as *const NanoFxModelDef) {
            store_catalog_model_id(block, trusted);
            apply_model(block, Some(trusted));
        }
    } else if block.model.is_null() && block.has_model_id && block.model_id_len == 1 {
        if let Some(legacy) = resolve_legacy_model(pending, block.model_id_raw[0]) {
            store_catalog_model_id(block, legacy);
            apply_model(block, Some(legacy));
        }
    }

    if trusted.is_none() {
        let mut inferred = infer_model_by_params(pending, float_count as u8);
        if model_ref(block.model).is_some_and(|m| m.param_count as usize == float_count) {
            inferred = None;
        } else if inferred.is_some() && block_model_write_recent(pending) {
            inferred = None;
        } else if inferred.is_some() && block.has_model_id && block.model_id_len >= 2 {
            inferred = None;
        }
        if let Some(inferred) = inferred {
            upgrade_block_model(block, inferred, floats);
            info!(target: TAG, "{label} inferred model: {}", model_name(inferred));
        } else if block.model.is_null() || block.param_count == 0 {
            set_generic_model(block, float_count as u8);
        }
    } else if block.model.is_null() || block.param_count == 0 {
        set_generic_model(block, float_count as u8);
    }

    let mut n = block.param_count as usize;
    if let Some(model) = model_ref(block.model) {
        n = n.min(model.param_count as usize);
    }
    n = n.min(float_count).min(NANO_FX_MAX_PARAMS);

    for i in 0..n {
        block.params[i].normalized = clampf(floats[i], 0.0, 1.0);
        let name = model_ref(block.model)
            .map(|model| c_str(unsafe { *model.param_names.add(i) }))
            .unwrap_or("");
        block.params[i].display = if name.is_empty() {
            block.params[i].normalized * 100.0
        } else {
            normalized_to_display(name, block.params[i].normalized)
        };
        block.params[i].valid = true;
    }

    block.params_loaded = true;
    info!(target: TAG, "{label} params loaded ({float_count} values)");

    if block.on && !block.has_model_id {
        unsafe { crate::state::nano_state_request_preset_refresh(c"FX model sync".as_ptr()) };
    }

    s.refresh_active = false;
    unsafe {
        esp_timer_stop(s.refresh_timeout_timer);
        esp_timer_start_once(s.refresh_gap_timer, FX_REFRESH_GAP_US);
    }
    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
    true
}

/* ---------------------------------------------------------------- writes */

#[no_mangle]
pub extern "C" fn nano_fx_write_gate_enabled(enabled: bool) -> bool {
    let s = fx();
    s.fx.gate.on = enabled;
    s.fx.gate.on_known = true;
    s.fx.gate.valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 16];
    let n = builders::global_toggle_payload(0x09, if enabled { 0x00 } else { 0x01 }, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }
    info!(target: TAG, "Gate {}", if enabled { "ON" } else { "OFF" });
    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
    if crate::ble::nano_ble_write_c304(
        (if enabled { c"Gate ON" } else { c"Gate OFF" }).as_ptr(),
        buf[..n].as_ptr(),
        buf[..n].len(),
    ) != ESP_OK {
        return false;
    }
    s.gate_assert_us = now_us();
    s.gate_assert_on = i32::from(enabled);
    true
}

#[no_mangle]
pub extern "C" fn nano_fx_write_gate_reduction(pct: u8) -> bool {
    let pct = pct.min(100);
    let s = fx();
    s.fx.gate.reduction_pct = pct;
    s.fx.gate.reduction_valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 20];
    let n = builders::gate_reduction_payload(pct, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }

    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
    if crate::ble::nano_ble_write_c304(
        c_label(&format!("Gate amount {pct}%")).as_ptr(),
        buf[..n].as_ptr(),
        buf[..n].len(),
    ) != ESP_OK {
        return false;
    }
    s.gate_amount_assert_us = now_us();
    s.gate_amount_assert_pct = pct as i32;
    // Amount writes must not let a stale poll flip bypass while the user is editing.
    if s.gate_assert_on < 0 {
        s.gate_assert_us = s.gate_amount_assert_us;
        s.gate_assert_on = i32::from(s.fx.gate.on);
    }
    true
}

/// Load model on device, enable block, queue param refresh.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_write_block_model(
    block: u32,
    model: *const NanoFxModelDef,
) -> bool {
    let Some(model) = model_ref(model) else {
        return false;
    };
    if block as usize >= NANO_FX_BLOCK_COUNT || !is_model_allowed(block, model) {
        return false;
    }

    if !crate::ble::nano_ble_connected() {
        return false;
    }

    let idx = block as usize;
    let mut buf = [0u8; 16];
    let n = builders::fx_model_select_payload(SLOT_CFG[idx].model_slot, model.id_bytes(), &mut buf);
    if n == 0 {
        return false;
    }

    let label = clip(
        &format!("{} model {}", BLOCK_LABELS[idx], model_name(model)),
        55,
    );
    info!(target: TAG, "{label}");
    if crate::ble::nano_ble_write_c304(
        c_label(&label).as_ptr(),
        buf[..n].as_ptr(),
        buf[..n].len(),
    ) != ESP_OK {
        return false;
    }

    let s = fx();
    let b = &mut s.fx.blocks[idx];
    store_catalog_model_id(b, model);
    apply_model(b, Some(model));
    b.valid = true;
    b.params_loaded = false;
    s.block_model_write_us[idx] = now_us();
    s.block_trusted_model[idx] = model as *const NanoFxModelDef;
    s.fx.valid = true;

    if !s.fx.blocks[idx].on {
        let mut en_buf = [0u8; 16];
        let en_n = builders::global_toggle_payload(SLOT_CFG[idx].enable_slot, 0x00, &mut en_buf);
        if en_n > 0 {
            s.fx.blocks[idx].on = true;
            s.block_on_asserted_us[idx] = now_us();
            let _ = crate::ble::nano_ble_write_c304(
                (c_label(&clip(&format!("{} ON", BLOCK_LABELS[idx]), 31))).as_ptr(),
                en_buf[..en_n].as_ptr(),
                en_buf[..en_n].len(),
            );
        }
    }

    enqueue_param_refresh(block);
    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };
    true
}

#[no_mangle]
pub extern "C" fn nano_fx_write_block_enabled(block: u32, enabled: bool) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let idx = block as usize;
    let s = fx();
    s.fx.blocks[idx].on = enabled;
    s.fx.blocks[idx].valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 16];
    let n = builders::global_toggle_payload(
        SLOT_CFG[idx].enable_slot,
        if enabled { 0x00 } else { 0x01 },
        &mut buf,
    );
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }

    let label = clip(
        &format!(
            "{} {}",
            BLOCK_LABELS[idx],
            if enabled { "ON" } else { "OFF" }
        ),
        31,
    );
    info!(target: TAG, "{label}");
    unsafe { crate::ui::screen::nano_ui_set_fx(&s.fx) };

    if crate::ble::nano_ble_write_c304(
        c_label(&label).as_ptr(),
        buf[..n].as_ptr(),
        buf[..n].len(),
    ) != ESP_OK {
        return false;
    }

    if enabled {
        s.block_on_asserted_us[idx] = now_us();
        enqueue_param_refresh(block);
    } else {
        s.block_on_asserted_us[idx] = 0;
        s.fx.blocks[idx].params_loaded = false;
    }
    true
}

#[no_mangle]
pub extern "C" fn nano_fx_write_param_normalized(
    block: u32,
    param_index: u8,
    normalized: f32,
) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let idx = block as usize;
    let s = fx();
    let Some(model) = model_ref(s.fx.blocks[idx].model) else {
        return false;
    };
    if param_index >= s.fx.blocks[idx].param_count {
        return false;
    }

    let name = c_str(unsafe { *model.param_names.add(param_index as usize) });
    let normalized = clampf(normalized, 0.0, 1.0);
    let param = &mut s.fx.blocks[idx].params[param_index as usize];
    param.normalized = normalized;
    param.display = normalized_to_display(name, normalized);
    param.valid = true;
    s.fx.valid = true;

    let mut buf = [0u8; 24];
    let n = builders::fx_param_payload(SLOT_CFG[idx].model_slot, param_index, normalized, &mut buf);
    if n == 0 || !crate::ble::nano_ble_connected() {
        return false;
    }

    let value = clip(&format_param(&s.fx.blocks[idx], param_index), 15);
    let label = clip(&format!("{} {name}={value}", BLOCK_LABELS[idx]), 47);
    crate::ble::nano_ble_write_c304(
        c_label(&label).as_ptr(),
        buf[..n].as_ptr(),
        buf[..n].len(),
    ) == ESP_OK
}

#[no_mangle]
pub extern "C" fn nano_fx_write_param_display(
    block: u32,
    param_index: u8,
    display_value: f32,
) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT {
        return false;
    }

    let s = fx();
    let b = &s.fx.blocks[block as usize];
    let Some(model) = model_ref(b.model) else {
        return false;
    };
    if param_index >= b.param_count {
        return false;
    }

    let name = c_str(unsafe { *model.param_names.add(param_index as usize) });
    let normalized = super::params::display_to_normalized(name, display_value);
    nano_fx_write_param_normalized(block, param_index, normalized)
}
