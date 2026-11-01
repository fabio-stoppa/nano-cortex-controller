use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

#[path = "performance/data.rs"]
pub mod data;
use data::{Bank, Config, SLOTS};

struct State {
    config: Option<Config>,
    bank: usize,
    slot: Option<usize>,
    version: u32,
    notice: &'static str,
}

struct Store(RefCell<State>);
// Touch, BLE and MIDI futures all run on the main executor task.
unsafe impl Sync for Store {}

static STATE: Store = Store(RefCell::new(State {
    config: None, bank: 0, slot: None, version: 1, notice: "",
}));

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R { f(&mut STATE.0.borrow_mut()) }

impl State {
    fn config(&mut self) -> &mut Config { self.config.get_or_insert_with(Config::default) }
}

pub fn load() {
    let config = crate::settings::load_performance_json()
        .and_then(|json| json.as_deref().map(Config::decode).transpose());
    match config {
        Ok(config) => with(|state| state.config = Some(config.unwrap_or_default())),
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
        state.notice = "Performance configuration saved";
        state.version = state.version.wrapping_add(1);
    });
}

pub fn export_json() -> Result<String, &'static str> {
    let config = with(|state| state.config().clone());
    config.encode()
}
pub fn validate_json(json: &str) -> Result<(), &'static str> { Config::decode(json).map(|_| ()) }
pub fn import_json(json: &str) -> Result<(), &'static str> {
    let config = Config::decode(json)?;
    crate::settings::save_performance_json(json)?;
    install(config);
    Ok(())
}

fn edit(f: impl FnOnce(&mut Config) -> Result<(), &'static str>) -> Result<(), &'static str> {
    let mut changed = with(|state| state.config().clone());
    f(&mut changed)?;
    let json = changed.encode()?;
    crate::settings::save_performance_json(&json)?;
    install(changed);
    Ok(())
}

pub fn add_bank(name: &str) -> Result<(), &'static str> {
    edit(|config| {
        config.banks.push(Bank { name: String::from(name), slots: [None; SLOTS] });
        Ok(())
    })?;
    with(|state| state.bank = state.config().banks.len() - 1);
    Ok(())
}
pub fn rename_bank(index: usize, name: &str) -> Result<(), &'static str> {
    edit(|config| {
        config.banks.get_mut(index).ok_or("Bank no longer exists")?.name = String::from(name);
        Ok(())
    })
}
pub fn delete_bank(index: usize) -> Result<(), &'static str> {
    edit(|config| {
        if index >= config.banks.len() { return Err("Bank no longer exists"); }
        config.banks.remove(index);
        Ok(())
    })
}
pub fn assign_slot(slot: usize, preset: Option<u8>) -> Result<(), &'static str> {
    let bank = bank();
    edit(|config| {
        let target = config.banks.get_mut(bank).ok_or("Create a bank first")?
            .slots.get_mut(slot).ok_or("Invalid bank slot")?;
        *target = preset;
        Ok(())
    })
}
pub fn select_bank(index: usize) -> Result<(), &'static str> {
    with(|state| {
        if index >= state.config().banks.len() { return Err("Bank no longer exists"); }
        state.bank = index;
        state.slot = None;
        state.notice = "";
        state.version = state.version.wrapping_add(1);
        Ok(())
    })
}
pub fn step_bank(delta: i8) -> Result<(), &'static str> {
    let next = with(|state| {
        let count = state.config().banks.len();
        if count == 0 { return Err("Create a bank first"); }
        Ok((state.bank as isize + isize::from(delta)).rem_euclid(count as isize) as usize)
    })?;
    select_bank(next)
}
pub fn recall_slot(slot: usize) -> Result<u8, &'static str> {
    if !crate::link::connected() { return Err("Connect the Nano before recalling a bank slot"); }
    with(|state| {
        let bank = state.bank;
        let preset = state.config().banks.get(bank).ok_or("Create a bank first")?
            .slots.get(slot).copied().flatten().ok_or("Assign a preset to this bank slot first")?;
        state.slot = Some(slot);
        state.version = state.version.wrapping_add(1);
        Ok(preset)
    })
}
pub fn banks() -> Vec<String> { with(|state| state.config().banks.iter().map(|bank| bank.name.clone()).collect()) }
pub fn slots() -> [Option<u8>; SLOTS] {
    with(|state| {
        let bank = state.bank;
        state.config().banks.get(bank).map_or([None; SLOTS], |bank| bank.slots)
    })
}
pub fn bank() -> usize { with(|state| state.bank) }
pub fn slot() -> Option<usize> { with(|state| state.slot) }
pub fn notice() -> &'static str { with(|state| state.notice) }
pub fn version() -> u32 { with(|state| state.version) }
