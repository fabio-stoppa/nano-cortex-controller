use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

#[path = "performance/data.rs"]
pub mod data;
#[path = "performance/restore.rs"]
pub mod restore;
use data::{Bank, Config, SLOTS};
use restore::{Plan, Progress, View};

enum Pending {
    Bank {
        preset: u8,
        ack: u32,
        requested: Option<u32>,
        deadline: i64,
    },
    Scene {
        index: usize,
        plan: Plan,
    },
    Effect {
        plan: Plan,
    },
}

struct State {
    config: Option<Config>,
    bank: usize,
    slot: Option<usize>,
    version: u32,
    notice: &'static str,
    scene: Option<usize>,
    pending: Option<Pending>,
}

struct Store(RefCell<State>);
// Touch, BLE and MIDI futures all run on the main executor task.
unsafe impl Sync for Store {}

static STATE: Store = Store(RefCell::new(State {
    config: None,
    bank: 0,
    slot: None,
    version: 1,
    notice: "",
    scene: None,
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
        .and_then(|json| json.as_deref().map(Config::decode).transpose());
    match config {
        Ok(config) => install(config.unwrap_or_default()),
        Err(error) => {
            with(|state| state.config = Some(Config::default()));
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
        state.bank = state.bank.min(config.banks.len().saturating_sub(1));
        state.config = Some(config);
        state.slot = None;
        state.scene = None;
        state.pending = None;
        state.notice = "Performance configuration saved";
        state.version = state.version.wrapping_add(1);
    });
}

pub fn export_json() -> Result<String, &'static str> {
    let config = with(|state| state.config().clone());
    config.encode()
}
pub fn validate_json(json: &str) -> Result<(), &'static str> {
    Config::decode(json).map(|_| ())
}
pub fn import_json(json: &str) -> Result<(), &'static str> {
    let config = Config::decode(json)?;
    crate::settings::save_performance_json(json)?;
    install(config);
    Ok(())
}

fn edit(f: impl FnOnce(&mut Config) -> Result<(), &'static str>) -> Result<(), &'static str> {
    if busy() {
        return Err("Wait for the current recall to finish");
    }
    let mut changed = with(|state| state.config().clone());
    f(&mut changed)?;
    let json = changed.encode()?;
    crate::settings::save_performance_json(&json)?;
    install(changed);
    Ok(())
}

pub fn add_bank(name: &str) -> Result<(), &'static str> {
    edit(|config| {
        config.banks.push(Bank {
            name: String::from(name),
            slots: [None; SLOTS],
            scenes: Default::default(),
        });
        Ok(())
    })?;
    with(|state| state.bank = state.config().banks.len() - 1);
    Ok(())
}
pub fn rename_bank(index: usize, name: &str) -> Result<(), &'static str> {
    edit(|config| {
        config
            .banks
            .get_mut(index)
            .ok_or("Bank no longer exists")?
            .name = String::from(name);
        Ok(())
    })
}
pub fn delete_bank(index: usize) -> Result<(), &'static str> {
    edit(|config| {
        if index >= config.banks.len() {
            return Err("Bank no longer exists");
        }
        config.banks.remove(index);
        Ok(())
    })
}
pub fn assign_slot(slot: usize, preset: Option<u8>) -> Result<(), &'static str> {
    let bank = bank();
    edit(|config| {
        let target = config
            .banks
            .get_mut(bank)
            .ok_or("Create a bank first")?
            .slots
            .get_mut(slot)
            .ok_or("Invalid bank slot")?;
        let changed = *target != preset;
        *target = preset;
        if changed {
            config.banks[bank].scenes[slot] = Default::default();
        }
        Ok(())
    })
}
pub fn select_bank(index: usize) -> Result<(), &'static str> {
    if busy() {
        return Err("Wait for the current recall to finish");
    }
    with(|state| {
        if index >= state.config().banks.len() {
            return Err("Bank no longer exists");
        }
        state.bank = index;
        state.slot = None;
        state.scene = None;
        state.notice = "";
        state.version = state.version.wrapping_add(1);
        Ok(())
    })
}
pub fn step_bank(delta: i8) -> Result<(), &'static str> {
    let next = with(|state| {
        let count = state.config().banks.len();
        if count == 0 {
            return Err("Create a bank first");
        }
        Ok((state.bank as isize + isize::from(delta)).rem_euclid(count as isize) as usize)
    })?;
    select_bank(next)
}
pub fn recall_slot(slot: usize) -> Result<u8, &'static str> {
    if !crate::link::connected() {
        return Err("Connect the Nano before recalling a bank slot");
    }
    with(|state| {
        let bank = state.bank;
        let preset = state
            .config()
            .banks
            .get(bank)
            .ok_or("Create a bank first")?
            .slots
            .get(slot)
            .copied()
            .flatten()
            .ok_or("Assign a preset to this bank slot first")?;
        state.slot = Some(slot);
        state.scene = None;
        state.version = state.version.wrapping_add(1);
        Ok(preset)
    })
}
pub fn banks() -> Vec<String> {
    with(|state| {
        state
            .config()
            .banks
            .iter()
            .map(|bank| bank.name.clone())
            .collect()
    })
}
pub fn slots() -> [Option<u8>; SLOTS] {
    with(|state| {
        let bank = state.bank;
        state
            .config()
            .banks
            .get(bank)
            .map_or([None; SLOTS], |bank| bank.slots)
    })
}
pub fn bank() -> usize {
    with(|state| state.bank)
}
pub fn slot() -> Option<usize> {
    with(|state| state.slot)
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

pub fn cancel(reason: &'static str) {
    let cancelled = with(|state| {
        state.scene = None;
        let pending = state.pending.take();
        if matches!(pending, Some(Pending::Bank { .. })) {
            state.slot = None;
        }
        pending.is_some()
    });
    if cancelled {
        report(reason);
    }
}

pub fn start_bank(slot: usize) -> Result<u8, &'static str> {
    if busy() || crate::state::nano_rx_mode_get() != 0 {
        return Err("Wait for Nano state transfer to finish");
    }
    let preset = recall_slot(slot)?;
    with(|state| {
        state.pending = Some(Pending::Bank {
            preset,
            ack: crate::state::preset_ack_generation(),
            requested: None,
            deadline: crate::clock::now_us() + 30_000_000,
        });
        state.notice = "Loading bank preset...";
    });
    Ok(preset)
}

pub fn scenes(slot: usize) -> Vec<Option<String>> {
    with(|state| {
        let bank = state.bank;
        state
            .config()
            .banks
            .get(bank)
            .and_then(|bank| bank.scenes.get(slot))
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

pub fn capture_scene(
    bank: usize,
    slot: usize,
    index: usize,
    name: &str,
) -> Result<(), &'static str> {
    if !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Connect the Nano and exit tuner before saving a scene");
    }
    let expected = with(|state| {
        state
            .config()
            .banks
            .get(bank)
            .and_then(|bank| bank.slots.get(slot))
            .copied()
            .flatten()
    })
    .ok_or("Assign and play this bank slot before storing a scene")?;
    let current = view();
    if current.preset != expected {
        return Err("Play this bank slot before storing its scene");
    }
    let snapshot = data::Snapshot::capture(&current)?;
    edit(|config| {
        let scene = config
            .banks
            .get_mut(bank)
            .and_then(|bank| bank.scenes.get_mut(slot))
            .and_then(|scenes| scenes.get_mut(index))
            .ok_or("Invalid scene slot")?;
        *scene = Some(data::Scene {
            name: String::from(name),
            snapshot,
        });
        Ok(())
    })?;
    with(|state| {
        state.bank = bank;
        state.slot = Some(slot);
        state.scene = None;
    });
    Ok(())
}

pub fn clear_scene(slot: usize, index: usize) -> Result<(), &'static str> {
    let bank = bank();
    edit(|config| {
        let scene = config
            .banks
            .get_mut(bank)
            .and_then(|bank| bank.scenes.get_mut(slot))
            .and_then(|scenes| scenes.get_mut(index))
            .ok_or("Invalid scene slot")?;
        *scene = None;
        Ok(())
    })
}

pub fn recall_scene(index: usize) -> Result<(), &'static str> {
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Wait for recall, connect the Nano and exit tuner first");
    }
    let (preset, snapshot) = with(|state| -> Result<_, &'static str> {
        let bank = state.bank;
        let slot = state
            .slot
            .ok_or("Play a bank slot before recalling a scene")?;
        let bank = state
            .config()
            .banks
            .get(bank)
            .ok_or("Bank no longer exists")?;
        let scene = bank.scenes[slot]
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("This scene is empty")?;
        Ok((
            bank.slots[slot].ok_or("This bank slot is unassigned")?,
            scene.snapshot.clone(),
        ))
    })?;
    if view().preset != preset {
        return Err("Play this bank slot again; Nano preset has changed");
    }
    let plan = Plan::scene(snapshot, preset, crate::clock::now_us())?;
    with(|state| {
        state.pending = Some(Pending::Scene { index, plan });
        state.scene = None;
        state.notice = "Recalling scene and verifying...";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
}

pub fn poll() -> Option<crate::link::Cmd> {
    let mut pending = with(|state| state.pending.take())?;
    let current = view();
    let now = crate::clock::now_us();
    let result = match &mut pending {
        Pending::Bank {
            preset,
            ack,
            requested,
            deadline,
        } => {
            if now >= *deadline {
                Progress::Failed("Bank preset load timed out; selection not verified")
            } else if current.preset != *preset {
                Progress::Failed("Nano preset changed while loading bank slot")
            } else if let Some(generation) = requested {
                if current.generation != *generation && current.idle && current.amp.valid {
                    Progress::Complete
                } else {
                    Progress::Waiting
                }
            } else if crate::state::preset_ack_generation() != *ack && current.idle {
                *requested = Some(current.generation);
                Progress::Write(crate::link::Cmd::PerformanceRefresh)
            } else {
                Progress::Waiting
            }
        }
        Pending::Scene { plan, .. } | Pending::Effect { plan } => plan.poll(&current, now),
    };
    match result {
        Progress::Complete => {
            with(|state| {
                state.scene = match pending {
                    Pending::Scene { index, .. } => Some(index),
                    _ => None,
                };
                state.notice = "Recall verified";
                state.version = state.version.wrapping_add(1);
            });
            None
        }
        Progress::Failed(error) => {
            with(|state| {
                state.scene = None;
                if matches!(pending, Pending::Bank { .. }) {
                    state.slot = None;
                }
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

pub fn fx_presets() -> Vec<Option<data::FxPreset>> {
    with(|state| state.config().fx_presets.to_vec())
}

pub fn capture_fx(index: usize, block: usize, name: &str) -> Result<(), &'static str> {
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Wait for recall, connect the Nano and exit tuner before storing FX");
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
    let bank = bank();
    let slot = slot();
    edit(|config| {
        let target = config
            .fx_presets
            .get_mut(index)
            .ok_or("Invalid FX preset slot")?;
        *target = Some(data::FxPreset {
            name: name.into(),
            source_block: block as u8,
            effect,
        });
        Ok(())
    })?;
    with(|state| {
        state.bank = bank;
        state.slot = slot;
    });
    Ok(())
}

pub fn clear_fx(index: usize) -> Result<(), &'static str> {
    let bank = bank();
    let slot = slot();
    edit(|config| {
        *config
            .fx_presets
            .get_mut(index)
            .ok_or("Invalid FX preset slot")? = None;
        Ok(())
    })?;
    with(|state| {
        state.bank = bank;
        state.slot = slot;
    });
    Ok(())
}

pub fn recall_fx(index: usize, block: usize) -> Result<(), &'static str> {
    if busy() || !crate::link::connected() || crate::tuner::nano_tuner_is_active() {
        return Err("Wait for recall, connect the Nano and exit tuner before recalling FX");
    }
    let effect = with(|state| {
        state
            .config()
            .fx_presets
            .get(index)
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
        state.pending = Some(Pending::Effect { plan });
        state.scene = None;
        state.notice = "Recalling FX preset and verifying...";
        state.version = state.version.wrapping_add(1);
    });
    Ok(())
}
