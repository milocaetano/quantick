use crate::style::ChartStyle;
use crate::surfaces::Surface;
use crate::surfaces::chart_settings::*;
use crate::surfaces::{SurfaceEnv, SurfaceResponse};
use eframe::egui;
use quantick_ui_model::chart_settings::{Intent, Model};
use std::time::Instant;
type ChartSettingsSurface = crate::surfaces::Adapter<Model, View>;
fn surface() -> ChartSettingsSurface {
    let mut surface = ChartSettingsSurface::new(View::default(), effects);
    crate::app::chart_settings_wiring::initialize(&mut surface.model_state);
    surface
}
fn open(surface: &mut ChartSettingsSurface, tab: ChartSettingsTab, style: ChartStyle) {
    surface.update(Intent::Open {
        tab,
        target: None,
        style,
    });
}
struct TestTab {
    surface: ChartSettingsSurface,
    tab: ChartSettingsTab,
}
impl TestTab {
    fn new(tab: ChartSettingsTab) -> Self {
        Self {
            surface: surface(),
            tab,
        }
    }
    fn opened(&mut self, env: &SurfaceEnv<'_>) {
        open(&mut self.surface, self.tab, *env.style);
    }
    fn draw(&mut self, ui: &mut egui::Ui, env: &SurfaceEnv<'_>) {
        self.surface.update(Intent::Refresh {
            style: *env.style,
            footprint: env.footprint.clone(),
            target: "Flow chart".into(),
            customized: env.footprint_customized,
        });
        let mut response = SurfaceResponse::default();
        self.surface.with_updates(
            |effects| response.model_effects.extend(effects.model_effects),
            |_, session| match self.tab {
                ChartSettingsTab::Candles => candles::draw(ui, session, env, 0),
                ChartSettingsTab::Footprint => footprint::draw(ui, session),
            },
        );
        for effect in response.model_effects {
            if let quantick_ui_model::HostEffect::ChartSettings(effects) = effect {
                crate::app::chart_settings_wiring::persist(
                    &mut self.surface.model_state,
                    effects.persistence,
                );
            }
        }
    }
}

fn preset_frame(
    tab: &mut TestTab,
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
                Box::new(TestTab::new(ChartSettingsTab::Candles)),
                "name this look",
            ),
            (
                Box::new(TestTab::new(ChartSettingsTab::Footprint)),
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
                let mut surface = surface();
                open(&mut surface, tab, style);
                let mut reached = false;
                for frame in 0..120 {
                    let mut events = vec![];
                    if frame > 5 && frame % 6 == 0 {
                        events.extend([
                            // Settings windows open centred (`crate::popup`).
                            egui::Event::PointerMoved(egui::pos2(width / 2.0, 390.0)),
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
                let style = ChartStyle::default();
                let env = env(&style, Instant::now());
                for which in ChartSettingsTab::ALL {
                    let mut tab = TestTab::new(which);
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

#[test]
fn a_closed_window_asks_for_nothing() {
    let ctx = egui::Context::default();
    let mut surface = surface();
    let mut result = SurfaceResponse::default();
    let _ = ctx.run(Default::default(), |ctx| {
        result = surface.draw(ctx, &SurfaceEnv::quiet(Instant::now()));
    });
    assert_eq!(result, SurfaceResponse::default());
}
#[test]
fn an_untouched_window_reports_no_edit_on_either_tab() {
    for tab in ChartSettingsTab::ALL {
        let ctx = egui::Context::default();
        let mut surface = surface();
        open(&mut surface, tab, ChartStyle::default());
        let mut result = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            result = surface.draw(ctx, &SurfaceEnv::quiet(Instant::now()));
        });
        assert_eq!(result, SurfaceResponse::default());
        assert!(surface.model_state.is_open_on(tab));
    }
}

#[test]
fn registered_settings_hooks_pin_before_first_paint_and_run_once() {
    for (hook, tab) in [
        ("QUANTICK_STYLE_PANEL", ChartSettingsTab::Candles),
        ("QUANTICK_FOOTPRINT_PANEL", ChartSettingsTab::Footprint),
    ] {
        let mut registry = crate::surfaces::Surfaces::default();
        crate::app::chart_settings_wiring::initialize(&mut registry.chart_settings.model_state);
        let ctx = egui::Context::default();
        let style = ChartStyle::default();
        let env = SurfaceEnv {
            picker_pane_id: 123,
            active_tab: 7,
            style: &style,
            ..SurfaceEnv::quiet(Instant::now())
        };
        crate::hooks::captured::set_on_this_thread(Some(&[(hook, "1")]));
        let _ = ctx.run(Default::default(), |ctx| {
            registry.draw_all(ctx, &env);
        });
        crate::hooks::captured::set_on_this_thread(None);
        assert!(registry.chart_settings.model_state.is_open_on(tab));
        assert_eq!(
            registry.chart_settings.model_state.editor.target,
            Some(quantick_chart::pane_style::StyleTarget {
                tab_id: 7,
                pane_id: 123
            })
        );
        registry.chart_settings.update(Intent::Close);
        let _ = ctx.run(Default::default(), |ctx| {
            registry.draw_all(ctx, &env);
        });
        assert!(!registry.chart_settings.model_state.is_open());
    }
}
