use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

#[path = "performance/data.rs"]
pub mod data;
#[path = "performance/restore.rs"]
pub mod restore;
use data::Config;
use restore::{Plan, Progress, View};

enum Pending {
    Scene {
        index: usize,
        plan: Plan,
    },
    Effect {
        index: usize,
        block: usize,
        plan: Plan,
    },
}

struct State {
    config: Option<Config>,
    load_error: Option<&'static str>,
    version: u32,
    notice: &'static str,
    scene: Option<usize>,
    scene_dirty: bool,
    preset: u8,
    fx_selected: [Option<usize>; 5],
    fx_models: [Vec<u8>; 5],
    fx_dirty: [bool; 5],
    pending: Option<Pending>,
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
    fx_selected: [None; 5],
    fx_models: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
    fx_dirty: [false; 5],
    pending: None,
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
    let config = crate::settings::load_performance_json()
        .and_then(|json| json.as_deref().map(Config::decode).transpose())
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
    log::warn!("Performance: {notice}");
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
        state.fx_selected = [None; 5];
        state.fx_models = Default::default();
        state.fx_dirty = [false; 5];
        state.pending = None;
        state.notice = "Performance configuration saved";
        state.version = state.version.wrapping_add(1);
    });
}

#[cfg(test)]
pub fn export_json() -> Result<String, &'static str> {
    let config = with(|state| state.config().clone());
    config.encode()
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
    crate::settings::save_performance_json(&json)?;
    with(|state| {
        state.config = Some(changed);
        state.notice = "Performance configuration saved";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
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
pub fn scene_dirty() -> bool {
    with(|state| state.scene_dirty)
}
pub fn fx_selected(block: usize) -> Option<usize> {
    sync_fx_selections();
    with(|state| state.fx_selected.get(block).copied().flatten())
}

fn sync_fx_selections() {
    let mut effects = crate::fx::types::NanoFxState::zeroed();
    crate::fx::nano_fx_snapshot(&mut effects);
    with(|state| {
        for (block, current) in effects.blocks.iter().enumerate() {
            if state.fx_selected[block].is_some()
                && (!effects.valid || !current.valid || current.model_id() != state.fx_models[block])
            {
                state.fx_selected[block] = None;
                state.fx_dirty[block] = false;
                state.version = state.version.wrapping_add(1);
            }
        }
    });
}
pub fn fx_dirty(block: usize) -> bool {
    with(|state| state.fx_dirty.get(block).copied().unwrap_or(false))
}

pub fn fx_model(block: usize) -> Option<Vec<u8>> {
    let mut effects = crate::fx::types::NanoFxState::zeroed();
    crate::fx::nano_fx_snapshot(&mut effects);
    let current = effects.blocks.get(block)?;
    if !effects.valid || !current.valid {
        return None;
    }
    let model = crate::fx::models::model_ref(current.model)?;
    if current.model_id() != model.id_bytes()
        || !crate::fx::models::is_model_allowed(block as u32, model)
    {
        return None;
    }
    Some(model.id_bytes().to_vec())
}

pub fn cancel(reason: &'static str) {
    let cancelled = with(|state| {
        let selected = state.scene.take().is_some()
            || state.fx_selected.iter().any(Option::is_some)
            || state.scene_dirty;
        state.scene_dirty = false;
        state.fx_selected = [None; 5];
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
    if !error.is_empty() { return Err(error); }
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

pub fn rename_scene(preset: usize, index: usize, name: &str) -> Result<(), &'static str> {
    let error = data::name_error(name);
    if !error.is_empty() { return Err(error); }
    edit(|config| {
        let scene = config.preset_scenes.iter_mut()
            .find(|collection| usize::from(collection.preset) == preset)
            .and_then(|collection| collection.scenes.get_mut(index))
            .and_then(Option::as_mut)
            .ok_or("This scene slot is empty")?;
        scene.name = name.into();
        Ok(())
    })
}

pub fn recall_scene(index: usize) -> Result<(), &'static str> {
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Wait for recall, connect the Nano and exit tuner first");
    }
    let preset = view().preset;
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
    let plan = Plan::scene(snapshot, preset, crate::clock::now_us())?;
    with(|state| {
        state.pending = Some(Pending::Scene { index, plan });
        state.scene = None;
        state.scene_dirty = false;
        state.fx_selected = [None; 5];
        state.notice = "Recalling scene and verifying...";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
}

pub fn poll() -> Option<crate::link::Cmd> {
    sync_fx_selections();
    let preset = crate::state::nano_state_preset_slot();
    let mut pending = with(|state| {
        if state.preset != preset {
            state.preset = preset;
            state.scene = None;
            state.scene_dirty = false;
            state.fx_selected = [None; 5];
            state.version = state.version.wrapping_add(1);
        }
        state.pending.take()
    })?;
    let current = view();
    let now = crate::clock::now_us();
    let result = match &mut pending {
        Pending::Scene { plan, .. } | Pending::Effect { plan, .. } => plan.poll(&current, now),
    };
    match result {
        Progress::Complete => {
            with(|state| {
                match pending {
                    Pending::Scene { index, .. } => {
                        state.scene = Some(index);
                        state.scene_dirty = false;
                        state.fx_selected = [None; 5];
                    }
                    Pending::Effect { index, block, .. } => {
                        state.fx_selected[block] = Some(index);
                        state.fx_models[block] = current.effects.blocks[block].model_id().to_vec();
                        state.fx_dirty[block] = false;
                        state.scene_dirty = state.scene.is_some();
                    }
                }
                state.notice = "Recall verified";
                state.version = state.version.wrapping_add(1);
            });
            None
        }
        Progress::Failed(error) => {
            with(|state| {
                state.scene = None;
                state.scene_dirty = false;
                state.fx_selected = [None; 5];
            });
            report(error);
            None
        }
        Progress::Waiting => {
            with(|state| state.pending = Some(pending));
            None
        }
        Progress::Write(command) => {
            with(|state| state.pending = Some(pending));
            Some(command)
        }
    }
}

pub fn fx_presets(model: &[u8]) -> Vec<Option<data::FxPreset>> {
    with(|state| {
        state
            .config()
            .model_presets
            .iter()
            .find(|collection| collection.model == model)
            .map_or_else(|| alloc::vec![None; data::FX_PRESETS_PER_MODEL], |collection| collection.presets.to_vec())
    })
}

pub fn disconnected() {
    cancel("Nano disconnected; recall may be partially applied");
    with(|state| {
        state.scene = None;
        state.scene_dirty = false;
        state.fx_selected = [None; 5];
        state.version = state.version.wrapping_add(1);
    });
}

pub fn sound_edited() {
    with(|state| {
        if state.scene.is_some() && !state.scene_dirty {
            state.scene_dirty = true;
            state.version = state.version.wrapping_add(1);
        }
    });
}

pub fn effect_edited(block: usize) {
    sound_edited();
    fx_selected(block);
    with(|state| {
        if block < state.fx_selected.len() && state.fx_selected[block].is_some() && !state.fx_dirty[block] {
            state.fx_dirty[block] = true;
            state.version = state.version.wrapping_add(1);
        }
    });
}

pub fn capture_fx(index: usize, block: usize, name: &str, model: &[u8]) -> Result<(), &'static str> {
    let error = data::name_error(name);
    if !error.is_empty() { return Err(error); }
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Wait for recall, connect the Nano and exit tuner before storing FX");
    }
    if fx_model(block).as_deref() != Some(model) {
        return Err("FX model changed or is not loaded; reopen FX preset saving");
    }
    let current = view();
    if !current.idle || !current.effects.valid {
        return Err("Wait for Nano FX state to load");
    }
    let effect = data::Effect::capture(
        current
            .effects
            .blocks
            .get(block)
            .ok_or("Invalid FX source block")?,
        block,
    )?;
    if effect.model.is_empty() || effect.model != model {
        return Err("FX model changed or is not loaded; reopen FX preset saving");
    }
    if index >= data::FX_PRESETS_PER_MODEL {
        return Err("Invalid FX preset slot");
    }
    let dirty: [bool; 5] = core::array::from_fn(|block| !effect.matches(&current.effects.blocks[block]));
    edit(|config| {
        if !config.model_presets.iter().any(|collection| collection.model == model) {
            config.model_presets.push(data::ModelPresets {
                model: model.to_vec(), presets: Default::default(),
            });
        }
        let collection = config.model_presets.iter_mut()
            .find(|collection| collection.model == model).ok_or("FX preset collection missing")?;
        collection.presets[index] = Some(data::FxPreset {
            name: name.into(),
            effect,
        });
        Ok(())
    })?;
    with(|state| {
        state.fx_selected[block] = Some(index);
        state.fx_models[block] = model.to_vec();
        state.fx_dirty[block] = false;
        for other in 0..5 {
            if other != block && state.fx_models[other] == model && state.fx_selected[other] == Some(index) {
                state.fx_dirty[other] = dirty[other];
            }
        }
    });
    Ok(())
}

pub fn clear_fx(index: usize, model: &[u8]) -> Result<(), &'static str> {
    edit(|config| {
        *config
            .model_presets
            .iter_mut()
            .find(|collection| collection.model == model)
            .and_then(|collection| collection.presets.get_mut(index))
            .ok_or("Invalid FX preset slot")? = None;
        Ok(())
    })?;
    with(|state| {
        for block in 0..5 {
            if state.fx_models[block] == model && state.fx_selected[block] == Some(index) {
                state.fx_selected[block] = None;
                state.fx_dirty[block] = false;
            }
        }
    });
    Ok(())
}

pub fn recall_fx(index: usize, block: usize) -> Result<(), &'static str> {
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Wait for recall, connect the Nano and exit tuner before recalling FX");
    }
    let model = fx_model(block).ok_or("Wait for the current FX model to load")?;
    let effect = with(|state| {
        state
            .config()
            .model_presets
            .iter()
            .find(|collection| collection.model == model)
            .and_then(|collection| collection.presets.get(index))
            .and_then(Option::as_ref)
            .map(|preset| preset.effect.clone())
    })
    .ok_or("This FX preset slot is empty")?;
    let current = view();
    if !current.idle || !current.amp.valid {
        return Err("Wait for the current Nano preset to load");
    }
    let plan = Plan::effect(effect, block, current.preset, crate::clock::now_us())?;
    with(|state| {
        state.pending = Some(Pending::Effect { index, block, plan });
        state.fx_selected[block] = None;
        state.fx_dirty[block] = false;
        state.notice = "Recalling FX preset and verifying...";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
}

pub fn rename_fx(index: usize, model: &[u8], name: &str) -> Result<(), &'static str> {
    let error = data::name_error(name);
    if !error.is_empty() { return Err(error); }
    edit(|config| {
        let preset = config.model_presets.iter_mut()
            .find(|collection| collection.model == model)
            .and_then(|collection| collection.presets.get_mut(index))
            .and_then(Option::as_mut)
            .ok_or("This FX preset slot is empty")?;
        preset.name = name.into();
        Ok(())
    })
}
