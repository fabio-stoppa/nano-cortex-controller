//! What the preset, effect, and tuner code paints.
//!
//! The names match the old screen entry points. The widgets live in `ui`.

extern crate alloc;

use alloc::string::String;

use nano_wire::editor::ExpressionAssignments;
use nano_wire::types::NanoAmpC;
use nano_wire::types::{CabSettings, DeviceSettings, Metering};

use crate::fx::types::NanoFxState;
use crate::tuner::types::NanoTunerC;

const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn nano_ui_set_amp(
    amp: Option<&NanoAmpC>,
    capture_category: Option<nano_wire::types::LibraryCategory>,
) {
    let Some(amp) = amp else {
        return;
    };
    if !amp.valid {
        return;
    }
    crate::ui::show_knob_from_radio(0, amp.gain);
    crate::ui::show_knob_from_radio(1, amp.level);
    crate::ui::show_knob_from_radio(2, amp.bass);
    crate::ui::show_knob_from_radio(3, amp.mid);
    crate::ui::show_knob_from_radio(4, amp.treble);
    if amp.have_capture_volume {
        crate::ui::show_knob_from_radio(5, amp.capture_volume);
    }

    let capture_name = crate::amp::nano_amp_capture_header_text(amp);
    let capture_active = amp.have_capture_slot && amp.capture_slot > 0;
    if capture_active {
        crate::ui::show_model(capture_name.trim());
    }
    let capture_kind = capture_category
        .map(|category| match category {
            nano_wire::types::LibraryCategory::Amp => "AMP HEAD",
            nano_wire::types::LibraryCategory::AmpCab => "AMP + CAB",
            nano_wire::types::LibraryCategory::Cab => "CAB",
            nano_wire::types::LibraryCategory::Pedal => "PEDAL",
            nano_wire::types::LibraryCategory::Other => "CAPTURE",
        })
        .unwrap_or("CAPTURE");
    let capture_title = alloc::format!("CAPTURE · {capture_kind}");
    let capture_value = if capture_active {
        capture_name.trim()
    } else if amp.have_capture_slot {
        "Bypassed"
    } else {
        "-"
    };
    let capture_slot = if capture_active {
        crate::amp::nano_amp_format_bank_line(amp.capture_slot)
    } else if amp.have_capture_slot {
        String::from("BYPASS")
    } else {
        String::from("-")
    };

    let active_cab_slot = amp.active_cab_slot();
    let cab_active = active_cab_slot.is_some();
    let cab_value = if cab_active && amp.have_cab_name {
        let end = amp
            .cab_name
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(amp.cab_name.len());
        String::from(String::from_utf8_lossy(&amp.cab_name[..end]).trim())
    } else if cab_active {
        String::from("-")
    } else if amp.have_cab_slot {
        String::from("Bypassed")
    } else {
        String::from("-")
    };
    let cab_slot = if let Some(slot) = active_cab_slot {
        alloc::format!("Slot {slot}")
    } else if amp.have_cab_slot {
        String::from("BYPASS")
    } else {
        String::from("-")
    };
    crate::ui::show_amp_sources(
        &capture_title,
        capture_value,
        &capture_slot,
        capture_active,
        &cab_value,
        &cab_slot,
        cab_active,
    );
}

pub fn nano_ui_library_updated() {
    crate::ui::library_updated();
}

pub fn nano_ui_set_ble_connected(connected: bool) {
    crate::ui::set_connected(connected);
}

pub fn nano_ui_set_tuner(tuner: &NanoTunerC) {
    let cents_f = crate::tuner::nano_tuner_cents_f();
    let (note, cents) = if !tuner.active {
        ("-", String::from("Tuner off"))
    } else if tuner.has_pitch && (0..12).contains(&tuner.note) {
        (
            NOTES[tuner.note as usize],
            alloc::format!("{:+.1} ct", cents_f),
        )
    } else {
        ("", String::new())
    };
    crate::ui::show_tuner(note, &cents, tuner.active, tuner.has_pitch, cents_f);
}

pub fn nano_ui_set_tuner_settings(reference_hz: f32, muted: bool) {
    crate::ui::show_tuner_settings(reference_hz, muted);
}

pub fn nano_ui_set_preset_dirty(dirty: bool) {
    crate::ui::show_save_dirty(dirty);
}

pub fn nano_ui_set_preset_name(name: &str) {
    crate::ui::show_preset(name);
}

pub fn nano_ui_refresh_preset_catalog() {
    crate::ui::refresh_preset_catalog();
}

pub fn nano_ui_set_device_settings(settings: &DeviceSettings) {
    crate::ui::show_device_settings(settings);
}

pub fn nano_ui_set_cab_settings(slot: u8, settings: &CabSettings) {
    crate::ui::show_cab_settings(slot, settings);
}

pub fn nano_ui_set_expression_assignments(slot: u8, assignments: &ExpressionAssignments) {
    crate::ui::show_expression_assignments(slot, assignments);
}

pub fn nano_ui_set_metering(meter: Metering) {
    crate::ui::show_metering(meter);
}

pub fn nano_ui_clear_tool_meters() {
    crate::ui::clear_tool_meters();
}

pub fn nano_ui_set_calibration(active: bool, min: Option<u8>, max: Option<u8>) {
    crate::ui::show_calibration(active, min, max);
}

pub fn nano_ui_set_protocol_status(status: &str) {
    crate::ui::show_protocol_status(status);
}

pub fn nano_ui_set_bpm(bpm: u16, valid: bool) {
    if valid {
        crate::ui::show_bpm(bpm);
    } else {
        crate::ui::clear_bpm();
    }
}

pub fn nano_ui_bpm_tap(bpm: u16) {
    crate::ui::show_bpm(bpm);
}

pub fn nano_ui_set_tempo_mode(active: bool) {
    crate::ui::show_tempo_page(active);
}

pub fn nano_ui_set_fx(fx: &NanoFxState) {
    if fx.gate.on_known {
        crate::ui::show_gate_from_radio(fx.gate.on);
    }
    if fx.gate.reduction_valid {
        crate::ui::show_gate_amount_from_radio(fx.gate.reduction_pct as f32 / 100.0);
    }
    crate::ui::show_fx(fx);
}

pub fn nano_ui_select_fx_block(block: u8) {
    crate::ui::show_chain(chain_for_block(block));
}

fn chain_for_block(block: u8) -> i32 {
    match block {
        0 => 0,
        1 => 1,
        2 => 3,
        3 => 4,
        _ => 5,
    }
}
