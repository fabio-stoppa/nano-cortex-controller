//! Protobuf field extraction, model apply/guess logic and preset parsing.

use core::ptr;

use log::{info, warn};

use nano_wire::varint::read_varint;

use super::state::{fx, now_us, SLOT_CFG, TAG};
use crate::fx::catalog::catalog;
use crate::fx::models::*;
use crate::fx::params::{clampf, normalized_to_display};
use crate::fx::types::*;
use crate::fx::util::c_str;

/* -------------------------------------------------------------- protobuf */

/// Advance past one field body; `None` when the blob is truncated.
pub(super) fn skip_wire(data: &[u8], i: usize, wire: u32) -> Option<usize> {
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

pub(super) fn le_f32(bytes: &[u8]) -> f32 {
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
pub(super) fn scan_gate_amount_field53(data: &[u8]) -> Option<f32> {
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
pub(super) fn scan_gate_bypass_field54(data: &[u8]) -> Option<u32> {
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

pub(super) fn store_catalog_model_id(block: &mut NanoFxBlock, model: &NanoFxModelDef) {
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

pub(super) fn apply_model(block: &mut NanoFxBlock, model: Option<&'static NanoFxModelDef>) {
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
pub(super) fn set_generic_model(block: &mut NanoFxBlock, count: u8) {
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
        s.generic_param_ptrs[i] = dst.as_ptr().cast::<core::ffi::c_char>();
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

pub(super) fn try_apply_catalog_model(block: &mut NanoFxBlock) {
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

pub(super) fn upgrade_block_model(
    block: &mut NanoFxBlock,
    model: &'static NanoFxModelDef,
    floats: &[f32],
) {
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

pub(super) fn infer_model_by_params(block: u32, param_count: u8) -> Option<&'static NanoFxModelDef> {
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

    let alt = super::state::MODEL_FIELD_LEGACY_ALT[block_idx];
    let fields: &[u32] = if alt != 0 {
        &[super::state::MODEL_FIELD_LEGACY[block_idx], alt]
    } else {
        &[super::state::MODEL_FIELD_LEGACY[block_idx]]
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

/* ------------------------------------------------------------------ gate */

pub(super) fn apply_gate_from_parsed(parsed: &NanoFxState) {
    if !parsed.gate.valid {
        return;
    }
    let s = fx();

    if parsed.gate.on_known {
        let remote_on = parsed.gate.on;
        let mut accept_on = true;
        if s.gate_assert_on >= 0 && s.gate_assert_us != 0 {
            if now_us() - s.gate_assert_us <= super::state::FX_ENABLE_ASSERT_US {
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
            if now_us() - s.gate_amount_assert_us <= super::state::FX_ENABLE_ASSERT_US {
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

pub(super) fn decode_float32_array(data: &[u8], out: &mut [f32]) -> usize {
    let mut count = 0;
    let mut i = 0usize;
    while i + 3 < data.len() && count < out.len() {
        out[count] = le_f32(&data[i..i + 4]);
        count += 1;
        i += 4;
    }
    count
}

pub(super) fn find_fx_param_response(data: &[u8]) -> usize {
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

/* -------------------------------------------------------------- preset */

pub(super) fn parse_preset(data: &[u8], out: &mut NanoFxState) -> bool {
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
