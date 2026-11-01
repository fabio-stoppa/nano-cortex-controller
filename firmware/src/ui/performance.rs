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
}

fn naming(app: &AppWindow, action: i32, bank: i32, text: &str) {
    app.set_performance_name_action(action);
    app.set_performance_name_bank(bank);
    app.set_kbd_title(
        if action == 1 {
            "New bank"
        } else {
            "Rename bank"
        }
        .into(),
    );
    app.set_kbd_text(text.into());
    app.set_kbd_error(crate::performance::data::name_error(text).into());
    app.set_kbd_open(true);
}

pub(super) fn register(app: &AppWindow) {
    app.on_performance_opened({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                app.set_settings_open(false);
                app.set_performance_open(true);
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
}

pub(super) fn accept_name(app: &AppWindow, name: &str) {
    let action = app.get_performance_name_action();
    app.set_performance_name_action(0);
    outcome(match action {
        1 => crate::performance::add_bank(name),
        2 => crate::performance::rename_bank(app.get_performance_name_bank() as usize, name),
        _ => Err("Invalid naming action"),
    });
}
