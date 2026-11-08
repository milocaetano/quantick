use super::*;
use crate::style::ChartStyle;

fn preset_frame(
    tab: &mut dyn SettingsTab,
    ctx: &egui::Context,
    env: &SurfaceEnv<'_>,
    events: Vec<egui::Event>,
    enabled: bool,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(520.0, 2000.0),
            )),
            time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    tab.draw(ui, env);
                });
            });
        },
    )
}

fn text_center(output: &egui::FullOutput, expected: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.galley.rect.translate(text.pos.to_vec2()).center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing painted {expected}"))
}

fn click_events(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn actual_preset_owners_accept_selection_save_duplicates_and_reject_disabled_input() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for (mut tab, hint) in [
            (
                Box::new(CandlesTab::default()) as Box<dyn SettingsTab>,
                "name this look",
            ),
            (
                Box::new(FootprintTab::default()) as Box<dyn SettingsTab>,
                "name this setup",
            ),
        ] {
            let ctx = egui::Context::default();
            let mut appearance = quantick_appearance::Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let style = ChartStyle::default();
            let env = env(&style, Instant::now());
            tab.opened(&env);
            for _ in 0..2 {
                let output = preset_frame(tab.as_mut(), &ctx, &env, vec![], true);
                let position = text_center(&output, hint);
                for pressed in [true, false] {
                    preset_frame(
                        tab.as_mut(),
                        &ctx,
                        &env,
                        click_events(position, pressed),
                        true,
                    );
                }
                let id = ctx
                    .memory(|memory| memory.focused())
                    .expect("actual preset field focused");
                preset_frame(
                    tab.as_mut(),
                    &ctx,
                    &env,
                    vec![egui::Event::Text("Readable".into())],
                    true,
                );
                let output = preset_frame(
                    tab.as_mut(),
                    &ctx,
                    &env,
                    vec![egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..Default::default()
                        },
                    }],
                    true,
                );
                let selection = egui::TextEdit::load_state(&ctx, id)
                    .unwrap()
                    .cursor
                    .char_range()
                    .unwrap()
                    .sorted();
                assert_eq!((selection[0].index, selection[1].index), (0, 8));
                let palette = crate::theme::get(&ctx);
                if !palette.resolved.dark {
                    use quantick_appearance::{PremultipliedRgba, contrast};
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|clipped| match &clipped.shape {
                            egui::Shape::Text(text)
                                if text.galley.job.text == "Readable"
                                    && (text.pos.y + text.galley.rect.center().y - position.y)
                                        .abs()
                                        < 10.0 =>
                            {
                                Some(text)
                            }
                            _ => None,
                        })
                        .expect("selected editor text is painted");
                    let ink = text
                        .override_text_color
                        .unwrap_or(text.galley.rows[0].visuals.mesh.vertices[0].color);
                    let bytes = |color: egui::Color32| {
                        PremultipliedRgba::from_premultiplied(color.to_array())
                    };
                    let background = contrast::over(
                        bytes(ctx.style().visuals.selection.bg_fill),
                        bytes(palette.inset()),
                    );
                    assert!(
                        contrast::contrast_ratio(bytes(ink), background) >= 4.5,
                        "selected input remains readable"
                    );
                }
                assert!(
                    output.shapes.iter().any(|clipped| matches!(&clipped.shape,
                    egui::Shape::Rect(rect) if rect.fill == ctx.style().visuals.selection.bg_fill)),
                    "selected input has its actual selection background"
                );
                let save = text_center(&output, "save preset");
                for pressed in [true, false] {
                    preset_frame(tab.as_mut(), &ctx, &env, click_events(save, pressed), true);
                }
                let output = preset_frame(tab.as_mut(), &ctx, &env, vec![], true);
                text_center(&output, hint);
                let saved = output
                    .shapes
                    .iter()
                    .filter(|clipped| {
                        matches!(&clipped.shape,
                    egui::Shape::Text(text) if text.galley.job.text == "Readable")
                    })
                    .count();
                assert_eq!(saved, 1, "saving the same name retains one preset");
                ctx.memory_mut(|memory| memory.surrender_focus(id));
            }
            let output = preset_frame(tab.as_mut(), &ctx, &env, vec![], false);
            let position = text_center(&output, hint);
            for pressed in [true, false] {
                preset_frame(
                    tab.as_mut(),
                    &ctx,
                    &env,
                    click_events(position, pressed),
                    false,
                );
            }
            let output = preset_frame(
                tab.as_mut(),
                &ctx,
                &env,
                vec![egui::Event::Text("Forbidden".into())],
                false,
            );
            text_center(&output, hint);
            assert!(ctx.memory(|memory| memory.focused()).is_none());
            assert!(!output.shapes.iter().any(|clipped| matches!(&clipped.shape,
                egui::Shape::Text(text) if text.galley.job.text.contains("Forbidden"))));
        }
    }
}

#[test]
fn settings_persistence_endpoints_are_reachable_through_the_real_scroll_area() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [1000.0, 1280.0] {
            for tab in ChartSettingsTab::ALL {
                let ctx = egui::Context::default();
                let mut appearance = quantick_appearance::Appearance::default();
                appearance.apply(source).unwrap();
                crate::theme::sync(&ctx, &appearance);
                let style = ChartStyle::default();
                let env = env(&style, Instant::now());
                let mut surface = ChartSettingsSurface::default();
                surface.open(tab, style);
                let mut reached = false;
                for frame in 0..120 {
                    let mut events = vec![];
                    if frame > 5 && frame % 6 == 0 {
                        events.extend([
                            egui::Event::PointerMoved(egui::pos2(180.0, 350.0)),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0.0, -120.0),
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]);
                    }
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 780.0),
                            )),
                            time: Some(f64::from(frame) / 60.0),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            surface.draw(ctx, &env);
                        },
                    );
                    for clipped in output.shapes {
                        if let egui::Shape::Text(text) = clipped.shape
                            && (text.galley.job.text.starts_with("Appearance changes")
                                || text
                                    .galley
                                    .job
                                    .text
                                    .starts_with("Changes apply to this chart"))
                        {
                            let bounds = text.galley.rect.translate(text.pos.to_vec2());
                            reached |= clipped.clip_rect.contains_rect(bounds);
                        }
                    }
                }
                assert!(
                    reached,
                    "the complete {tab:?} persistence note is reachable by wheel"
                );
            }
        }
    }
}

#[test]
fn supporting_guidance_fits_narrow_surfaces() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [320.0, 520.0] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let ctx = egui::Context::default();
                let mut appearance = quantick_appearance::Appearance::default();
                appearance.apply(source).unwrap();
                crate::theme::sync(&ctx, &appearance);
                let mut surface = ChartSettingsSurface::default();
                let style = ChartStyle::default();
                let env = env(&style, Instant::now());
                for tab in surface.tabs() {
                    let mut input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 4000.0),
                        )),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .native_pixels_per_point = Some(scale);
                    let output = ctx.run(input, |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            tab.draw(ui, &env);
                        });
                    });
                    let note = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text)
                                if text.galley.job.text.starts_with("Appearance changes")
                                    || text
                                        .galley
                                        .job
                                        .text
                                        .starts_with("Changes apply to this chart") =>
                            {
                                Some(text)
                            }
                            _ => None,
                        })
                        .expect("the settings persistence note is painted");
                    assert!(note.pos.x + note.galley.rect.right() <= width + 0.5);
                    if width < 400.0 {
                        assert!(
                            note.galley.rows.len() >= 2,
                            "consequences wrap instead of shrinking"
                        );
                    }
                    assert!(note.galley.job.sections[0].format.font_id.size >= 12.0);
                }
            }
        }
    }
}

fn env(style: &ChartStyle, now: Instant) -> SurfaceEnv<'_> {
    SurfaceEnv {
        style,
        ..SurfaceEnv::quiet(now)
    }
}

fn draw(
    surface: &mut ChartSettingsSurface,
    ctx: &egui::Context,
    env: &SurfaceEnv<'_>,
) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(Default::default(), |ctx| {
        response = surface.draw(ctx, env);
    });
    response
}

fn pending(open: bool, now: Instant) -> ChartSettingsSurface {
    ChartSettingsSurface {
        session: quantick_control_handlers::chart_settings::ChartSettingsSession {
            open,
            ..Default::default()
        },
        log_pending: true,
        last_change: Some(now),
        ..ChartSettingsSurface::default()
    }
}

/// A closed window reads nothing and asks for nothing.
#[test]
fn a_closed_window_asks_for_nothing() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    let mut surface = ChartSettingsSurface::default();
    let response = draw(&mut surface, &ctx, &env(&style, Instant::now()));
    assert_eq!(response, SurfaceResponse::default());
    assert!(!surface.is_open());
}

/// An open window on a frame nobody touched reports no edit on either tab —
/// otherwise every open window would bump the style or rewrite the footprint
/// file at the refresh rate.
#[test]
fn an_untouched_window_reports_no_edit_on_either_tab() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    for tab in ChartSettingsTab::ALL {
        let mut surface = ChartSettingsSurface::default();
        surface.open(tab, style);
        let response = draw(&mut surface, &ctx, &env(&style, Instant::now()));
        assert_eq!(response, SurfaceResponse::default(), "{tab:?}");
        assert!(surface.is_open_on(tab));
    }
}

/// One window, one flag: the doors switch the tab rather than stacking a
/// second window, and closing remembers the tab for the next opening.
#[test]
fn every_door_reaches_the_one_window() {
    let style = ChartStyle::default();
    let mut surface = ChartSettingsSurface::default();
    surface.open(ChartSettingsTab::Footprint, style);
    assert!(surface.is_open_on(ChartSettingsTab::Footprint));
    surface.open(ChartSettingsTab::Candles, style);
    assert!(
        surface.is_open_on(ChartSettingsTab::Candles),
        "switched to candles"
    );
    surface.close();
    assert!(!surface.is_open());
    assert_eq!(surface.tab(), ChartSettingsTab::Candles, "remembered");
}

/// Opening starts the sitting on the style in force, before any frame is
/// drawn; Reset returns there however far the chart moved; closing forgets
/// it.
#[test]
fn reset_returns_to_the_look_the_window_opened_on() {
    let ctx = egui::Context::default();
    let opened_on = ChartStyle::default();
    let mut surface = ChartSettingsSurface::default();
    surface.open(ChartSettingsTab::Candles, opened_on);
    let mut moved = opened_on;
    moved.candles = crate::style::CandlePreset::Glass.style();
    moved.canvas.grid_enabled = !opened_on.canvas.grid_enabled;
    assert_eq!(
        surface.candles.reset(&moved),
        Some(opened_on),
        "the sitting began at the door, not at the first frame"
    );
    // A later frame over the moved style does not move the starting point.
    draw(&mut surface, &ctx, &env(&moved, Instant::now()));
    assert_eq!(surface.candles.reset(&moved), Some(opened_on));
    surface.close();
    assert_eq!(
        surface.candles.reset(&moved),
        None,
        "closing ends the sitting"
    );
    surface.open(ChartSettingsTab::Footprint, moved);
    assert_eq!(
        surface.candles.reset(&opened_on),
        Some(moved),
        "a new sitting starts on the look in force, whichever tab opened it"
    );
}

/// The debounce holds a change back while the gesture runs, lets it go once
/// the trader stops, then stays quiet however many frames follow.
#[test]
fn the_log_waits_for_the_gesture_to_settle() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    let start = Instant::now();
    let mut surface = pending(true, start);
    let response = draw(
        &mut surface,
        &ctx,
        &env(&style, start + STYLE_LOG_DEBOUNCE / 2),
    );
    assert!(response.log_style_change.is_none(), "still mid-gesture");
    let response = draw(&mut surface, &ctx, &env(&style, start + STYLE_LOG_DEBOUNCE));
    assert!(response.log_style_change.is_some(), "the gesture settled");
    let response = draw(
        &mut surface,
        &ctx,
        &env(&style, start + STYLE_LOG_DEBOUNCE * 4),
    );
    assert!(response.log_style_change.is_none(), "one change, one line");
}

/// Closing does not make the record wait out a timer; an open window
/// mid-gesture does wait; nothing pending logs nothing.
#[test]
fn closing_the_window_logs_without_waiting() {
    let now = Instant::now();
    assert!(!pending(true, now).should_log(now), "the debounce holds");
    assert!(pending(false, now).should_log(now), "the window left");
    assert!(!ChartSettingsSurface::default().should_log(now));
}

/// Closed between frames, the window still flushes the
/// edit on its next draw, once, naming the preset a click applied.
#[test]
fn closing_between_frames_still_records_the_edit() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    let now = Instant::now();
    let mut surface = pending(true, now);
    surface.pending_preset = Some(crate::style::CandlePreset::Classic);
    surface.close();
    assert!(!surface.is_open());
    let response = draw(&mut surface, &ctx, &env(&style, now));
    assert_eq!(
        response.log_style_change,
        Some(StyleLogRequest {
            applied_preset: Some(crate::style::CandlePreset::Classic)
        })
    );
    let response = draw(&mut surface, &ctx, &env(&style, now));
    assert!(response.log_style_change.is_none(), "recorded once");
}
