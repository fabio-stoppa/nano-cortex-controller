//! What the preset, effect, and tuner code paints.
//!
//! The names match the old screen entry points. The widgets live in `ui`.

extern crate alloc;

use alloc::string::String;

use nano_wire::types::NanoAmpC;

use crate::fx::types::NanoFxState;
use crate::tuner::types::NanoTunerC;

const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn nano_ui_set_amp(amp: Option<&NanoAmpC>) {
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
    if amp.have_capture_name {
        let end = amp
            .capture_name
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(amp.capture_name.len());
        let name = String::from_utf8_lossy(&amp.capture_name[..end]);
        crate::ui::show_model(name.trim());
    }
}

pub fn nano_ui_set_ble_connected(connected: bool) {
    crate::ui::set_connected(connected);
}

pub fn nano_ui_set_tuner(tuner: &NanoTunerC) {
    let (note, cents) = if !tuner.active {
        ("-", String::from("Tuner off"))
    } else if tuner.has_pitch && (0..12).contains(&tuner.note) {
        (
            NOTES[tuner.note as usize],
            alloc::format!("{:+.0} ct", tuner.cents),
        )
    } else {
        ("...", String::from("Listening..."))
    };
    crate::ui::show_tuner(note, &cents, tuner.active, tuner.has_pitch, tuner.cents as f32);
}

pub fn nano_ui_set_preset_dirty(dirty: bool) {
    crate::ui::show_save_dirty(dirty);
}

pub fn nano_ui_set_preset_name(name: &str) {
    crate::ui::show_preset(name);
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
