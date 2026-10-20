//! Slint screens for the 800×480 panel.
//!
//! The layout follows the LVGL UI: header, chain rail, amp / effect / tuner /
//! tempo pages, and the connecting overlay. Landscape is the panel's scan.
//! Portrait is a taller layout rotated into that same framebuffer.

extern crate alloc;

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::UnsafeCell;

use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, RenderingRotation, RepaintBufferType, Rgb565Pixel,
};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant};
use slint::{LogicalPosition, PhysicalSize};

use crate::bounce::{Framebuffer, HEIGHT, WIDTH};
use crate::fx::types::NanoFxState;
use crate::link::Cmd;

slint::include_modules!();

struct Backend {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for Backend {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
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
    fx_page: usize,
    amp_title: String,
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
    let weak = app.as_weak();
    app.on_chain_tapped({
        let weak = weak.clone();
        move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            leave_tools(&app);
            app.set_chain(index);
            if let Some(ui) = state().as_mut() {
                ui.fx_page = 0;
            }
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
                if app.get_page() == 3 {
                    crate::link::push_cmd(Cmd::TempoExit);
                }
                app.set_page(2);
                app.set_model("Tuner".into());
                app.set_tuner_active(true);
                crate::link::push_cmd(Cmd::Tuner(true));
            }
        }
    });
    app.on_tempo_opened({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if app.get_page() == 3 {
                crate::link::push_cmd(Cmd::TempoExit);
                show_home(&app);
            } else {
                if app.get_page() == 2 {
                    app.set_tuner_active(false);
                    crate::link::push_cmd(Cmd::Tuner(false));
                }
                app.set_page(3);
                app.set_model("Tempo".into());
                crate::link::push_cmd(Cmd::TempoEnter);
            }
        }
    });
    app.on_tempo_tapped(move || {
        crate::link::push_cmd(Cmd::Tempo);
    });
    app.on_preset_stepped(move |delta| {
        let delta = delta.clamp(-1, 1) as i8;
        if delta != 0 {
            crate::link::push_cmd(Cmd::Preset(delta));
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
    app.on_save_tapped(move || crate::link::push_cmd(Cmd::Save));
    app.on_copy_tapped(move || crate::link::push_cmd(Cmd::Copy));
    app.on_library_tapped(move || crate::link::push_cmd(Cmd::Library));
    app.on_param_changed(move |cell, amount| send_fx_param(cell, amount));
    app.on_param_paged(move |delta| page_fx(delta));
    app.on_layout_tapped({
        let weak = weak.clone();
        move |mode| {
            if let Some(app) = weak.upgrade() {
                app.set_layout_mode(mode);
            }
        }
    });
    app.on_slider_moved(move |id, amount| {
        send_slider(id, amount);
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
        fx_page: 0,
        amp_title: String::from("Amp"),
    });
}

/// One scan line in internal RAM. Slint blends into this, then the finished
/// span is stored in the PSRAM frame. Blending straight into PSRAM contends
/// with the bounce copy and the picture breaks up.
struct PartialLines {
    fb: *mut Rgb565Pixel,
    line: [Rgb565Pixel; WIDTH],
}

impl LineBufferProvider for PartialLines {
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Rgb565Pixel]),
    ) {
        let start = range.start;
        let len = range.len();
        if start.saturating_add(len) > WIDTH || line >= HEIGHT {
            return;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(self.fb.add(line * WIDTH + start), self.line.as_mut_ptr(), len);
        }
        render_fn(&mut self.line[..len]);
        unsafe {
            core::ptr::copy_nonoverlapping(self.line.as_ptr(), self.fb.add(line * WIDTH + start), len);
        }
    }
}

fn redraw(framebuffer: &mut Framebuffer) {
    let ui = state().as_mut().unwrap();
    apply_layout(ui);
    slint::platform::update_timers_and_animations();
    let rotation = ui.rotation;
    let fb = framebuffer.as_pixels_mut().as_mut_ptr().cast::<Rgb565Pixel>();
    ui.window.draw_if_needed(|renderer| {
        renderer.set_rendering_rotation(rotation);
        renderer.render_by_line(PartialLines {
            fb,
            line: [Rgb565Pixel(0); WIDTH],
        });
    });
}

pub fn set_connected(connected: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_connecting(!connected);
    ui.app.set_ble_on(connected);
    ui.app.set_model_enabled(connected);
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

pub fn show_preset(name: &str) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_preset(name.into());
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
    ui.app.set_tempo_bpm(alloc::format!("{bpm}").into());
    ui.window.request_redraw();
}

pub fn clear_bpm() {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.app.set_bpm_text("- BPM".into());
    ui.app.set_tempo_bpm("-".into());
    ui.window.request_redraw();
}

pub fn show_tempo_page(active: bool) {
    ensure();
    let ui = state().as_mut().unwrap();
    if active {
        ui.app.set_page(3);
        ui.app.set_model("Tempo".into());
    } else if ui.app.get_page() == 3 {
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

pub fn show_tuner(note: &str, cents: &str, active: bool, has_pitch: bool, cents_f: f32) {
    ensure();
    let ui = state().as_mut().unwrap();
    let was = ui.app.get_tuner_active();
    let c = cents_f.clamp(-50.0, 50.0);
    let pos = if has_pitch {
        (c + 50.0) / 100.0
    } else {
        0.5
    };
    ui.app.set_tuner_active(active);
    ui.app.set_tuner_note(note.into());
    ui.app.set_tuner_cents(cents.into());
    ui.app.set_tuner_pos(pos);
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

pub fn show_chain(index: i32) {
    ensure();
    let ui = state().as_mut().unwrap();
    ui.fx_page = 0;
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
    if page == 3 {
        crate::link::push_cmd(Cmd::TempoExit);
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
        match chain {
            0 => {
                ui.app.set_tab_0(label.as_str().into());
                ui.app.set_on_0(on);
            }
            1 => {
                ui.app.set_tab_1(label.as_str().into());
                ui.app.set_on_1(on);
            }
            3 => {
                ui.app.set_tab_3(label.as_str().into());
                ui.app.set_on_3(on);
            }
            4 => {
                ui.app.set_tab_4(label.as_str().into());
                ui.app.set_on_4(on);
            }
            5 => {
                ui.app.set_tab_5(label.as_str().into());
                ui.app.set_on_5(on);
            }
            _ => {}
        }
    }
}

fn paint_fx_page(ui: &mut State) {
    let Some(idx) = fx_block_for_chain(ui.app.get_chain()) else {
        clear_fx_cells(&ui.app);
        ui.app.set_fx_page("".into());
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
    let pages = if count > 8 { (count + 7) / 8 } else { 1 };
    if ui.fx_page >= pages {
        ui.fx_page = 0;
    }
    let page_text = if count > 8 {
        alloc::format!("{} / {}", ui.fx_page + 1, pages)
    } else {
        String::new()
    };
    ui.app.set_fx_page(page_text.as_str().into());
    let model = unsafe { block.model.as_ref() };
    for cell in 0..8 {
        let display_i = ui.fx_page * 8 + cell;
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

fn page_fx(delta: i32) {
    let Some(ui) = state().as_mut() else {
        return;
    };
    if !ui.fx_valid {
        return;
    }
    let Some(idx) = fx_block_for_chain(ui.app.get_chain()) else {
        return;
    };
    let count = ui.fx.blocks[idx].param_count as usize;
    let pages = if count > 8 { (count + 7) / 8 } else { 1 };
    let next = (ui.fx_page as i32 + delta).clamp(0, pages as i32 - 1) as usize;
    if next == ui.fx_page {
        return;
    }
    ui.fx_page = next;
    paint_fx_page(ui);
    ui.window.request_redraw();
}

fn send_fx_param(cell: i32, amount: f32) {
    if !(0..8).contains(&cell) {
        return;
    }
    let amount = amount.clamp(0.0, 1.0);
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
    let display_i = ui.fx_page * 8 + cell as usize;
    if display_i >= block.param_count as usize {
        return;
    }
    let wire = crate::fx::nano_fx_param_wire_index(model, display_i);
    if wire >= block.param_count as usize {
        return;
    }
    let name = crate::fx::models::param_name(model, wire);
    let mut painted = block;
    painted.params[wire].normalized = amount;
    painted.params[wire].display = crate::fx::nano_fx_normalized_to_display(name, amount);
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
    for cell in 0..8 {
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

fn apply_layout(ui: &mut State) {
    let mode = ui.app.get_layout_mode();
    let (width, height, rotation, portrait) = match mode {
        1 => (
            HEIGHT,
            WIDTH,
            RenderingRotation::Rotate90,
            true,
        ),
        2 => (
            HEIGHT,
            WIDTH,
            RenderingRotation::Rotate270,
            true,
        ),
        3 => (
            WIDTH,
            HEIGHT,
            RenderingRotation::Rotate180,
            false,
        ),
        _ => (
            WIDTH,
            HEIGHT,
            RenderingRotation::NoRotation,
            false,
        ),
    };
    if ui.rotation == rotation {
        return;
    }
    ui.rotation = rotation;
    ui.app.set_portrait(portrait);
    ui.window
        .set_size(PhysicalSize::new(width as u32, height as u32));
    ui.window.request_redraw();
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
