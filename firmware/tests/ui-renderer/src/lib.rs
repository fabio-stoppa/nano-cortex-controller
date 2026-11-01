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
            for (x, y) in [(207.0, 104.0), (215.0, 125.0), (267.0, 167.0)] {
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
            picked.set(0);
        }
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
                name: "Clean capture with a long descriptive name and extra words".into(),
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
            for y in 104..168 {
                let ink = (26..270).any(|x| pixels[y * 800 + x].0 == 0xef7d);
                if ink && !previous_ink {
                    lines += 1;
                }
                previous_ink = ink;
            }
            assert_eq!(
                lines, 2,
                "popup kind {kind} did not show exactly two text lines"
            );
            app.set_model_open(false);
            window.request_redraw();
            render(window, &mut pixels, RenderingRotation::NoRotation, true);
        }
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
    ) {
        let flipped = rotation == RenderingRotation::Rotate180;
        window.draw_if_needed(|renderer| {
            renderer.set_rendering_rotation(if flipped {
                RenderingRotation::NoRotation
            } else {
                rotation
            });
            if full {
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
            }
            renderer.render_by_line(lines::PartialLines::new(pixels.as_mut_ptr(), flipped));
            renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
        });
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
        app.on_name_error(|name| {
            if name.len() < 4 {
                "At least 4 characters".into()
            } else {
                "".into()
            }
        });
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
                app.set_kbd_text(text.into());
            }
        });
        app.show().unwrap();
        check_preset_menu_hit_target(&app, &window);
        check_two_line_popup_names(&app, &window);
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
            render(&window, &mut pixels, rotation, false);
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
                    render(&window, &mut pixels, rotation, false);
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
