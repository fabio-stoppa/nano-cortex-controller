use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

#[path = "scenes/data.rs"]
pub mod data;
#[path = "scenes/restore.rs"]
pub mod restore;
use data::Config;
use restore::{Plan, Progress, View};

struct Pending {
    index: usize,
    plan: Plan,
    started_us: i64,
}

struct State {
    config: Option<Config>,
    load_error: Option<&'static str>,
    version: u32,
    notice: &'static str,
    scene: Option<usize>,
    scene_dirty: bool,
    preset: u8,
    pending: Option<Pending>,
    autosave: bool,
    autosave_edit: Option<(u8, i64)>,
}

struct Store(RefCell<State>);
// Touch, BLE and MIDI futures all run on the main executor task.
unsafe impl Sync for Store {}

static STATE: Store = Store(RefCell::new(State {
    config: None,
    load_error: None,
    version: 1,
    notice: "",
    scene: None,
    scene_dirty: false,
    preset: 0,
    pending: None,
    autosave: false,
    autosave_edit: None,
}));

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.0.borrow_mut())
}

impl State {
    fn config(&mut self) -> &mut Config {
        self.config.get_or_insert_with(Config::default)
    }
}

pub fn load() {
    let config = crate::settings::load_scenes_json()
        .and_then(|json| {
            json.as_deref()
                .map(|json| {
                    let (config, migrated) = Config::decode_with_migration(json)?;
                    if migrated {
                        crate::settings::save_scenes_json(&config.encode()?)?;
                        log::warn!("Scenes migrated to six slots per preset; scenes 7-8 removed");
                    }
                    Ok(config)
                })
                .transpose()
        })
        .map(Option::unwrap_or_default);
    match config {
        Ok(config) => {
            install(config);
        }
        Err(error) => {
            install(Config::default());
            with(|state| state.load_error = Some(error));
            report(error);
        }
    }
}

pub fn report(notice: &'static str) {
    log::warn!("Scenes: {notice}");
    with(|state| {
        state.notice = notice;
        state.version = state.version.wrapping_add(1);
    });
}

fn install(config: Config) {
    with(|state| {
        state.config = Some(config);
        state.load_error = None;
        state.scene = None;
        state.scene_dirty = false;
        state.preset = crate::state::nano_state_preset_slot();
        state.pending = None;
        state.autosave_edit = None;
        state.notice = "Scenes saved";
        state.version = state.version.wrapping_add(1);
    });
}

pub fn export_json() -> Result<String, &'static str> {
    with(|state| {
        if let Some(error) = state.load_error {
            return Err(error);
        }
        state.config().encode()
    })
}

pub fn import_json(json: &str) -> Result<(), &'static str> {
    let config = Config::decode(json)?;
    crate::settings::save_scenes_json(&config.encode()?)?;
    install(config);
    Ok(())
}
fn edit(f: impl FnOnce(&mut Config) -> Result<(), &'static str>) -> Result<(), &'static str> {
    if let Some(error) = with(|state| state.load_error) {
        return Err(error);
    }
    if busy() {
        return Err("Wait for the current recall to finish");
    }
    let mut changed = with(|state| state.config().clone());
    f(&mut changed)?;
    let json = changed.encode()?;
    crate::settings::save_scenes_json(&json)?;
    with(|state| {
        state.config = Some(changed);
        state.notice = "Scenes saved";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
}

pub fn keep_master_eq() -> bool {
    with(|state| state.config().keep_master_eq)
}

pub fn set_keep_master_eq(enabled: bool) -> Result<(), &'static str> {
    edit(|config| {
        config.keep_master_eq = enabled;
        Ok(())
    })
}

pub fn notice() -> &'static str {
    with(|state| state.notice)
}
pub fn version() -> u32 {
    with(|state| state.version)
}

pub fn view() -> View {
    let mut amp = nano_wire::types::NanoAmpC::from(&nano_wire::types::Amp::default());
    let mut effects = crate::fx::types::NanoFxState::zeroed();
    crate::state::nano_state_snapshot_amp(&mut amp);
    crate::fx::nano_fx_snapshot(&mut effects);
    let (cab_slot, mut cab) = crate::state::nano_state_cab_settings_snapshot();
    if amp.active_cab_slot() != Some(cab_slot) {
        cab = Default::default();
    }
    View {
        amp,
        verified_amp: crate::state::nano_state_verified_amp(),
        effects,
        cab,
        generation: crate::state::snapshot_generation(),
        cab_generation: crate::state::cab_generation(),
        preset: crate::state::nano_state_preset_slot(),
        idle: crate::state::nano_rx_mode_get() == 0,
    }
}

pub fn busy() -> bool {
    with(|state| state.pending.is_some())
}
pub fn scene() -> Option<usize> {
    with(|state| state.scene)
}
pub fn recalling_scene() -> Option<usize> {
    with(|state| state.pending.as_ref().map(|pending| pending.index))
}
pub fn scene_dirty() -> bool {
    with(|state| state.scene_dirty)
}
pub fn cancel(reason: &'static str) {
    let cancelled = with(|state| {
        state.autosave_edit = None;
        let selected = state.scene.take().is_some() || state.scene_dirty;
        state.scene_dirty = false;
        if selected {
            state.version = state.version.wrapping_add(1);
        }
        let pending = state.pending.take();
        pending.is_some()
    });
    if cancelled {
        report(reason);
    }
}

pub fn scenes(preset: usize) -> Vec<Option<String>> {
    with(|state| {
        state
            .config()
            .preset_scenes
            .iter()
            .find(|collection| usize::from(collection.preset) == preset)
            .map(|collection| &collection.scenes)
            .map_or_else(
                || alloc::vec![None; data::SCENES],
                |scenes| {
                    scenes
                        .iter()
                        .map(|scene| scene.as_ref().map(|scene| scene.name.clone()))
                        .collect()
                },
            )
    })
}

pub fn capture_scene(preset: u8, index: usize, name: &str) -> Result<(), &'static str> {
    let error = data::name_error(name);
    if !error.is_empty() {
        return Err(error);
    }
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Connect the Nano and exit tuner before saving a scene");
    }
    let current = view();
    if !(1..=64).contains(&preset) || current.preset != preset {
        return Err("Nano preset changed; reopen the scene selector before saving");
    }
    let snapshot = data::Snapshot::capture(&current)?;
    edit(|config| {
        if !config
            .preset_scenes
            .iter()
            .any(|collection| collection.preset == preset)
        {
            config.preset_scenes.push(data::PresetScenes {
                preset,
                scenes: Default::default(),
            });
        }
        let scene = config
            .preset_scenes
            .iter_mut()
            .find(|collection| collection.preset == preset)
            .and_then(|collection| collection.scenes.get_mut(index))
            .ok_or("Invalid scene slot")?;
        *scene = Some(data::Scene {
            name: String::from(name),
            snapshot,
        });
        Ok(())
    })?;
    with(|state| {
        state.scene = Some(index);
        state.scene_dirty = false;
        state.preset = preset;
    });
    Ok(())
}

pub fn clear_scene(preset: usize, index: usize) -> Result<(), &'static str> {
    edit(|config| {
        let scene = config
            .preset_scenes
            .iter_mut()
            .find(|collection| usize::from(collection.preset) == preset)
            .and_then(|collection| collection.scenes.get_mut(index))
            .ok_or("Invalid scene slot")?;
        *scene = None;
        Ok(())
    })?;
    with(|state| {
        if usize::from(state.preset) == preset && state.scene == Some(index) {
            state.scene = None;
            state.scene_dirty = false;
        }
    });
    Ok(())
}

pub fn clear_all_scenes() -> Result<(), &'static str> {
    if busy() {
        return Err("Wait for the current recall to finish");
    }
    // Explicit, confirmed clearing must also recover an unreadable configuration.
    let mut config = Config::default();
    config.keep_master_eq = keep_master_eq();
    crate::settings::save_scenes_json(&config.encode()?)?;
    install(config);
    Ok(())
}

pub fn rename_scene(preset: usize, index: usize, name: &str) -> Result<(), &'static str> {
    let error = data::name_error(name);
    if !error.is_empty() {
        return Err(error);
    }
    edit(|config| {
        let scene = config
            .preset_scenes
            .iter_mut()
            .find(|collection| usize::from(collection.preset) == preset)
            .and_then(|collection| collection.scenes.get_mut(index))
            .and_then(Option::as_mut)
            .ok_or("This scene slot is empty")?;
        scene.name = name.into();
        Ok(())
    })
}

pub fn recall_scene(index: usize) -> Result<(), &'static str> {
    if !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Connect the Nano and exit tuner before recalling a scene");
    }
    let mut current = view();
    if with(|state| {
        state.preset == current.preset
            && state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.index == index)
    }) {
        return Ok(());
    }
    if busy() {
        // An interrupted recall can still have stale readback; restore all target values.
        current.idle = false;
    }
    let preset = current.preset;
    let snapshot = with(|state| -> Result<_, &'static str> {
        let collection = state
            .config()
            .preset_scenes
            .iter()
            .find(|collection| collection.preset == preset)
            .ok_or("This Nano preset has no saved scenes")?;
        let scene = collection
            .scenes
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("This scene is empty")?;
        Ok(scene.snapshot.clone())
    })?;
    let started_us = crate::clock::now_us();
    let keep_master_eq = keep_master_eq();
    let plan = Plan::scene(snapshot, &current, started_us, keep_master_eq)?;
    with(|state| {
        state.preset = preset;
        state.pending = Some(Pending {
            index,
            plan,
            started_us,
        });
        state.autosave_edit = None;
        state.scene = None;
        state.scene_dirty = false;
        state.notice = "Recalling scene and verifying...";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
}

pub fn poll() -> Option<crate::link::Cmd> {
    let preset = crate::state::nano_state_preset_slot();
    let mut pending = with(|state| {
        if state.preset != preset {
            state.preset = preset;
            state.scene = None;
            state.scene_dirty = false;
            state.version = state.version.wrapping_add(1);
        }
        state.pending.take()
    })?;
    let current = view();
    let now = crate::clock::now_us();
    let result = pending.plan.poll(&current, now);
    match result {
        Progress::Complete => {
            log::info!(
                "Scene {} verified in {} us",
                pending.index + 1,
                now - pending.started_us
            );
            with(|state| {
                state.scene = Some(pending.index);
                state.scene_dirty = false;
                state.notice = "Recall verified";
                state.version = state.version.wrapping_add(1);
            });
            None
        }
        Progress::Failed(error) => {
            with(|state| {
                state.scene = None;
                state.scene_dirty = false;
            });
            report(error);
            None
        }
        Progress::Waiting => {
            with(|state| state.pending = Some(pending));
            None
        }
        Progress::Write(command) => {
            if matches!(&command, crate::link::Cmd::SceneRefresh) {
                log::info!(
                    "Scene {} writes sent in {} us; awaiting readback (not audio latency)",
                    pending.index + 1,
                    now - pending.started_us
                );
            }
            if !matches!(
                &command,
                crate::link::Cmd::SceneRefresh | crate::link::Cmd::SceneFxRefresh
            ) {
                crate::state::nano_state_note_sound_edit();
            }
            with(|state| state.pending = Some(pending));
            Some(command)
        }
    }
}

pub fn disconnected() {
    cancel("Nano disconnected; recall may be partially applied");
    with(|state| {
        state.autosave_edit = None;
        state.scene = None;
        state.scene_dirty = false;
        state.version = state.version.wrapping_add(1);
    });
}

pub fn sound_edited() {
    crate::state::nano_state_note_sound_edit();
    with(|state| {
        state.autosave_edit = Some((
            crate::state::nano_state_preset_slot(),
            crate::clock::now_us(),
        ));
        if state.scene.is_some() && !state.scene_dirty {
            state.scene_dirty = true;
            state.version = state.version.wrapping_add(1);
        }
    });
}

pub fn set_autosave(enabled: bool) {
    with(|state| {
        state.autosave = enabled;
        state.autosave_edit = None;
    });
}

/// Called after queued edits have been sent and the Nano state is idle.
pub fn autosave_due() -> bool {
    with(|state| {
        let Some((preset, edited)) = state.autosave_edit else {
            return false;
        };
        if preset != crate::state::nano_state_preset_slot() || !crate::link::connected() {
            state.autosave_edit = None;
            return false;
        }
        if !state.autosave || crate::clock::now_us() - edited < 5_000_000 {
            return false;
        }
        if state.pending.is_some()
            || crate::state::nano_rx_mode_get() != 0
            || crate::tuner::nano_tuner_is_active()
        {
            return false;
        }
        state.autosave_edit = None;
        true
    })
}

pub fn autosave_selected() -> Result<(), &'static str> {
    let preset = crate::state::nano_state_preset_slot();
    if scene_dirty() {
        if let Some(index) = scene() {
            let name = scenes(usize::from(preset))
                .get(index)
                .cloned()
                .flatten()
                .ok_or("Autosave scene no longer exists")?;
            capture_scene(preset, index, &name)?;
        }
    }
    Ok(())
}
