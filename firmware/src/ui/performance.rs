use super::{list_columns, AppWindow, ListRows};
use crate::link::Cmd;
use alloc::{format, string::String};
use core::sync::atomic::{AtomicU32, Ordering};
use slint::ComponentHandle;

static SHOWN: AtomicU32 = AtomicU32::new(0);

fn outcome(result: Result<(), &'static str>) {
    if let Err(error) = result {
        crate::performance::report(error);
    }
}

pub(super) fn refresh(app: &AppWindow, force: bool) {
    let version = crate::performance::version();
    if !force && SHOWN.swap(version, Ordering::Relaxed) == version {
        return;
    }
    SHOWN.store(version, Ordering::Relaxed);
    app.set_performance_banks(slint::ModelRc::new(slint::VecModel::from(
        crate::performance::banks()
            .into_iter()
            .map(slint::SharedString::from)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    let names = crate::state::nano_state_preset_names();
    let slots = crate::performance::slots()
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let label = match preset {
                Some(preset) => format!(
                    "{}: {} {}",
                    index + 1,
                    preset,
                    names
                        .get(usize::from(*preset) - 1)
                        .map_or("", String::as_str)
                ),
                None => format!("{}: (unassigned)", index + 1),
            };
            slint::SharedString::from(label)
        })
        .collect::<alloc::vec::Vec<_>>();
    app.set_performance_slots(slint::ModelRc::new(slint::VecModel::from(slots)));
    app.set_performance_bank(crate::performance::bank() as i32);
    app.set_performance_slot(crate::performance::slot().map_or(-1, |index| index as i32));
    app.set_performance_notice(crate::performance::notice().into());
    app.set_performance_busy(crate::performance::busy());
    app.set_gig_assigned(slint::ModelRc::new(slint::VecModel::from(
        crate::performance::slots()
            .iter()
            .map(Option::is_some)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    let scenes = crate::performance::scenes(crate::performance::slot().unwrap_or(usize::MAX));
    app.set_gig_scene_assigned(slint::ModelRc::new(slint::VecModel::from(
        scenes
            .iter()
            .map(Option::is_some)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    app.set_gig_scenes(slint::ModelRc::new(slint::VecModel::from(
        scenes
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                slint::SharedString::from(format!(
                    "{}: {}",
                    index + 1,
                    name.as_deref().unwrap_or("(empty)")
                ))
            })
            .collect::<alloc::vec::Vec<_>>(),
    )));
    app.set_gig_scene(crate::performance::scene().map_or(-1, |index| index as i32));
    let presets = crate::performance::fx_presets()
        .into_iter()
        .enumerate()
        .map(|(index, preset)| {
            let label = match preset {
                Some(preset) => format!(
                    "{}: {}{}",
                    index + 1,
                    preset.name,
                    if preset
                        .effect
                        .validate(app.get_fx_preset_block() as usize)
                        .is_err()
                    {
                        " (incompatible)"
                    } else {
                        ""
                    }
                ),
                None => format!("{}: (empty)", index + 1),
            };
            slint::SharedString::from(label)
        })
        .collect::<alloc::vec::Vec<_>>();
    app.set_fx_presets(slint::ModelRc::new(slint::VecModel::from(presets)));
    let slot = app.get_performance_edit_slot();
    let scenes = crate::performance::scenes(slot as usize)
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            format!("{}: {}", index + 1, name.as_deref().unwrap_or("(empty)")).into()
        })
        .collect::<alloc::vec::Vec<slint::SharedString>>();
    app.set_performance_scenes(slint::ModelRc::new(slint::VecModel::from(scenes)));
    app.set_performance_scene(if slot == app.get_performance_slot() {
        crate::performance::scene().map_or(-1, |index| index as i32)
    } else {
        -1
    });
}

fn naming(app: &AppWindow, action: i32, bank: i32, text: &str) {
    app.set_performance_name_action(action);
    app.set_performance_name_bank(bank);
    app.set_kbd_title(
        match action {
            1 => "New bank",
            3 => "Store current sound as scene",
            4 => "Store selected block as FX preset",
            _ => "Rename bank",
        }
        .into(),
    );
    app.set_kbd_text(text.into());
    app.set_kbd_error(crate::performance::data::name_error(text).into());
    app.set_kbd_open(true);
}

pub(super) fn register(app: &AppWindow) {
    app.on_gig_bank_stepped(|delta| {
        outcome(crate::performance::step_bank(delta.clamp(-1, 1) as i8))
    });
    app.on_gig_scene_recall(|index| {
        if !crate::link::connected() {
            crate::performance::report("Connect the Nano before recalling a scene");
            return;
        }
        crate::link::push_cmd(Cmd::SceneRecall(index as u8));
    });
    app.on_fx_presets_opened({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                app.set_fx_preset_block(
                    super::fx_block_for_chain(app.get_chain()).unwrap_or(0) as i32
                );
                app.set_fx_presets_open(true);
                refresh(&app, true);
                if crate::link::connected() {
                    crate::link::push_cmd(Cmd::PerformanceFxRefresh);
                }
            }
        }
    });
    app.on_fx_preset_block_selected({
        let weak = app.as_weak();
        move |block| {
            if let Some(app) = weak.upgrade() {
                app.set_fx_preset_block(block);
                refresh(&app, true);
            }
        }
    });
    app.on_fx_preset_recall({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if !crate::link::connected() {
                    crate::performance::report("Connect the Nano before recalling FX");
                    return;
                }
                crate::link::push_cmd(Cmd::FxPresetRecall {
                    index: index as u8,
                    block: app.get_fx_preset_block() as u8,
                });
            }
        }
    });
    app.on_fx_preset_store({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if !crate::link::connected() {
                    crate::performance::report("Connect the Nano before storing FX");
                    return;
                }
                app.set_fx_preset_edit(index);
                let name = crate::performance::fx_presets()
                    .get(index as usize)
                    .and_then(Option::as_ref)
                    .map_or_else(|| format!("FX {}", index + 1), |preset| preset.name.clone());
                naming(&app, 4, -1, &name);
            }
        }
    });
    app.on_fx_preset_delete({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_fx_preset_edit(index);
                confirmation(
                    &app,
                    7,
                    "Delete FX preset?",
                    "Only this controller FX preset is removed.",
                );
            }
        }
    });
    app.on_performance_opened({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                app.set_settings_open(false);
                app.set_performance_open(true);
                app.set_performance_edit_slot(-1);
                refresh(&app, true);
            }
        }
    });
    app.on_performance_add({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                naming(&app, 1, -1, "");
            }
        }
    });
    app.on_performance_rename({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if let Some(name) = crate::performance::banks().get(index as usize) {
                    naming(&app, 2, index, name);
                } else {
                    crate::performance::report("Bank no longer exists");
                }
            }
        }
    });
    app.on_performance_selected(|index| outcome(crate::performance::select_bank(index as usize)));
    app.on_performance_clear(|index| {
        outcome(crate::performance::assign_slot(index as usize, None))
    });
    app.on_performance_recall(|index| {
        if !crate::link::connected() {
            crate::performance::report("Connect the Nano before recalling a bank slot");
        } else {
            crate::link::push_cmd(Cmd::BankSlot(index as u8));
        }
    });
    app.on_performance_delete({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_performance_delete_bank(index);
                app.set_confirm_title("Delete bank?".into());
                app.set_confirm_message(
                    "Controller assignments will be removed. Nano presets are unchanged.".into(),
                );
                app.set_confirm_label("Delete".into());
                app.set_confirm_danger(true);
                app.set_confirm_open(true);
            }
        }
    });
    app.on_performance_assign({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_performance_assign_slot(index);
                let names = crate::state::nano_state_preset_names();
                let mut rows = ListRows::new(list_columns(&app));
                rows.header("Preset slots");
                rows.item("(unassigned)", 0);
                for preset in 1..=64 {
                    rows.item(
                        &format!(
                            "{} {}",
                            preset,
                            names.get(preset - 1).map_or("(empty)", String::as_str)
                        ),
                        preset as i32,
                    );
                }
                rows.show(&app, 6, "Assign bank slot", -1);
            }
        }
    });
    app.on_performance_scenes_opened({
        let weak = app.as_weak();
        move |slot| {
            if let Some(app) = weak.upgrade() {
                app.set_performance_edit_slot(slot);
                refresh(&app, true);
                if crate::link::connected() {
                    crate::link::push_cmd(Cmd::PerformanceFxRefresh);
                }
            }
        }
    });
    app.on_performance_scene_recall({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                let slot = app.get_performance_edit_slot();
                if !crate::link::connected() {
                    crate::performance::report("Connect the Nano before recalling a scene");
                    return;
                }
                if crate::performance::scenes(slot as usize)
                    .get(index as usize)
                    .is_none_or(Option::is_none)
                {
                    crate::performance::report("This scene is empty");
                    return;
                }
                if crate::performance::slot() != Some(slot as usize) {
                    crate::link::push_cmd(Cmd::BankSlot(slot as u8));
                }
                crate::link::push_cmd(Cmd::SceneRecall(index as u8));
            }
        }
    });
    app.on_performance_scene_store({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_performance_edit_scene(index);
                let name = crate::performance::scenes(app.get_performance_edit_slot() as usize)
                    .get(index as usize)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| format!("Scene {}", index + 1));
                naming(&app, 3, app.get_performance_bank(), &name);
            }
        }
    });
    app.on_performance_scene_clear({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_performance_edit_scene(index);
                confirmation(
                    &app,
                    4,
                    "Delete scene?",
                    "Only this controller snapshot is removed.",
                );
            }
        }
    });
}

pub(super) fn accept_name(app: &AppWindow, name: &str) {
    let action = app.get_performance_name_action();
    app.set_performance_name_action(0);
    if action == 4 {
        let existing = crate::performance::fx_presets()
            .get(app.get_fx_preset_edit() as usize)
            .is_some_and(Option::is_some);
        if existing {
            confirmation(app, 6, "Replace FX preset?", "Replace this shelf slot with the selected block's current model, parameters and bypass.");
        } else {
            capture_fx(app);
        }
        return;
    }
    if action == 3 {
        let existing = crate::performance::scenes(app.get_performance_edit_slot() as usize)
            .get(app.get_performance_edit_scene() as usize)
            .is_some_and(Option::is_some);
        if existing {
            confirmation(
                app,
                3,
                "Replace scene?",
                "Replace this saved snapshot with the current Nano sound.",
            );
        } else {
            capture_scene(app);
        }
        return;
    }
    outcome(match action {
        1 => crate::performance::add_bank(name),
        2 => crate::performance::rename_bank(app.get_performance_name_bank() as usize, name),
        _ => Err("Invalid naming action"),
    });
}

fn confirmation(app: &AppWindow, action: i32, title: &str, message: &str) {
    app.set_performance_delete_bank(-1);
    app.set_performance_confirm_action(action);
    app.set_confirm_title(title.into());
    app.set_confirm_message(message.into());
    app.set_confirm_label(
        if action == 4 || action == 7 {
            "Delete"
        } else {
            "Replace"
        }
        .into(),
    );
    app.set_confirm_danger(true);
    app.set_confirm_open(true);
}

fn capture_fx(app: &AppWindow) {
    crate::link::push_cmd(Cmd::FxPresetCapture {
        index: app.get_fx_preset_edit() as u8,
        block: app.get_fx_preset_block() as u8,
        name: app.get_kbd_text().as_str().into(),
    });
}

fn capture_scene(app: &AppWindow) {
    if !crate::link::connected() {
        crate::performance::report("Connect the Nano before storing a scene");
        return;
    }
    crate::link::push_cmd(Cmd::SceneCapture {
        bank: app.get_performance_name_bank() as u8,
        slot: app.get_performance_edit_slot() as u8,
        scene: app.get_performance_edit_scene() as u8,
        name: String::from(app.get_kbd_text().as_str()),
    });
}

pub(super) fn assign(app: &AppWindow, preset: i32) {
    let slot = app.get_performance_assign_slot() as usize;
    let target = if preset == 0 {
        None
    } else {
        Some(preset as u8)
    };
    let existing = crate::performance::slots().get(slot).copied().flatten();
    if existing != target && crate::performance::scenes(slot).iter().any(Option::is_some) {
        app.set_performance_pending_preset(preset);
        confirmation(app, 5, "Replace bank assignment?", "Changing this preset assignment clears its four saved scenes. Nano presets are unchanged.");
    } else {
        outcome(crate::performance::assign_slot(slot, target));
    }
}

pub(super) fn confirm(app: &AppWindow) -> bool {
    let action = app.get_performance_confirm_action();
    app.set_performance_confirm_action(0);
    match action {
        6 => capture_fx(app),
        7 => outcome(crate::performance::clear_fx(
            app.get_fx_preset_edit() as usize
        )),
        3 => capture_scene(app),
        4 => outcome(crate::performance::clear_scene(
            app.get_performance_edit_slot() as usize,
            app.get_performance_edit_scene() as usize,
        )),
        5 => {
            let preset = app.get_performance_pending_preset();
            outcome(crate::performance::assign_slot(
                app.get_performance_assign_slot() as usize,
                if preset == 0 {
                    None
                } else {
                    Some(preset as u8)
                },
            ));
        }
        _ => return false,
    }
    true
}
