//! Public `nano_fx_*` C ABI entry points.

use core::ptr;

use esp_idf_svc::sys::{esp_timer_start_once, esp_timer_stop};
use log::{debug, info, warn};

use super::parse::{
    apply_gate_from_parsed, apply_model, decode_float32_array, dump_has_fx_bypass_field,
    dump_has_fx_model_id_fields, find_fx_param_response, infer_model_by_params, parse_preset,
    scan_gate_amount_field53, scan_gate_bypass_field54, set_generic_model, store_catalog_model_id,
    try_apply_catalog_model, upgrade_block_model,
};
use super::refresh::{
    block_model_write_recent, clear_model_write_trust, ensure_refresh_timers,
    schedule_amount_ui, trusted_model_for,
};
use super::state::{
    fx, fx_publish, now_us, push_fx_ui, FX_AMOUNT_REFRESH_GAP_US, FX_ENABLE_ASSERT_US,
    FX_REFRESH_GAP_US, FX_SEQ, TAG,
};
use super::write;
use crate::fx::models::*;
use crate::fx::params::{clampf, normalized_to_display};
use crate::fx::types::*;
use crate::fx::util::c_str;

pub fn nano_fx_reset() {
    let s = fx();
    s.fx = NanoFxState::zeroed();
    s.block_on_asserted_us = [0; NANO_FX_BLOCK_COUNT];
    s.block_model_write_us = [0; NANO_FX_BLOCK_COUNT];
    s.block_trusted_model = [ptr::null(); NANO_FX_BLOCK_COUNT];
    s.gate_assert_us = 0;
    s.gate_assert_on = -1;
    s.gate_amount_assert_us = 0;
    s.gate_amount_assert_pct = -1;
    s.last_amount_raw = -1;
    s.pedal_fx_focus = -1;
    s.amount_log_block = -1;
    s.amount_log_pending = false;
    s.live_param_us = 0;
    s.live_param_block = -1;
    s.amount_refresh = false;
    s.refresh_stale_until_us = 0;
    s.reassert_focus_after_sync = false;
    s.suppress_fx_param_ui = false;
    s.refresh_q_len = 0;
    s.refresh_q_head = 0;
    s.refresh_active = false;
    for timer in [
        s.refresh_timeout_timer,
        s.refresh_gap_timer,
        s.begin_refresh_timer,
        s.amount_ui_timer,
    ] {
        if !timer.is_null() {
            unsafe { esp_timer_stop(timer) };
        }
    }
    fx_publish();
}

pub fn nano_fx_get() -> *const NanoFxState {
    let s = fx();
    if s.fx.valid {
        &s.fx as *const NanoFxState
    } else {
        ptr::null()
    }
}

/// Consistent copy for the UI task (seqlock). Returns false when FX is not valid.
pub fn nano_fx_snapshot(out: &mut NanoFxState) -> bool {
    FX_SEQ.read(|| {
        let s = fx();
        if s.fx.valid {
            *out = s.fx;
            true
        } else {
            false
        }
    })
}

/// Parse gate + FX block models/bypass from preset state blob.
pub fn nano_fx_parse_preset(data: &[u8], out: &mut NanoFxState) -> bool {
    if data.len() > crate::memsafe::BLOB_MAX {
        return false;
    }
    parse_preset(data, out)
}

/// True when the dump carries a gate amount (f53 fixed32 in [0,1]).
///
/// Amount alone is enough for UI: omitted f54 means ON (proto3 default). Bypass
/// alone still leaves GATE AMNT as "-".
pub fn nano_fx_dump_has_gate_fields(data: &[u8]) -> bool {
    if data.len() > crate::memsafe::BLOB_MAX {
        return false;
    }
    scan_gate_amount_field53(data).is_some()
}

/// True when live FX gate state can fill INPUT GATE + GATE AMNT (not "-").
pub fn nano_fx_gate_ui_ready() -> bool {
    let g = &fx().fx.gate;
    g.valid && g.on_known && g.reduction_valid
}

/// True when dump includes modern FX model-id fields (48–52).
pub fn nano_fx_dump_has_model_fields(data: &[u8]) -> bool {
    if data.len() > crate::memsafe::BLOB_MAX {
        return false;
    }
    dump_has_fx_model_id_fields(data)
}

/// Salvage gate ON/OFF + amount from any blob that contains f53/f54 tags
/// (full preset dump or a late high-bit C305 tail after a mid-dump gap).
pub fn nano_fx_try_apply_gate_bytes(data: &[u8]) -> bool {
    if data.len() > crate::memsafe::BLOB_MAX {
        return false;
    }
    if data.len() < 6 {
        return false;
    }
    if scan_gate_amount_field53(data).is_none() && scan_gate_bypass_field54(data).is_none() {
        return false;
    }

    let mut parsed = NanoFxState::reset_with_ids();
    if !parse_preset(data, &mut parsed) || !parsed.gate.valid {
        return false;
    }

    apply_gate_from_parsed(&parsed);
    push_fx_ui();
    true
}

/// After preset parse, queue param refresh for active blocks.
pub fn nano_fx_begin_param_refresh() {
    ensure_refresh_timers();
    let s = fx();
    unsafe {
        esp_timer_stop(s.begin_refresh_timer);
        esp_timer_start_once(s.begin_refresh_timer, 1);
    }
}

/// Forget UI/BLE model write asserts so the next preset dump can replace slots.
pub fn nano_fx_clear_model_trust() {
    clear_model_write_trust();
}

/// Parse preset blob into internal FX state and start param refresh.
pub fn nano_fx_sync_from_preset(data: &[u8]) {
    if data.len() > crate::memsafe::BLOB_MAX {
        return;
    }
    sync_from_preset(data);
}

fn sync_from_preset(data: &[u8]) {
    let mut parsed = NanoFxState::reset_with_ids();

    if !parse_preset(data, &mut parsed) {
        let s = fx();
        if s.suppress_fx_param_ui {
            s.suppress_fx_param_ui = false;
            push_fx_ui();
        }
        return;
    }

    if parsed.gate.valid {
        apply_gate_from_parsed(&parsed);
    }

    let dump_has_models = dump_has_fx_model_id_fields(data);
    let dump_has_bypass = dump_has_fx_bypass_field(data);
    let s = fx();
    let preset_switch = s.reassert_focus_after_sync;
    for i in 0..NANO_FX_BLOCK_COUNT {
        let src = &parsed.blocks[i];
        let dst = &mut s.fx.blocks[i];

        // Only apply on/off when field 31 was present. Model-only parses leave
        // on=false by default and must not blank a known live enable state.
        if dump_has_bypass && src.valid {
            if src.on {
                dst.on = true;
                s.block_on_asserted_us[i] = 0;
            } else if s.block_on_asserted_us[i] != 0
                && now_us() - s.block_on_asserted_us[i] < FX_ENABLE_ASSERT_US
            {
                // Keep a just-enabled block against a stale dump echo.
            } else {
                dst.on = false;
                s.block_on_asserted_us[i] = 0;
            }
            dst.valid = true;
        } else if src.valid {
            dst.valid = true;
        }

        if src.model_id_len == 0 {
            // Full dump with model-id fields but nothing for this slot: clear
            // sticky boot/UI leftovers. Truncated dumps keep prior state.
            if dump_has_models && !block_model_write_recent(i as u32) {
                s.block_trusted_model[i] = ptr::null();
                s.block_model_write_us[i] = 0;
                dst.model = ptr::null();
                dst.model_id_len = 0;
                dst.has_model_id = false;
                dst.param_count = 0;
                dst.params_loaded = false;
            }
            continue;
        }

        // Only shield local UI/BLE model writes during the assert window.
        // Unbounded `block_trusted_model` / `params_loaded` made the first FX
        // slot (and any UI-changed model) stick across every preset.
        if block_model_write_recent(i as u32) {
            continue;
        }
        s.block_trusted_model[i] = ptr::null();
        s.block_model_write_us[i] = 0;

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
        } else if preset_switch {
            // Same model, new preset values — force reload; UI holds prior knobs
            // and shows "…" until the refresh reply lands.
            dst.params_loaded = false;
            for p in &mut dst.params {
                p.valid = false;
            }
        }
    }

    s.fx.valid = true;
    push_fx_ui();
    nano_fx_begin_param_refresh();

    // Preset footswitch: Nano may keep or change Amount focus — re-assert ours.
    if s.reassert_focus_after_sync {
        s.reassert_focus_after_sync = false;
        if s.pedal_fx_focus >= 0 {
            let slot = s.pedal_fx_focus as u8;
            let _ = write::write_fx_focus(slot);
        }
    }
}

/// Footswitch / preset ACK: next sync should realign Nano FX focus with the UI.
pub fn nano_fx_note_preset_change() {
    let s = fx();
    s.reassert_focus_after_sync = true;
    s.suppress_fx_param_ui = true;
    s.last_amount_raw = -1;
    s.amount_log_pending = false;
    s.live_param_block = -1;
    s.live_param_us = 0;
}

/// True while FX param sliders are held until the visible block's refresh lands.
pub fn nano_fx_params_ui_held() -> bool {
    fx().suppress_fx_param_ui
}

/// Release the param-UI hold once the on-screen block (or the whole queue) is ready.
pub(super) fn maybe_release_param_ui_hold() {
    let s = fx();
    if !s.suppress_fx_param_ui {
        return;
    }
    let sel = if s.pedal_fx_focus >= 0 {
        s.pedal_fx_focus as usize
    } else {
        0
    };
    let block = &s.fx.blocks[sel.min(NANO_FX_BLOCK_COUNT - 1)];
    let selected_ready = !block.on || block.params_loaded || block.model.is_null();
    let queue_idle = !s.refresh_active && s.refresh_q_head >= s.refresh_q_len;
    if selected_ready || queue_idle {
        s.suppress_fx_param_ui = false;
        push_fx_ui();
    }
}

/// Nano FX-button selected a slot (model slot / block id 0..=4).
pub fn nano_fx_on_pedal_fx_select(model_slot: u8) {
    if model_slot as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }
    let s = fx();
    let changed = s.pedal_fx_focus != model_slot as i8;
    s.pedal_fx_focus = model_slot as i8;
    if changed {
        info!(
            target: TAG,
            "FX focus -> {}", BLOCK_LABELS[model_slot as usize]
        );
    }
    // Keep the on-screen block aligned with the Nano-selected FX.
    crate::ui::screen::nano_ui_select_fx_block(model_slot);
}

/// UI chain tab selected an FX block — mirror focus onto the Nano Amount target.
pub fn nano_fx_focus_from_ui(model_slot: u8) {
    if model_slot as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }
    let s = fx();
    let changed = s.pedal_fx_focus != model_slot as i8;
    s.pedal_fx_focus = model_slot as i8;
    if !changed {
        return;
    }
    info!(
        target: TAG,
        "FX focus (UI) -> {}", BLOCK_LABELS[model_slot as usize]
    );
    let _ = write::write_fx_focus(model_slot);
}

fn apply_block_param(block: &mut NanoFxBlock, idx: usize, normalized: f32) -> bool {
    let n = clampf(normalized, 0.0, 1.0);
    if block.params[idx].valid && (block.params[idx].normalized - n).abs() < 0.0005 {
        return false;
    }
    block.params[idx].normalized = n;
    let name = model_ref(block.model)
        .map(|model| c_str(unsafe { *model.param_names.add(idx) }))
        .unwrap_or("");
    block.params[idx].display = if name.is_empty() {
        n * 100.0
    } else {
        normalized_to_display(name, n)
    };
    block.params[idx].valid = true;
    true
}

/// Live FX param write/echo from the device (includes Amount-driven changes).
///
/// Applies the exact slot/index/value — no local Amount→param guessing.
/// When the param layout is unknown, falls back to a single-block poll.
pub fn nano_fx_on_live_param(model_slot: u8, param_index: u8, normalized: f32) {
    if model_slot as usize >= NANO_FX_BLOCK_COUNT {
        return;
    }
    let s = fx();
    let focus_changed = s.pedal_fx_focus != model_slot as i8;
    s.pedal_fx_focus = model_slot as i8;
    s.live_param_block = model_slot as i8;
    s.live_param_us = now_us();
    if focus_changed {
        crate::ui::screen::nano_ui_select_fx_block(model_slot);
    }

    let idx = param_index as usize;
    if idx >= NANO_FX_MAX_PARAMS {
        return;
    }

    let block = &mut s.fx.blocks[model_slot as usize];
    // Apply whenever the model layout is known. Do not require params_loaded —
    // Amount polls clear that flag while a refresh is in flight.
    if block.param_count == 0 || idx >= block.param_count as usize {
        s.amount_log_block = model_slot as i8;
        s.amount_log_pending = true;
        schedule_amount_ui();
        return;
    }

    if apply_block_param(block, idx, normalized) {
        s.fx.valid = true;
        push_fx_ui();
    }
}

/// Pedal AMOUNT knob moved (raw 0..255).
///
/// Hot path: record focus + raw only. After debounce, poll that block (or skip
/// if a live param echo already updated the UI). Never guess Amount→param.
pub fn nano_fx_on_amount_knob(raw: u8, model_slot: Option<u8>) {
    let s = fx();
    if let Some(slot) = model_slot {
        if (slot as usize) < NANO_FX_BLOCK_COUNT {
            let focus_changed = s.pedal_fx_focus != slot as i8;
            s.pedal_fx_focus = slot as i8;
            if focus_changed {
                crate::ui::screen::nano_ui_select_fx_block(slot);
            }
        }
    }

    if s.pedal_fx_focus < 0 {
        s.last_amount_raw = raw as i16;
        return;
    }

    if s.last_amount_raw == raw as i16 {
        return;
    }
    s.last_amount_raw = raw as i16;
    s.amount_log_block = s.pedal_fx_focus;
    s.amount_log_pending = true;
    schedule_amount_ui();
}

/// Feed C305; returns true if consumed (FX param refresh response).
pub fn nano_fx_feed_c305(data: &[u8]) -> bool {
    if data.len() > crate::memsafe::PKT_MAX {
        return false;
    }
    if data.len() < 10 {
        return false;
    }
    feed_c305(data)
}

fn feed_c305(data: &[u8]) -> bool {
    let len = data.len();
    let hdr = find_fx_param_response(data);
    if hdr >= len || hdr + 6 > len {
        return false;
    }

    let s = fx();
    let now = now_us();
    if s.refresh_stale_until_us != 0 && now < s.refresh_stale_until_us {
        debug!(target: TAG, "Dropping stale FX param response (post-timeout guard)");
        return true;
    }
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
        s.amount_refresh = false;
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
    let trusted = trusted_model_for(pending);
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
        // Already resolved to a real catalog model from the preset dump —
        // param-count guesses used to overwrite the chain on boot.
        if model_ref(block.model).is_some_and(|m| m.id_len > 0) {
            inferred = None;
        } else if model_ref(block.model).is_some_and(|m| m.param_count as usize == float_count) {
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

    let was_amount = s.amount_refresh;
    s.amount_refresh = false;
    s.refresh_stale_until_us = 0;

    block.params_loaded = true;
    info!(target: TAG, "{label} params loaded ({float_count} values)");

    if block.on && !block.has_model_id {
        crate::state::nano_state_request_preset_refresh("FX model sync");
    }

    s.refresh_active = false;
    let gap = if was_amount || s.suppress_fx_param_ui {
        FX_AMOUNT_REFRESH_GAP_US
    } else {
        FX_REFRESH_GAP_US
    };
    unsafe {
        esp_timer_stop(s.refresh_timeout_timer);
        esp_timer_start_once(s.refresh_gap_timer, gap);
    }
    if s.suppress_fx_param_ui {
        maybe_release_param_ui_hold();
    } else {
        push_fx_ui();
    }
    true
}

/* ---------------------------------------------------------------- writes */

pub fn nano_fx_write_gate_enabled(enabled: bool) -> bool {
    write::write_gate_enabled(enabled)
}

pub fn nano_fx_write_gate_reduction(pct: u8) -> bool {
    write::write_gate_reduction(pct)
}

/// Load model on device, enable block, queue param refresh.
pub fn nano_fx_write_block_model(block: u32, model: &NanoFxModelDef) -> bool {
    write::write_block_model(block, model)
}

pub fn nano_fx_write_block_enabled(block: u32, enabled: bool) -> bool {
    write::write_block_enabled(block, enabled)
}

pub fn nano_fx_write_param_normalized(block: u32, param_index: u8, normalized: f32) -> bool {
    write::write_param_normalized(block, param_index, normalized)
}

pub fn nano_fx_write_param_display(block: u32, param_index: u8, display_value: f32) -> bool {
    write::write_param_display(block, param_index, display_value)
}
