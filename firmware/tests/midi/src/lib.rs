#![allow(dead_code)]

extern crate alloc;

#[path = "../../../src/fx/catalog.rs"]
pub mod catalog;
#[path = "../../../src/usb_storage/disk.rs"]
mod disk;
#[path = "../../../src/hci.rs"]
mod hci;
#[path = "../../../src/settings/json_record.rs"]
mod json_record;
#[path = "../../../src/link.rs"]
mod link;
#[path = "../../../src/midi.rs"]
mod midi;
#[path = "../../../src/settings/midi_record.rs"]
mod midi_record;
#[path = "../../../src/fx/models.rs"]
pub mod models;
#[path = "../../../src/fx/params.rs"]
pub mod params;
#[path = "../../../src/performance.rs"]
mod performance;
#[path = "../../../src/syncutil.rs"]
mod syncutil;
#[path = "../../../src/fx/types.rs"]
pub mod types;
#[path = "../../../src/fx/util.rs"]
pub mod util;

mod fx {
    pub use crate::models;
    pub use crate::params;
    pub use crate::types;
    pub use models::nano_fx_param_wire_index;

    std::thread_local! {
        pub static SNAPSHOT: core::cell::RefCell<types::NanoFxState> =
            const { core::cell::RefCell::new(types::NanoFxState::zeroed()) };
    }

    pub fn nano_fx_snapshot(out: &mut types::NanoFxState) -> bool {
        SNAPSHOT.with(|s| {
            *out = *s.borrow();
            out.valid
        })
    }
}

mod clock {
    use core::sync::atomic::{AtomicI64, Ordering};
    pub static NOW: AtomicI64 = AtomicI64::new(0);
    pub fn now_us() -> i64 {
        NOW.load(Ordering::Relaxed)
    }
}

mod tuner {
    pub fn nano_tuner_is_active() -> bool {
        false
    }
}

mod state {
    use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
    use nano_wire::types::{Amp, CabSettings, NanoAmpC};
    pub static PRESET: AtomicU8 = AtomicU8::new(1);
    pub static GENERATION: AtomicU32 = AtomicU32::new(1);
    pub static ACK: AtomicU32 = AtomicU32::new(0);
    std::thread_local! {
        pub static AMP: core::cell::RefCell<NanoAmpC> = core::cell::RefCell::new(NanoAmpC::from(&Amp::default()));
    }
    pub fn nano_state_snapshot_amp(out: &mut NanoAmpC) -> bool {
        AMP.with(|amp| {
            *out = *amp.borrow();
            out.valid
        })
    }
    pub fn nano_state_cab_settings_snapshot() -> (u8, CabSettings) {
        (0, CabSettings::default())
    }
    pub fn snapshot_generation() -> u32 {
        GENERATION.load(Ordering::Relaxed)
    }
    pub fn cab_generation() -> u32 {
        0
    }
    pub fn preset_ack_generation() -> u32 {
        ACK.load(Ordering::Relaxed)
    }
    pub fn nano_state_preset_slot() -> u8 {
        PRESET.load(Ordering::Relaxed)
    }
    pub fn nano_rx_mode_get() -> u32 {
        0
    }
}

mod settings {
    use super::midi_record;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    pub static FAIL: AtomicBool = AtomicBool::new(false);
    pub static SAVES: AtomicUsize = AtomicUsize::new(0);
    pub static RECORD: Mutex<Option<String>> = Mutex::new(None);
    pub static PERFORMANCE: Mutex<Option<String>> = Mutex::new(None);

    pub fn load_performance_json() -> Result<Option<String>, &'static str> {
        Ok(PERFORMANCE.lock().unwrap().clone())
    }
    pub fn save_performance_json(json: &str) -> Result<(), &'static str> {
        if FAIL.load(Ordering::Relaxed) {
            return Err("Mock flash save failed");
        }
        *PERFORMANCE.lock().unwrap() = Some(json.to_owned());
        Ok(())
    }

    pub fn load_midi(count: usize) -> Option<[u16; midi_record::MAX]> {
        let json = RECORD.lock().unwrap();
        let keys = crate::midi::keys_from_json(json.as_ref()?).ok()?;
        let mut result = [0; midi_record::MAX];
        for (out, key) in result.iter_mut().take(count).zip(keys) {
            *out = key;
        }
        Some(result)
    }

    pub fn save_midi(keys: &[u16]) -> bool {
        crate::midi::json_for_keys(keys)
            .and_then(|json| save_midi_json(&json))
            .is_ok()
    }

    pub fn load_midi_json() -> Result<Option<String>, &'static str> {
        Ok(RECORD.lock().unwrap().clone())
    }

    pub fn save_midi_json(json: &str) -> Result<(), &'static str> {
        SAVES.fetch_add(1, Ordering::Relaxed);
        if FAIL.load(Ordering::Relaxed) {
            return Err("Mock flash save failed");
        }
        *RECORD.lock().unwrap() = Some(json.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn reset() {
        link::set_up(false);
        link::set_up(true);
        clock::NOW.store(10_000_000, Ordering::Relaxed);
        settings::FAIL.store(false, Ordering::Relaxed);
        settings::SAVES.store(0, Ordering::Relaxed);
        for i in 0..midi::ACTION_COUNT {
            midi::clear(i);
        }
        midi::persist();
        settings::SAVES.store(0, Ordering::Relaxed);
        fx::SNAPSHOT.with(|s| *s.borrow_mut() = fx::types::NanoFxState::zeroed());
    }

    fn index(name: &str) -> usize {
        (0..midi::ACTION_COUNT)
            .find(|&i| midi::action_name(i) == name)
            .unwrap()
    }

    fn cc(number: u8, value: u8) {
        midi::feed(&[0x80, 0x80, 0xb0, number, value]);
    }

    fn learn_cc(action: usize, number: u8) {
        midi::toggle_learn(action);
        cc(number, 127);
        assert!(
            link::pop_cmd().is_none(),
            "Learning must not execute existing targets"
        );
    }

    fn queued() -> Vec<(usize, u8)> {
        let mut result = vec![];
        while let Some(cmd) = link::pop_cmd() {
            if let link::Cmd::MidiAction { index, value, .. } = cmd {
                result.push((index, value));
            } else {
                panic!("Expected MIDI target");
            }
        }
        result
    }

    #[test]
    fn custom_banks_edit_persist_recall_and_refuse_failed_saves() {
        let _guard = LOCK.lock().unwrap();
        reset();
        *settings::PERFORMANCE.lock().unwrap() = None;
        performance::load();
        assert!(performance::recall_slot(0).is_err());
        performance::add_bank("Set 1").unwrap();
        performance::assign_slot(0, Some(64)).unwrap();
        performance::add_bank("Set 2").unwrap();
        performance::assign_slot(1, Some(3)).unwrap();
        performance::step_bank(1).unwrap();
        assert_eq!(performance::bank(), 0);
        assert_eq!(performance::recall_slot(0).unwrap(), 64);
        performance::step_bank(-1).unwrap();
        assert_eq!(performance::recall_slot(1).unwrap(), 3);
        performance::load();
        assert_eq!(performance::banks(), ["Set 1", "Set 2"]);
        assert!(performance::rename_bank(1, "set 1").is_err());
        let before = performance::export_json().unwrap();
        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(performance::delete_bank(0).is_err());
        assert_eq!(performance::export_json().unwrap(), before);
        settings::FAIL.store(false, Ordering::Relaxed);
        performance::delete_bank(0).unwrap();
        assert_eq!(performance::banks(), ["Set 2"]);
        link::set_up(false);
        assert!(performance::recall_slot(1).is_err());
        assert!(matches!(
            midi::command(index("Next bank"), 127, false),
            Some(link::Cmd::BankStep(1))
        ));
        assert!(matches!(
            midi::command(index("Recall bank slot 4"), 127, false),
            Some(link::Cmd::BankSlot(3))
        ));
    }

    #[test]
    fn scenes_persist_failed_saves_preserve_data_and_bank_ack_is_required() {
        let _guard = LOCK.lock().unwrap();
        reset();
        *settings::PERFORMANCE.lock().unwrap() = None;
        performance::load();
        performance::add_bank("Live").unwrap();
        performance::assign_slot(0, Some(1)).unwrap();
        state::AMP.with(|amp| amp.borrow_mut().valid = true);
        fx::SNAPSHOT.with(|snapshot| {
            let mut view = snapshot.borrow_mut();
            *view = types::NanoFxState::reset_with_ids();
            view.valid = true;
            view.gate.on_known = true;
            for block in &mut view.blocks {
                block.valid = true;
            }
        });
        state::PRESET.store(1, Ordering::Relaxed);
        state::ACK.store(0, Ordering::Relaxed);
        performance::capture_scene(0, 0, 0, "Clean").unwrap();
        assert_eq!(performance::scenes(0)[0].as_deref(), Some("Clean"));
        let before = performance::export_json().unwrap();
        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(performance::capture_scene(0, 0, 0, "Lead").is_err());
        assert_eq!(performance::export_json().unwrap(), before);
        settings::FAIL.store(false, Ordering::Relaxed);
        performance::start_bank(0).unwrap();
        assert!(
            performance::poll().is_none(),
            "Optimistic preset selection is not acknowledgment"
        );
        assert!(performance::busy());
        assert!(performance::select_bank(0).is_err());
        state::ACK.fetch_add(1, Ordering::Relaxed);
        assert!(matches!(
            performance::poll(),
            Some(link::Cmd::PerformanceRefresh)
        ));
        assert!(performance::poll().is_none());
        assert!(performance::busy());
        state::GENERATION.fetch_add(1, Ordering::Relaxed);
        performance::poll();
        assert!(!performance::busy());
        performance::recall_scene(0).unwrap();
        assert!(performance::busy());
        performance::cancel("Test interruption");
        assert!(performance::scene().is_none());
        performance::assign_slot(0, Some(2)).unwrap();
        assert!(performance::scenes(0).iter().all(Option::is_none));
    }

    #[test]
    fn shared_learn_clear_and_relearn_preserve_other_targets() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        learn_cc(1, 7);
        cc(7, 127);
        assert_eq!(queued(), [(0, 127), (1, 127)]);
        midi::clear(0);
        cc(7, 127);
        assert_eq!(queued(), [(1, 127)]);
        learn_cc(0, 8);
        cc(7, 127);
        cc(8, 127);
        assert_eq!(queued(), [(1, 127), (0, 127)]);
    }

    #[test]
    fn all_targets_survive_and_continuous_values_coalesce_by_action() {
        let _guard = LOCK.lock().unwrap();
        reset();
        for i in 0..midi::ACTION_COUNT {
            learn_cc(i, 7);
        }
        cc(7, 127);
        assert_eq!(queued().len(), midi::ACTION_COUNT);
        let json = midi::export_json().unwrap();
        assert!(json.len() <= disk::MAX_JSON);
        midi::import_json(&json).unwrap();
        assert_eq!(midi::export_json().unwrap(), json);
        reset();
        let a = index("Amp gain");
        let b = index("Post FX 2 knob 24");
        learn_cc(a, 7);
        learn_cc(b, 7);
        cc(7, 10);
        cc(7, 70);
        assert_eq!(queued(), [(a, 70), (b, 70)]);
    }

    #[test]
    fn zero_cc_releases_buttons_but_updates_continuous_targets() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let parameter = index("Amp gain");
        learn_cc(0, 7);
        learn_cc(parameter, 7);
        cc(7, 0);
        assert_eq!(queued(), [(parameter, 0)]);
        cc(7, 127);
        cc(7, 0);
        assert_eq!(queued(), [(0, 127), (parameter, 0)]);
    }

    #[test]
    fn parameter_learn_requires_cc_and_note_off_never_triggers() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let parameter = index("Amp gain");
        midi::toggle_learn(parameter);
        midi::feed(&[0x80, 0x80, 0x90, 60, 100]);
        assert!(midi::rows()[parameter].learning);
        assert!(midi::rows()[parameter].assigned.is_empty());
        assert!(midi::last_text().contains("CC"));
        cc(7, 127);
        assert!(!midi::rows()[parameter].learning);
        midi::toggle_learn(0);
        midi::feed(&[0x80, 0x80, 0x90, 60, 100]);
        midi::feed(&[0x80, 0x80, 0x80, 60, 0]);
        midi::feed(&[0x80, 0x80, 0x90, 60, 0]);
        assert!(queued().is_empty());
        midi::feed(&[0x80, 0x80, 0x90, 60, 100]);
        assert_eq!(queued(), [(0, 100)]);
    }

    #[test]
    fn program_changes_channels_and_multi_message_packets() {
        let _guard = LOCK.lock().unwrap();
        reset();
        midi::toggle_learn(0);
        midi::feed(&[0x80, 0x80, 0xc0, 3]);
        midi::toggle_learn(1);
        midi::feed(&[0x80, 0x80, 0xc0, 3]);
        midi::feed(&[0x80, 0x80, 0xc1, 3]);
        assert!(queued().is_empty());
        midi::feed(&[0x80, 0x80, 0xc0, 3]);
        assert_eq!(queued(), [(0, 0), (1, 0)]);
        reset();
        learn_cc(0, 7);
        learn_cc(1, 8);
        midi::feed(&[0x80, 0x80, 0xb0, 7, 127, 0x81, 0xb0, 8, 127]);
        assert_eq!(queued(), [(0, 127), (1, 127)]);
    }

    #[test]
    fn queue_overflow_is_atomic_and_disconnect_clears_every_path() {
        let _guard = LOCK.lock().unwrap();
        reset();
        for _ in 0..link::COMMAND_CAPACITY - 1 {
            link::push_cmd(link::Cmd::Save);
        }
        assert!(!link::push_midi([0, 1].into_iter(), 127, false));
        for _ in 0..link::COMMAND_CAPACITY - 1 {
            assert!(matches!(link::pop_cmd(), Some(link::Cmd::Save)));
        }
        assert!(link::pop_cmd().is_none());
        for n in 0..8 {
            assert!(link::write(&[n]));
        }
        assert!(!link::write(&[9]));
        for n in 0..8 {
            assert_eq!(link::pop_write(), Some(vec![n]));
        }
        link::push_cmd(link::Cmd::Save);
        link::write(&[1]);
        link::set_param(0, 0, 0.5);
        link::set_up(false);
        assert!(link::pop_cmd().is_none());
        assert!(link::pop_write().is_none());
        assert!(link::take_param().is_none());
        assert!(!link::push_midi([0].into_iter(), 127, false));
    }

    #[test]
    fn continuous_coalescing_does_not_cross_discrete_commands() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let gain = index("Amp gain");
        assert!(link::push_midi([gain].into_iter(), 10, true));
        link::push_cmd(link::Cmd::PresetAt(2));
        assert!(link::push_midi([gain].into_iter(), 70, true));
        assert!(matches!(
            link::pop_cmd(),
            Some(link::Cmd::MidiAction { value: 10, .. })
        ));
        assert!(matches!(link::pop_cmd(), Some(link::Cmd::PresetAt(2))));
        assert!(matches!(
            link::pop_cmd(),
            Some(link::Cmd::MidiAction { value: 70, .. })
        ));
    }

    #[test]
    fn bindings_persist_shared_keys_and_retry_failed_saves() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        learn_cc(midi::ACTION_COUNT - 1, 7);
        settings::FAIL.store(true, Ordering::Relaxed);
        midi::persist();
        assert_eq!(settings::SAVES.load(Ordering::Relaxed), 1);
        assert!(midi::last_text().contains("not saved"));
        midi::persist();
        assert_eq!(settings::SAVES.load(Ordering::Relaxed), 1);
        settings::FAIL.store(false, Ordering::Relaxed);
        clock::NOW.store(15_000_000, Ordering::Relaxed);
        midi::persist();
        let saved = midi::keys_from_json(&settings::load_midi_json().unwrap().unwrap()).unwrap();
        assert_ne!(saved[0], 0);
        assert_eq!(saved[0], saved[midi::ACTION_COUNT - 1]);
        midi::clear(0);
        midi::clear(midi::ACTION_COUNT - 1);
        midi::load();
        assert_eq!(
            midi::rows()[0].assigned,
            midi::rows()[midi::ACTION_COUNT - 1].assigned
        );
        assert!(!midi::rows()[0].assigned.is_empty());
    }

    #[test]
    fn expanded_actions_and_endpoints_keep_legacy_indices() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert_eq!(midi::ACTION_COUNT, 267);
        assert_eq!(midi::action_name(13), "Pre FX 1 knob 1");
        assert_eq!(midi::action_name(33), "Amp gain");
        assert_eq!(midi::action_name(41), "Cab low-pass");
        for value in [0, 127] {
            let raw = if value == 0 { 0 } else { 255 };
            assert!(matches!(midi::command(33, value, true),
                Some(link::Cmd::AmpKnob { knob: 0, raw: actual }) if actual == raw));
            assert!(matches!(midi::command(index("Gate amount"), value, true),
                Some(link::Cmd::GateAmount(actual)) if actual == if value == 0 { 0 } else { 100 }));
            assert!(
                matches!(midi::command(index("USB playback level"), value, true),
                Some(link::Cmd::UsbPlayback(db)) if db == if value == 0 { -40.0 } else { 0.0 })
            );
        }
        assert!(matches!(
            midi::command(index("Select preset 64"), 0, false),
            Some(link::Cmd::PresetAt(64))
        ));
        assert!(matches!(
            midi::command(index("Select capture 25"), 0, false),
            Some(link::Cmd::CaptureAt(25))
        ));
        assert!(matches!(
            midi::command(index("Unmute output"), 0, false),
            Some(link::Cmd::Mute {
                input: false,
                muted: false
            })
        ));
        assert!(midi::command(midi::ACTION_COUNT, 127, true).is_none());
        assert!(midi::command(33, 127, false).is_none());
    }

    #[test]
    fn fx_knobs_use_real_catalog_display_order_and_reject_missing_controls() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let model = fx::models::find_model_by_name("Dual Delay").unwrap();
        fx::SNAPSHOT.with(|s| {
            let mut s = s.borrow_mut();
            s.valid = true;
            s.blocks[3].valid = true;
            s.blocks[3].model = model;
            s.blocks[3].param_count = model.param_count;
        });
        let action = index("Post FX 2 knob 5");
        let Some(link::Cmd::Param(param)) = midi::command(action, 100, true) else {
            panic!("FX parameter command missing");
        };
        assert_eq!(param.block, 3);
        assert_eq!(param.wire, fx::models::param_wire_index(model, 4) as u8);
        assert_eq!(
            param.norm,
            fx::params::normalize_for_model(model, usize::from(param.wire), 100.0 / 127.0)
        );
        assert!(midi::command(index("Post FX 2 knob 22"), 127, true).is_some());
        assert!(midi::command(index("Post FX 2 knob 24"), 127, true).is_none());
        fx::SNAPSHOT.with(|s| s.borrow_mut().blocks[3].param_count = 3);
        assert!(midi::command(action, 127, true).is_none());
    }

    #[test]
    fn clear_all_cancels_learning_and_pending_midi_but_not_other_commands() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        learn_cc(1, 7);
        cc(7, 127);
        link::push_cmd(link::Cmd::Save);
        midi::toggle_learn(2);
        midi::clear_all();
        assert!(midi::rows()
            .iter()
            .all(|row| row.assigned.is_empty() && !row.learning));
        assert!(matches!(link::pop_cmd(), Some(link::Cmd::Save)));
        assert!(link::pop_cmd().is_none());
        midi::persist();
        assert!(settings::load_midi(midi::ACTION_COUNT)
            .unwrap()
            .iter()
            .all(|key| *key == 0));
    }

    #[test]
    fn json_import_export_round_trip_and_invalid_imports_are_transactional() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        learn_cc(index("Amp gain"), 7);
        let original = midi::export_json().unwrap();
        midi::clear_all();
        midi::import_json(&original).unwrap();
        assert_eq!(midi::export_json().unwrap(), original);
        let invalid = [
            r#"{"format":"other","version":1,"bindings":[]}"#,
            r#"{"format":"nano-midi","version":2,"bindings":[]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":0,"kind":"cc","channel":0,"number":7}]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":0,"kind":"cc","channel":17,"number":7}]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":0,"kind":"cc","channel":1,"number":128}]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":999999,"kind":"cc","channel":1,"number":7}]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":33,"kind":"note","channel":1,"number":7}]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":0,"kind":"cc","channel":1,"number":7},{"action":0,"kind":"cc","channel":1,"number":8}]}"#,
            r#"{"format":"nano-midi","version":1,"bindings":[],"unexpected":true}"#,
            "{",
        ];
        for json in invalid {
            assert!(midi::import_json(json).is_err(), "{json}");
            assert_eq!(midi::export_json().unwrap(), original);
        }
        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(midi::import_json(r#"{"format":"nano-midi","version":1,"bindings":[]}"#).is_err());
        assert_eq!(midi::export_json().unwrap(), original);
    }
}
