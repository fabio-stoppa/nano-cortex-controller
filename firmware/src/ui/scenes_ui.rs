use super::AppWindow;
use crate::link::Cmd;
use alloc::{format, string::String};
use core::sync::atomic::{AtomicU32, Ordering};
use slint::ComponentHandle;

static SHOWN: AtomicU32 = AtomicU32::new(0);

fn outcome(result: Result<(), &'static str>) {
    if let Err(error) = result {
        crate::scenes::report(error);
    }
}

pub(super) fn refresh(app: &AppWindow, force: bool) {
    let version = crate::scenes::version();
    if !force && SHOWN.swap(version, Ordering::Relaxed) == version {
        return;
    }
    SHOWN.store(version, Ordering::Relaxed);
    app.set_scene_library_notice(crate::scenes::notice().into());
    app.set_scene_library_busy(crate::scenes::busy());
    app.set_scene_dirty(crate::scenes::scene_dirty());
    let names = crate::state::nano_state_preset_names();
    let scene_preset = app.get_scene_preset();
    let name = scene_preset
        .checked_sub(1)
        .and_then(|index| names.get(index as usize))
        .map_or("", String::as_str);
    app.set_scene_title(format!("{scene_preset}: {name} scenes").into());
    let preset = crate::state::nano_state_preset_slot();
    let current = crate::scenes::scenes(usize::from(preset));
    let selected = crate::scenes::scene();
    let label = selected.and_then(|index| current.get(index).and_then(Option::as_deref));
    app.set_scene_label(
        match label {
            Some(name) => format!(
                "{}{}",
                name,
                if crate::scenes::scene_dirty() {
                    " *"
                } else {
                    ""
                }
            ),
            None => String::from("Scene"),
        }
        .into(),
    );
    app.set_gig_scene(selected.map_or(-1, |index| index as i32));
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
    let scenes = crate::scenes::scenes(app.get_scene_preset() as usize);
    app.set_scene_assigned(slint::ModelRc::new(slint::VecModel::from(
        scenes
            .iter()
            .map(Option::is_some)
            .collect::<alloc::vec::Vec<_>>(),
    )));
    app.set_scene_library_scenes(slint::ModelRc::new(slint::VecModel::from(
        scenes
            .into_iter()
            .map(|name| name.unwrap_or_else(|| String::from("(empty)")).into())
            .collect::<alloc::vec::Vec<slint::SharedString>>(),
    )));
    app.set_scene_library_scene(if app.get_scene_preset() == i32::from(preset) {
        selected.map_or(-1, |index| index as i32)
    } else {
        -1
    });
}

fn naming(app: &AppWindow, action: i32, text: &str) {
    app.set_scene_library_name_action(action);
    app.set_kbd_title(
        if action == 3 {
            "Save current sound as scene"
        } else {
            "Rename scene"
        }
        .into(),
    );
    app.set_kbd_text(text.into());
    app.set_kbd_error(crate::scenes::data::name_error(text).into());
    app.set_kbd_open(true);
}

pub(super) fn register(app: &AppWindow) {
    app.on_header_scene_action({
        let weak = app.as_weak();
        move |action| {
            let Some(app) = weak.upgrade() else { return; };
            if !crate::link::connected() || crate::scenes::busy() {
                crate::scenes::report("Scene action unavailable while disconnected or busy");
                return;
            }
            let Some(scene) = crate::scenes::scene() else {
                crate::scenes::report("Select a scene before editing it");
                return;
            };
            app.set_scene_preset(i32::from(crate::state::nano_state_preset_slot()));
            match action {
                0 => app.invoke_scene_library_scene_rename(scene as i32),
                1 => app.invoke_scene_library_scene_store(scene as i32),
                2 => app.invoke_scene_library_scene_clear(scene as i32),
                _ => crate::scenes::report("Invalid scene action"),
            }
            refresh(&app, true);
        }
    });
    app.on_gig_scene_recall(|index| crate::link::push_cmd(Cmd::SceneRecall(index as u8)));
    app.on_scene_library_opened({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                if !crate::link::connected() {
                    crate::scenes::report("Connect the Nano before opening scenes");
                    return;
                }
                app.set_scene_preset(i32::from(crate::state::nano_state_preset_slot()));
                app.set_scene_library_open(true);
                refresh(&app, true);
                crate::link::push_cmd(Cmd::SceneFxRefresh);
            }
        }
    });
    app.on_scene_library_scene_recall({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if app.get_scene_preset() != i32::from(crate::state::nano_state_preset_slot()) {
                    crate::scenes::report("Nano preset changed; reopen the scene selector");
                    return;
                }
                recall_scene(&app, index);
            }
        }
    });
    app.on_scene_library_scene_store({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                app.set_scene_library_edit_scene(index);
                let name = crate::scenes::scenes(app.get_scene_preset() as usize)
                    .get(index as usize)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| format!("Scene {}", index + 1));
                if crate::scenes::scenes(app.get_scene_preset() as usize)
                    .get(index as usize)
                    .is_some_and(Option::is_some)
                {
                    if crate::scenes::data::name_error(&name).is_empty() {
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
    app.on_scene_library_scene_clear({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                outcome(crate::scenes::clear_scene(
                    app.get_scene_preset() as usize,
                    index as usize,
                ));
            }
        }
    });
    app.on_scene_library_scene_rename({
        let weak = app.as_weak();
        move |index| {
            if let Some(app) = weak.upgrade() {
                if let Some(name) = crate::scenes::scenes(app.get_scene_preset() as usize)
                    .get(index as usize)
                    .cloned()
                    .flatten()
                {
                    app.set_scene_library_edit_scene(index);
                    naming(&app, 6, &name);
                } else {
                    crate::scenes::report("This scene slot is empty");
                }
            }
        }
    });
}

pub(super) fn accept_name(app: &AppWindow, name: &str) {
    let action = app.get_scene_library_name_action();
    app.set_scene_library_name_action(0);
    match action {
        3 => capture_scene(app),
        6 => outcome(crate::scenes::rename_scene(
            app.get_scene_preset() as usize,
            app.get_scene_library_edit_scene() as usize,
            name,
        )),
        _ => crate::scenes::report("Invalid scene naming action"),
    }
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
        scene: app.get_scene_library_edit_scene() as u8,
        name: String::from(app.get_kbd_text().as_str()),
    });
}
