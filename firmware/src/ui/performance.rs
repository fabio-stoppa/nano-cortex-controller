use super::AppWindow;
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
    let block = super::fx_block_for_chain(app.get_chain()).unwrap_or(0) as i32;
    let model = crate::performance::fx_model(block as usize).unwrap_or_default();
    let model_key = crate::performance::data::model_key(&model);
    let fx_selected = crate::performance::fx_selected(block as usize);
    let version = crate::performance::version();
    if !force
        && SHOWN.swap(version, Ordering::Relaxed) == version
        && app.get_fx_preset_block() == block
        && app.get_fx_preset_model().as_str() == model_key
    {
        return;
    }
    SHOWN.store(version, Ordering::Relaxed);
    app.set_fx_preset_block(block);
    app.set_fx_preset_model(model_key.into());
    app.set_performance_notice(crate::performance::notice().into());
    app.set_performance_busy(crate::performance::busy());
    app.set_scene_dirty(crate::performance::scene_dirty());
    let names = crate::state::nano_state_preset_names();
    let scene_preset = app.get_scene_preset();
    let name = scene_preset.checked_sub(1).and_then(|index| names.get(index as usize))
        .map_or("", String::as_str);
    app.set_scene_title(format!("{scene_preset} - {name} scenes").into());
    let preset = crate::state::nano_state_preset_slot();
    let current = crate::performance::scenes(usize::from(preset));
    let selected = crate::performance::scene();
    let label = selected.and_then(|index| current.get(index).and_then(Option::as_deref));
    app.set_scene_label(
        match label {
            Some(name) => format!(
                "{}{}",
                name,
                if crate::performance::scene_dirty() {
                    " *"
                } else {
                    ""
                }
            ),
            None => String::from("Scene"),
        }
        .into(),
    );
    app.set_gig_scene(crate::performance::scene().map_or(-1, |index| index as i32));
    app.set_gig_scene_assigned(slint::ModelRc::new(slint::VecModel::from(
        current
            .iter()
            .map(Option::is_some)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    let labels = |scenes: alloc::vec::Vec<Option<String>>| {
        slint::ModelRc::new(slint::VecModel::from(
            scenes
                .into_iter()
                .enumerate()
                .map(|(index, name)| {
                    format!("{}: {}", index + 1, name.as_deref().unwrap_or("(empty)")).into()
                })
                .collect::<alloc::vec::Vec<slint::SharedString>>(),
        ))
    };
    app.set_gig_scenes(labels(current));
    let scenes = crate::performance::scenes(app.get_scene_preset() as usize);
    app.set_scene_assigned(slint::ModelRc::new(slint::VecModel::from(
        scenes
            .iter()
            .map(Option::is_some)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    app.set_performance_scenes(slint::ModelRc::new(slint::VecModel::from(
        scenes.into_iter().map(|name| name.unwrap_or_else(|| String::from("(empty)")).into())
            .collect::<alloc::vec::Vec<slint::SharedString>>(),
    )));
    app.set_performance_scene(if app.get_scene_preset() == i32::from(preset) {
        selected.map_or(-1, |index| index as i32)
    } else {
        -1
    });
    let presets = crate::performance::fx_presets(&model);
    app.set_fx_preset_assigned(slint::ModelRc::new(slint::VecModel::from(
        presets
            .iter()
            .map(Option::is_some)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    app.set_fx_presets(slint::ModelRc::new(slint::VecModel::from(
        presets
            .into_iter()
            .enumerate()
            .map(|(index, preset)| {
                preset
                    .map_or_else(|| format!("+ {}", index + 1), |preset| preset.name)
                    .into()
            })
            .collect::<alloc::vec::Vec<slint::SharedString>>(),
    )));
    app.set_fx_preset_selected(
        fx_selected.map_or(-1, |index| index as i32),
    );
    app.set_fx_preset_dirty(crate::performance::fx_dirty(block as usize));
}

fn stored_model(app: &AppWindow) -> Result<alloc::vec::Vec<u8>, &'static str> {
    model_for_key(app.get_fx_store_model().as_str())
}

fn model_for_key(key: &str) -> Result<alloc::vec::Vec<u8>, &'static str> {
    crate::fx::catalog::catalog().iter()
        .find(|model| crate::performance::data::model_key(model.id_bytes()) == key)
        .map(|model| model.id_bytes().to_vec())
        .ok_or("FX preset model is not loaded")
}

fn naming(app: &AppWindow, action: i32, text: &str) {
    app.set_performance_name_action(action);
    app.set_kbd_title(
        if action == 3 {
            "Save current sound as scene"
        } else if action == 6 {
            "Rename scene"
        } else if action == 5 {
            "Rename effect preset"
        } else {
            "Save current effect preset"
        }
        .into(),
    );
    app.set_kbd_text(text.into());
    app.set_kbd_error(crate::performance::data::name_error(text).into());
    app.set_kbd_open(true);
}

pub(super) fn register(app: &AppWindow) {
    app.on_gig_scene_recall(|index| crate::link::push_cmd(Cmd::SceneRecall(index as u8)));
    app.on_performance_opened({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                if !crate::link::connected() {
                    crate::performance::report("Connect the Nano before opening scenes");
                    return;
                }
                app.set_scene_preset(i32::from(crate::state::nano_state_preset_slot()));
                app.set_performance_open(true);
                refresh(&app, true);
                crate::link::push_cmd(Cmd::PerformanceFxRefresh);
            }
        }
    });
    app.on_fx_preset_recall({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                let model = match model_for_key(app.get_fx_preset_model().as_str()) {
                    Ok(model) => model,
                    Err(error) => { crate::performance::report(error); return; }
                };
                crate::link::push_cmd(Cmd::FxPresetRecall {
                    index: index as u8,
                    block: app.get_fx_preset_block() as u8,
                    model: Some(model),
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
                app.set_fx_store_block(app.get_fx_preset_block());
                app.set_fx_store_model(app.get_fx_preset_model());
                app.set_fx_store_preset(i32::from(crate::state::nano_state_preset_slot()));
                crate::link::push_cmd(Cmd::PerformanceFxRefresh);
                let model = match stored_model(&app) {
                    Ok(model) => model,
                    Err(error) => { crate::performance::report(error); return; }
                };
                let presets = crate::performance::fx_presets(&model);
                let name = presets
                    .get(index as usize)
                    .and_then(Option::as_ref)
                    .map_or_else(|| format!("FX {}", index + 1), |preset| preset.name.clone());
                if presets
                    .get(index as usize).is_some_and(Option::is_some)
                    && crate::performance::data::name_error(&name).is_empty()
                {
                    app.set_kbd_text(name.into());
                    confirmation(&app, 6, "Save FX preset changes?",
                        "Replace this model preset with the current parameters and bypass. All slots using it share this preset.");
                } else {
                    naming(&app, 4, &name);
                }
            }
        }
    });
    app.on_fx_preset_rename({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_fx_preset_edit(index);
                app.set_fx_store_block(app.get_fx_preset_block());
                app.set_fx_store_model(app.get_fx_preset_model());
                let model = match stored_model(&app) {
                    Ok(model) => model,
                    Err(error) => { crate::performance::report(error); return; }
                };
                if let Some(preset) = crate::performance::fx_presets(&model)
                    .get(index as usize).and_then(Option::as_ref)
                {
                    naming(&app, 5, &preset.name);
                } else {
                    crate::performance::report("This FX preset slot is empty");
                }
            }
        }
    });
    app.on_fx_preset_delete({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_fx_preset_edit(index);
                app.set_fx_store_block(app.get_fx_preset_block());
                app.set_fx_store_model(app.get_fx_preset_model());
                confirmation(
                    &app,
                    7,
                    "Delete FX preset?",
                    "Remove this model preset from all slots on the controller. The current sound is unchanged.",
                );
            }
        }
    });
    app.on_performance_scene_recall({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if app.get_scene_preset() != i32::from(crate::state::nano_state_preset_slot()) {
                    crate::performance::report("Nano preset changed; reopen the scene selector");
                    return;
                }
                recall_scene(&app, index);
            }
        }
    });
    app.on_performance_scene_store({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_performance_edit_scene(index);
                let name = crate::performance::scenes(app.get_scene_preset() as usize)
                    .get(index as usize)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| format!("Scene {}", index + 1));
                if crate::performance::scenes(app.get_scene_preset() as usize)
                    .get(index as usize).is_some_and(Option::is_some)
                {
                    if crate::performance::data::name_error(&name).is_empty() {
                        app.set_kbd_text(name.into());
                        capture_scene(&app);
                    } else {
                        naming(&app, 3, &name);
                    }
                } else {
                    naming(&app, 3, &name);
                }
            }
        }
    });
    app.on_performance_scene_clear({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                outcome(crate::performance::clear_scene(app.get_scene_preset() as usize, index as usize));
            }
        }
    });
    app.on_performance_scene_rename({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if let Some(name) = crate::performance::scenes(app.get_scene_preset() as usize)
                    .get(index as usize).cloned().flatten()
                {
                    app.set_performance_edit_scene(index);
                    naming(&app, 6, &name);
                } else {
                    crate::performance::report("This scene slot is empty");
                }
            }
        }
    });
}

pub(super) fn accept_name(app: &AppWindow, name: &str) {
    let action = app.get_performance_name_action();
    app.set_performance_name_action(0);
    if action == 3 {
        capture_scene(app);
        return;
    }
    if action == 6 {
        outcome(crate::performance::rename_scene(
            app.get_scene_preset() as usize, app.get_performance_edit_scene() as usize, name,
        ));
        return;
    }
    if action == 5 {
        outcome(stored_model(app).and_then(|model| crate::performance::rename_fx(
            app.get_fx_preset_edit() as usize, &model, name,
        )));
        return;
    }
    let existing = match action {
        4 => match stored_model(app) {
            Ok(model) => crate::performance::fx_presets(&model)
                .get(app.get_fx_preset_edit() as usize).is_some_and(Option::is_some),
            Err(error) => { crate::performance::report(error); return; }
        },
        _ => {
            crate::performance::report("Invalid performance naming action");
            return;
        }
    };
    if existing {
        confirmation(app, 6, "Replace saved sound?",
            "Replace this controller snapshot with the current sound. The Nano preset is not saved.");
    } else if action == 4 {
        capture_fx(app);
    } else {
        capture_scene(app);
    }
}

fn confirmation(app: &AppWindow, action: i32, title: &str, message: &str) {
    app.set_performance_confirm_action(action);
    app.set_confirm_title(title.into());
    app.set_confirm_message(message.into());
    app.set_confirm_label(
        if action == 4 || action == 7 {
            "Delete"
        } else {
            "Save"
        }
        .into(),
    );
    app.set_confirm_danger(true);
    app.set_confirm_open(true);
}

fn capture_fx(app: &AppWindow) {
    let model = match stored_model(app) {
        Ok(model) => model,
        Err(error) => { crate::performance::report(error); return; }
    };
    crate::link::push_cmd(Cmd::FxPresetCapture {
        index: app.get_fx_preset_edit() as u8,
        block: app.get_fx_store_block() as u8,
        preset: app.get_fx_store_preset() as u8,
        name: app.get_kbd_text().as_str().into(),
        model,
    });
}

fn recall_scene(app: &AppWindow, index: i32) {
    crate::link::push_cmd(Cmd::SceneRecallAt {
        preset: app.get_scene_preset() as u8,
        scene: index as u8,
    });
}

fn capture_scene(app: &AppWindow) {
    crate::link::push_cmd(Cmd::SceneCapture {
        preset: app.get_scene_preset() as u8,
        scene: app.get_performance_edit_scene() as u8,
        name: String::from(app.get_kbd_text().as_str()),
    });
}

pub(super) fn confirm(app: &AppWindow) -> bool {
    let action = app.get_performance_confirm_action();
    app.set_performance_confirm_action(0);
    match action {
        6 => capture_fx(app),
        7 => outcome(stored_model(app).and_then(|model| crate::performance::clear_fx(
            app.get_fx_preset_edit() as usize, &model,
        ))),
        _ => return false,
    }
    true
}
