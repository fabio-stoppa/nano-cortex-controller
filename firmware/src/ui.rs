//! Slint screens for the 800×480 panel.
//!
//! The layout follows the LVGL UI: header (with the tap-tempo readout), chain
//! rail, amp / effect / tuner pages, and the connecting overlay. Landscape is the panel's scan.
//! Portrait is a taller layout rotated into that same framebuffer.

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use core::sync::atomic::{AtomicU8, Ordering};
use core::cell::UnsafeCell;

use core::cell::RefCell;
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, RenderingRotation, RepaintBufferType, Rgb565Pixel,
};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant};
use slint::{LogicalPosition, PhysicalSize};

use nano_wire::editor::CabFloat;
use nano_wire::editor::{
    ExpBypass, ExpBypassTarget, ExpRange, ExpRangeTarget, ExpressionAssignments,
};

use crate::bounce::{Framebuffer, HEIGHT, WIDTH};
use crate::fx::types::NanoFxState;
use crate::link::Cmd;

slint::include_modules!();

mod lines;
mod presets;
mod performance;
use lines::PartialLines;

struct Backend {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for Backend {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        core::time::Duration::from_micros(
            esp_hal::time::Instant::now()
                .duration_since_epoch()
                .as_micros(),
        )
    }
}

struct State {
    window: Rc<MinimalSoftwareWindow>,
    app: AppWindow,
    rotation: RenderingRotation,
    pressed: bool,
    last_x: f32,
    last_y: f32,
    fx: NanoFxState,
    fx_valid: bool,
    amp_title: String,
    expression_slot: u8,
    expression: ExpressionAssignments,
}

struct Slot(UnsafeCell<Option<State>>);
unsafe impl Sync for Slot {}

static STATE: Slot = Slot(UnsafeCell::new(None));

fn state() -> &'static mut Option<State> {
    unsafe { &mut *STATE.0.get() }
}

pub fn frame(framebuffer: &mut Framebuffer) {
    ensure();
    redraw(framebuffer);
}

pub fn keyboard_open() -> bool {
    state().as_ref().is_some_and(|ui| ui.app.get_kbd_open())
}

pub fn pointer(framebuffer: &mut Framebuffer, point: Option<(u16, u16)>) {
    ensure();
    let ui = state().as_mut().unwrap();
    let window = ui.window.window();
    if let Some((x, y)) = point {
        let (x, y) = to_logical(ui.rotation, x, y);
        ui.last_x = x;
        ui.last_y = y;
        let position = LogicalPosition::new(x, y);
        let event = if ui.pressed {
            WindowEvent::PointerMoved { position }
        } else {
            WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            }
        };
        ui.pressed = true;
        window.dispatch_event(event);
    } else if ui.pressed {
        ui.pressed = false;
        window.dispatch_event(WindowEvent::PointerReleased {
            position: LogicalPosition::new(ui.last_x, ui.last_y),
            button: PointerEventButton::Left,
        });
    }
    redraw(framebuffer);
}

fn ensure() {
    if state().is_some() {
        return;
    }
    // One framebuffer, repainted in place. A second full frame in PSRAM
    // fights the bounce copy, and on this panel that shows up as flicker.
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    slint::platform::set_platform(Box::new(Backend {
        window: window.clone(),
    }))
    .expect("slint platform");

    let app = AppWindow::new().expect("slint ui");
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    if let Some(mode) = crate::settings::load_layout() {
        app.set_layout_mode(mode);
    }
    crate::midi::load();
    crate::performance::load();
    performance::register(&app);
    let weak = app.as_weak();
    app.on_chain_tapped({
        let weak = weak.clone();
        move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            leave_tools(&app);
            app.set_chain(index);
            if index == 2 {
                app.set_page(0);
                show_amp_title();
            } else if fx_block_for_chain(index).is_some() {
                app.set_page(1);
                let slot = fx_block_for_chain(index).unwrap_or(0) as u8;
                crate::link::push_cmd(Cmd::Focus(slot));
                if let Some(ui) = state().as_mut() {
                    paint_fx_page(ui);
                    ui.window.request_redraw();
                }
            }
        }
    });
    app.on_chain_held({
        let weak = weak.clone();
        move |index| {
            if fx_block_for_chain(index).is_none() {
                return;
            }
            let Some(app) = weak.upgrade() else {
                return;
            };
            app.invoke_chain_tapped(index);
            app.invoke_model_list_tapped();
        }
    });
    app.on_chain_double({
        let weak = weak.clone();
        move |index| {
            let Some(block) = fx_block_for_chain(index) else {
                return;
            };
            let Some(app) = weak.upgrade() else {
                return;
            };
            let on = !chain_is_on(&app, index);
            set_chain_on(&app, index, on);
            crate::link::push_cmd(Cmd::Block {
                slot: block as u8,
                on,
            });
        }
    });
    app.on_tuner_tapped({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if app.get_page() == 2 {
                app.set_tuner_active(false);
                crate::link::push_cmd(Cmd::Tuner(false));
                show_home(&app);
            } else {
                app.set_page(2);
                app.set_model("Tuner".into());
                app.set_tuner_active(true);
                crate::link::push_cmd(Cmd::Tuner(true));
            }
        }
    });
    app.on_tuner_reference_set({
        let weak = weak.clone();
        move |hz| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let reference_hz = (hz as f32).clamp(400.0, 480.0);
            if (app.get_tuner_reference_hz() - reference_hz).abs() < 0.5 {
                return;
            }
            app.set_tuner_reference_hz(reference_hz);
            app.set_tuner_reference(format!("{reference_hz:.0} Hz").into());
            crate::link::push_cmd(Cmd::TunerReference(reference_hz as u16));
        }
    });
    app.on_tuner_mute_tapped({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let muted = !app.get_tuner_muted();
            app.set_tuner_muted(muted);
            crate::link::push_cmd(Cmd::TunerOutputMute(muted));
        }
    });
    app.on_tools_opened({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if app.get_settings_open() {
                app.set_settings_open(false);
                return;
            }
            if app.get_page() == 2 {
                leave_tools(&app);
                show_home(&app);
            }
            MIDI_SHOWN.store(0, core::sync::atomic::Ordering::Relaxed);
            app.set_settings_open(true);
            crate::link::push_cmd(Cmd::ToolsOpen);
        }
    });
    app.on_tools_refresh(move || crate::link::push_cmd(Cmd::ToolsOpen));
    app.on_midi_closed(|| crate::midi::cancel_learn());
    app.on_midi_learn(|index| crate::midi::toggle_learn(index.max(0) as usize));
    app.on_midi_clear(|index| crate::midi::clear(index.max(0) as usize));
    app.on_midi_clear_all(crate::midi::clear_all);
    app.on_midi_usb_storage(crate::usb_storage::request);
    app.on_usb_storage_cancel(crate::usb_storage::cancel);
    app.on_usb_storage_restart(|| esp_hal::system::software_reset());
    app.on_tools_jack_selected(move |expression| {
        crate::link::push_cmd(Cmd::JackMode(if expression {
            nano_wire::types::JackMode::Expression
        } else {
            nano_wire::types::JackMode::Midi
        }));
    });
    app.on_tools_usb_set(move |amount| {
        let db = -40.0 + amount.clamp(0.0, 1.0) * 40.0;
        crate::link::push_cmd(Cmd::UsbPlayback(db));
    });
    app.on_tools_mute_set(move |input, muted| {
        crate::link::push_cmd(Cmd::Mute { input, muted });
    });
    app.on_tools_calibration_start(move || {
        crate::link::push_cmd(Cmd::ExpressionCalibrationStart);
    });
    app.on_tools_calibration_save(move || {
        crate::link::push_cmd(Cmd::ExpressionCalibrationSave);
    });
    app.on_tools_expression_range_toggle(|target| {
        edit_expression_range(target, 0, 0);
    });
    app.on_tools_expression_range_adjust(|target, field, delta| {
        edit_expression_range(target, field + 1, delta);
    });
    app.on_tools_expression_bypass_toggle(|target| {
        edit_expression_bypass(target, 0);
    });
    app.on_tools_expression_bypass_adjust(|target, action| {
        edit_expression_bypass(target, action + 1);
    });
    app.on_tools_expression_save(|| {
        if let Some(ui) = state().as_ref() {
            if ui.expression_slot != 0 {
                crate::link::push_cmd(Cmd::ExpressionSave {
                    slot: ui.expression_slot,
                    assignments: ui.expression.clone(),
                });
            }
        }
    });
    app.on_tempo_tapped(move || {
        crate::link::push_cmd(Cmd::Tempo);
    });
    app.on_preset_list_tapped({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                open_preset_list(&app);
            }
        }
    });
    app.on_capture_stepped(move |delta| {
        let delta = delta.clamp(-1, 1) as i8;
        if delta != 0 {
            crate::link::push_cmd(Cmd::Capture(delta));
        }
    });
    app.on_model_stepped({
        let weak = weak.clone();
        move |delta| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let Some(block) = fx_block_for_chain(app.get_chain()) else {
                return;
            };
            let delta = delta.clamp(-1, 1) as i8;
            if delta != 0 {
                crate::link::push_cmd(Cmd::Model {
                    block: block as u8,
                    delta,
                });
            }
        }
    });
    app.on_model_list_tapped({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if app.get_page() == 0 {
                crate::link::push_cmd(Cmd::Library);
                return;
            }
            let Some(block) = fx_block_for_chain(app.get_chain()) else {
                return;
            };
            let block = block as u32;
            let mut fx = NanoFxState::zeroed();
            if !crate::fx::nano_fx_snapshot(&mut fx) {
                return;
            }
            let current = fx.blocks[block as usize].model;
            let count = crate::fx::nano_fx_allowed_model_count(block);
            let mut rows = ListRows::new(list_columns(&app));
            let mut selected = -1;
            for category in 0..32u32 {
                let label = crate::fx::models::nano_fx_category_label(category);
                if label == "-" {
                    continue;
                }
                let mut started = false;
                for index in 0..count {
                    let Some(model) = crate::fx::nano_fx_get_allowed_model(block, index) else {
                        continue;
                    };
                    if crate::fx::models::model_category(model) != category {
                        continue;
                    }
                    if !started {
                        rows.header(label);
                        started = true;
                    }
                    if core::ptr::eq(model as *const _, current) {
                        selected = index as i32;
                    }
                    rows.item(crate::fx::models::model_name(model), index as i32);
                }
            }
            rows.show(&app, 0, crate::fx::models::block_label(block), selected);
        }
    });
    app.on_capture_list_tapped({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                open_capture_list(&app);
            }
        }
    });
    app.on_cab_list_tapped({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            log::info!("Cab list tapped");
            if crate::state::nano_state_library_snapshot().cabs.is_empty() {
                crate::link::push_cmd(Cmd::Library);
            }
            open_cab_list(&app);
        }
    });
    app.on_model_chosen({
        let weak = weak.clone();
        move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if !(0..=i32::from(u16::MAX)).contains(&index) {
                return;
            }
            match app.get_list_kind() {
                1 if index <= 255 => crate::link::push_cmd(Cmd::CaptureAt(index as u8)),
                2 => crate::link::push_cmd(Cmd::CabAt(index as u16)),
                3 if index <= 255 => crate::link::push_cmd(Cmd::PresetAt(index as u8)),
                kind @ (4 | 5) if index <= 255 => choose_target(&app, kind, index as u8),
                6 if (0..=64).contains(&index) => performance::assign(&app, index),
                0 if index <= 255 => {
                    let Some(block) = fx_block_for_chain(app.get_chain()) else {
                        return;
                    };
                    crate::link::push_cmd(Cmd::ModelAt {
                        block: block as u8,
                        index: index as u8,
                    });
                }
                _ => {}
            }
        }
    });
    app.on_model_preset_action_chosen({
        let weak = weak.clone();
        move |index, action| {
            if let Some(app) = weak.upgrade() {
                if !(1..=64).contains(&index) {
                    return;
                }
                ACTION_SLOT.store(index as u8, Ordering::Relaxed);
                preset_action(&app, action);
            }
        }
    });
    app.on_kbd_key({
        let weak = weak.clone();
        move |key| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let mut text = String::from(app.get_kbd_text().as_str());
            if key.as_str() == "\u{8}" {
                text.pop();
            } else if key.len() == 1 && text.len() < nano_wire::editor::PRESET_NAME_LIMIT {
                text.push_str(key.as_str());
            }
            app.set_kbd_text(text.as_str().into());
            app.set_kbd_error(if app.get_performance_name_action() != 0 {
                crate::performance::data::name_error(&text)
            } else {
                preset_name_error(&text)
            }.into());
        }
    });
    app.on_kbd_accepted({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let name = String::from(app.get_kbd_text().as_str());
            if app.get_performance_name_action() != 0 {
                performance::accept_name(&app, &name);
                return;
            }
            if preset_name_error(&name).is_empty() {
                crate::link::push_cmd(Cmd::PresetRename {
                    slot: ACTION_SLOT.load(Ordering::Relaxed),
                    name,
                });
            }
        }
    });
    app.on_confirm_accepted({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                if performance::confirm(&app) { return; }
                let bank = app.get_performance_delete_bank();
                if bank >= 0 {
                    app.set_performance_delete_bank(-1);
                    if let Err(error) = crate::performance::delete_bank(bank as usize) { crate::performance::report(error); }
                    return;
                }
            }
            match CONFIRM.load(Ordering::Relaxed) {
        CONFIRM_DELETE => {
            crate::link::push_cmd(Cmd::PresetDelete(ACTION_SLOT.load(Ordering::Relaxed)));
        }
        kind @ (LIST_MOVE | LIST_COPY) => run_transfer(kind),
        _ => {}
            }
        }
    });
    app.on_save_tapped(move || crate::link::push_cmd(Cmd::Save));
    app.on_copy_tapped(move || crate::link::push_cmd(Cmd::Copy));
    app.on_bt_picked(move |index| {
        if index >= 0 {
            crate::ble::pick(index as usize);
        }
    });
    app.on_bt_forget(move |index| {
        if index >= 0 {
            crate::ble::forget(index as usize);
        }
    });
    app.on_library_tapped(move || crate::link::push_cmd(Cmd::Library));
    app.on_param_changed(move |cell, amount| send_fx_param(cell, amount));
    app.on_layout_tapped({
        let weak = weak.clone();
        move |mode| {
            if let Some(app) = weak.upgrade() {
                app.set_layout_mode(mode);
                crate::settings::save_layout(mode);
            }
        }
    });
    app.on_slider_moved(move |id, amount| {
        send_slider(id, amount);
    });
    app.on_cab_slider_moved(move |id, amount| {
        let Some(which) = (match id {
            0 => Some(CabFloat::Level),
            1 => Some(CabFloat::HighPass),
            2 => Some(CabFloat::LowPass),
            _ => None,
        }) else {
            return;
        };
        let Some(ui) = state().as_mut() else {
            return;
        };
        if !ui.app.get_cab_active() {
            return;
        }
        let amount = amount.clamp(0.0, 1.0);
        let text = cab_float_display(which, amount);
        match which {
            CabFloat::Level => {
                ui.app.set_cab_output_amount(amount);
                ui.app.set_cab_output_text(text.into());
            }
            CabFloat::HighPass => {
                ui.app.set_cab_high_pass_amount(amount);
                ui.app.set_cab_high_pass_text(text.into());
            }
            CabFloat::LowPass => {
                ui.app.set_cab_low_pass_amount(amount);
                ui.app.set_cab_low_pass_text(text.into());
            }
        }
        ui.window.request_redraw();
        crate::link::push_cmd(Cmd::CabParam {
            which,
            normalized: amount,
        });
    });
    app.on_gate_tapped(move || {
        let on = state()
            .as_ref()
            .map(|ui| ui.app.get_gate_text().as_str() != "ON")
            .unwrap_or(true);
        show_gate(on);
        crate::link::push_cmd(crate::link::Cmd::Gate(on));
    });
    app.show().expect("slint show");

    *state() = Some(State {
        window,
        app,
        rotation: RenderingRotation::NoRotation,
        pressed: false,
        last_x: 0.0,
        last_y: 0.0,
        fx: NanoFxState::zeroed(),
        fx_valid: false,
        amp_title: String::from("Amp"),
        expression_slot: 0,
        expression: ExpressionAssignments::default(),
    });
}

/// Vertical pitch of picker options: the 68px option height in app.slint plus an 8px gap.
const ROW_STEP: f32 = 76.0;
/// Preset cards are 94px tall in app.slint to fit three-line names.
const PRESET_ROW_STEP: f32 = 102.0;

/// Rows for the picker. Each header starts a tab; its items fill columns and
/// are positioned from the top of that tab's content.
struct ListRows {
    items: alloc::vec::Vec<ModelItem>,
    columns: usize,
    step: f32,
    slot: usize,
    y: f32,
    section: i32,
    header_at: Option<usize>,
}

impl ListRows {
    fn new(columns: usize) -> Self {
        Self::with_step(columns, ROW_STEP)
    }

    fn with_step(columns: usize, step: f32) -> Self {
        Self {
            items: alloc::vec::Vec::new(),
            columns,
            step,
            slot: 0,
            y: 0.0,
            section: -1,
            header_at: None,
        }
    }

    /// Closes the open tab by recording its content height on its header.
    fn close_section(&mut self) {
        if self.slot % self.columns != 0 {
            self.y += self.step;
        }
        if let Some(at) = self.header_at.take() {
            self.items[at].height = self.y;
        }
    }

    fn header(&mut self, label: &str) {
        self.close_section();
        self.slot = 0;
        self.y = 0.0;
        self.section += 1;
        self.header_at = Some(self.items.len());
        self.items.push(ModelItem {
            name: label.into(),
            header: true,
            index: -1,
            col: 0,
            y: 0.0,
            section: self.section,
            height: 0.0,
        });
    }

    fn item(&mut self, name: &str, index: i32) {
        self.items.push(ModelItem {
            name: name.into(),
            header: false,
            index,
            col: (self.slot % self.columns) as i32,
            y: self.y,
            section: self.section.max(0),
            height: 0.0,
        });
        self.slot += 1;
        if self.slot % self.columns == 0 {
            self.y += self.step;
        }
    }

    fn show(mut self, app: &AppWindow, kind: i32, title: &str, current: i32) {
        self.close_section();
        let section = self
            .items
            .iter()
            .find(|item| !item.header && item.index == current)
            .map_or(0, |item| item.section);
        app.set_model_list_height(self.y);
        app.set_model_list(slint::ModelRc::new(slint::VecModel::from(self.items)));
        app.set_model_list_current(current);
        app.set_model_list_section(section);
        app.set_model_list_title(title.into());
        app.set_list_kind(kind);
        app.set_model_open(true);
    }
}

fn list_columns(app: &AppWindow) -> usize {
    if app.get_portrait() {
        2
    } else {
        3
    }
}

fn amp_snapshot() -> nano_wire::types::NanoAmpC {
    let mut amp = nano_wire::types::NanoAmpC::from(&nano_wire::types::Amp::default());
    crate::state::nano_state_snapshot_amp(&mut amp);
    amp
}

fn open_capture_list(app: &AppWindow) {
    let amp = amp_snapshot();
    let mut rows = ListRows::new(list_columns(app));
    for bank in 0..5u32 {
        rows.header(crate::amp::nano_amp_bank_label(bank));
        for entry in crate::amp::catalog()
            .iter()
            .filter(|entry| entry.bank == bank)
        {
            let name = crate::amp::capture_name(entry.slot).unwrap_or("-");
            rows.item(name, i32::from(entry.slot));
        }
    }
    let current = if amp.have_capture_slot {
        i32::from(amp.capture_slot)
    } else {
        -1
    };
    rows.show(app, 1, "Captures", current);
}

fn open_preset_list(app: &AppWindow) {
    let names = crate::state::nano_state_preset_names();
    let mut rows = ListRows::with_step(list_columns(app), PRESET_ROW_STEP);
    if names.is_empty() {
        rows.header("Loading presets...");
    }
    const PER_TAB: usize = 12;
    for (position, name) in names.iter().enumerate() {
        if position % PER_TAB == 0 {
            let last = (position + PER_TAB).min(names.len());
            rows.header(&alloc::format!("{}-{}", position + 1, last));
        }
        rows.item(name, position as i32 + 1);
    }
    let current = i32::from(crate::state::nano_state_preset_slot());
    rows.show(app, 3, "Presets", current);
}

const LIST_MOVE: u8 = 4;
const LIST_COPY: u8 = 5;
const CONFIRM_DELETE: u8 = 1;

/// Preset the action menu is acting on (1..=64), and what the open confirm
/// dialog will do.
static ACTION_SLOT: AtomicU8 = AtomicU8::new(0);
static ACTION_DST: AtomicU8 = AtomicU8::new(0);
static CONFIRM: AtomicU8 = AtomicU8::new(0);

fn preset_name_error(text: &str) -> &'static str {
    let slot = ACTION_SLOT.load(Ordering::Relaxed);
    if text.trim().is_empty() {
        "Name can't be empty or only spaces"
    } else if text.len() < nano_wire::editor::PRESET_NAME_MIN {
        "At least 4 characters"
    } else if !nano_wire::editor::preset_name_valid(text) {
        "Letters, digits, space, - and _ only"
    } else if crate::state::nano_state_preset_name_taken(slot, text) {
        "Another preset already has this name"
    } else {
        ""
    }
}

fn slot_name(slot: u8) -> String {
    crate::state::nano_state_preset_names()
        .get(usize::from(slot).wrapping_sub(1))
        .cloned()
        .unwrap_or_default()
}

fn preset_action(app: &AppWindow, action: i32) {
    let slot = ACTION_SLOT.load(Ordering::Relaxed);
    if !(1..=64).contains(&slot) {
        return;
    }
    let name = slot_name(slot);
    match action {
        0 => {
            let start = if nano_wire::editor::preset_name_valid(&name) {
                name.as_str()
            } else {
                ""
            };
            app.set_kbd_title(alloc::format!("Rename preset {slot}").as_str().into());
            app.set_kbd_text(start.into());
            app.set_kbd_error(preset_name_error(start).into());
            app.set_kbd_open(true);
        }
        3 => {
            CONFIRM.store(CONFIRM_DELETE, Ordering::Relaxed);
            app.set_confirm_title(alloc::format!("Delete preset {slot}?").as_str().into());
            app.set_confirm_message(alloc::format!("\"{name}\" will be removed from the pedal.").as_str().into());
            app.set_confirm_label("Delete".into());
            app.set_confirm_danger(true);
            app.set_confirm_open(true);
        }
        1 => open_target_list(app, LIST_MOVE),
        2 => open_target_list(app, LIST_COPY),
        _ => {}
    }
}

/// Every slot, empty ones included, as the destination for a move or copy.
fn open_target_list(app: &AppWindow, kind: u8) {
    let names = crate::state::nano_state_preset_names();
    let mut rows = ListRows::with_step(list_columns(app), PRESET_ROW_STEP);
    const PER_TAB: usize = 12;
    for position in 0..64usize {
        if position % PER_TAB == 0 {
            let last = (position + PER_TAB).min(64);
            rows.header(&alloc::format!("{}-{}", position + 1, last));
        }
        let name = names.get(position).map_or("", String::as_str);
        let label = if name.is_empty() { "(empty)" } else { name };
        rows.item(label, position as i32 + 1);
    }
    let title = if kind == LIST_MOVE { "Move to..." } else { "Copy to..." };
    rows.show(app, i32::from(kind), title, -1);
}

fn choose_target(app: &AppWindow, kind: i32, dst: u8) {
    let src = ACTION_SLOT.load(Ordering::Relaxed);
    if !(1..=64).contains(&src) || !(1..=64).contains(&dst) || src == dst {
        return;
    }
    let kind = kind as u8;
    ACTION_DST.store(dst, Ordering::Relaxed);
    let existing = slot_name(dst);
    if existing.is_empty() {
        run_transfer(kind);
        return;
    }
    CONFIRM.store(kind, Ordering::Relaxed);
    app.set_confirm_title(alloc::format!("Overwrite preset {dst}?").as_str().into());
    app.set_confirm_message(
        alloc::format!(
            "\"{existing}\" will be deleted and replaced by \"{}\". Unsaved edits are lost.",
            slot_name(src)
        )
        .as_str()
        .into(),
    );
    app.set_confirm_label("Overwrite".into());
    app.set_confirm_danger(false);
    app.set_confirm_open(true);
}

fn run_transfer(kind: u8) {
    crate::link::push_cmd(Cmd::PresetTransfer {
        src: ACTION_SLOT.load(Ordering::Relaxed),
        dst: ACTION_DST.load(Ordering::Relaxed),
        remove_src: kind == LIST_MOVE,
    });
}

fn open_cab_list(app: &AppWindow) {
    let amp = amp_snapshot();
    let cab_end = amp
        .cab_name
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(amp.cab_name.len());
    let cab_name = String::from_utf8_lossy(&amp.cab_name[..cab_end]).into_owned();
    let library = crate::state::nano_state_library_snapshot();
    let mut rows = ListRows::new(list_columns(app));
    let mut current = -1;
    if library.cabs.is_empty() {
        // Captures arrive in the same reply, so having them means it loaded.
        rows.header(if library.captures.is_empty() {
            "Loading library..."
        } else {
            "No IRs in library"
        });
    }
    for (label, user) in [("Factory", false), ("User", true)] {
        let mut first = true;
        for (position, cab) in library
            .cabs
            .iter()
            .enumerate()
            .filter(|(_, cab)| cab.user == user)
        {
            if first {
                rows.header(label);
                first = false;
            }
            if amp.have_cab_name && cab.name.eq_ignore_ascii_case(&cab_name) {
                current = position as i32;
            }
            rows.item(&cab.name, position as i32);
        }
    }
    rows.show(app, 2, "Cab / IR", current);
}

/// Rebuilds the cab list when the library reply arrives while it is open.
pub fn library_updated() {
    ensure();
    let ui = state().as_mut().unwrap();
    if ui.app.get_model_open() && ui.app.get_list_kind() == 2 {
        open_cab_list(&ui.app);
        ui.window.request_redraw();
    }
}

fn redraw(framebuffer: &mut Framebuffer) {
    let ui = state().as_mut().unwrap();
    let layout_changed = apply_layout(ui);
    slint::platform::update_timers_and_animations();
    // The old layout's pixels are still in the framebuffer, and a rotation
    // change alone does not mark them dirty. Repaint the whole frame once.
    let rotation = ui.rotation;
    // Slint 1.13's mirrored glyph sampling can underflow during partial
    // redraws. Flip finished spans instead; touch mapping still uses rotation.
    let flipped = rotation == RenderingRotation::Rotate180;
    let rendering_rotation = if flipped {
        RenderingRotation::NoRotation
    } else {
        rotation
    };
    let fb = framebuffer
        .as_pixels_mut()
        .as_mut_ptr()
        .cast::<Rgb565Pixel>();
    ui.window.draw_if_needed(|renderer| {
        renderer.set_rendering_rotation(rendering_rotation);
        if layout_changed {
            renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
        }
        renderer.render_by_line(PartialLines::new(fb, flipped));
        renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
    });
}

static LINK_UP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static LINK_UP_AT_MS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static LIBRARY_ASKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static LOADED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static LOAD_TICK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// How long to wait for the pedal's dump before showing the app anyway.
const LOAD_TIMEOUT_MS: u32 = 20_000;

/// Keeps the welcome screen up after the link opens until the amp state, preset
/// names and library have arrived, so the app never shows half-loaded data.
pub fn refresh_loading() {
    use core::sync::atomic::Ordering::Relaxed;
    if !LINK_UP.load(Relaxed) || LOADED.load(Relaxed) {
        return;
    }
    if LOAD_TICK.fetch_add(1, Relaxed) % 8 != 0 {
        return;
    }
    let Some(ui) = state().as_mut() else {
        return;
    };
    let waited = (Instant::now().as_millis() as u32).wrapping_sub(LINK_UP_AT_MS.load(Relaxed));
    let status = if !crate::state::nano_state_amp_ready() {
        "Loading amp state..."
    } else if crate::state::nano_state_preset_names().is_empty() {
        "Loading presets..."
    } else {
        // Ask for the library once the dump is in, so the state and library streams don't mix.
        if !LIBRARY_ASKED.swap(true, Relaxed) {
            crate::ble::LIBRARY_WANTED.store(true, Relaxed);
        }
        let library = crate::state::nano_state_library_snapshot();
        if library.captures.is_empty() && library.cabs.is_empty() {
            "Loading library..."
        } else {
            ""
        }
    };
    if status.is_empty() || waited > LOAD_TIMEOUT_MS {
        LOADED.store(true, Relaxed);
        ui.app.set_load_status("".into());
        ui.app.set_connecting(false);
    } else if ui.app.get_load_status() != status {
        ui.app.set_load_status(status.into());
    }
    ui.window.request_redraw();
}

pub fn set_connected(connected: bool) {
    use core::sync::atomic::Ordering::Relaxed;
    ensure();
    let ui = state().as_mut().unwrap();
    LINK_UP.store(connected, Relaxed);
    LOADED.store(false, Relaxed);
    LIBRARY_ASKED.store(false, Relaxed);
    LOAD_TICK.store(0, Relaxed);
    LINK_UP_AT_MS.store(Instant::now().as_millis() as u32, Relaxed);
    ui.app.set_connecting(true);
    if !connected {
        ui.app.set_load_status("".into());
    }
    ui.app.set_ble_on(connected);
    ui.app.set_model_enabled(connected);
    ui.window.request_redraw();
}

pub fn set_bluetooth_busy() {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_bt_busy(true);
    ui.window.request_redraw();
}

static MIDI_SHOWN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Repaints the MIDI page when its bindings change and saves new ones.
pub fn refresh_midi() {
    if let Some(ui) = state().as_ref() { performance::refresh(&ui.app, false); }
    if state().as_ref().is_some_and(|ui| ui.app.get_usb_storage_open()) {
        return;
    }
    crate::midi::persist();
    let version = crate::midi::version();
    if MIDI_SHOWN.load(core::sync::atomic::Ordering::Relaxed) == version {
        return;
    }

    let Some(ui) = state().as_mut() else {
        return;
    };
    if !ui.app.get_settings_open() {
        return;
    }
    MIDI_SHOWN.store(version, core::sync::atomic::Ordering::Relaxed);
    let rows: alloc::vec::Vec<MidiRow> = crate::midi::rows()
        .into_iter()
        .map(|r| MidiRow {
            name: r.name.as_str().into(),
            assigned: r.assigned.as_str().into(),
            learning: r.learning,
        })
        .collect();
    ui.app
        .set_midi_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    ui.app
        .set_midi_last(crate::midi::last_text().as_str().into());
    ui.window.request_redraw();
}

pub fn usb_storage_status(message: &str, finished: bool) {
    if let Some(ui) = state().as_mut() {
        ui.app.set_settings_open(false);
        ui.app.set_model_open(false);
        ui.app.set_kbd_open(false);
        ui.app.set_confirm_open(false);
        ui.app.set_connecting(false);
        ui.app.set_usb_storage_open(true);
        ui.app.set_usb_storage_finished(finished);
        ui.app.set_usb_storage_message(message.into());
        ui.window.request_redraw();
    }
}

/// Same as tapping the tuner button, so the page switch stays in one place.
pub fn midi_toggle_tuner() {
    if let Some(ui) = state().as_mut() {
        ui.app.invoke_tuner_tapped();
    }
}

static BT_SHOWN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static BT_OPEN_SEEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Refreshes the picker rows while the list is open, repainting only on change.
pub fn refresh_bluetooth(devices: &[crate::ble::Seen]) {
    ensure();
    let ui = state().as_mut().unwrap();
    let best = |nano: bool| {
        devices
            .iter()
            .filter(|d| d.nano == nano)
            .map(|d| i32::from(d.state))
            .max()
            .unwrap_or(-1)
    };
    let (amp, foot) = (best(true), best(false));
    if ui.app.get_conn_amp() != amp || ui.app.get_conn_foot() != foot {
        ui.app.set_conn_amp(amp);
        ui.app.set_conn_foot(foot);
        ui.window.request_redraw();
    }
    if !ui.app.get_bt_open() {
        BT_OPEN_SEEN.store(false, core::sync::atomic::Ordering::Relaxed);
        return;
    }
    let mut hash = 17u32;
    for d in devices {
        for b in d.name.bytes().chain([d.state, d.nano as u8]) {
            hash = hash.wrapping_mul(31).wrapping_add(u32::from(b));
        }
    }
    hash = hash.wrapping_mul(31).wrapping_add(devices.len() as u32);
    let open_changed = BT_OPEN_SEEN.swap(true, core::sync::atomic::Ordering::Relaxed) != true;
    if !open_changed && BT_SHOWN.swap(hash, core::sync::atomic::Ordering::Relaxed) == hash {
        return;
    }
    BT_SHOWN.store(hash, core::sync::atomic::Ordering::Relaxed);
    let rows: alloc::vec::Vec<BtDevice> = devices
        .iter()
        .map(|d| BtDevice {
            name: d.name.as_str().into(),
            kind: if d.nano {
                "Amp".into()
            } else {
                "Footswitch".into()
            },
            state: d.state as i32,
        })
        .collect();
    ui.app
        .set_bt_devices(slint::ModelRc::new(slint::VecModel::from(rows)));
    ui.window.request_redraw();
}

pub fn show_knob_from_radio(id: u8, raw: u8) -> bool {
    if finger_holds(id) {
        return false;
    }
    show_knob(id, raw);
    true
}

pub fn show_gate_from_radio(on: bool) -> bool {
    if finger_holds(7) {
        return false;
    }
    show_gate(on);
    true
}

pub fn show_gate_amount_from_radio(norm: f32) -> bool {
    if finger_holds(6) {
        return false;
    }
    show_gate_amount(norm);
    true
}

pub fn show_knob(id: u8, raw: u8) {
    ensure();
    let ui = state().as_mut().unwrap();
    let text: slint::SharedString = alloc::format!("{}", pct(raw)).into();
    let amount = raw as f32 / 255.0;
    match id {
        0 => {
            ui.app.set_gain_text(text);
            ui.app.set_gain_amount(amount);
        }
        1 => {
            ui.app.set_master_text(text);
            ui.app.set_master_amount(amount);
        }
        2 => {
            ui.app.set_bass_text(text);
            ui.app.set_bass_amount(amount);
        }
        3 => {
            ui.app.set_mid_text(text);
            ui.app.set_mid_amount(amount);
        }
        4 => {
            ui.app.set_treble_text(text);
            ui.app.set_treble_amount(amount);
        }
        5 => {
            ui.app.set_cap_text(text);
            ui.app.set_cap_amount(amount);
        }
        _ => return,
    }
    ui.window.request_redraw();
}

pub fn show_gate(on: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_gate_text(if on { "ON" } else { "OFF" }.into());
    ui.app.set_gate_amount(if on { 1.0 } else { 0.0 });
    ui.window.request_redraw();
}

pub fn show_gate_amount(norm: f32) {
    ensure();
    let ui = state().as_mut().unwrap();
    let norm = norm.clamp(0.0, 1.0);
    let pct = (norm * 100.0) as u8;
    ui.app.set_gate_amnt_text(alloc::format!("{pct}").into());
    ui.app.set_gate_amnt_amount(norm);
    ui.window.request_redraw();
}

pub fn show_gig(enabled: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.invoke_gig_mode_set(enabled);
    performance::refresh(&ui.app, true);
    ui.window.request_redraw();
}

pub fn show_preset(name: &str) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_preset(name.into());
    if ui.app.get_model_open() && ui.app.get_list_kind() == 3 {
        ui.app
            .set_model_list_current(i32::from(crate::state::nano_state_preset_slot()));
    }
    ui.window.request_redraw();
}

pub fn refresh_preset_catalog() {
    ensure();
    let names = crate::state::nano_state_preset_names();
    let ui = state().as_mut().unwrap();
    performance::refresh(&ui.app, true);
    presets::refresh_preset_rows(&ui.app, &names);
    ui.window.request_redraw();
}

pub fn show_model(name: &str) {
    if name.is_empty() {
        return;
    }
    ensure();
    let ui = state().as_mut().unwrap();
    ui.amp_title = String::from(name);
    if ui.app.get_page() == 0 {
        let title = ui.amp_title.clone();
        ui.app.set_model(title.as_str().into());
        ui.app.set_model_enabled(true);
        ui.window.request_redraw();
    }
}

pub fn show_amp_sources(
    capture_title: &str,
    capture_value: &str,
    capture_slot: &str,
    capture_active: bool,
    cab_value: &str,
    cab_slot: &str,
    cab_active: bool,
) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_capture_title(capture_title.into());
    ui.app.set_capture_value(capture_value.into());
    ui.app.set_capture_slot(capture_slot.into());
    ui.app.set_capture_active(capture_active);
    ui.app.set_cab_value(cab_value.into());
    ui.app.set_cab_slot(cab_slot.into());
    ui.app.set_cab_active(cab_active);
    ui.window.request_redraw();
}

pub fn show_save_dirty(dirty: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_save_dirty(dirty);
    ui.window.request_redraw();
}

pub fn show_fx(fx: &NanoFxState) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.fx = *fx;
    ui.fx_valid = fx.valid;
    paint_chain(ui);
    paint_fx_page(ui);
    ui.window.request_redraw();
}

pub fn show_bpm(bpm: u16) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_bpm_text(alloc::format!("{bpm} BPM").into());
    ui.app.set_tempo_bpm_value(i32::from(bpm));
    ui.window.request_redraw();
}

pub fn clear_bpm() {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_bpm_text("- BPM".into());
    ui.app.set_tempo_bpm_value(0);
    ui.window.request_redraw();
}

pub fn show_tempo_page(active: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_tempo_active(active);
    ui.window.request_redraw();
}

pub fn show_tuner(note: &str, cents: &str, active: bool, has_pitch: bool, cents_f: f32) {
    ensure();
    let ui = state().as_mut().unwrap();
    let was = ui.app.get_tuner_active();
    let c = cents_f.clamp(-50.0, 50.0);
    let pos = if has_pitch { (c + 50.0) / 100.0 } else { 0.5 };
    ui.app.set_tuner_active(active);
    ui.app.set_tuner_note(note.into());
    ui.app.set_tuner_cents(cents.into());
    ui.app.set_tuner_pos(pos);
    ui.app.set_tuner_has_pitch(has_pitch);
    ui.app.set_tuner_in_tune(has_pitch && c.abs() <= 2.0);
    if active && !was {
        ui.app.set_page(2);
        ui.app.set_model("Tuner".into());
    } else if !active && was && ui.app.get_page() == 2 {
        let chain = ui.app.get_chain();
        ui.app.set_page(home_page(chain));
        if chain == 2 {
            let title = ui.amp_title.clone();
            ui.app.set_model(title.as_str().into());
        } else {
            paint_fx_page(ui);
        }
    }
    ui.window.request_redraw();
}

pub fn show_tuner_settings(reference_hz: f32, muted: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    let reference_hz = reference_hz.clamp(400.0, 480.0);
    ui.app.set_tuner_reference_hz(reference_hz);
    ui.app
        .set_tuner_reference(format!("{reference_hz:.0} Hz").into());
    ui.app.set_tuner_muted(muted);
    ui.window.request_redraw();
}

pub fn show_device_settings(settings: &nano_wire::types::DeviceSettings) {
    ensure();
    let ui = state().as_mut().unwrap();
    let (jack, expression) = match settings.jack_mode {
        Some(nano_wire::types::JackMode::Midi) => ("MIDI", false),
        Some(nano_wire::types::JackMode::Expression) => ("Expression", true),
        None => ("Unknown", false),
    };
    ui.app.set_tools_jack_mode(jack.into());
    ui.app.set_tools_jack_expression(expression);
    ui.app.set_tools_usb_level(
        settings
            .usb_playback_db
            .map(|db| alloc::format!("{db:.1} dB"))
            .unwrap_or_else(|| String::from("-"))
            .into(),
    );
    if let Some(db) = settings.usb_playback_db {
        ui.app
            .set_tools_usb_amount(((db + 40.0) / 40.0).clamp(0.0, 1.0));
    }
    ui.window.request_redraw();
}

pub fn show_cab_settings(slot: u8, settings: &nano_wire::types::CabSettings) {
    ensure();
    let ui = state().as_mut().unwrap();
    let output = settings
        .output
        .map(|norm| cab_float_display(CabFloat::Level, norm))
        .unwrap_or_else(|| String::from("-- dB"));
    let high_pass = settings
        .high_pass
        .map(|norm| cab_float_display(CabFloat::HighPass, norm))
        .unwrap_or_else(|| String::from("-- Hz"));
    let low_pass = settings
        .low_pass
        .map(|norm| cab_float_display(CabFloat::LowPass, norm))
        .unwrap_or_else(|| String::from("-- Hz"));
    let active = (1..=5).contains(&slot);
    ui.app.set_cab_output_text(
        if active {
            output.as_str()
        } else {
            "-- dB"
        }
        .into(),
    );
    ui.app.set_cab_output_amount(if active {
        settings.output.unwrap_or(0.0).clamp(0.0, 1.0)
    } else {
        0.0
    });
    ui.app.set_cab_high_pass_text(
        if active {
            high_pass.as_str()
        } else {
            "-- Hz"
        }
        .into(),
    );
    ui.app.set_cab_high_pass_amount(if active {
        settings.high_pass.unwrap_or(0.0).clamp(0.0, 1.0)
    } else {
        0.0
    });
    ui.app.set_cab_low_pass_text(
        if active {
            low_pass.as_str()
        } else {
            "-- Hz"
        }
        .into(),
    );
    ui.app.set_cab_low_pass_amount(if active {
        settings.low_pass.unwrap_or(0.0).clamp(0.0, 1.0)
    } else {
        0.0
    });
    let text = if active {
        alloc::format!("slot {slot} · {output} · HP {high_pass} · LP {low_pass}")
    } else {
        String::from("No active cab")
    };
    ui.app.set_tools_cab_settings(text.into());
    ui.window.request_redraw();
}

fn cab_float_display(which: CabFloat, normalized: f32) -> String {
    let normalized = normalized.clamp(0.0, 1.0);
    match which {
        CabFloat::Level => {
            let zero_db = 0.662_122_19_f32;
            let db = if normalized <= zero_db {
                normalized / zero_db * 96.0 - 96.0
            } else {
                (normalized - zero_db) / (1.0 - zero_db) * 12.0
            };
            alloc::format!("{db:.1} dB")
        }
        CabFloat::HighPass => alloc::format!("{:.0} Hz", 20.0 + normalized * 780.0),
        CabFloat::LowPass => alloc::format!("{:.0} Hz", 1000.0 + normalized * 19000.0),
    }
}

pub fn show_expression_assignments(
    slot: u8,
    assignments: &nano_wire::editor::ExpressionAssignments,
) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.expression_slot = slot;
    ui.expression = assignments.clone();
    publish_expression(ui);
    ui.window.request_redraw();
}

fn expression_summary(slot: u8, assignments: &ExpressionAssignments) -> String {
    let text = if slot == 0 {
        String::from("Not loaded")
    } else if assignments.ranges.is_empty() && assignments.bypasses.is_empty() {
        String::from("None assigned")
    } else {
        let mut text = String::new();
        for (target, range) in &assignments.ranges {
            let name = match target {
                ExpRangeTarget::Gain => "Gain",
                ExpRangeTarget::Bass => "Bass",
                ExpRangeTarget::Mid => "Mid",
                ExpRangeTarget::Treble => "Treble",
                ExpRangeTarget::Pre1Amount => "Pre1",
                ExpRangeTarget::Pre2Amount => "Pre2",
                ExpRangeTarget::Post1Amount => "Post1",
                ExpRangeTarget::Post2Amount => "Post2",
                ExpRangeTarget::Post3Amount => "Post3",
                ExpRangeTarget::CaptureVolume => "Volume",
                ExpRangeTarget::Gate => "Gate",
            };
            if !text.is_empty() {
                text.push_str(" · ");
            }
            let min = (u16::from(range.min) * 100 + 127) / 255;
            let max = (u16::from(range.max) * 100 + 127) / 255;
            text.push_str(&alloc::format!(
                "{name} {min}-{max}%{}",
                if range.inverted { " rev" } else { "" }
            ));
        }
        for (target, bypass) in &assignments.bypasses {
            let name = match target {
                ExpBypassTarget::Capture => "Capture",
                ExpBypassTarget::Cab => "Cab",
                ExpBypassTarget::Pre1 => "Pre1",
                ExpBypassTarget::Pre2 => "Pre2",
                ExpBypassTarget::Post1 => "Post1",
                ExpBypassTarget::Post2 => "Post2",
                ExpBypassTarget::Post3 => "Post3",
                ExpBypassTarget::Gate => "Gate",
            };
            let detail = match bypass {
                ExpBypass::HeelToe { inverted, delay_ms } => alloc::format!(
                    "heel/toe{} {}ms",
                    if *inverted { " rev" } else { "" },
                    delay_ms
                ),
                ExpBypass::Switch { inverted, latch } => alloc::format!(
                    "switch{}{}",
                    if *inverted { " rev" } else { "" },
                    if *latch { " latch" } else { "" }
                ),
                ExpBypass::Stop { delay_ms } => {
                    alloc::format!("stop {}ms", delay_ms)
                }
            };
            if !text.is_empty() {
                text.push_str(" · ");
            }
            text.push_str(&alloc::format!("{name} {detail}"));
        }
        text
    };
    if slot == 0 {
        text
    } else {
        alloc::format!(
            "preset {slot} · {} sweeps · {} switches: {text}",
            assignments.ranges.len(),
            assignments.bypasses.len()
        )
    }
}

fn publish_expression(ui: &mut State) {
    const RANGES: [(ExpRangeTarget, &str); 11] = [
        (ExpRangeTarget::Gain, "Gain"),
        (ExpRangeTarget::Bass, "Bass"),
        (ExpRangeTarget::Mid, "Mid"),
        (ExpRangeTarget::Treble, "Treble"),
        (ExpRangeTarget::Pre1Amount, "Pre FX 1 amount"),
        (ExpRangeTarget::Pre2Amount, "Pre FX 2 amount"),
        (ExpRangeTarget::Post1Amount, "Post FX 1 amount"),
        (ExpRangeTarget::Post2Amount, "Post FX 2 amount"),
        (ExpRangeTarget::Post3Amount, "Post FX 3 amount"),
        (ExpRangeTarget::CaptureVolume, "Capture volume"),
        (ExpRangeTarget::Gate, "Gate amount"),
    ];
    const BYPASSES: [(ExpBypassTarget, &str); 8] = [
        (ExpBypassTarget::Capture, "Capture"),
        (ExpBypassTarget::Cab, "Cab"),
        (ExpBypassTarget::Pre1, "Pre FX 1"),
        (ExpBypassTarget::Pre2, "Pre FX 2"),
        (ExpBypassTarget::Post1, "Post FX 1"),
        (ExpBypassTarget::Post2, "Post FX 2"),
        (ExpBypassTarget::Post3, "Post FX 3"),
        (ExpBypassTarget::Gate, "Gate"),
    ];
    let ranges = RANGES
        .iter()
        .map(|(target, name)| {
            let current = ui
                .expression
                .ranges
                .iter()
                .find(|(candidate, _)| candidate == target)
                .map(|(_, value)| *value);
            ExpressionRangeRow {
                target: *target as i32,
                name: (*name).into(),
                enabled: current.is_some(),
                summary: current
                    .map(|range| {
                        let min = (u16::from(range.min) * 100 + 127) / 255;
                        let max = (u16::from(range.max) * 100 + 127) / 255;
                        alloc::format!(
                            "{min}-{max}%{}",
                            if range.inverted { " Rev" } else { "" }
                        )
                        .into()
                    })
                    .unwrap_or_else(|| "Not assigned".into()),
                inverted: current.is_some_and(|range| range.inverted),
            }
        })
        .collect::<alloc::vec::Vec<_>>();
    let bypasses = BYPASSES
        .iter()
        .map(|(target, name)| {
            let current = ui
                .expression
                .bypasses
                .iter()
                .find(|(candidate, _)| candidate == target)
                .map(|(_, value)| *value);
            let (enabled, summary, mode, inverted, latch) = match current {
                Some(ExpBypass::HeelToe { inverted, delay_ms }) => (
                    true,
                    alloc::format!(
                        "Heel/toe · {delay_ms}ms{}",
                        if inverted { " · reversed" } else { "" }
                    ),
                    0,
                    inverted,
                    false,
                ),
                Some(ExpBypass::Switch { inverted, latch }) => (
                    true,
                    alloc::format!(
                        "Footswitch{}{}",
                        if inverted { " · reversed" } else { "" },
                        if latch { " · latch" } else { "" }
                    ),
                    1,
                    inverted,
                    latch,
                ),
                Some(ExpBypass::Stop { delay_ms }) => (
                    true,
                    alloc::format!("Stop · {delay_ms}ms"),
                    2,
                    false,
                    false,
                ),
                None => (false, String::from("Not assigned"), 0, false, false),
            };
            ExpressionBypassRow {
                target: *target as i32,
                name: (*name).into(),
                enabled,
                summary: summary.into(),
                mode,
                inverted,
                latch,
            }
        })
        .collect::<alloc::vec::Vec<_>>();
    ui.app.set_tools_expression_summary(
        expression_summary(ui.expression_slot, &ui.expression).into(),
    );
    ui.app.set_tools_expression_ranges(slint::ModelRc::new(slint::VecModel::from(ranges)));
    ui.app
        .set_tools_expression_bypasses(slint::ModelRc::new(slint::VecModel::from(bypasses)));
}

fn range_target(value: i32) -> Option<ExpRangeTarget> {
    match value {
        4 => Some(ExpRangeTarget::Gain),
        5 => Some(ExpRangeTarget::Bass),
        6 => Some(ExpRangeTarget::Mid),
        7 => Some(ExpRangeTarget::Treble),
        8 => Some(ExpRangeTarget::Pre1Amount),
        9 => Some(ExpRangeTarget::Pre2Amount),
        10 => Some(ExpRangeTarget::Post1Amount),
        11 => Some(ExpRangeTarget::Post2Amount),
        12 => Some(ExpRangeTarget::Post3Amount),
        13 => Some(ExpRangeTarget::CaptureVolume),
        21 => Some(ExpRangeTarget::Gate),
        _ => None,
    }
}

fn edit_expression_range(target: i32, action: i32, delta: i32) {
    let Some(target) = range_target(target) else {
        return;
    };
    let Some(ui) = state().as_mut() else {
        return;
    };
    if ui.expression_slot == 0 {
        return;
    }
    let ranges = &mut ui.expression.ranges;
    if action == 0 {
        if let Some(index) = ranges.iter().position(|(candidate, _)| *candidate == target) {
            ranges.remove(index);
        } else {
            ranges.push((
                target,
                ExpRange {
                    inverted: false,
                    min: 0,
                    max: u8::MAX,
                },
            ));
        }
    } else if let Some((_, range)) = ranges.iter_mut().find(|(candidate, _)| *candidate == target) {
        match action {
            1 => range.min = (i32::from(range.min) + delta * 13).clamp(0, 255) as u8,
            2 => range.max = (i32::from(range.max) + delta * 13).clamp(0, 255) as u8,
            3 => range.inverted = !range.inverted,
            _ => return,
        }
    }
    publish_expression(ui);
    ui.window.request_redraw();
}

fn bypass_target(value: i32) -> Option<ExpBypassTarget> {
    match value {
        14 => Some(ExpBypassTarget::Capture),
        15 => Some(ExpBypassTarget::Cab),
        16 => Some(ExpBypassTarget::Pre1),
        17 => Some(ExpBypassTarget::Pre2),
        18 => Some(ExpBypassTarget::Post1),
        19 => Some(ExpBypassTarget::Post2),
        20 => Some(ExpBypassTarget::Post3),
        22 => Some(ExpBypassTarget::Gate),
        _ => None,
    }
}

fn edit_expression_bypass(target: i32, action: i32) {
    let Some(target) = bypass_target(target) else {
        return;
    };
    let Some(ui) = state().as_mut() else {
        return;
    };
    if ui.expression_slot == 0 {
        return;
    }
    let bypasses = &mut ui.expression.bypasses;
    let found = bypasses.iter().position(|(candidate, _)| *candidate == target);
    if action == 0 {
        if let Some(index) = found {
            bypasses.remove(index);
        } else {
            bypasses.push((
                target,
                ExpBypass::HeelToe {
                    inverted: false,
                    delay_ms: 0,
                },
            ));
        }
    } else if let Some(index) = found {
        let (_, current) = bypasses[index];
        let updated = match (current, action) {
            (ExpBypass::HeelToe { .. }, 1) => ExpBypass::Switch {
                inverted: false,
                latch: false,
            },
            (ExpBypass::Switch { .. }, 1) => ExpBypass::Stop { delay_ms: 0 },
            (ExpBypass::Stop { .. }, 1) => ExpBypass::HeelToe {
                inverted: false,
                delay_ms: 0,
            },
            (ExpBypass::HeelToe { inverted, delay_ms }, 2) => ExpBypass::HeelToe {
                inverted,
                delay_ms: delay_ms.saturating_sub(100),
            },
            (ExpBypass::Stop { delay_ms }, 2) => ExpBypass::Stop {
                delay_ms: delay_ms.saturating_sub(100),
            },
            (ExpBypass::HeelToe { inverted, delay_ms }, 3) => ExpBypass::HeelToe {
                inverted,
                delay_ms: delay_ms.saturating_add(100).min(2000),
            },
            (ExpBypass::Stop { delay_ms }, 3) => ExpBypass::Stop {
                delay_ms: delay_ms.saturating_add(100).min(2000),
            },
            (ExpBypass::HeelToe { inverted, delay_ms }, 4) => ExpBypass::HeelToe {
                inverted: !inverted,
                delay_ms,
            },
            (ExpBypass::Switch { inverted, latch }, 4) => ExpBypass::Switch {
                inverted: !inverted,
                latch,
            },
            (ExpBypass::Switch { inverted, latch }, 5) => ExpBypass::Switch {
                inverted,
                latch: !latch,
            },
            _ => return,
        };
        bypasses[index].1 = updated;
    }
    publish_expression(ui);
    ui.window.request_redraw();
}

pub fn show_metering(meter: nano_wire::types::Metering) {
    ensure();
    let ui = state().as_mut().unwrap();
    let raw = meter.value.min(u32::from(u8::MAX));
    let text: slint::SharedString = alloc::format!("{raw}/255").into();
    let amount = raw as f32 / f32::from(u8::MAX);
    match meter.kind {
        nano_wire::types::MeteringKind::CaptureInput => {
            ui.app.set_tools_meter_input(text);
            ui.app.set_tools_meter_input_amount(amount);
        }
        nano_wire::types::MeteringKind::CaptureOutput => {
            ui.app.set_tools_meter_output(text);
            ui.app.set_tools_meter_output_amount(amount);
        }
        nano_wire::types::MeteringKind::ExpressionPedal => {
            ui.app.set_tools_meter_expression(text);
            ui.app.set_tools_meter_expression_amount(amount);
        }
    }
    ui.window.request_redraw();
}

pub fn clear_tool_meters() {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_tools_meter_input("-".into());
    ui.app.set_tools_meter_output("-".into());
    ui.app.set_tools_meter_expression("-".into());
    ui.app.set_tools_meter_input_amount(0.0);
    ui.app.set_tools_meter_output_amount(0.0);
    ui.app.set_tools_meter_expression_amount(0.0);
    ui.window.request_redraw();
}

pub fn show_calibration(active: bool, min: Option<u8>, max: Option<u8>) {
    ensure();
    let ui = state().as_mut().unwrap();
    let status = if active {
        match (min, max) {
            (Some(min), Some(max)) => alloc::format!("Move pedal · {min}..{max}"),
            _ => String::from("Move pedal heel to toe"),
        }
    } else {
        match (min, max) {
            (Some(min), Some(max)) => alloc::format!("Saved {min}..{max}"),
            _ => String::from("Idle"),
        }
    };
    ui.app.set_tools_calibration_status(status.into());
    ui.window.request_redraw();
}

pub fn show_protocol_status(status: &str) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_tools_protocol_status(status.into());
    ui.window.request_redraw();
}

pub fn show_chain(index: i32) {
    ensure();
    let ui = state().as_mut().unwrap();

    ui.app.set_chain(index);
    if index == 2 {
        ui.app.set_page(0);
        let title = ui.amp_title.clone();
        ui.app.set_model(title.as_str().into());
    } else {
        ui.app.set_page(1);
        paint_fx_page(ui);
    }
    ui.window.request_redraw();
}

fn leave_tools(app: &AppWindow) {
    let page = app.get_page();
    if page == 2 {
        app.set_tuner_active(false);
        crate::link::push_cmd(Cmd::Tuner(false));
    }
}

fn show_home(app: &AppWindow) {
    let page = home_page(app.get_chain());
    app.set_page(page);
    if page == 0 {
        show_amp_title();
    } else if let Some(ui) = state().as_mut() {
        paint_fx_page(ui);
        ui.window.request_redraw();
    }
}

fn show_amp_title() {
    let Some(ui) = state().as_mut() else {
        return;
    };
    if ui.app.get_page() != 0 {
        return;
    }
    let title = ui.amp_title.clone();
    ui.app.set_model(title.as_str().into());
    ui.app.set_model_enabled(true);
}

fn home_page(chain: i32) -> i32 {
    if fx_block_for_chain(chain).is_some() {
        1
    } else {
        0
    }
}

fn fx_block_for_chain(index: i32) -> Option<usize> {
    match index {
        0 => Some(0),
        1 => Some(1),
        3 => Some(2),
        4 => Some(3),
        5 => Some(4),
        _ => None,
    }
}

fn chain_is_on(app: &AppWindow, index: i32) -> bool {
    match index {
        0 => app.get_on_0(),
        1 => app.get_on_1(),
        3 => app.get_on_3(),
        4 => app.get_on_4(),
        5 => app.get_on_5(),
        _ => false,
    }
}

fn set_chain_on(app: &AppWindow, index: i32, on: bool) {
    match index {
        0 => app.set_on_0(on),
        1 => app.set_on_1(on),
        3 => app.set_on_3(on),
        4 => app.set_on_4(on),
        5 => app.set_on_5(on),
        _ => {}
    }
}

fn paint_chain(ui: &mut State) {
    for chain in [0, 1, 3, 4, 5] {
        let Some(block_idx) = fx_block_for_chain(chain) else {
            continue;
        };
        let block = &ui.fx.blocks[block_idx];
        let label = if ui.fx_valid && block.valid {
            let abbr = crate::fx::nano_fx_model_chain_acronym(block);
            if abbr.is_empty() || abbr == "-" {
                String::from("FX")
            } else {
                abbr
            }
        } else {
            String::from("FX")
        };
        let on = ui.fx_valid && block.valid && block.on;
        let category = if ui.fx_valid && block.valid {
            crate::fx::models::model_ref(block.model)
                .map_or(0, |model| crate::fx::models::model_category(model) as i32)
        } else {
            0
        };
        match chain {
            0 => {
                ui.app.set_tab_0(label.as_str().into());
                ui.app.set_on_0(on);
                ui.app.set_category_0(category);
            }
            1 => {
                ui.app.set_tab_1(label.as_str().into());
                ui.app.set_on_1(on);
                ui.app.set_category_1(category);
            }
            3 => {
                ui.app.set_tab_3(label.as_str().into());
                ui.app.set_on_3(on);
                ui.app.set_category_3(category);
            }
            4 => {
                ui.app.set_tab_4(label.as_str().into());
                ui.app.set_on_4(on);
                ui.app.set_category_4(category);
            }
            5 => {
                ui.app.set_tab_5(label.as_str().into());
                ui.app.set_on_5(on);
                ui.app.set_category_5(category);
            }
            _ => {}
        }
    }
}

fn paint_fx_page(ui: &mut State) {
    let Some(idx) = fx_block_for_chain(ui.app.get_chain()) else {
        clear_fx_cells(&ui.app);
        return;
    };
    let block = ui.fx.blocks[idx];
    if ui.app.get_page() == 1 && ui.fx_valid && block.valid {
        let text = crate::fx::nano_fx_block_model_text(&block);
        ui.app.set_model(text.as_str().into());
        ui.app.set_model_enabled(true);
    }
    let count = if ui.fx_valid && block.valid && !block.model.is_null() {
        block.param_count as usize
    } else {
        0
    };
    let model = unsafe { block.model.as_ref() };
    for cell in 0..crate::fx::types::NANO_FX_MAX_PARAMS {
        let display_i = cell;
        let Some(model) = model else {
            set_fx_cell(&ui.app, cell, "-", "-", 0.0);
            continue;
        };
        if display_i >= count {
            set_fx_cell(&ui.app, cell, "-", "-", 0.0);
            continue;
        }
        let wire = crate::fx::nano_fx_param_wire_index(model, display_i);
        if wire >= block.param_count as usize {
            set_fx_cell(&ui.app, cell, "-", "-", 0.0);
            continue;
        }
        let name = short_param_name(crate::fx::models::param_name(model, wire));
        if fx_finger_holds(idx as u8, wire as u8) {
            set_fx_name(&ui.app, cell, &name);
            continue;
        }
        if block.params[wire].valid {
            let value = crate::fx::nano_fx_format_param(&block, wire as u8);
            let amount = block.params[wire].normalized.clamp(0.0, 1.0);
            set_fx_cell(&ui.app, cell, &name, &value, amount);
        } else {
            set_fx_cell(&ui.app, cell, &name, "-", 0.0);
        }
    }
}

fn send_fx_param(cell: i32, amount: f32) {
    if !(0..crate::fx::types::NANO_FX_MAX_PARAMS as i32).contains(&cell) {
        return;
    }
    let Some(ui) = state().as_mut() else {
        return;
    };
    if !ui.fx_valid {
        return;
    }
    let Some(idx) = fx_block_for_chain(ui.app.get_chain()) else {
        return;
    };
    let block = ui.fx.blocks[idx];
    let Some(model) = (unsafe { block.model.as_ref() }) else {
        return;
    };
    let display_i = cell as usize;
    if display_i >= block.param_count as usize {
        return;
    }
    let wire = crate::fx::nano_fx_param_wire_index(model, display_i);
    if wire >= block.param_count as usize {
        return;
    }
    let amount = crate::fx::params::normalize_for_model(model, wire, amount);
    let name = crate::fx::models::param_name(model, wire);
    let mut painted = block;
    painted.params[wire].normalized = amount;
    painted.params[wire].display =
        crate::fx::params::normalized_to_display_for_model(model, wire, amount);
    painted.params[wire].valid = true;
    let value = crate::fx::nano_fx_format_param(&painted, wire as u8);
    let short = short_param_name(name);
    set_fx_cell(&ui.app, cell as usize, &short, &value, amount);
    ui.window.request_redraw();
    mark_fx_finger(idx as u8, wire as u8);
    crate::link::set_param(idx as u8, wire as u8, amount);
}

fn short_param_name(full: &str) -> String {
    let full = if full == "Feedback Depth" {
        "Fbk Depth"
    } else {
        full.split('(').next().unwrap_or(full).trim_end()
    };
    let mut out = String::new();
    for ch in full.chars() {
        if out.len() + ch.len_utf8() > 11 {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        String::from("-")
    } else {
        out
    }
}

fn clear_fx_cells(app: &AppWindow) {
    for cell in 0..crate::fx::types::NANO_FX_MAX_PARAMS {
        set_fx_cell(app, cell, "-", "-", 0.0);
    }
}

fn set_fx_name(app: &AppWindow, cell: usize, name: &str) {
    let name = slint::SharedString::from(name);
    match cell {
        0 => app.set_fx_n0(name),
        1 => app.set_fx_n1(name),
        2 => app.set_fx_n2(name),
        3 => app.set_fx_n3(name),
        4 => app.set_fx_n4(name),
        5 => app.set_fx_n5(name),
        6 => app.set_fx_n6(name),
        7 => app.set_fx_n7(name),
        8 => app.set_fx_n8(name),
        9 => app.set_fx_n9(name),
        10 => app.set_fx_n10(name),
        11 => app.set_fx_n11(name),
        12 => app.set_fx_n12(name),
        13 => app.set_fx_n13(name),
        14 => app.set_fx_n14(name),
        15 => app.set_fx_n15(name),
        16 => app.set_fx_n16(name),
        17 => app.set_fx_n17(name),
        18 => app.set_fx_n18(name),
        19 => app.set_fx_n19(name),
        20 => app.set_fx_n20(name),
        21 => app.set_fx_n21(name),
        22 => app.set_fx_n22(name),
        23 => app.set_fx_n23(name),
        _ => {}
    }
}

fn set_fx_cell(app: &AppWindow, cell: usize, name: &str, value: &str, amount: f32) {
    let name = slint::SharedString::from(name);
    let value = slint::SharedString::from(value);
    match cell {
        0 => {
            app.set_fx_n0(name);
            app.set_fx_v0(value);
            app.set_fx_a0(amount);
        }
        1 => {
            app.set_fx_n1(name);
            app.set_fx_v1(value);
            app.set_fx_a1(amount);
        }
        2 => {
            app.set_fx_n2(name);
            app.set_fx_v2(value);
            app.set_fx_a2(amount);
        }
        3 => {
            app.set_fx_n3(name);
            app.set_fx_v3(value);
            app.set_fx_a3(amount);
        }
        4 => {
            app.set_fx_n4(name);
            app.set_fx_v4(value);
            app.set_fx_a4(amount);
        }
        5 => {
            app.set_fx_n5(name);
            app.set_fx_v5(value);
            app.set_fx_a5(amount);
        }
        6 => {
            app.set_fx_n6(name);
            app.set_fx_v6(value);
            app.set_fx_a6(amount);
        }
        7 => {
            app.set_fx_n7(name);
            app.set_fx_v7(value);
            app.set_fx_a7(amount);
        }
        8 => {
            app.set_fx_n8(name);
            app.set_fx_v8(value);
            app.set_fx_a8(amount);
        }
        9 => {
            app.set_fx_n9(name);
            app.set_fx_v9(value);
            app.set_fx_a9(amount);
        }
        10 => {
            app.set_fx_n10(name);
            app.set_fx_v10(value);
            app.set_fx_a10(amount);
        }
        11 => {
            app.set_fx_n11(name);
            app.set_fx_v11(value);
            app.set_fx_a11(amount);
        }
        12 => {
            app.set_fx_n12(name);
            app.set_fx_v12(value);
            app.set_fx_a12(amount);
        }
        13 => {
            app.set_fx_n13(name);
            app.set_fx_v13(value);
            app.set_fx_a13(amount);
        }
        14 => {
            app.set_fx_n14(name);
            app.set_fx_v14(value);
            app.set_fx_a14(amount);
        }
        15 => {
            app.set_fx_n15(name);
            app.set_fx_v15(value);
            app.set_fx_a15(amount);
        }
        16 => {
            app.set_fx_n16(name);
            app.set_fx_v16(value);
            app.set_fx_a16(amount);
        }
        17 => {
            app.set_fx_n17(name);
            app.set_fx_v17(value);
            app.set_fx_a17(amount);
        }
        18 => {
            app.set_fx_n18(name);
            app.set_fx_v18(value);
            app.set_fx_a18(amount);
        }
        19 => {
            app.set_fx_n19(name);
            app.set_fx_v19(value);
            app.set_fx_a19(amount);
        }
        20 => {
            app.set_fx_n20(name);
            app.set_fx_v20(value);
            app.set_fx_a20(amount);
        }
        21 => {
            app.set_fx_n21(name);
            app.set_fx_v21(value);
            app.set_fx_a21(amount);
        }
        22 => {
            app.set_fx_n22(name);
            app.set_fx_v22(value);
            app.set_fx_a22(amount);
        }
        23 => {
            app.set_fx_n23(name);
            app.set_fx_v23(value);
            app.set_fx_a23(amount);
        }
        _ => {}
    }
}

static FX_FINGER: Mutex<CriticalSectionRawMutex, RefCell<Option<(u8, u8, Instant)>>> =
    Mutex::new(RefCell::new(None));

fn mark_fx_finger(block: u8, wire: u8) {
    FX_FINGER.lock(|held| *held.borrow_mut() = Some((block, wire, Instant::now())));
}

fn fx_finger_holds(block: u8, wire: u8) -> bool {
    FX_FINGER.lock(|held| match *held.borrow() {
        Some((b, w, at)) => b == block && w == wire && at.elapsed() < FINGER_HOLD,
        None => false,
    })
}

fn pct(raw: u8) -> u16 {
    u16::from(raw) * 100 / 255
}

/// Returns true when the layout changed, so the caller repaints every pixel.
fn apply_layout(ui: &mut State) -> bool {
    let mode = ui.app.get_layout_mode();
    let (width, height, rotation, portrait) = match mode {
        1 => (HEIGHT, WIDTH, RenderingRotation::Rotate90, true),
        2 => (HEIGHT, WIDTH, RenderingRotation::Rotate270, true),
        3 => (WIDTH, HEIGHT, RenderingRotation::Rotate180, false),
        _ => (WIDTH, HEIGHT, RenderingRotation::NoRotation, false),
    };
    if ui.rotation == rotation {
        return false;
    }
    ui.rotation = rotation;
    ui.app.set_portrait(portrait);
    ui.window
        .set_size(PhysicalSize::new(width as u32, height as u32));
    ui.window.request_redraw();
    true
}

const FINGER_HOLD: Duration = Duration::from_millis(500);

static FINGER: Mutex<CriticalSectionRawMutex, RefCell<[Option<Instant>; 8]>> =
    Mutex::new(RefCell::new([None; 8]));
static LAST_SENT: Mutex<CriticalSectionRawMutex, RefCell<[Option<u8>; 8]>> =
    Mutex::new(RefCell::new([None; 8]));

fn send_slider(id: i32, amount: f32) {
    let id = id as u8;
    let amount = amount.clamp(0.0, 1.0);
    let scale = if id == 6 { 100.0 } else { 255.0 };
    let raw = (amount * scale).min(scale);
    let raw = (raw + 0.5).min(scale) as u8;
    if id == 6 {
        show_gate_amount(amount);
    } else {
        show_knob(id, raw);
    }
    queue_write(id, raw);
}

fn queue_write(id: u8, raw: u8) {
    let slot = id as usize;
    if slot >= 8 {
        return;
    }
    let fresh = LAST_SENT.lock(|sent| {
        let mut sent = sent.borrow_mut();
        if sent[slot] == Some(raw) {
            false
        } else {
            sent[slot] = Some(raw);
            true
        }
    });
    FINGER.lock(|held| held.borrow_mut()[slot] = Some(Instant::now()));
    if fresh {
        crate::ble::SLIDER.signal(crate::ble::SliderWrite { id, raw });
    }
}

fn finger_holds(id: u8) -> bool {
    let slot = id as usize;
    if slot >= 8 {
        return false;
    }
    FINGER.lock(|held| match held.borrow()[slot] {
        Some(at) => at.elapsed() < FINGER_HOLD,
        None => false,
    })
}

fn to_logical(rotation: RenderingRotation, x: u16, y: u16) -> (f32, f32) {
    let x = x as f32;
    let y = y as f32;
    let width = WIDTH as f32;
    let height = HEIGHT as f32;
    match rotation {
        RenderingRotation::Rotate90 => (y, width - 1.0 - x),
        RenderingRotation::Rotate180 => (width - 1.0 - x, height - 1.0 - y),
        RenderingRotation::Rotate270 => (height - 1.0 - y, x),
        RenderingRotation::NoRotation => (x, y),
        _ => (x, y),
    }
}
