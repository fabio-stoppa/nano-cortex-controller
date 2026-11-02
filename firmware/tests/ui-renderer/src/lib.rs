slint::include_modules!();

#[cfg(test)]
mod preview {
    include!(concat!(env!("OUT_DIR"), "/preview.rs"));
}

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

    fn check_preview_workflow(
        window: &MinimalSoftwareWindow,
        time: &Rc<std::cell::Cell<std::time::Duration>>,
    ) {
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        };
        for portrait in [false, true] {
            let (width, height) = if portrait { (480, 800) } else { (800, 480) };
            let app = preview::Preview::new().unwrap();
            app.set_preview_portrait(portrait);
            app.show().unwrap();
            window.set_size(PhysicalSize::new(width, height));
            let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
            let mut draw = || {
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                    renderer.render(&mut pixels, width as usize);
                });
            };
            draw();
            let position = LogicalPosition::new(40.0, if portrait { 720.0 } else { 90.0 });
            let original_on = app.get_preview_fx_on();
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            time.set(time.get() + std::time::Duration::from_millis(451));
            slint::platform::update_timers_and_animations();
            assert_eq!(app.get_preview_fx_on(), !original_on, "Holding FX did not toggle bypass");
            assert_eq!(app.get_popup(), "none", "Holding FX opened model selection");
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
            assert_eq!(app.get_preview_fx_on(), !original_on, "Releasing hold toggled FX again");
            tap(position.x, position.y);
            time.set(time.get() + std::time::Duration::from_millis(100));
            slint::platform::update_timers_and_animations();
            tap(position.x, position.y);
            assert_eq!(app.get_popup(), "model", "Double-tapping FX did not open selection");
            assert_eq!(app.get_preview_fx_on(), !original_on, "Double-tapping FX changed bypass");
            app.set_popup("none".into());
            slint::platform::update_timers_and_animations();
            for _ in 0..2 {
                draw();
                tap(
                    if portrait { 100.0 } else { width as f32 - 110.0 },
                    if portrait { 50.0 } else { 38.0 },
                );
                assert_eq!(app.get_popup(), "scenes");
                draw();
                tap(35.0, 35.0);
                assert_eq!(app.get_popup(), "none");
            }
            for popup in ["scenes", "gig", "none"] {
                app.set_popup(popup.into());
                draw();
                assert_eq!(
                    app.get_popup(),
                    popup,
                    "Preview popup navigation changed unexpectedly"
                );
            }
            app.hide().unwrap();
        }
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

    fn check_tempo_and_gig_title(
        app: &AppWindow,
        window: &MinimalSoftwareWindow,
        time: &Rc<std::cell::Cell<std::time::Duration>>,
    ) {
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let draw = |pixels: &mut [Rgb565Pixel], width| {
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(pixels, width);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
        };
        app.set_page(0);
        app.set_tempo_bpm_value(120);
        app.set_bpm_text("120 BPM".into());
        app.set_capture_value("Capture".into());
        app.set_cab_value("Cab".into());
        for portrait in [false, true] {
            let width = if portrait { 480 } else { 800 };
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, if portrait { 800 } else { 480 }));
            for gig in [false, true] {
                app.set_gig_open(gig);
                for global in [false, true] {
                    app.set_tempo_global(global);
                    draw(&mut pixels, width as usize);
                    time.set(time.get() + std::time::Duration::from_millis(500));
                    slint::platform::update_timers_and_animations();
                    draw(&mut pixels, width as usize);
                    let bright = pixels.clone();
                    time.set(time.get() + std::time::Duration::from_millis(160));
                    slint::platform::update_timers_and_animations();
                    draw(&mut pixels, width as usize);
                    let contrast = bright[..58 * width as usize]
                        .iter()
                        .zip(&pixels[..58 * width as usize])
                        .map(|(on, off)| (on.0 >> 11).saturating_sub(off.0 >> 11))
                        .max()
                        .unwrap();
                    assert!(
                        contrast >= 20,
                        "Beat flash has insufficient contrast: portrait={portrait}, gig={gig}, global={global}"
                    );
                }
                if gig {
                    for page in [0, 2] {
                        app.set_page(page);
                        app.set_preset("".into());
                        draw(&mut pixels, width as usize);
                        let empty = pixels.clone();
                        app.set_preset("Summer Drive".into());
                        assert_eq!(app.get_preset().as_str(), "Summer Drive");
                        draw(&mut pixels, width as usize);
                        let changed = (12..42)
                            .flat_map(|y| (66..width as usize - 170).map(move |x| y * width as usize + x))
                            .filter(|&index| pixels[index] != empty[index])
                            .count();
                        assert!(
                            changed > 100,
                            "Gig preset title is missing: portrait={portrait}, page={page}"
                        );
                    }
                    app.set_page(0);
                }
            }
        }
        app.set_tempo_bpm_value(0);
        app.set_tempo_global(true);
        app.set_gig_open(false);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
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
                    assert!(
                        pixel >> 11 < gray >> 11,
                        "confirm backdrop not dimmed at {x},{y}"
                    );
                } else {
                    assert_eq!(pixel, gray, "popup {popup} not fullscreen at {x},{y}");
                }
            }
            if popup == 2 {
                assert_eq!(
                    pixels[240 * 800 + 400].0,
                    gray,
                    "confirm card not main gray"
                );
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
            selected: false,
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

    fn check_header_tools_menu(app: &AppWindow, window: &MinimalSoftwareWindow) {
        fn render(
            window: &MinimalSoftwareWindow,
            pixels: &mut [Rgb565Pixel],
            rotation: RenderingRotation,
            fresh: bool,
        ) {
            use slint::platform::WindowAdapter;
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(rotation);
                if fresh {
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                }
                renderer.render(pixels, window.window().size().width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
        }
        let weak = app.as_weak();
        app.on_tools_opened(move || weak.unwrap().set_settings_open(true));
        let closed = Rc::new(std::cell::Cell::new(0));
        app.on_midi_closed({
            let closed = closed.clone();
            move || closed.set(closed.get() + 1)
        });
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        };
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        for portrait in [false, true] {
            let (width, height) = if portrait { (480, 800) } else { (800, 480) };
            app.set_portrait(portrait);
            app.set_page(0);
            app.set_model_enabled(true);
            window.set_size(PhysicalSize::new(width, height));
            for (row, section) in [(1, 2), (2, 1), (3, 0)] {
                app.set_settings_open(false);
                window.request_redraw();
                render(window, &mut pixels, RenderingRotation::NoRotation, true);
                let dot = ((0xe0u16 >> 3) << 11) | ((0xe0u16 >> 2) << 5) | (0xe0u16 >> 3);
                assert_eq!(
                    pixels[36 * width as usize + width as usize - 23].0,
                    dot,
                    "header dots are not aligned to the right edge"
                );
                tap(width as f32 - 35.0, 38.0);
                assert!(app.get_header_menu_open(), "menu button missed: portrait={portrait}, row={row}");
                assert!(!app.get_settings_open(), "dots opened Settings directly");
                window.request_redraw();
                render(window, &mut pixels, RenderingRotation::NoRotation, true);
                tap(width as f32 - 100.0, 94.0 + row as f32 * 59.0);
                assert!(!app.get_header_menu_open());
                assert!(app.get_settings_open());
                assert_eq!(app.get_tools_section(), section);
                assert_eq!(app.get_settings_tab(), 0);
                assert!(!app.get_bt_open());
                window.request_redraw();
                render(window, &mut pixels, RenderingRotation::NoRotation, true);
                if section != 0 {
                    // Every tab remains accessible, including via header scrolling in portrait.
                    for tab in 0..(if section == 1 { 4 } else { 14 }) {
                        for _ in 0..3 {
                            for x in (100..width - 14).step_by(12) {
                                tap(x as f32, 38.0);
                                window.request_redraw();
                                render(window, &mut pixels, RenderingRotation::NoRotation, true);
                                if (if section == 1 { app.get_pedal_tab() } else { app.get_midi_tab() }) == tab {
                                    break;
                                }
                            }
                            if (if section == 1 { app.get_pedal_tab() } else { app.get_midi_tab() }) == tab {
                                break;
                            }
                            window.dispatch_event(WindowEvent::PointerScrolled {
                                position: LogicalPosition::new(width as f32 - 100.0, 38.0),
                                delta_x: -200.0,
                                delta_y: 0.0,
                            });
                            window.request_redraw();
                            render(window, &mut pixels, RenderingRotation::NoRotation, true);
                        }
                        assert_eq!(
                            if section == 1 { app.get_pedal_tab() } else { app.get_midi_tab() },
                            tab,
                            "tab {tab} unreachable in section {section}, portrait={portrait}"
                        );
                    }
                }
                let before = closed.get();
                tap(35.0, 35.0);
                assert!(!app.get_settings_open());
                assert!(closed.get() > before, "closing tools did not cancel learning");
            }
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            tap(width as f32 - 35.0, 38.0);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            tap(20.0, height as f32 - 20.0);
            assert!(!app.get_header_menu_open());
            assert!(!app.get_settings_open(), "outside menu tap activated underlying UI");
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            tap(width as f32 - 35.0, 38.0);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            tap(width as f32 - 100.0, 94.0);
            assert!(app.get_gig_open(), "First header menu option did not open Gig mode");
            assert!(!app.get_header_menu_open());
            assert!(!app.get_settings_open());
            app.invoke_gig_mode_set(false);

            // The asterisk must survive elision and disappear when the Nano is saved.
            app.set_preset("A very long Nano preset name that needs to be elided".into());
            app.set_save_dirty(false);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            let clean = pixels.clone();
            app.set_save_dirty(true);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            let marker_x = if portrait { 322 } else { 542 };
            assert!(
                (20..54).any(|y| (marker_x..marker_x + 18).any(|x| {
                    pixels[y * width as usize + x] != clean[y * width as usize + x]
                })),
                "unsaved marker missing on an elided header, portrait={portrait}"
            );
            app.set_save_dirty(false);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            assert_eq!(pixels, clean, "saving did not remove the Nano marker");
        }
        app.set_preset("-".into());
        app.set_tools_section(0);
        app.set_pedal_tab(0);
        app.set_midi_tab(0);
        app.set_portrait(false);
        app.on_midi_closed(|| {});
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_tuner_and_welcome_layout(app: &AppWindow, window: &MinimalSoftwareWindow) {
        fn render(
            window: &MinimalSoftwareWindow,
            pixels: &mut [Rgb565Pixel],
            rotation: RenderingRotation,
            _fresh: bool,
        ) {
            use slint::platform::WindowAdapter;
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(rotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(pixels, window.window().size().width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
        }
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        };
        app.on_tuner_mute_tapped({
            let weak = app.as_weak();
            move || {
                let app = weak.unwrap();
                app.set_tuner_muted(!app.get_tuner_muted());
            }
        });
        app.on_tuner_tapped({
            let weak = app.as_weak();
            move || weak.unwrap().set_page(0)
        });
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let background = ((30u16 >> 3) << 11) | ((30u16 >> 2) << 5) | (30u16 >> 3);
        for portrait in [false, true] {
            let (width, height) = if portrait { (480, 800) } else { (800, 480) };
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_page(0);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            let header = pixels[..60 * width as usize].to_vec();
            for gig in [false, true] {
                app.set_gig_open(gig);
                let expected_header = if gig {
                    render(window, &mut pixels, RenderingRotation::NoRotation, true);
                    pixels[..60 * width as usize].to_vec()
                } else {
                    header.clone()
                };
                app.set_page(2);
                app.set_tuner_muted(false);
                window.request_redraw();
                render(window, &mut pixels, RenderingRotation::NoRotation, true);
                assert_eq!(
                    &pixels[..header.len()],
                    expected_header.as_slice(),
                    "Tuner lost its originating header: portrait={portrait}, gig={gig}"
                );
                assert_eq!(
                    pixels[90 * width as usize + 12].0,
                    background,
                    "Tuner display should use the screen background"
                );
                tap(30.0, height as f32 - 40.0);
                assert!(app.get_tuner_muted(), "Tuner mute control is unreachable");
                tap(width as f32 - 50.0, height as f32 - 40.0);
                assert_eq!(app.get_page(), 0, "Tuner EXIT control is unreachable");
            }
            app.set_gig_open(false);
            app.set_connecting(true);
            app.set_app_version("test".into());
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
            for y in 0..height as usize {
                for x in 0..width as usize {
                    if x < 12 || x >= width as usize - 12 || y < 12 || y >= height as usize - 12 {
                        assert_eq!(
                            pixels[y * width as usize + x].0,
                            background,
                            "Welcome content outside 12px margin: ({x}, {y}), portrait={portrait}"
                        );
                    }
                }
            }
            app.set_connecting(false);
        }
        app.set_app_version("".into());
        app.set_tuner_muted(false);
        app.set_portrait(false);
        app.on_tuner_mute_tapped(|| {});
        app.on_tuner_tapped(|| {});
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_fx_cell_contrast(
        app: &AppWindow,
        window: &MinimalSoftwareWindow,
        time: &Rc<std::cell::Cell<std::time::Duration>>,
    ) {
        app.set_page(1);
        app.set_fx_n0("Mix".into());
        app.set_fx_n1("Rate".into());
        app.set_fx_n2("Depth".into());
        app.set_fx_n3("Feedback".into());
        let changed = Rc::new(std::cell::Cell::new(-1));
        app.on_param_changed({
            let changed = changed.clone();
            move |index, _| changed.set(index)
        });
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
            let (x, y) = if portrait { (30, 75) } else { (150, 75) };
            let cell = ((38u16 >> 3) << 11) | ((38u16 >> 2) << 5) | (38u16 >> 3);
            assert_eq!(
                pixels[y * width as usize + x].0,
                cell,
                "FX cell not lightened: portrait={portrait}"
            );
            assert_ne!(pixels[y * width as usize + x].0, pixels[0].0);
            if portrait {
                for x in [30, 182, 334] {
                    assert_eq!(pixels[75 * width as usize + x].0, cell, "Portrait FX row needs three columns");
                }
                assert_eq!(pixels[183 * width as usize + 30].0, cell, "Fourth FX parameter did not wrap");
                let start = LogicalPosition::new(30.0, 225.0);
                window.dispatch_event(WindowEvent::PointerPressed {
                    position: start,
                    button: PointerEventButton::Left,
                });
                time.set(time.get() + std::time::Duration::from_millis(210));
                slint::platform::update_timers_and_animations();
                window.dispatch_event(WindowEvent::PointerMoved {
                    position: LogicalPosition::new(90.0, 225.0),
                });
                window.dispatch_event(WindowEvent::PointerReleased {
                    position: LogicalPosition::new(90.0, 225.0),
                    button: PointerEventButton::Left,
                });
                assert_eq!(changed.get(), 3, "Wrapped FX slider changed the wrong parameter");
            }
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
        app.set_fx_n1("-".into());
        app.set_fx_n2("-".into());
        app.set_fx_n3("-".into());
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
        let scene_library_clears = Rc::new(std::cell::RefCell::new(Vec::new()));
        app.on_scene_library_clear_all({
            let scene_library_clears = scene_library_clears.clone();
            move |kind| scene_library_clears.borrow_mut().push(kind)
        });
        let preferences_cleared = Rc::new(std::cell::Cell::new(0));
        app.on_preferences_clear_all({
            let preferences_cleared = preferences_cleared.clone();
            move || preferences_cleared.set(preferences_cleared.get() + 1)
        });
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let red = ((0xf8u16 >> 3) << 11) | ((0x28u16 >> 2) << 5) | (0x1au16 >> 3);
        let click = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        };
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_kbd_open(false);
            app.set_model_open(false);
            app.set_settings_open(true);
            app.set_tools_section(2);
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
            assert_eq!(pixels, previous, "Removed MIDI information row is still rendered");
            app.set_tools_section(0);
            app.set_settings_tab(0);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let mut button_span = 0;
            for x in 14..width as usize - 14 {
                if pixels[326 * width as usize + x].0 == 0 {
                    button_span += 1;
                    assert!(button_span < 140, "Tools buttons stretched in portrait={portrait}");
                } else {
                    button_span = 0;
                }
            }
            window.dispatch_event(WindowEvent::PointerScrolled {
                position: LogicalPosition::new(width as f32 / 2.0, 300.0),
                delta_x: 0.0,
                delta_y: -100_000.0,
            });
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let (button_y, buttons) = (100..height as usize).rev().find_map(|y| {
                let mut spans = Vec::new();
                let mut start = None;
                for x in 0..width as usize {
                    if pixels[y * width as usize + x].0 == 0 {
                        start.get_or_insert(x);
                    } else if let Some(left) = start.take() {
                        if (64..=140).contains(&(x - left)) {
                            spans.push((left + x) as f32 / 2.0);
                        }
                    }
                }
                (spans.len() == 3).then_some((y as f32, spans))
            }).expect("Clear row must expose three reachable buttons");
            click(buttons[0], button_y);
            assert!(app.get_confirm_open(), "clear MIDI missed portrait={portrait}");
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
            assert!(
                right > left && bottom > top,
                "Missing destructive confirmation button"
            );
            let x = (left + right) as f32 / 2.0;
            let y = (top + bottom) as f32 / 2.0;
            let before = cleared.get();
            click(5.0, 5.0);
            assert!(
                app.get_confirm_open(),
                "Outside tap must not confirm or dismiss"
            );
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
            assert_eq!(
                preset_action.get(),
                0,
                "MIDI confirmation triggered preset operation"
            );
            let midi_clears_before = cleared.get();
            for (kind, button_x, title) in [
                (2, buttons[1], "Clear all scenes?"),
                (3, buttons[2], "Reset controller preferences?"),
            ] {
                let before = scene_library_clears.borrow().len();
                let preferences_before = preferences_cleared.get();
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                click(button_x, button_y);
                assert!(app.get_confirm_open(), "clear button {kind} is unreachable");
                assert_eq!(app.get_confirm_title(), title);
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                click(5.0, 5.0);
                assert!(app.get_confirm_open());
                click(width as f32 - x, y);
                assert!(!app.get_confirm_open());
                assert_eq!(scene_library_clears.borrow().len(), before, "Cancel cleared data");
                assert_eq!(preferences_cleared.get(), preferences_before);
                app.invoke_clear_requested(kind);
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                click(x, y);
                assert!(!app.get_confirm_open());
                if kind == 2 {
                    assert_eq!(scene_library_clears.borrow().as_slice().last(), Some(&kind));
                    assert_eq!(scene_library_clears.borrow().len(), before + 1);
                    assert_eq!(preferences_cleared.get(), preferences_before);
                } else {
                    assert_eq!(scene_library_clears.borrow().len(), before);
                    assert_eq!(preferences_cleared.get(), preferences_before + 1);
                }
                assert_eq!(cleared.get(), midi_clears_before, "Gig scene clear also cleared MIDI");
                assert_eq!(preset_action.get(), 0);
            }
            for success in [true, false] {
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                let before = pixels.clone();
                app.invoke_clear_completed(success, "Configuration reset result".into());
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                assert_ne!(pixels, before, "Missing result modal");
                let fill = if success { 0x19df } else { red };
                let points: Vec<_> = pixels.iter().enumerate()
                    .filter(|(_, pixel)| pixel.0 == fill)
                    .map(|(i, _)| (i % width as usize, i / width as usize))
                    .collect();
                assert!(!points.is_empty(), "Missing result modal OK button");
                let left = points.iter().map(|p| p.0).min().unwrap();
                let right = points.iter().map(|p| p.0).max().unwrap();
                let top = points.iter().map(|p| p.1).min().unwrap();
                let bottom = points.iter().map(|p| p.1).max().unwrap();
                click((left + right) as f32 / 2.0, (top + bottom) as f32 / 2.0);
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    renderer.render(&mut pixels, width as usize);
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                });
                assert_eq!(pixels, before, "OK must dismiss the result without changing the row");
            }
        }
        app.set_settings_open(false);
        app.set_settings_tab(0);
        app.set_tools_section(0);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_full_midi_list(app: &AppWindow, window: &MinimalSoftwareWindow) {
        struct Actions {
            visited: std::cell::RefCell<std::collections::BTreeSet<usize>>,
            tracker: slint::ModelNotify,
        }
        impl slint::Model for Actions {
            type Data = MidiRow;
            fn row_count(&self) -> usize {
                257
            }
            fn row_data(&self, index: usize) -> Option<MidiRow> {
                if index >= self.row_count() {
                    return None;
                }
                self.visited.borrow_mut().insert(index);
                Some(MidiRow {
                    index: index as i32,
                    name: format!("Action {index}").into(),
                    assigned: "CC 7 ch1".into(),
                    learning: false,
                })
            }
            fn model_tracker(&self) -> &dyn slint::ModelTracker {
                &self.tracker
            }
        }
        let actions = Rc::new(Actions {
            visited: Default::default(),
            tracker: Default::default(),
        });
        let learned = Rc::new(std::cell::Cell::new(-1));
        app.on_midi_learn({
            let learned = learned.clone();
            move |index| learned.set(index)
        });
        let cleared = Rc::new(std::cell::Cell::new(-1));
        app.on_midi_clear({
            let cleared = cleared.clone();
            move |index| cleared.set(index)
        });
        app.set_midi_rows(actions.clone().into());
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        let render = |pixels: &mut [Rgb565Pixel], width| {
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(pixels, width);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
        };
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            learned.set(-1);
            cleared.set(-1);
            app.set_settings_open(false);
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            render(&mut pixels, width as usize);
            actions.visited.borrow_mut().clear();
            app.set_settings_open(true);
            app.set_tools_section(2);
            render(&mut pixels, width as usize);
            assert!(
                actions.visited.borrow().len() <= 20,
                "Opening MIDI eagerly fetched {} of 257 actions",
                actions.visited.borrow().len()
            );
            for _ in 0..3 {
                window.dispatch_event(WindowEvent::PointerScrolled {
                    position: LogicalPosition::new(width as f32 / 2.0, height as f32 / 2.0),
                    delta_x: 0.0,
                    delta_y: -100_000.0,
                });
                render(&mut pixels, width as usize);
            }
            assert!(
                actions.visited.borrow().contains(&256),
                "Final MIDI action is unreachable"
            );
            for y in (height - 100..height).step_by(2) {
                let position = LogicalPosition::new(width as f32 - 190.0, y as f32);
                window.dispatch_event(WindowEvent::PointerPressed {
                    position,
                    button: PointerEventButton::Left,
                });
                window.dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
                if learned.get() == 256 {
                    let position = LogicalPosition::new(width as f32 - 78.0, y as f32);
                    window.dispatch_event(WindowEvent::PointerPressed {
                        position,
                        button: PointerEventButton::Left,
                    });
                    window.dispatch_event(WindowEvent::PointerReleased {
                        position,
                        button: PointerEventButton::Left,
                    });
                    break;
                }
            }
            assert_eq!(learned.get(), 256, "Virtualized Learn changed action index");
            assert_eq!(cleared.get(), 256, "Virtualized Clear changed action index");
        }
        app.set_midi_rows(slint::ModelRc::default());
        app.set_settings_open(false);
        app.set_settings_tab(0);
        app.set_tools_section(0);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_scene_screen(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let openings = Rc::new(std::cell::Cell::new(0));
        app.on_scene_library_opened({
            let openings = openings.clone();
            move || openings.set(openings.get() + 1)
        });
        let stores = Rc::new(std::cell::Cell::new(-1));
        let renames = Rc::new(std::cell::Cell::new(-1));
        let deletes = Rc::new(std::cell::Cell::new(-1));
        let recalls = Rc::new(std::cell::Cell::new(-1));
        app.on_scene_library_scene_store({
            let stores = stores.clone();
            move |index| stores.set(index)
        });
        app.on_scene_library_scene_rename({
            let renames = renames.clone();
            move |index| renames.set(index)
        });
        app.on_scene_library_scene_clear({
            let deletes = deletes.clone();
            move |index| deletes.set(index)
        });
        app.on_scene_library_scene_recall({
            let recalls = recalls.clone();
            let weak = app.as_weak();
            move |index| {
                recalls.set(index);
                let app = weak.upgrade().unwrap();
                app.set_scene_library_scene(index);
                app.set_scene_dirty(false);
            }
        });
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            window.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        };
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        app.set_scene_preset(12);
        app.set_scene_title("12 - Summer Drive scenes".into());
        app.set_scene_library_scene(0);
        app.set_scene_dirty(true);
        app.set_scene_assigned(slint::ModelRc::new(slint::VecModel::from(vec![
            true, true, true, true, true, false,
        ])));
        app.set_scene_library_scenes(slint::ModelRc::new(slint::VecModel::from(
            (1..=6).map(|index| format!("Scene name {index}").into()).collect::<Vec<_>>(),
        )));
        app.set_scene_library_notice("Preview only - must not be displayed".into());
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_connecting(false);
            app.set_scene_library_open(false);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            let before = openings.get();
            let position = LogicalPosition::new(
                if portrait { 100.0 } else { width as f32 - 110.0 },
                if portrait { 50.0 } else { 38.0 },
            );
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
            assert_eq!(
                openings.get(),
                before + 1,
                "Header scene selector is inaccessible"
            );
            app.set_scene_library_open(true);
            app.set_scene_title("A very long Nano preset title ".repeat(8).into());
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                renderer.render(&mut pixels, width as usize);
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            });
            assert_eq!(
                app.get_scene_title().as_str(),
                "A very long Nano preset title ".repeat(8),
                "Truncation modified the full title"
            );
            let background = pixels[18 * width as usize + width as usize - 1];
            assert!((18..58).all(|y| (width as usize - 14..width as usize).all(|x|
                pixels[y * width as usize + x] == background
            )), "Long scene popup title overflowed its right margin");
            let columns = if portrait { 2 } else { 3 };
            let card_width = (width as f32 - 28.0 - (columns - 1) as f32 * 8.0) / columns as f32;
            app.set_scene_library_scene(0);
            app.set_scene_dirty(false);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                renderer.render(&mut pixels, width as usize);
            });
            let sample = |x: f32, y: usize| y * width as usize + x as usize;
            assert_ne!(pixels[sample(55.0, 160)].0, pixels[sample(card_width + 63.0, 160)].0, "Selected scene is not spotlighted");
            let clean = pixels.clone();
            app.set_scene_dirty(true);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, width as usize);
            });
            let marker_x = (14.0 + card_width - 84.0) as usize;
            assert!((100..165).any(|y| (marker_x - 8..marker_x + 8).any(|x|
                pixels[y * width as usize + x] != clean[y * width as usize + x]
            )), "Selected scene lacks its unsaved asterisk");
            assert!((80..142).all(|y| (card_width as usize + 30..width as usize - 25).all(|x|
                pixels[y * width as usize + x] == clean[y * width as usize + x]
            )), "Unsaved marker leaked onto another scene");
            tap(card_width + 50.0, 135.0);
            assert_eq!(recalls.get(), 1, "Card body does not recall scene ({width}x{height})");
            assert!(app.get_scene_library_open(), "Scene recall closed the menu");
            assert_eq!(app.get_scene_library_scene(), 1);
            assert!(!app.get_scene_dirty());
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, width as usize);
            });
            let mut menu_action = |index: i32, action: i32| {
                let col = index % columns;
                let row = index / columns;
                tap(14.0 + col as f32 * (card_width + 8.0) + card_width - 30.0, 135.0 + row as f32 * 102.0);
                assert_eq!(app.get_scene_menu_index(), index, "Three-dot button did not open scene menu");
                assert_eq!(recalls.get(), 1, "Menu button recalled its card");
                window.request_redraw();
                window.draw_if_needed(|renderer| { renderer.render(&mut pixels, width as usize); });
                let x = (14.0 + col as f32 * (card_width + 8.0) + card_width - 42.0)
                    .clamp(8.0, width as f32 - 228.0);
                let y = (90.0 + row as f32 * 102.0).clamp(8.0, height as f32 - 188.0);
                let separator_x = (x + 20.0) as usize;
                let separator_y = (y + 120.0) as usize;
                assert_ne!(
                    pixels[separator_y * width as usize + separator_x],
                    pixels[(separator_y - 1) * width as usize + separator_x],
                    "Scene menu lacks a separator before Delete"
                );
                tap(x + 40.0, y + 30.0 + action as f32 * 60.0);
                assert!(!app.get_confirm_open(), "Scene action opened a confirmation");
                if index != 5 || action == 1 {
                    assert_eq!(app.get_scene_menu_index(), -1, "Scene action left menu open");
                } else {
                    tap(5.0, 5.0);
                    assert_eq!(app.get_scene_menu_index(), -1, "Outside tap did not dismiss menu");
                }
                window.request_redraw();
                window.draw_if_needed(|renderer| { renderer.render(&mut pixels, width as usize); });
            };
            menu_action(0, 0);
            assert_eq!(renames.get(), 0, "Rename menu action is inaccessible");
            menu_action(0, 1);
            assert_eq!(stores.get(), 0, "Save menu action is inaccessible");
            menu_action(0, 2);
            assert_eq!(deletes.get(), 0, "Delete menu action is inaccessible");
            assert_eq!(recalls.get(), 1, "Action button also recalled its card");
            menu_action(4, 1);
            assert_eq!(stores.get(), 4, "Fifth scene is inaccessible");
            renames.set(-1);
            deletes.set(-1);
            menu_action(5, 0);
            menu_action(5, 2);
            assert_eq!(renames.get(), -1, "Empty scene allowed rename");
            assert_eq!(deletes.get(), -1, "Empty scene allowed delete");
            menu_action(5, 1);
            assert_eq!(stores.get(), 5, "Sixth scene cannot be saved");
            if portrait {
                app.set_scene_menu_index(0);
                app.set_scene_library_busy(true);
                slint::platform::update_timers_and_animations();
                assert_eq!(app.get_scene_menu_index(), -1, "Busy state left scene menu open");
                app.set_scene_library_busy(false);
            } else {
                window.request_redraw();
                window.draw_if_needed(|renderer| { renderer.render(&mut pixels, width as usize); });
                recalls.set(-1);
                tap(2.0 * (card_width + 8.0) + 50.0, 135.0);
                assert_eq!(recalls.get(), 2, "Landscape does not have three columns");
            }
        }
        app.set_scene_library_open(false);
        app.set_scene_dirty(false);
        app.set_connecting(false);
        app.set_portrait(false);
        window.set_size(PhysicalSize::new(800, 480));
    }

    fn check_gig_screen(app: &AppWindow, window: &MinimalSoftwareWindow) {
        let scenes = Rc::new(std::cell::Cell::new((0, -1)));
        let tempo = Rc::new(std::cell::Cell::new(0));
        app.on_gig_scene_recall({
            let scenes = scenes.clone();
            move |index| scenes.set((scenes.get().0 + 1, index))
        });
        app.on_tempo_tapped({
            let tempo = tempo.clone();
            move || tempo.set(tempo.get() + 1)
        });
        app.on_tuner_tapped({
            let weak = app.as_weak();
            move || {
                if let Some(app) = weak.upgrade() {
                    let active = app.get_page() != 2;
                    app.set_page(if active { 2 } else { 0 });
                    app.set_tuner_active(active);
                }
            }
        });
        app.set_preset("1 Clean".into());
        app.set_gig_scenes(slint::ModelRc::new(slint::VecModel::from(
            (1..=6)
                .map(|index| format!("Scene {index}").into())
                .collect::<Vec<_>>(),
        )));
        app.set_gig_scene_assigned(slint::ModelRc::new(slint::VecModel::from(vec![true; 6])));
        app.set_settings_open(true);
        app.set_scene_library_open(true);
        app.set_model_open(true);
        app.set_confirm_open(true);
        app.set_kbd_open(true);
        app.invoke_gig_mode_set(true);
        assert!(app.get_gig_open());
        assert!(
            !app.get_settings_open() && !app.get_scene_library_open()
        );
        assert!(!app.get_model_open() && !app.get_confirm_open() && !app.get_kbd_open());
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        };
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        for (width, height, portrait) in [(800, 480, false), (480, 800, true)] {
            app.set_portrait(portrait);
            window.set_size(PhysicalSize::new(width, height));
            app.set_connecting(false);
            app.set_page(0);
            app.invoke_gig_mode_set(true);
            let mut draw = || {
                window.request_redraw();
                window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(RenderingRotation::NoRotation);
                    renderer.render(&mut pixels, width as usize);
                });
            };
            draw();
            let before = scenes.get().0;
            tap(width as f32 - 60.0, height as f32 - if portrait { 160.0 } else { 110.0 });
            assert_eq!(scenes.get(), (before + 1, 5), "Last scene inaccessible");
            app.invoke_tuner_tapped();
            assert_eq!(app.get_page(), 2);
            assert!(app.get_gig_open());
            draw();
            tap(width as f32 - 55.0, height as f32 - 40.0);
            assert_eq!(app.get_page(), 0, "Tuner exit did not return to gig view");
            assert!(app.get_gig_open());
            draw();
            let before = tempo.get();
            tap(width as f32 - 70.0, 35.0);
            assert_eq!(tempo.get(), before + 1);
            let before = (scenes.get(), tempo.get());
            app.set_scene_library_busy(true);
            draw();
            tap(100.0, 160.0);
            tap(width as f32 - 60.0, height as f32 - if portrait { 160.0 } else { 110.0 });
            tap(80.0, height as f32 - 45.0);
            assert_eq!(
                (scenes.get(), tempo.get()),
                before,
                "Busy recall controls remained active"
            );
            assert_eq!(app.get_page(), 0);
            app.set_scene_library_busy(false);
            app.set_connecting(true);
            draw();
            tap(100.0, 160.0);
            assert_eq!(
                scenes.get(),
                before.0,
                "Disconnected recall remained active"
            );
            tap(33.0, 35.0);
            assert!(
                !app.get_gig_open(),
                "Editor exit was covered by connecting overlay"
            );
        }
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
        assert_eq!(
            pixels[85 * 800 + 130].0,
            0,
            "capture card still has an outline"
        );
        assert_eq!(
            pixels[85 * 800 + 452].0,
            0,
            "capture card still has an outline"
        );
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
                let expected = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
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
                        let tab_x = 14 + (slot % 3) * 152;
                        let tab_y = 700 + (slot / 3) * 48;
                        let (x, y) = if portrait {
                            (tab_x + 11, tab_y + 10)
                        } else {
                            (24, 97 + 67 * slot)
                        };
                        assert_eq!(
                            pixels[y * width as usize + x].0,
                            if active { expected } else { 0 },
                            "category {category}, slot {slot}, portrait {portrait}, active {active}"
                        );
                        if !active {
                            let (left, right, top, bottom) = if portrait {
                                (tab_x + 1, tab_x + 140, tab_y + 1, tab_y + 39)
                            } else {
                                (14, 119, 70 + 67 * slot, 124 + 67 * slot)
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
    fn amp_cards_align_with_chain_top() {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        slint::platform::set_platform(Box::new(Platform {
            window: window.clone(),
            time: Rc::new(std::cell::Cell::new(std::time::Duration::ZERO)),
        }))
        .unwrap();
        let app = AppWindow::new().unwrap();
        app.set_connecting(false);
        app.set_page(0);
        app.show().unwrap();
        window.set_size(PhysicalSize::new(800, 480));
        let mut pixels = vec![Rgb565Pixel(0); 800 * 480];
        window.request_redraw();
        render(&window, &mut pixels, RenderingRotation::NoRotation, true);
        let top = |x: usize| (60..120).find(|y| pixels[y * 800 + x].0 == 0).unwrap();
        assert_eq!(top(290), top(60), "Amp card and FX chain top edges differ");
        assert_eq!(top(620), top(60), "Cab card and FX chain top edges differ");
        app.hide().unwrap();
        let preview = preview::Preview::new().unwrap();
        preview.show().unwrap();
        window.set_size(PhysicalSize::new(800, 480));
        window.request_redraw();
        render(&window, &mut pixels, RenderingRotation::NoRotation, true);
        let background = pixels[0].0;
        let top = |x: usize| (60..120).find(|y| pixels[y * 800 + x].0 != background).unwrap();
        assert_eq!(top(290), top(60), "Preview amp card and FX chain top edges differ");
        assert_eq!(top(620), top(60), "Preview cab card and FX chain top edges differ");
        preview.hide().unwrap();
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
        check_tempo_and_gig_title(&app, &window, &time);
        check_long_names(&app, &window);
        check_backspace_repeat(&app, &window, &time);
        check_preset_menu_hit_target(&app, &window);
        check_two_line_popup_names(&app, &window);
        check_source_card_names(&app, &window);
        check_fx_category_colors(&app, &window);
        check_fullscreen_popups(&app, &window);
        check_header_tools_menu(&app, &window);
        check_fx_cell_contrast(&app, &window, &time);
        check_midi_clear_confirmation(&app, &window);
        check_full_midi_list(&app, &window);
        check_scene_screen(&app, &window);
        check_gig_screen(&app, &window);
        check_tuner_and_welcome_layout(&app, &window);
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
        app.hide().unwrap();
        check_preview_workflow(&window, &time);
    }
}
