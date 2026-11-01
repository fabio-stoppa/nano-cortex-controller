slint::include_modules!();

extern crate alloc;

#[cfg(test)]
mod bounce {
    pub const WIDTH: usize = 800;
    pub const HEIGHT: usize = 480;
}

#[cfg(test)]
#[path = "../../../src/ui/lines.rs"]
mod lines;

#[cfg(test)]
#[path = "../../../src/ui/presets.rs"]
mod presets;

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::software_renderer::{
        LineBufferProvider, MinimalSoftwareWindow, RenderingRotation, RepaintBufferType,
        Rgb565Pixel,
    };
    use slint::platform::{PointerEventButton, WindowEvent};
    use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
    use std::rc::Rc;

    fn check_catalog_refresh(app: &AppWindow) {
        use slint::Model;
        let names = vec!["Renamed Clean".to_string(), "".to_string()];
        for kind in 0..=5 {
            let header = ModelItem {
                name: "1-12".into(),
                header: true,
                index: -1,
                ..Default::default()
            };
            let preset = ModelItem {
                name: "Clean 2 Simple".into(),
                index: 1,
                col: 1,
                y: 76.0,
                section: 2,
                ..Default::default()
            };
            let deleted = ModelItem {
                name: "Deleted".into(),
                index: 2,
                ..Default::default()
            };
            let rows = Rc::new(slint::VecModel::from(vec![
                header.clone(),
                preset.clone(),
                deleted,
            ]));
            app.set_model_list(rows.clone().into());
            app.set_list_kind(kind);
            app.set_model_open(true);
            app.set_model_list_section(2);
            app.set_model_list_current(1);
            app.set_preset_menu_index(1);
            presets::refresh_preset_rows(app, &names);
            let updated = rows.row_data(1).unwrap();
            assert_eq!(
                updated.name.as_str(),
                if kind >= 3 {
                    "Renamed Clean"
                } else {
                    "Clean 2 Simple"
                }
            );
            assert_eq!(updated.col, preset.col);
            assert_eq!(updated.y, preset.y);
            assert_eq!(updated.section, preset.section);
            assert_eq!(rows.row_data(0).unwrap(), header);
            assert_eq!(
                rows.row_data(2).unwrap().name.as_str(),
                match kind {
                    3 => "",
                    4 | 5 => "(empty)",
                    _ => "Deleted",
                }
            );
            assert!(app.get_model_open());
            assert_eq!(app.get_model_list_section(), 2);
            assert_eq!(app.get_model_list_current(), 1);
            assert_eq!(app.get_preset_menu_index(), 1);
            app.set_model_open(false);
            presets::refresh_preset_rows(app, &["Ignored".to_string()]);
            assert_eq!(rows.row_data(1).unwrap(), updated);
        }
        app.set_model_open(false);
    }

    fn check_preset_menu_hit_target(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let picked = Rc::new(std::cell::Cell::new(0));
        app.on_model_chosen({
            let picked = picked.clone();
            move |_| picked.set(picked.get() + 1)
        });
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
        app.set_list_kind(3);
        for title in ["1-12", ""] {
            app.set_model_list(slint::ModelRc::new(slint::VecModel::from(vec![
                ModelItem {
                    name: title.into(),
                    header: true,
                    index: -1,
                    height: 76.0,
                    ..Default::default()
                },
                ModelItem {
                    name: "Clean 2 Simple".into(),
                    index: 1,
                    ..Default::default()
                },
            ])));
            app.set_model_list_height(76.0);
            app.set_model_list_section(0);
            let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
            app.set_model_open(false);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            for (x, y) in [(203.0, 92.0), (210.0, 113.0), (263.0, 155.0)] {
                app.set_model_open(true);
                app.set_preset_menu_index(-1);
                window.request_redraw();
                render(window, &mut pixels, RenderingRotation::NoRotation, true);
                let position = LogicalPosition::new(x, y);
                window.dispatch_event(WindowEvent::PointerPressed {
                    position,
                    button: PointerEventButton::Left,
                });
                render(window, &mut pixels, RenderingRotation::NoRotation, false);
                window.dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
                assert_eq!(app.get_preset_menu_index(), 1, "menu tap at {x},{y}");
                assert_eq!(picked.get(), 0, "menu tap selected the preset");
            }
            app.set_preset_menu_index(-1);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            let position = LogicalPosition::new(100.0, 125.0);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
            assert_eq!(picked.get(), 1, "preset body no longer selects");
            assert!(app.get_model_open(), "preset pick closed the popup");
            assert_eq!(app.get_model_list_current(), 1);
            picked.set(0);
        }
        app.set_list_kind(1);
        app.set_model_list_current(-1);
        window.request_redraw();
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        render(window, &mut pixels, RenderingRotation::NoRotation, true);
        let position = LogicalPosition::new(100.0, 125.0);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(picked.get(), 1);
        assert!(!app.get_model_open(), "capture pick kept the popup open");
        app.set_list_kind(3);
        app.set_model_open(false);
        app.set_preset_menu_index(-1);
    }

    fn check_two_line_popup_names(app: &AppWindow, window: &MinimalSoftwareWindow) {
        app.set_model_list_current(-1);
        app.set_model_list(slint::ModelRc::new(slint::VecModel::from(vec![
            ModelItem {
                name: "1-12".into(),
                header: true,
                index: -1,
                height: 76.0,
                ..Default::default()
            },
            ModelItem {
                name: "Clean capture with a long descriptive name and extra words to wrap".into(),
                index: 1,
                ..Default::default()
            },
        ])));
        for kind in 1..=5 {
            app.set_list_kind(kind);
            app.set_model_open(true);
            let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            let mut lines = 0;
            let mut previous_ink = false;
            let expected = if kind >= 3 { 3 } else { 2 };
            for y in 104..196 {
                let ink = (26..270).any(|x| pixels[y * 800 + x].0 == 0xef7d);
                if ink && !previous_ink {
                    lines += 1;
                }
                previous_ink = ink;
            }
            assert_eq!(
                lines, expected,
                "popup kind {kind} did not show exactly {expected} text lines"
            );
            app.set_model_open(false);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
        }
    }

    fn check_backspace_repeat(
        app: &AppWindow,
        window: &MinimalSoftwareWindow,
        time: &std::cell::Cell<std::time::Duration>,
    ) {
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
        app.set_kbd_open(true);
        let mut px = vec![Rgb565Pixel(0); 800 * 480];
        window.request_redraw();
        render(window, &mut px, RenderingRotation::NoRotation, true);
        let backspace = LogicalPosition::new(750.0, 380.0);
        let mut hold = |text: &str, ms: u64| {
            app.set_kbd_text(text.into());
            window.dispatch_event(WindowEvent::PointerPressed {
                position: backspace,
                button: PointerEventButton::Left,
            });
            for _ in 0..ms / 10 {
                time.set(time.get() + std::time::Duration::from_millis(10));
                slint::platform::update_timers_and_animations();
                window.dispatch_event(WindowEvent::PointerMoved {
                    position: backspace,
                });
                render(window, &mut px, RenderingRotation::NoRotation, false);
            }
            window.dispatch_event(WindowEvent::PointerReleased {
                position: backspace,
                button: PointerEventButton::Left,
            });
            render(window, &mut px, RenderingRotation::NoRotation, false);
            app.get_kbd_text().to_string()
        };
        assert_eq!(hold("Summer Drive", 50), "Summer Driv", "tap deletes one");
        assert_eq!(
            hold("Summer Drive", 350),
            "Summer Driv",
            "repeated before the 400ms delay"
        );
        // 400ms pause, then every 60ms: 1 + 5 repeats by 700ms.
        assert_eq!(
            hold("Summer Drive", 700),
            "Summer",
            "hold did not repeat every 60ms"
        );
        assert_eq!(hold("Summer Drive", 2000), "", "long hold leaves text");
        assert_eq!(
            hold("Summer Drive", 50),
            "Summer Driv",
            "repeat leaked into next tap"
        );
        app.set_kbd_open(false);
        app.set_kbd_text("".into());
        window.request_redraw();
        render(window, &mut px, RenderingRotation::NoRotation, true);
    }

    fn check_long_names(app: &AppWindow, window: &MinimalSoftwareWindow) {
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
        app.set_model_enabled(true);
        app.set_bpm_text("114 BPM".into());
        let mut px = vec![Rgb565Pixel(0); 800 * 480];
        let mut header = |name: &str| {
            app.set_preset(name.into());
            window.request_redraw();
            render(window, &mut px, RenderingRotation::NoRotation, true);
            px[..48 * 800].to_vec()
        };
        let short = header("Clean");
        let long = header(&"Really Long Preset Name ".repeat(3));
        for y in 0..48 {
            for x in (0..214).chain(586..800) {
                assert_eq!(
                    short[y * 800 + x].0,
                    long[y * 800 + x].0,
                    "title drew over {x},{y}"
                );
            }
        }
        app.set_preset("-".into());

        app.set_kbd_open(true);
        let mut name = |text: String| {
            app.set_kbd_text(text.into());
            window.request_redraw();
            render(window, &mut px, RenderingRotation::NoRotation, false);
            px.clone()
        };
        let empty = name(String::new());
        let ending = name(format!("Q{}END", "W".repeat(40)));
        assert!(ending != empty, "long name is not drawn");
        assert!(
            name(format!("Z{}END", "W".repeat(40))) == ending,
            "long name shows its start instead of its end"
        );

        let position = LogicalPosition::new(300.0, 300.0);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        let before = px.clone();
        let glow = render(window, &mut px, RenderingRotation::NoRotation, false);
        assert!(glow > 0 && glow < 6000, "key press repainted {glow} pixels");
        assert!(px != before, "pressed key does not glow");
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        render(window, &mut px, RenderingRotation::NoRotation, false);
        app.set_kbd_open(false);
        app.set_kbd_text("".into());
        window.request_redraw();
        render(window, &mut px, RenderingRotation::NoRotation, true);
    }

    struct Platform {
        window: Rc<MinimalSoftwareWindow>,
        time: Rc<std::cell::Cell<std::time::Duration>>,
    }

    impl slint::platform::Platform for Platform {
        fn create_window_adapter(
            &self,
        ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
            Ok(self.window.clone())
        }

        fn duration_since_start(&self) -> std::time::Duration {
            self.time.get()
        }
    }

    fn render(
        window: &MinimalSoftwareWindow,
        pixels: &mut [Rgb565Pixel],
        rotation: RenderingRotation,
        full: bool,
    ) -> usize {
        struct Counted {
            inner: lines::PartialLines,
            pixels: usize,
        }
        impl LineBufferProvider for &mut Counted {
            type TargetPixel = Rgb565Pixel;
            fn process_line(
                &mut self,
                line: usize,
                range: std::ops::Range<usize>,
                render: impl FnOnce(&mut [Rgb565Pixel]),
            ) {
                self.pixels += range.len();
                self.inner.process_line(line, range, render);
            }
        }
        let flipped = rotation == RenderingRotation::Rotate180;
        let mut counted = Counted {
            inner: lines::PartialLines::new(pixels.as_mut_ptr(), flipped),
            pixels: 0,
        };
        window.draw_if_needed(|renderer| {
            renderer.set_rendering_rotation(if flipped {
                RenderingRotation::NoRotation
            } else {
                rotation
            });
            if full {
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
            }
            renderer.render_by_line(&mut counted);
            renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
        });
        counted.pixels
    }

    fn check_fullscreen_popups(app: &AppWindow, window: &MinimalSoftwareWindow) {
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
        app.set_page(0);
        app.set_chain(2);
        app.set_on_0(true);
        app.set_category_0(9);
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let gray = ((30u16 >> 3) << 11) | ((30u16 >> 2) << 5) | (30u16 >> 3);
        for popup in 0..4 {
            app.set_model_open(popup == 0);
            app.set_settings_open(popup == 1);
            app.set_confirm_open(popup == 2);
            app.set_kbd_open(popup == 3);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            // Edge pixels, including where the first FX tab sits, show only the popup surface.
            // The confirmation is a card: same gray in the middle, dimmed content around it.
            for (x, y) in [(5, 5), (5, 100), (5, 470), (795, 240), (400, 476)] {
                let pixel = pixels[y * 800 + x].0;
                if popup == 2 {
                    assert!(pixel >> 11 < gray >> 11, "confirm backdrop not dimmed at {x},{y}");
                } else {
                    assert_eq!(pixel, gray, "popup {popup} not fullscreen at {x},{y}");
                }
            }
            if popup == 2 {
                assert_eq!(pixels[240 * 800 + 400].0, gray, "confirm card not main gray");
            }
            let position = LogicalPosition::new(5.0, 470.0);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
            assert!(
                app.get_model_open()
                    || app.get_settings_open()
                    || app.get_confirm_open()
                    || app.get_kbd_open(),
                "edge tap dismissed popup {popup}"
            );
        }
        app.set_kbd_open(false);
        app.set_on_0(false);
        app.set_category_0(0);

        app.set_bt_devices(slint::ModelRc::new(slint::VecModel::from(vec![BtDevice {
            name: "Nano Cortex".into(),
            kind: "Amp".into(),
            state: 2,
        }])));
        app.set_settings_open(true);
        app.set_settings_tab(0);
        assert!(!app.get_bt_open());
        app.set_settings_tab(4);
        assert!(app.get_bt_open(), "Bluetooth tab not reported as open");
        window.request_redraw();
        render(window, &mut pixels, RenderingRotation::NoRotation, true);
        let blue = ((0x1au16 >> 3) << 11) | ((0x3au16 >> 2) << 5) | (0xf8u16 >> 3);
        assert!(
            pixels.iter().filter(|p| p.0 == blue).count() > 10_000,
            "connected device row missing from the Bluetooth tab"
        );
        app.set_settings_open(false);
        app.set_settings_tab(0);
        assert!(!app.get_bt_open());
    }

    fn check_fx_cell_contrast(app: &AppWindow, window: &MinimalSoftwareWindow) {
        app.set_page(1);
        app.set_fx_n0("Mix".into());
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        for portrait in [false, true] {
            app.set_portrait(portrait);
            let width = if portrait { 480 } else { 800 };
            window.set_size(PhysicalSize::new(width, if portrait { 800 } else { 480 }));
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let (x, y) = if portrait { (30, 125) } else { (150, 75) };
            let cell = ((38u16 >> 3) << 11) | ((38u16 >> 2) << 5) | (38u16 >> 3);
            assert_eq!(pixels[y * width as usize + x].0, cell);
            assert_ne!(pixels[y * width as usize + x].0, pixels[0].0);
            if !portrait {
                app.set_page(0);
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                // Inside the INPUT GATE cell, left of its label.
                assert_eq!(pixels[215 * 800 + 175].0, cell, "amp cell not lightened");
                app.set_page(1);
            }

        }
        app.set_fx_n0("-".into());
        app.set_page(0);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_midi_clear_confirmation(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let cleared = Rc::new(std::cell::Cell::new(0));
        let preset_action = Rc::new(std::cell::Cell::new(0));
        app.on_midi_clear_all({
            let cleared = cleared.clone();
            move || cleared.set(cleared.get() + 1)
        });
        app.on_confirm_accepted({
            let preset_action = preset_action.clone();
            move || preset_action.set(preset_action.get() + 1)
        });
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let red = ((0xf8u16 >> 3) << 11) | ((0x28u16 >> 2) << 5) | (0x1au16 >> 3);
        let click = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position, button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position, button: PointerEventButton::Left,
            });
        };
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_kbd_open(false);
            app.set_model_open(false);
            app.set_settings_open(true);
            app.set_settings_tab(3);
            app.set_midi_last("CC 7 ch 1".into());
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let previous = pixels.clone();
            app.set_midi_last("Note 9 ch 1".into());
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let last_bottom = (0..height as usize).rev().find(|&y| {
                (0..width as usize).any(|x| pixels[y * width as usize + x].0 != previous[y * width as usize + x].0)
            }).expect("Last MIDI message is not rendered");
            let grey = ((0x90u16 >> 3) << 11) | ((0x90u16 >> 2) << 5) | (0x90u16 >> 3);
            let help_top = (last_bottom + 1..last_bottom + 40).find(|&y| {
                (14..width as usize - 140).any(|x| pixels[y * width as usize + x].0 == grey)
            }).expect("Shared-message help is not below the last message");
            assert!(help_top - last_bottom <= 17, "MIDI header gap is {} pixels", help_top - last_bottom);
            click(width as f32 - 70.0, last_bottom as f32 + 6.0);
            assert!(app.get_confirm_open());
            assert_eq!(app.get_confirm_title(), "Clear all MIDI bindings?");
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let mut left = width as usize;
            let mut right = 0;
            let mut top = height as usize;
            let mut bottom = 0;
            for (i, pixel) in pixels.iter().enumerate() {
                if pixel.0 == red {
                    left = left.min(i % width as usize);
                    right = right.max(i % width as usize);
                    top = top.min(i / width as usize);
                    bottom = bottom.max(i / width as usize);
                }
            }
            assert!(right > left && bottom > top, "Missing destructive confirmation button");
            let x = (left + right) as f32 / 2.0;
            let y = (top + bottom) as f32 / 2.0;
            let before = cleared.get();
            click(5.0, 5.0);
            assert!(app.get_confirm_open(), "Outside tap must not confirm or dismiss");
            click(width as f32 - x, y);
            assert!(!app.get_confirm_open(), "Cancel must close confirmation");
            assert_eq!(cleared.get(), before, "Cancel cleared MIDI bindings");
            app.invoke_midi_clear_all_requested();
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            click(x, y);
            assert!(!app.get_confirm_open());
            assert_eq!(cleared.get(), before + 1);
            assert_eq!(preset_action.get(), 0, "MIDI confirmation triggered preset operation");
        }
        app.set_settings_open(false);
        app.set_settings_tab(0);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_bank_screen(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let additions = Rc::new(std::cell::Cell::new(0));
        app.on_performance_add({
            let additions = additions.clone();
            move || additions.set(additions.get() + 1)
        });
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        app.set_performance_banks(slint::ModelRc::new(slint::VecModel::from(vec!["Set 1".into(), "Set 2".into()])));
        app.set_performance_slots(slint::ModelRc::new(slint::VecModel::from(vec![
            "1: 4 Clean".into(), "2: 8 Lead".into(), "3: (unassigned)".into(), "4: (unassigned)".into(),
        ])));
        app.set_performance_notice("".into());
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_connecting(true);
            app.set_performance_open(true);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let before = additions.get();
            let position = LogicalPosition::new(100.0, 116.0);
            window.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            window.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
            assert_eq!(additions.get(), before + 1, "Banks screen is covered or Add is inaccessible");
        }
        app.set_performance_open(false);
        app.set_connecting(false);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_usb_storage_screen(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let started = Rc::new(std::cell::Cell::new(0));
        let cancelled = Rc::new(std::cell::Cell::new(0));
        let restarted = Rc::new(std::cell::Cell::new(0));
        app.on_midi_usb_storage({
            let started = started.clone();
            move || started.set(started.get() + 1)
        });
        app.on_usb_storage_cancel({
            let cancelled = cancelled.clone();
            move || cancelled.set(cancelled.get() + 1)
        });
        app.on_usb_storage_restart({
            let restarted = restarted.clone();
            move || restarted.set(restarted.get() + 1)
        });
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let background = ((30u16 >> 3) << 11) | ((30u16 >> 2) << 5) | (30u16 >> 3);
        let click = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position, button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position, button: PointerEventButton::Left,
            });
        };
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_settings_open(false);
            app.set_model_open(false);
            app.set_kbd_open(false);
            app.set_confirm_open(false);
            app.set_connecting(true);
            app.set_usb_storage_open(false);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let starts_before = started.get();
            click(width as f32 - 62.0, 28.0);
            assert_eq!(started.get(), starts_before + 1, "Offline USB entry is inaccessible");
            app.set_usb_storage_open(true);
            app.set_usb_storage_finished(false);
            app.set_usb_storage_message("Edit midi.json, then safely eject the drive.".into());
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            assert_eq!(pixels[5 * width as usize + 5].0, background, "USB screen covered by connecting overlay");
            click(width as f32 - 62.0, 28.0);
            assert_eq!(started.get(), starts_before + 1, "Underlying connecting controls still active");
            let before = cancelled.get();
            click(width as f32 / 2.0, height as f32 - 50.0);
            assert_eq!(cancelled.get(), before + 1);
            assert_eq!(restarted.get(), before);
            app.set_usb_storage_finished(true);
            app.set_usb_storage_message("Configuration saved. Restart controller.".into());
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            click(width as f32 / 2.0, height as f32 - 50.0);
            assert_eq!(restarted.get(), before + 1);
            assert_eq!(cancelled.get(), before + 1);
        }
        app.set_usb_storage_open(false);
        app.set_connecting(false);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_source_card_names(app: &AppWindow, window: &MinimalSoftwareWindow) {
        app.set_page(0);
        app.set_capture_active(true);
        app.set_capture_value("Rew D20 Clean NF53 with a long capture name".into());
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        window.request_redraw();
        render(window, &mut pixels, RenderingRotation::NoRotation, true);
        // Card spans x 130..452: no outline means its edge is plain black.
        assert_eq!(pixels[85 * 800 + 130].0, 0, "capture card still has an outline");
        assert_eq!(pixels[85 * 800 + 452].0, 0, "capture card still has an outline");
        let mut lines = 0;
        let mut previous_ink = false;
        for y in 64..136 {
            let ink = (176..453).any(|x| pixels[y * 800 + x].0 == 0xef7d);
            if ink && !previous_ink {
                lines += 1;
            }
            previous_ink = ink;
            assert!(
                (453..461).all(|x| pixels[y * 800 + x].0 != 0xef7d),
                "capture name spilled past its card"
            );
        }
        assert_eq!(lines, 2, "capture name did not wrap to two lines");
        app.set_capture_value("-".into());
        app.set_capture_active(false);
    }

    fn check_fx_category_colors(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let palette = [
            (248u8, 138u8, 26u8),
            (248, 193, 26),
            (248, 240, 26),
            (240, 26, 248),
            (136, 248, 26),
            (26, 248, 88),
            (248, 26, 138),
            (136, 26, 248),
            (26, 193, 248),
            (26, 58, 248),
        ];
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        app.set_page(0);
        app.set_chain(2);
        for portrait in [false, true] {
            app.set_portrait(portrait);
            let width = if portrait { 480 } else { 800 };
            window.set_size(PhysicalSize::new(width, if portrait { 800 } else { 480 }));
            for category in 0..=10 {
                app.set_category_0(category);
                app.set_category_1(category);
                app.set_category_3(category);
                app.set_category_4(category);
                app.set_category_5(category);
                let (r, g, b) = if category == 0 {
                    (144, 144, 144)
                } else {
                    let rgb = palette[category as usize - 1];
                    // Equal RGB extrema give identical HSV/HSL saturation and brightness.
                    assert_eq!(rgb.0.min(rgb.1).min(rgb.2), 26);
                    assert_eq!(rgb.0.max(rgb.1).max(rgb.2), 248);
                    rgb
                };
                let expected = ((r as u16 >> 3) << 11)
                    | ((g as u16 >> 2) << 5)
                    | (b as u16 >> 3);
                for active in [true, false] {
                    app.set_on_0(active);
                    app.set_on_1(active);
                    app.set_on_3(active);
                    app.set_on_4(active);
                    app.set_on_5(active);
                    window.request_redraw();
                    window.draw_if_needed(|renderer| {
                        renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                        renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                        renderer.render(&mut pixels, width as usize);
                        renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                    });
                    for slot in [0, 1, 3, 4, 5] {
                        // Portrait tabs are 69px wide; the selected AMP tab is wider.
                        let tab_x = [14, 87, 160, 251, 324, 397][slot];
                        let (x, y) = if portrait {
                            (tab_x + 11, 744)
                        } else {
                            (20, 70 + 67 * slot)
                        };
                        assert_eq!(
                            pixels[y * width as usize + x].0,
                            if active { expected } else { 0 },
                            "category {category}, slot {slot}, portrait {portrait}, active {active}"
                        );
                        if !active {
                            let (left, right, top, bottom) = if portrait {
                                (tab_x + 1, tab_x + 68, 734, 786)
                            } else {
                                (14, 125, 64 + 68 * slot, 119 + 68 * slot)
                            };
                            assert!(
                                (top..bottom).any(|row| {
                                    (left..right).any(|col| {
                                        pixels[row * width as usize + col].0 == expected
                                    })
                                }),
                                "bypassed category {category} lost its colored label in slot {slot}"
                            );
                        }
                    }
                }
            }
        }
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
        app.set_category_0(0);
        app.set_category_1(0);
        app.set_category_3(0);
        app.set_category_4(0);
        app.set_category_5(0);
    }

    #[test]
    fn flipped_partial_spans_preserve_pixel_order_and_untouched_pixels() {
        use crate::bounce::{HEIGHT, WIDTH};
        let mut pixels: Vec<_> = (0..WIDTH * HEIGHT)
            .map(|index| Rgb565Pixel(index as u16))
            .collect();
        let before = pixels.clone();
        let row = 27;
        let range = 31..48;
        let mut provider = lines::PartialLines::new(pixels.as_mut_ptr(), true);
        provider.process_line(row, range.clone(), |span| {
            for (offset, pixel) in span.iter_mut().enumerate() {
                let physical = (HEIGHT - 1 - row) * WIDTH + WIDTH - 1 - range.start - offset;
                assert_eq!(pixel.0, before[physical].0);
                pixel.0 = 0xffff;
            }
        });
        let start = (HEIGHT - 1 - row) * WIDTH + WIDTH - range.end;
        for (index, pixel) in pixels.iter().enumerate() {
            let expected = if (start..start + range.len()).contains(&index) {
                0xffff
            } else {
                before[index].0
            };
            assert_eq!(pixel.0, expected, "pixel {index}");
        }
    }

    #[test]
    fn rename_keyboard_renders_in_all_orientations() {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        let time = Rc::new(std::cell::Cell::new(std::time::Duration::ZERO));
        slint::platform::set_platform(Box::new(Platform {
            window: window.clone(),
            time: time.clone(),
        }))
        .unwrap();
        let app = AppWindow::new().unwrap();
        check_catalog_refresh(&app);
        app.set_connecting(false);
        app.on_kbd_key({
            let weak = app.as_weak();
            move |key| {
                let app = weak.upgrade().unwrap();
                let mut text = app.get_kbd_text().to_string();
                if key == "\u{8}" {
                    text.pop();
                } else if text.len() < 64 {
                    text.push_str(key.as_str());
                }
                app.set_kbd_error(
                    if text.len() < 4 {
                        "At least 4 characters"
                    } else {
                        ""
                    }
                    .into(),
                );
                app.set_kbd_text(text.into());
            }
        });
        app.show().unwrap();
        check_long_names(&app, &window);
        check_backspace_repeat(&app, &window, &time);
        check_preset_menu_hit_target(&app, &window);
        check_two_line_popup_names(&app, &window);
        check_source_card_names(&app, &window);
        check_fx_category_colors(&app, &window);
        check_fullscreen_popups(&app, &window);
        check_fx_cell_contrast(&app, &window);
        check_midi_clear_confirmation(&app, &window);
        check_usb_storage_screen(&app, &window);
        check_bank_screen(&app, &window);
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];

        for (width, height, rotation, portrait) in [
            (800, 480, RenderingRotation::NoRotation, false),
            (480, 800, RenderingRotation::Rotate90, true),
            (480, 800, RenderingRotation::Rotate270, true),
            (800, 480, RenderingRotation::Rotate180, false),
        ] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_kbd_open(false);
            window.request_redraw();
            render(&window, &mut pixels, rotation, true);
            app.set_kbd_title("Rename preset 2".into());
            app.set_kbd_open(true);
            app.set_kbd_text("Clean 2 Simple".into());
            window.request_redraw();
            render(&window, &mut pixels, rotation, false);
            let cursor_on = pixels.clone();
            time.set(time.get() + std::time::Duration::from_millis(501));
            slint::platform::update_timers_and_animations();
            let blink_pixels = render(&window, &mut pixels, rotation, false);
            assert!(
                blink_pixels <= 500,
                "cursor blink repainted more than its glyph"
            );
            assert!(pixels != cursor_on, "cursor did not blink off");
            time.set(time.get() + std::time::Duration::from_millis(501));
            slint::platform::update_timers_and_animations();
            render(&window, &mut pixels, rotation, false);
            assert!(pixels == cursor_on, "cursor did not blink back on");
            for name in [
                "Summer Drive",
                "Clean 2 Simple",
                "",
                "1234",
                &"W".repeat(64),
            ] {
                app.set_kbd_text(name.into());
                window.request_redraw();
                render(&window, &mut pixels, rotation, false);
                for len in (0..name.len()).rev().chain(0..=name.len()) {
                    app.set_kbd_text(name[..len].into());
                    slint::platform::update_timers_and_animations();
                    window.request_redraw();
                    let painted = render(&window, &mut pixels, rotation, false);
                    assert!(
                        painted < 800 * 480 / 2,
                        "keystroke repainted {painted} pixels for {name:?}, len {len}"
                    );
                }
            }
            for y in (100..height.min(450)).step_by(23) {
                for x in (25..width - 25).step_by(23) {
                    app.set_kbd_open(true);
                    app.set_kbd_text("Summer Drive".into());
                    let position = LogicalPosition::new(x as f32, y as f32);
                    for event in [
                        WindowEvent::PointerPressed {
                            position,
                            button: PointerEventButton::Left,
                        },
                        WindowEvent::PointerReleased {
                            position,
                            button: PointerEventButton::Left,
                        },
                    ] {
                        window.dispatch_event(event);
                        slint::platform::update_timers_and_animations();
                        window.request_redraw();
                        render(&window, &mut pixels, rotation, false);
                    }
                }
            }
            assert!(pixels.iter().any(|pixel| pixel.0 != 0));
            let incremental = pixels.clone();
            window.request_redraw();
            render(&window, &mut pixels, rotation, true);
            assert_eq!(
                incremental, pixels,
                "partial redraw differs from full redraw"
            );
            if rotation == RenderingRotation::Rotate180 {
                let mut reference = vec![Rgb565Pixel(0); pixels.len()];
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer
                        .render_by_line(lines::PartialLines::new(reference.as_mut_ptr(), false));
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                reference.reverse();
                assert!(
                    pixels == reference,
                    "flipped output differs from a 180-degree rotated full frame"
                );
            }
        }
    }
}
