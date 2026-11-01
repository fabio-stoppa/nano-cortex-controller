use super::AppWindow;
use alloc::string::String;
use slint::Model;

pub(super) fn refresh_preset_rows(app: &AppWindow, names: &[String]) {
    let kind = app.get_list_kind();
    if !app.get_model_open() || !(3..=5).contains(&kind) {
        return;
    }
    let rows = app.get_model_list();
    for row in 0..rows.row_count() {
        let Some(mut item) = rows.row_data(row) else {
            continue;
        };
        if item.header || item.index < 1 {
            continue;
        }
        let name = names
            .get(item.index as usize - 1)
            .map_or("", String::as_str);
        let label = if kind != 3 && name.is_empty() {
            "(empty)"
        } else {
            name
        };
        if item.name != label {
            item.name = label.into();
            rows.set_row_data(row, item);
        }
    }
}
