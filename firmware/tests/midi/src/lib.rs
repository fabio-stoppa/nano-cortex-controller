#![allow(dead_code)]

extern crate alloc;

#[path = "../../../src/ble/advertisement.rs"]
mod advertisement;
#[path = "../../../src/ble/connection.rs"]
mod connection;
#[path = "../../../src/ble/pairing.rs"]
mod pairing;

#[cfg(test)]
mod connection_tests;

#[path = "../../../src/usb_storage/disk.rs"]
mod usb_disk;
#[path = "../../../src/usb_storage/config.rs"]
mod usb_config;
#[path = "../../../src/settings/preferences.rs"]
pub mod preferences;
#[path = "../../../src/settings/selected_device.rs"]
mod selected_device;

#[path = "../../../src/fx/catalog.rs"]
pub mod catalog;
#[path = "../../../src/hci.rs"]
mod hci;
#[path = "../../../src/settings/json_record.rs"]
mod json_record;
#[path = "../../../src/settings/reset.rs"]
mod reset_record;
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
#[path = "../../../src/scenes.rs"]
mod scenes;
#[path = "../../../src/syncutil.rs"]
mod syncutil;
#[path = "../../../src/fx/types.rs"]
pub mod types;
#[path = "../../../src/fx/util.rs"]
pub mod util;

mod fx {
    #[cfg(test)]
    pub use crate::catalog;
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
    pub fn nano_state_note_sound_edit() {
        DIRTY.store(true, Ordering::Relaxed);
    }
    use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
    pub static DIRTY: AtomicBool = AtomicBool::new(false);
    use nano_wire::types::{Amp, CabSettings, NanoAmpC};
    pub static PRESET: AtomicU8 = AtomicU8::new(1);
    pub static GENERATION: AtomicU32 = AtomicU32::new(1);
    pub static ACK: AtomicU32 = AtomicU32::new(0);
    std::thread_local! {
        pub static AMP: core::cell::RefCell<NanoAmpC> = core::cell::RefCell::new(NanoAmpC::from(&Amp::default()));
    }
    pub fn nano_state_verified_amp() -> NanoAmpC {
        AMP.with(|amp| *amp.borrow())
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
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    pub use crate::preferences;

    pub static FAIL: AtomicBool = AtomicBool::new(false);
    pub static FAIL_MIDI: AtomicBool = AtomicBool::new(false);
    pub static SAVES: AtomicUsize = AtomicUsize::new(0);
    pub static SCENE_SAVES: AtomicUsize = AtomicUsize::new(0);
    pub static RECORD: Mutex<Option<String>> = Mutex::new(None);
    pub static SCENE_RECORD: Mutex<Option<String>> = Mutex::new(None);
    pub static PREFERENCES: Mutex<(u8, bool)> = Mutex::new((0, false));
    pub static PREFERENCE_SAVES: AtomicUsize = AtomicUsize::new(0);
    pub static FAIL_PREFERENCES: AtomicBool = AtomicBool::new(false);

    pub fn import_preferences(preferences: &preferences::Preferences) -> Result<(), &'static str> {
        PREFERENCE_SAVES.fetch_add(1, Ordering::Relaxed);
        if FAIL_PREFERENCES.load(Ordering::Relaxed) {
            return Err("Mock preferences save failed");
        }
        *PREFERENCES.lock().unwrap() = (preferences.orientation.layout(), preferences.autosave);
        Ok(())
    }

    pub fn load_scenes_json() -> Result<Option<String>, &'static str> {
        Ok(SCENE_RECORD.lock().unwrap().clone())
    }
    pub fn save_scenes_json(json: &str) -> Result<(), &'static str> {
        SCENE_SAVES.fetch_add(1, Ordering::Relaxed);
        if FAIL.load(Ordering::Relaxed) {
            return Err("Mock flash save failed");
        }
        *SCENE_RECORD.lock().unwrap() = Some(json.to_owned());
        Ok(())
    }

    pub fn save_midi(keys: &[u16], zero_is_press: &[bool]) -> bool {
        crate::midi::json_for_bindings(keys, zero_is_press)
            .and_then(|json| save_midi_json(&json))
            .is_ok()
    }

    pub fn load_midi_json() -> Result<Option<String>, &'static str> {
        Ok(RECORD.lock().unwrap().clone())
    }

    pub fn save_midi_json(json: &str) -> Result<(), &'static str> {
        SAVES.fetch_add(1, Ordering::Relaxed);
        if FAIL.load(Ordering::Relaxed) || FAIL_MIDI.load(Ordering::Relaxed) {
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
        settings::FAIL_MIDI.store(false, Ordering::Relaxed);
        settings::FAIL_PREFERENCES.store(false, Ordering::Relaxed);
        settings::PREFERENCE_SAVES.store(0, Ordering::Relaxed);
        *settings::PREFERENCES.lock().unwrap() = (0, false);
        settings::SCENE_SAVES.store(0, Ordering::Relaxed);
        settings::SAVES.store(0, Ordering::Relaxed);
        for i in 0..midi::ACTION_COUNT {
            midi::clear(i);
        }
        midi::persist();
        settings::SAVES.store(0, Ordering::Relaxed);
        fx::SNAPSHOT.with(|s| *s.borrow_mut() = fx::types::NanoFxState::zeroed());
        state::PRESET.store(1, Ordering::Relaxed);
        state::GENERATION.store(1, Ordering::Relaxed);
        state::ACK.store(0, Ordering::Relaxed);
        state::AMP.with(|amp| {
            *amp.borrow_mut() = nano_wire::types::NanoAmpC::from(&nano_wire::types::Amp::default())
        });
    }

    fn index(name: &str) -> usize {
        (0..midi::ACTION_COUNT)
            .find(|&i| midi::action_name(i) == name)
            .unwrap()
    }

    #[test]
    fn usb_validates_all_files_before_any_save_and_reports_partial_failure() {
        let _guard = LOCK.lock().unwrap();
        reset();
        *settings::SCENE_RECORD.lock().unwrap() = None;
        scenes::load();
        let original_scenes = scenes::export_json().unwrap();
        let changed_scenes = r#"{"format":"nano-scenes","version":3,"preset_scenes":[{"preset":2,"scenes":[null,null,null,null,null,null,null,null]}]}"#;
        let migrated_scenes = scenes::data::Config::decode(changed_scenes).unwrap().encode().unwrap();
        let original_midi = midi::export_json().unwrap();
        let mut keys = midi::keys_from_json(&original_midi).unwrap();
        keys[0] = 0x900a;
        let changed_midi = midi::json_for_keys(&keys).unwrap();
        let preferences = preferences::Preferences::new(3, true).unwrap().encode().unwrap();
        assert_ne!(changed_midi, original_midi);
        settings::SAVES.store(0, Ordering::Relaxed);
        settings::SCENE_SAVES.store(0, Ordering::Relaxed);

        assert!(usb_config::import(&changed_midi, "{}", &preferences).is_err());
        assert!(usb_config::import("{}", changed_scenes, &preferences).is_err());
        assert!(usb_config::import(&changed_midi, changed_scenes, "{}").is_err());
        assert_eq!(settings::SAVES.load(Ordering::Relaxed), 0);
        assert_eq!(settings::SCENE_SAVES.load(Ordering::Relaxed), 0);
        assert_eq!(settings::PREFERENCE_SAVES.load(Ordering::Relaxed), 0);
        assert_eq!(*settings::PREFERENCES.lock().unwrap(), (0, false));
        assert_eq!(midi::export_json().unwrap(), original_midi);
        assert_eq!(scenes::export_json().unwrap(), original_scenes);

        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(usb_config::import(&changed_midi, changed_scenes, &preferences).is_err());
        assert_eq!(settings::SAVES.load(Ordering::Relaxed), 0);
        assert_eq!(midi::export_json().unwrap(), original_midi);
        assert_eq!(scenes::export_json().unwrap(), original_scenes);
        assert_eq!(settings::PREFERENCE_SAVES.load(Ordering::Relaxed), 0);
        settings::FAIL.store(false, Ordering::Relaxed);

        settings::FAIL_MIDI.store(true, Ordering::Relaxed);
        assert!(usb_config::import(&changed_midi, changed_scenes, &preferences)
            .unwrap_err()
            .contains("Scenes saved, but MIDI save failed"));
        assert_eq!(midi::export_json().unwrap(), original_midi);
        assert_eq!(scenes::export_json().unwrap(), migrated_scenes);
        assert_eq!(
            settings::SCENE_RECORD.lock().unwrap().as_deref(),
            Some(migrated_scenes.as_str())
        );
        settings::FAIL_MIDI.store(false, Ordering::Relaxed);
        assert_eq!(settings::PREFERENCE_SAVES.load(Ordering::Relaxed), 0);

        settings::FAIL_PREFERENCES.store(true, Ordering::Relaxed);
        assert!(usb_config::import(&changed_midi, changed_scenes, &preferences)
            .unwrap_err().contains("MIDI and scenes saved, but preferences save failed"));
        assert_eq!(midi::export_json().unwrap(), changed_midi);
        assert_eq!(*settings::PREFERENCES.lock().unwrap(), (0, false));
        settings::FAIL_PREFERENCES.store(false, Ordering::Relaxed);

        usb_config::import(&changed_midi, changed_scenes, &preferences).unwrap();
        assert_eq!(midi::export_json().unwrap(), changed_midi);
        assert_eq!(
            settings::RECORD.lock().unwrap().as_deref(),
            Some(changed_midi.as_str())
        );
        assert_eq!(*settings::PREFERENCES.lock().unwrap(), (3, true));
    }

    #[test]
    fn usb_scene_export_does_not_hide_unreadable_saved_data() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let previous = settings::SCENE_RECORD.lock().unwrap().replace("invalid JSON".into());
        scenes::load();
        assert_eq!(scenes::export_json().unwrap_err(), "Invalid scenes.json");
        *settings::SCENE_RECORD.lock().unwrap() = previous;
        scenes::load();
        assert!(scenes::export_json().is_ok());
    }

    #[test]
    fn explicit_scene_clear_recovers_invalid_saved_json_without_clearing_other_data() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        midi::persist();
        let midi_before = settings::RECORD.lock().unwrap().clone();
        let preferences_before = *settings::PREFERENCES.lock().unwrap();
        *settings::SCENE_RECORD.lock().unwrap() = Some("invalid JSON".into());
        scenes::load();
        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(scenes::clear_all_scenes().is_err());
        assert!(scenes::export_json().is_err());
        assert_eq!(settings::SCENE_RECORD.lock().unwrap().as_deref(), Some("invalid JSON"));
        settings::FAIL.store(false, Ordering::Relaxed);
        scenes::clear_all_scenes().unwrap();
        scenes::load();
        let config = scenes::data::Config::decode(&scenes::export_json().unwrap()).unwrap();
        assert!(config.preset_scenes.is_empty());
        assert_eq!(*settings::RECORD.lock().unwrap(), midi_before);
        assert_eq!(*settings::PREFERENCES.lock().unwrap(), preferences_before);
        assert!(link::pop_cmd().is_none());
    }

    #[test]
    fn explicit_midi_reset_replaces_invalid_saved_json_even_with_no_loaded_bindings() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let scenes_before = settings::SCENE_RECORD.lock().unwrap().clone();
        let preferences_before = *settings::PREFERENCES.lock().unwrap();
        *settings::RECORD.lock().unwrap() = Some("invalid JSON".into());
        midi::load();
        assert!(!midi::notice().is_empty());
        settings::FAIL_MIDI.store(true, Ordering::Relaxed);
        assert!(midi::clear_all().is_err());
        assert_eq!(settings::RECORD.lock().unwrap().as_deref(), Some("invalid JSON"));
        settings::FAIL_MIDI.store(false, Ordering::Relaxed);
        midi::clear_all().unwrap();
        midi::load();
        assert!(midi::notice().is_empty());
        assert!(midi::keys_from_json(&settings::load_midi_json().unwrap().unwrap())
            .unwrap().iter().all(|key| *key == 0));
        assert_eq!(*settings::SCENE_RECORD.lock().unwrap(), scenes_before);
        assert_eq!(*settings::PREFERENCES.lock().unwrap(), preferences_before);
    }

    #[test]
    fn failed_midi_reset_retains_live_bindings_and_success_clears_only_midi_commands() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        midi::persist();
        let before = midi::export_json().unwrap();
        settings::FAIL_MIDI.store(true, Ordering::Relaxed);
        assert!(midi::clear_all().is_err());
        assert_eq!(midi::export_json().unwrap(), before);
        assert_eq!(settings::RECORD.lock().unwrap().as_deref(), Some(before.as_str()));
        settings::FAIL_MIDI.store(false, Ordering::Relaxed);
        cc(7, 127);
        link::push_cmd(link::Cmd::Save);
        midi::clear_all().unwrap();
        assert!(matches!(link::pop_cmd(), Some(link::Cmd::Save)));
        assert!(link::pop_cmd().is_none());
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
    fn retired_set_bindings_are_preserved_but_do_not_execute() {
        let _guard = LOCK.lock().unwrap();
        reset();
        learn_cc(0, 7);
        let mut config: serde_json::Value =
            serde_json::from_str(&midi::export_json().unwrap()).unwrap();
        config["bindings"][0]["action"] = 249.into();
        let json = serde_json::to_string(&config).unwrap();
        midi::import_json(&json).unwrap();
        cc(7, 127);
        assert!(queued().is_empty());
        assert!(midi::rows()[249].assigned.contains("7"));
        assert!(!midi::visible_rows()
            .iter()
            .any(|row| row.name.contains("bank")));
        assert!(midi::command(index("Next bank"), 127, false).is_none());
        assert!(midi::command(index("Recall bank slot 4"), 127, false).is_none());
    }

    #[test]
    fn scenes_are_preset_scoped_and_failed_saves_preserve_data() {
        let _guard = LOCK.lock().unwrap();
        reset();
        *settings::SCENE_RECORD.lock().unwrap() = None;
        scenes::load();
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
        assert!(scenes::capture_scene(2, 0, "Clean").is_err());
        scenes::capture_scene(1, 0, "Clean").unwrap();
        assert_eq!(scenes::scenes(1)[0].as_deref(), Some("Clean"));
        assert_eq!(scenes::scene(), Some(0));
        let before = scenes::export_json().unwrap();
        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(scenes::capture_scene(1, 0, "Lead").is_err());
        assert_eq!(scenes::export_json().unwrap(), before);
        settings::FAIL.store(false, Ordering::Relaxed);
        scenes::recall_scene(0).unwrap();
        assert!(scenes::busy());
        assert_eq!(scenes::recalling_scene(), Some(0));
        assert_eq!(scenes::scene(), None, "Pending recall is not verified");
        for _ in 0..64 {
            if matches!(scenes::poll(), Some(link::Cmd::SceneRefresh)) {
                state::GENERATION.fetch_add(1, Ordering::Relaxed);
            }
            if !scenes::busy() {
                break;
            }
        }
        assert!(!scenes::busy());
        assert_eq!(scenes::recalling_scene(), None);
        assert_eq!(scenes::scene(), Some(0));
        scenes::sound_edited();
        assert_eq!(scenes::scene(), Some(0));
        assert!(scenes::scene_dirty());
        let before = scenes::export_json().unwrap();
        assert!(scenes::rename_scene(1, 0, "12345678901234567").is_err());
        assert_eq!(scenes::export_json().unwrap(), before);
        scenes::rename_scene(1, 0, "1234567890123456").unwrap();
        assert!(scenes::scene_dirty(), "Renaming saved unsaved sound edits");
        assert_eq!(scenes::scene(), Some(0));
        scenes::rename_scene(1, 0, "Clean").unwrap();
        assert!(scenes::capture_scene(1, 5, "12345678901234567").is_err());
        scenes::capture_scene(1, 5, "Sixth").unwrap();
        assert_eq!(scenes::scenes(1).len(), 6);
        assert_eq!(scenes::scene(), Some(5));
        assert!(!scenes::scene_dirty());
        assert!(scenes::capture_scene(1, 6, "Overflow").is_err());
        assert!(scenes::recall_scene(6).is_err());
        scenes::recall_scene(5).unwrap();
        for _ in 0..64 {
            if matches!(scenes::poll(), Some(link::Cmd::SceneRefresh)) {
                state::GENERATION.fetch_add(1, Ordering::Relaxed);
            }
            if !scenes::busy() { break; }
        }
        assert_eq!(scenes::scene(), Some(5));
        assert!(!scenes::busy());
        scenes::recall_scene(0).unwrap();
        assert_eq!(scenes::recalling_scene(), Some(0));
        scenes::cancel("Test interruption");
        assert_eq!(scenes::recalling_scene(), None);
        assert!(scenes::scene().is_none());
        state::PRESET.store(2, Ordering::Relaxed);
        scenes::poll();
        assert!(scenes::scene().is_none());
        assert!(scenes::recall_scene(0).is_err());
        scenes::capture_scene(2, 0, "Other").unwrap();
        scenes::load();
        assert_eq!(scenes::scenes(1)[0].as_deref(), Some("Clean"));
        assert_eq!(scenes::scenes(1)[5].as_deref(), Some("Sixth"));
        assert_eq!(scenes::scenes(2)[0].as_deref(), Some("Other"));
        scenes::clear_scene(2, 0).unwrap();
        assert!(scenes::scenes(2)[0].is_none());
        assert_eq!(scenes::scenes(1)[0].as_deref(), Some("Clean"));
    }

    #[test]
    fn controller_sound_edits_mark_the_nano_preset_dirty() {
        let _guard = LOCK.lock().unwrap();
        reset();
        state::DIRTY.store(false, Ordering::Relaxed);
        scenes::sound_edited();
        assert!(state::DIRTY.load(Ordering::Relaxed));
        state::DIRTY.store(false, Ordering::Relaxed);
        scenes::sound_edited();
        assert!(state::DIRTY.load(Ordering::Relaxed));
    }

    #[test]
    fn autosave_debounces_edits_and_cancels_on_disable_or_preset_change() {
        let _guard = LOCK.lock().unwrap();
        reset();
        scenes::load();
        scenes::set_autosave(false);
        clock::NOW.store(0, Ordering::Relaxed);
        scenes::sound_edited();
        clock::NOW.store(6_000_000, Ordering::Relaxed);
        assert!(!scenes::autosave_due());
        scenes::set_autosave(true);
        assert!(!scenes::autosave_due(), "Enabling must not save old edits");
        scenes::sound_edited();
        clock::NOW.store(10_999_999, Ordering::Relaxed);
        assert!(!scenes::autosave_due());
        scenes::sound_edited();
        clock::NOW.store(15_999_998, Ordering::Relaxed);
        assert!(!scenes::autosave_due(), "Repeated edits must restart the timer");
        clock::NOW.store(15_999_999, Ordering::Relaxed);
        assert!(scenes::autosave_due());
        assert!(!scenes::autosave_due(), "Autosave must fire only once");
        scenes::sound_edited();
        state::PRESET.store(2, Ordering::Relaxed);
        clock::NOW.store(21_000_000, Ordering::Relaxed);
        assert!(!scenes::autosave_due(), "Never save another Nano preset");
        state::PRESET.store(1, Ordering::Relaxed);
        assert!(!scenes::autosave_due());
        scenes::sound_edited();
        scenes::set_autosave(false);
        clock::NOW.store(27_000_000, Ordering::Relaxed);
        assert!(!scenes::autosave_due());
    }

    #[test]
    fn autosave_updates_the_selected_scene_without_creating_empty_slots() {
        let _guard = LOCK.lock().unwrap();
        reset();
        *settings::SCENE_RECORD.lock().unwrap() = None;
        scenes::load();
        scenes::set_autosave(true);
        state::AMP.with(|amp| amp.borrow_mut().valid = true);
        fx::SNAPSHOT.with(|snapshot| {
            let mut view = snapshot.borrow_mut();
            *view = types::NanoFxState::reset_with_ids();
            view.valid = true;
            view.gate.on_known = true;
            for block in &mut view.blocks { block.valid = true; }
        });
        scenes::capture_scene(1, 0, "Scene").unwrap();
        scenes::sound_edited();
        assert!(scenes::scene_dirty());
        settings::FAIL.store(true, Ordering::Relaxed);
        let before = scenes::export_json().unwrap();
        assert!(scenes::autosave_selected().is_err());
        assert_eq!(scenes::export_json().unwrap(), before);
        assert!(scenes::scene_dirty());
        settings::FAIL.store(false, Ordering::Relaxed);
        scenes::autosave_selected().unwrap();
        assert!(!scenes::scene_dirty());
        assert_eq!(scenes::scenes(1)[0].as_deref(), Some("Scene"));
        assert!(scenes::scenes(1)[1..].iter().all(Option::is_none));
        scenes::set_autosave(false);
    }

    #[test]
    fn bulk_scene_clears_are_persistent_isolated_and_transactional() {
        let _guard = LOCK.lock().unwrap();
        reset();
        *settings::SCENE_RECORD.lock().unwrap() = None;
        scenes::load();
        state::AMP.with(|amp| amp.borrow_mut().valid = true);
        fx::SNAPSHOT.with(|snapshot| {
            let mut view = snapshot.borrow_mut();
            *view = types::NanoFxState::reset_with_ids();
            view.valid = true;
            view.gate.on_known = true;
            for block in &mut view.blocks { block.valid = true; }
        });
        for preset in [1, 64] {
            state::PRESET.store(preset, Ordering::Relaxed);
            scenes::capture_scene(preset, 0, "Scene").unwrap();
        }
        let before = scenes::export_json().unwrap();
        settings::FAIL.store(true, Ordering::Relaxed);
        assert!(scenes::clear_all_scenes().is_err());
        assert_eq!(scenes::export_json().unwrap(), before);
        assert_eq!(scenes::scene(), Some(0));
        settings::FAIL.store(false, Ordering::Relaxed);
        scenes::load();
        for preset in [1, 64] {
            assert_eq!(scenes::scenes(preset)[0].as_deref(), Some("Scene"));
        }
        scenes::clear_all_scenes().unwrap();
        assert_eq!(scenes::scene(), None);
        assert!(!scenes::scene_dirty());
        scenes::load();
        for preset in [1, 64] {
            assert!(scenes::scenes(preset).iter().all(Option::is_none));
        }
        assert!(link::pop_cmd().is_none(), "Clears must not change the Nano sound");
        state::PRESET.store(1, Ordering::Relaxed);
        scenes::capture_scene(1, 0, "Busy").unwrap();
        scenes::recall_scene(0).unwrap();
        assert!(scenes::busy());
        let before = scenes::export_json().unwrap();
        assert!(scenes::clear_all_scenes().is_err());
        assert_eq!(scenes::export_json().unwrap(), before);
        scenes::cancel("Test interruption");
    }

    #[test]
    fn obsolete_scene_records_are_rejected_without_migration() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let legacy = r#"{"format":"nano-scenes","version":1,"banks":[{"name":"Old Set"}],"preset_scenes":[{"preset":1}],"fx_presets":[{}],"block_presets":[{}]}"#;
        *settings::SCENE_RECORD.lock().unwrap() = Some(legacy.into());
        scenes::load();
        assert_eq!(scenes::notice(), "Invalid scenes.json");
        assert_eq!(settings::SCENE_RECORD.lock().unwrap().as_deref(), Some(legacy));
        assert!(scenes::clear_scene(1, 0).is_err(), "Failed reset allowed a destructive save");
        scenes::clear_all_scenes().unwrap();
        assert!(scenes::data::Config::decode(&scenes::export_json().unwrap())
            .unwrap().preset_scenes.is_empty());
        *settings::SCENE_RECORD.lock().unwrap() = None;
        scenes::load();
        state::AMP.with(|amp| amp.borrow_mut().valid = true);
        fx::SNAPSHOT.with(|snapshot| {
            let mut view = snapshot.borrow_mut();
            *view = types::NanoFxState::reset_with_ids();
            view.valid = true;
            view.gate.on_known = true;
            for block in &mut view.blocks { block.valid = true; }
        });
        scenes::capture_scene(1, 0, "New").unwrap();
        scenes::load();
        assert_eq!(scenes::scenes(1)[0].as_deref(), Some("New"));
    }

    #[test]
    fn gig_midi_modes_and_external_preset_changes_clear_stale_selection() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert!(matches!(
            midi::command(267, 127, false),
            Some(link::Cmd::Gig(true))
        ));
        assert!(matches!(
            midi::command(268, 127, false),
            Some(link::Cmd::Gig(false))
        ));
        *settings::SCENE_RECORD.lock().unwrap() = None;
        scenes::load();
        state::PRESET.store(1, Ordering::Relaxed);
        state::AMP.with(|amp| amp.borrow_mut().valid = true);
        fx::SNAPSHOT.with(|snapshot| {
            let mut snapshot = snapshot.borrow_mut();
            *snapshot = types::NanoFxState::reset_with_ids();
            snapshot.valid = true;
            snapshot.gate.on_known = true;
            for block in &mut snapshot.blocks {
                block.valid = true;
            }
        });
        scenes::capture_scene(1, 0, "Test").unwrap();
        assert_eq!(scenes::scene(), Some(0));
        state::PRESET.store(2, Ordering::Relaxed);
        scenes::poll();
        assert_eq!(scenes::scene(), None);
        state::PRESET.store(1, Ordering::Relaxed);
        scenes::capture_scene(1, 0, "Test").unwrap();
        link::set_up(false);
        assert!(scenes::scene().is_none());
    }

    #[test]
    fn all_new_actions_fit_the_persisted_json_and_atomic_multi_target_queue() {
        let _guard = LOCK.lock().unwrap();
        reset();
        for action in (0..midi::ACTION_COUNT).filter(|index| midi::active_action(*index)) {
            learn_cc(action, 127);
        }
        let json = midi::export_json().unwrap();
        assert!(
            json.len() <= 48 * 1024,
            "Full mapping exceeds midi.json capacity"
        );
        let decoded: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            decoded["bindings"].as_array().unwrap().len(),
            midi::visible_rows().len()
        );
        cc(127, 127);
        assert_eq!(
            queued().len(),
            midi::visible_rows().len(),
            "Shared message lost a mapped action"
        );
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
        for i in (0..midi::ACTION_COUNT).filter(|index| midi::active_action(*index)) {
            learn_cc(i, 7);
        }
        cc(7, 127);
        assert_eq!(queued().len(), midi::visible_rows().len());
        let json = midi::export_json().unwrap();
        assert!(json.len() <= json_record::MAX_JSON);
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
    fn rapid_midi_and_touch_bursts_leave_no_stale_edits_after_disconnect() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let a = index("Amp gain");
        let b = index("Post FX 2 knob 24");
        learn_cc(a, 7);
        learn_cc(b, 7);
        for value in 0..10_000 {
            cc(7, (value % 128) as u8);
            link::set_param(0, 0, (value % 128) as f32 / 127.0);
        }
        assert_eq!(queued(), [(a, 15), (b, 15)]);
        assert!(link::take_param().is_some());
        link::push_cmd(link::Cmd::Save);
        link::set_param(1, 1, 0.9);
        link::set_up(false);
        link::set_up(true);
        assert!(link::pop_cmd().is_none());
        assert!(link::take_param().is_none());
        assert!(link::pop_write().is_none());
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
    fn learned_zero_cc_press_triggers_button_and_survives_reload() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let actions = [
            (index("Tuner"), 1),
            (index("Recall scene 1"), 2),
            (index("Recall scene 2"), 3),
        ];
        for &(action, number) in &actions {
            midi::toggle_learn(action);
            cc(number, 0);
        }
        assert!(queued().is_empty());
        for &(action, _) in &actions {
            assert!(midi::rows()[action].assigned.contains("press=0"));
        }

        let saved = midi::export_json().unwrap();
        assert!(saved.contains("\"zero_is_press\": true"));
        midi::import_json(&saved).unwrap();

        for (index, &(action, number)) in actions.iter().enumerate() {
            cc(number, 127);
            assert!(queued().is_empty(), "release must not trigger action {action}");
            cc(number, 0);
            assert_eq!(queued(), [(action, 0)]);
            let Some(command) = midi::command(action, 0, false) else {
                panic!("learned action {action} must map to a command");
            };
            let expected = match index {
                0 => matches!(command, link::Cmd::TunerToggle),
                1 => matches!(command, link::Cmd::SceneRecall(0)),
                2 => matches!(command, link::Cmd::SceneRecall(1)),
                _ => false,
            };
            assert!(expected, "learned action {action} mapped to the wrong command");
        }
    }

    #[test]
    fn version_one_midi_bindings_keep_nonzero_press_behavior() {
        let _guard = LOCK.lock().unwrap();
        reset();
        midi::import_json(
            r#"{"format":"nano-midi","version":1,"bindings":[{"action":6,"kind":"cc","channel":1,"number":7}]}"#,
        )
        .unwrap();
        cc(7, 0);
        assert!(queued().is_empty());
        cc(7, 127);
        assert_eq!(queued(), [(6, 127)]);
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
    fn touch_and_midi_commands_wake_writer_and_disconnect_clears_wakeup() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert!(!link::COMMAND_READY.signaled());
        link::push_cmd(link::Cmd::SceneRecall(0));
        assert!(link::COMMAND_READY.signaled());
        assert!(matches!(link::pop_cmd(), Some(link::Cmd::SceneRecall(0))));
        link::COMMAND_READY.reset();
        assert!(link::push_midi([0].into_iter(), 127, false));
        assert!(link::COMMAND_READY.signaled());
        link::COMMAND_READY.reset();
        link::set_param(0, 0, 0.5);
        assert!(link::COMMAND_READY.signaled());
        link::set_up(false);
        assert!(!link::COMMAND_READY.signaled());
        link::push_cmd(link::Cmd::SceneRecall(0));
        assert!(!link::COMMAND_READY.signaled());
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
        link::push_cmd(link::Cmd::Copy);
        link::push_cmd(link::Cmd::PresetDelete(4));
        link::push_cmd(link::Cmd::PresetTransfer {
            src: 1,
            dst: 2,
            remove_src: true,
        });
        link::write(&[1]);
        link::set_param(0, 0, 0.5);
        link::set_up(false);
        assert!(link::pop_cmd().is_none());
        assert!(link::pop_write().is_none());
        assert!(link::take_param().is_none());
        assert!(!link::push_midi([0].into_iter(), 127, false));
        link::push_cmd(link::Cmd::Save);
        link::set_param(1, 2, 0.75);
        assert!(!link::write(&[1]));
        link::set_up(true);
        assert!(link::pop_cmd().is_none());
        assert!(link::pop_write().is_none());
        assert!(link::take_param().is_none());
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
    fn live_writes_precede_background_reads_but_scene_writes_stay_fifo() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let background = [0x01, 0x02];
        let mut live = [0u8; 16];
        let live_len = nano_wire::amp_knob_payload(nano_wire::Knob::Gain, 99, &mut live);
        assert!(link::write(&background));
        assert!(link::write(&live[..live_len]));
        assert_eq!(link::pop_write(), Some(live[..live_len].to_vec()));
        assert_eq!(link::pop_write(), Some(background.to_vec()));

        assert!(link::write(&background));
        assert!(link::write(&live[..live_len]));
        assert_eq!(
            link::pop_write_with_scene_busy(true),
            Some(background.to_vec())
        );
        assert_eq!(
            link::pop_write_with_scene_busy(true),
            Some(live[..live_len].to_vec())
        );
    }

    #[test]
    fn write_labels_survive_queueing_for_outcome_reporting() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert!(link::write_labeled(c"Delete preset 4", &[0x01, 0x02]));
        assert_eq!(
            link::pop_write_entry(false),
            Some((vec![0x01, 0x02], Some(String::from("Delete preset 4"))))
        );
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
    fn midi_categories_cover_visible_actions_without_changing_indices() {
        let cases = [
            ("Tuner", 0),
            ("Tap tempo", 0),
            ("USB playback level", 0),
            ("Mute input", 0),
            ("Next preset", 0),
            ("Previous capture", 5),
            ("Save preset", 0),
            ("Select preset 1", 1),
            ("Select preset 16", 1),
            ("Select preset 17", 2),
            ("Select preset 32", 2),
            ("Select preset 33", 3),
            ("Select preset 48", 3),
            ("Select preset 49", 4),
            ("Select preset 64", 4),
            ("Select capture 25", 5),
            ("Recall scene 6", 6),
            ("Enter gig view", 6),
            ("Noise gate", 7),
            ("Gate amount", 7),
            ("Amp gain", 7),
            ("Capture volume", 7),
            ("Cab low-pass", 7),
            ("Pre FX 1 on/off", 8),
            ("Pre FX 1 knob 1", 9),
            ("Post FX 3 knob 24", 13),
            ("Pre FX 1 next model", 8),
        ];
        for (name, category) in cases {
            assert_eq!(midi::action_category(index(name)), category, "{name}");
        }
        let mut counts = [0; 14];
        for (row, action) in (0..midi::ACTION_COUNT)
            .filter(|action| midi::active_action(*action))
            .enumerate()
        {
            counts[midi::action_category(action) as usize] += 1;
            assert_eq!(midi::visible_action(row), Some(action));
        }
        assert_eq!(counts.iter().sum::<usize>(), 257);
        assert!(counts.iter().all(|count| *count > 0));
        assert!(counts.iter().all(|count| *count <= 27), "Category too large: {counts:?}");
        assert_eq!(&counts[1..5], &[16; 4]);
        assert_eq!(&counts[9..], &[24; 5]);
    }

    #[test]
    fn scene_migration_is_persisted_and_does_not_overwrite_on_save_failure() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let old = r#"{"format":"nano-scenes","version":3,"preset_scenes":[{"preset":1,"scenes":[null,null,null,null,null,null,null,null]}]}"#;
        *settings::SCENE_RECORD.lock().unwrap() = Some(old.into());
        settings::FAIL.store(true, Ordering::Relaxed);
        scenes::load();
        assert!(scenes::export_json().is_err());
        assert_eq!(settings::SCENE_RECORD.lock().unwrap().as_deref(), Some(old));
        settings::FAIL.store(false, Ordering::Relaxed);
        scenes::load();
        let saved = settings::SCENE_RECORD.lock().unwrap().clone().unwrap();
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["version"], 4);
        assert_eq!(value["preset_scenes"][0]["scenes"].as_array().unwrap().len(), 6);
        assert_eq!(scenes::export_json().unwrap(), saved);
        scenes::import_json(old).unwrap();
        assert_eq!(settings::SCENE_RECORD.lock().unwrap().as_deref(), Some(saved.as_str()));
    }

    #[test]
    fn retired_scene_midi_bindings_are_removed_without_renumbering_survivors() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let old = r#"{"format":"nano-midi","version":2,"bindings":[{"action":270,"kind":"cc","channel":1,"number":6},{"action":271,"kind":"cc","channel":1,"number":7},{"action":272,"kind":"cc","channel":1,"number":8}]}"#;
        midi::import_json(old).unwrap();
        let saved = settings::RECORD.lock().unwrap().clone().unwrap();
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        let bindings = value["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0]["action"], 270);
        assert_eq!(midi::keys_from_json(&saved).unwrap()[270], 0x9006);
        assert!(midi::command(271, 127, false).is_none());
        assert!(midi::command(272, 127, false).is_none());
        *settings::RECORD.lock().unwrap() = Some(old.into());
        midi::load();
        assert_eq!(settings::RECORD.lock().unwrap().as_deref(), Some(saved.as_str()));
    }

    #[test]
    fn expanded_actions_and_endpoints_use_compact_indices() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert_eq!(midi::ACTION_COUNT, 271);
        assert_eq!(midi::visible_rows().len(), 257);
        assert_eq!(midi::visible_action(249), Some(263));
        assert_eq!(midi::visible_action(253), Some(267));
        assert_eq!(midi::visible_action(254), Some(268));
        assert_eq!(midi::visible_action(256), Some(270));
        assert_eq!(midi::visible_action(257), None);
        assert!(!midi::active_action(271));
        assert!(!midi::active_action(272));
        for scene in 4..6 {
            let index = 269 + scene - 4;
            assert_eq!(midi::action_name(index), format!("Recall scene {}", scene + 1));
            assert!(matches!(midi::command(index, 127, false), Some(link::Cmd::SceneRecall(slot)) if usize::from(slot) == scene));
        }
        assert!(!midi::active_action(249));
        assert!(midi::active_action(267));
        assert!(midi::active_action(268));
        assert_eq!(midi::action_name(267), "Enter gig view");
        assert_eq!(midi::action_name(268), "Exit gig view");
        assert!(matches!(midi::command(267, 127, false), Some(link::Cmd::Gig(true))));
        assert!(matches!(midi::command(268, 127, false), Some(link::Cmd::Gig(false))));
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
        midi::clear_all().unwrap();
        assert!(midi::rows()
            .iter()
            .all(|row| row.assigned.is_empty() && !row.learning));
        assert!(matches!(link::pop_cmd(), Some(link::Cmd::Save)));
        assert!(link::pop_cmd().is_none());
        midi::persist();
        assert!(midi::keys_from_json(&settings::load_midi_json().unwrap().unwrap())
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
        midi::clear_all().unwrap();
        midi::import_json(&original).unwrap();
        assert_eq!(midi::export_json().unwrap(), original);
        let invalid = [
            r#"{"format":"other","version":1,"bindings":[]}"#,
            r#"{"format":"nano-midi","version":3,"bindings":[]}"#,
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
