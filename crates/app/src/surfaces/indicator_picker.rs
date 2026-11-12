//! Centered indicator library and active-slot management.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use crate::{theme, toolbar::ToolbarAction};
use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_stores::indicator_picker::{Category, Choice};
use quantick_ui_model::UpdateSession;
use quantick_ui_model::indicator_picker::{
    ActiveAction, ActiveEntry, Effect, Intent, Model, State, Status,
};

pub(crate) type IndicatorPickerSurface = super::Adapter<Model, View>;

#[derive(Default)]
pub(crate) struct View {
    keyboard: Option<(Vec<egui::Event>, std::collections::HashSet<egui::Key>)>,
}
fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}

impl super::View<Model> for View {
    /// Hold keyboard input away from the chrome until the modal paints it.
    fn capture_keyboard(&mut self, state: &State, ctx: &egui::Context) {
        if state.picker.target.is_some() {
            self.keyboard = Some(ctx.input_mut(|input| {
                let mut keyboard = Vec::new();
                input.events.retain(|event| {
                    if keyboard_event(event) {
                        keyboard.push(event.clone());
                        false
                    } else {
                        true
                    }
                });
                (keyboard, std::mem::take(&mut input.keys_down))
            }));
        }
    }

    fn id(&self) -> &'static str {
        "indicator-picker"
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) -> Option<Intent> {
        if crate::hooks::captured::var("QUANTICK_INDICATOR_PICKER").is_some_and(|v| v == "1") {
            Some(Intent::Open((env.active_tab, env.picker_pane_id)))
        } else {
            None
        }
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
    fn paint_updated(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        _env: Option<&SurfaceEnv<'_>>,
    ) {
        let ctx = target.context();
        let palette = theme::get(ctx);
        if session.state().picker.target.is_none() {
            return;
        }
        if let Some((events, keys)) = self.keyboard.take() {
            ctx.input_mut(|input| {
                input.events.extend(events);
                input.keys_down = keys;
            });
        }
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut intents = Vec::new();
        let mut open = true;
        let available = ctx.screen_rect().size();
        let width = (available.x - 32.0).clamp(240.0, 760.0);
        let height = (available.y - 120.0).clamp(100.0, 440.0);
        // The foreground scrim consumes pointer input before it reaches the chart.
        let scrim = egui::Area::new(egui::Id::new("indicator-picker-scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(ctx.screen_rect().min)
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(available, egui::Sense::click_and_drag());
                ui.painter().rect_filled(rect, 0.0, palette.modal_scrim());
                response
            })
            .inner;
        egui::Window::new("Indicators")
            .id(egui::Id::new(self.id()))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .fixed_size(egui::vec2(width, height + 60.0))
            .frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Indicators")
                            .font(palette.font_at(quantick_appearance::FontRole::Heading, 20.0))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        open &= !ui
                            .button(icons::X)
                            .on_hover_text("Close indicators")
                            .clicked();
                    });
                });
                ui.separator();
                let mut query = session.state().picker.query.clone();
                let search = ui.add_sized(
                    [ui.available_width(), 30.0],
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Search indicators")
                        .char_limit(256),
                );
                if search.changed() {
                    session.update(Intent::Query(query));
                }
                if session.state().focus_requested {
                    search.request_focus();
                    session.update(Intent::FocusHandled);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(if width < 460.0 { 110.0 } else { 160.0 });
                        let mut category = session.state().picker.category;
                        for value in Category::ALL {
                            ui.selectable_value(&mut category, value, value.label());
                        }
                        if category != session.state().picker.category {
                            session.update(Intent::Category(category));
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .id_salt("indicator-results")
                            .max_height(height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if let Some(message) = session.state().empty_message() {
                                    ui.label(message);
                                }
                                if session.state().picker.category == Category::Active {
                                    draw_active(ui, &session.state().active, &mut intents);
                                } else {
                                    for entry in &session.state().entries {
                                        if let Choice::Script(index) = &entry.choice {
                                            ui.horizontal(|ui| {
                                                ui.label(&entry.name);
                                                if ui.small_button("View source").clicked() {
                                                    intents.push(Intent::ViewSource(*index));
                                                }
                                                if ui.small_button("Add to chart").clicked() {
                                                    intents.push(Intent::SelectEntry(
                                                        entry.choice.clone(),
                                                    ));
                                                }
                                            });
                                            continue;
                                        }
                                        if ui
                                            .allocate_ui_with_layout(
                                                egui::vec2(ui.available_width(), 32.0),
                                                egui::Layout::left_to_right(egui::Align::Center)
                                                    .with_main_align(egui::Align::Min)
                                                    .with_main_justify(true),
                                                |ui| ui.selectable_label(false, &entry.name),
                                            )
                                            .inner
                                            .clicked()
                                        {
                                            intents.push(Intent::SelectEntry(entry.choice.clone()));
                                        }
                                    }
                                }
                            });
                    });
                });
            });
        // A press raises the layer under it, and the scrim shares this layer
        // order: left alone, it would cover the window and take every click.
        ctx.move_to_top(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(self.id()),
        ));
        if !open || escape || scrim.clicked() {
            intents.push(Intent::Close);
        }
        // The chart paints after surfaces; it must never see this modal's keys.
        ctx.input_mut(|input| {
            input.events.retain(|event| !keyboard_event(event));
            input.keys_down.clear();
        });
        for intent in intents {
            session.update(intent);
        }
    }
}

fn draw_active(ui: &mut egui::Ui, entries: &[ActiveEntry], intents: &mut Vec<Intent>) {
    let palette = theme::get(ui.ctx());
    for entry in entries {
        ui.horizontal(|ui| {
            // Status dot: honest state at a glance — errored beats hidden.
            let (dot, color) = match entry.status() {
                Status::Errored => (icons::WARNING_CIRCLE, palette.sell()),
                Status::Stale => (icons::WARNING, palette.accent()),
                Status::Hidden => (icons::EYE_SLASH, palette.text_muted()),
                Status::Running => (icons::CIRCLE, palette.buy()),
            };
            ui.label(egui::RichText::new(dot).color(color));
            ui.label(&entry.label);
            for (action, icon) in [
                (
                    ActiveAction::ToggleHidden,
                    if entry.hidden {
                        icons::EYE
                    } else {
                        icons::EYE_SLASH
                    },
                ),
                (ActiveAction::Settings, icons::GEAR),
                (ActiveAction::Remove, icons::TRASH),
            ] {
                if ui.small_button(icon).on_hover_text(action.hint()).clicked() {
                    intents.push(Intent::ActiveAction(entry.slot, action));
                }
            }
        });
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_INDICATOR_PICKER"];

pub(crate) fn effects(effects: Vec<Effect>) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    for effect in effects {
        match effect {
            Effect::Add(Choice::Native(id)) => response
                .indicator_actions
                .push(ToolbarAction::AddNative(id)),
            Effect::Add(Choice::Script(index)) => response
                .indicator_actions
                .push(ToolbarAction::AddScriptIndicator(index)),
            Effect::ViewSource(id) => response
                .script_editor_actions
                .push(quantick_stores::script_library::ScriptEditorAction::Open { id }),
            Effect::Active(slot, action) => response.indicator_actions.push(match action {
                ActiveAction::ToggleHidden => ToolbarAction::ToggleIndicatorHidden(slot),
                ActiveAction::Settings => ToolbarAction::OpenIndicatorSettings(slot),
                ActiveAction::Remove => ToolbarAction::RemoveIndicator(slot),
            }),
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::Surface;
    use std::time::Instant;
    fn frame(
        surface: &mut IndicatorPickerSurface,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        size: egui::Vec2,
    ) -> (egui::FullOutput, SurfaceResponse) {
        frame_with_entries(surface, ctx, events, size, &[])
    }
    fn frame_with_entries(
        surface: &mut IndicatorPickerSurface,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        size: egui::Vec2,
        entries: &[ActiveEntry],
    ) -> (egui::FullOutput, SurfaceResponse) {
        let mut response = SurfaceResponse::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                let env = SurfaceEnv::quiet(Instant::now());
                surface.update(Intent::Refresh {
                    target: (0, 0),
                    scripts: vec![],
                    active: entries.to_vec(),
                });
                response = surface.draw(ctx, &env);
            },
        );
        (output, response)
    }
    fn text_rect(output: &egui::FullOutput, needle: &str) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == needle => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
    }
    #[test]
    fn registered_hook_opens_before_paint_and_captures_the_next_frame() {
        let ctx = egui::Context::default();
        let mut registry = crate::surfaces::Surfaces::default();
        crate::hooks::captured::set_on_this_thread(Some(&[("QUANTICK_INDICATOR_PICKER", "1")]));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            registry.capture_keyboard(ctx);
            assert!(
                registry
                    .indicator_picker
                    .model_state
                    .picker
                    .target
                    .is_none()
            );
            registry.draw_all(ctx, &SurfaceEnv::quiet(Instant::now()));
            assert_eq!(
                registry.indicator_picker.model_state.picker.target,
                Some((0, 0))
            );
            assert!(!registry.indicator_picker.model_state.focus_requested);
        });
        crate::hooks::captured::set_on_this_thread(None);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Text("ema".into())],
                ..Default::default()
            },
            |ctx| {
                registry.capture_keyboard(ctx);
                assert!(ctx.input(|input| input.events.is_empty()));
                registry.draw_all(ctx, &SurfaceEnv::quiet(Instant::now()));
            },
        );
        assert_eq!(registry.indicator_picker.model_state.picker.query, "ema");
        registry.indicator_picker.update(Intent::Close);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Text("free".into())],
                ..Default::default()
            },
            |ctx| {
                registry.capture_keyboard(ctx);
                assert!(ctx.input(|input| {
                    input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::Text(text) if text == "free"))
                }));
            },
        );
    }
    #[test]
    fn picker_focuses_search_filters_and_escape_dismisses_at_compact_size() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::new(View::default(), effects);
        surface.model_state.picker.open((0, 0));
        let size = egui::vec2(600.0, 400.0);
        for _ in 0..3 {
            frame(&mut surface, &ctx, vec![], size);
        }
        frame(
            &mut surface,
            &ctx,
            vec![egui::Event::Text("no-match".into())],
            size,
        );
        assert_eq!(
            surface.model_state.picker.query, "no-match",
            "search owns keyboard focus"
        );
        let (output, _) = frame(&mut surface, &ctx, vec![], size);
        assert!(text_rect(&output, "No indicators match your search.").is_some());
        frame(
            &mut surface,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            size,
        );
        assert!(surface.model_state.picker.target.is_none());
    }
    #[test]
    fn filtered_native_click_returns_original_toolbar_action() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::new(View::default(), effects);
        surface.model_state.picker.open((0, 0));
        surface.model_state.picker.query = "cVd".into();
        let size = egui::vec2(1000.0, 700.0);
        let mut output = frame(&mut surface, &ctx, vec![], size).0;
        for _ in 0..3 {
            output = frame(&mut surface, &ctx, vec![], size).0;
        }
        assert!(text_rect(&output, "EMA(9) on close").is_none());
        let at = text_rect(&output, "CVD pane")
            .expect("filtered row visible")
            .center();
        let pointer = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut surface,
            &ctx,
            vec![egui::Event::PointerMoved(at), pointer(true)],
            size,
        );
        let (_, response) = frame(&mut surface, &ctx, vec![pointer(false)], size);
        assert_eq!(
            response.indicator_actions,
            vec![ToolbarAction::AddNative("native.cvd")]
        );
        assert!(surface.model_state.picker.target.is_none());
    }
    #[test]
    fn picker_custom_close_dismisses_and_header_and_results_align_left() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::new(View::default(), effects);
        surface.model_state.picker.open((0, 0));
        let size = egui::vec2(1000.0, 700.0);
        let mut output = frame(&mut surface, &ctx, vec![], size).0;
        for _ in 0..3 {
            output = frame(&mut surface, &ctx, vec![], size).0;
        }
        let title = text_rect(&output, "Indicators").unwrap();
        let category = text_rect(&output, "All indicators").unwrap();
        let ema = text_rect(&output, "EMA(9) on close").unwrap();
        let cvd = text_rect(&output, "CVD pane").unwrap();
        assert!((title.left() - category.left()).abs() < 12.0);
        assert!((ema.left() - cvd.left()).abs() < 1.0);
        let at = text_rect(&output, icons::X).unwrap().center();
        for pressed in [true, false] {
            frame(&mut surface, &ctx, click(at, pressed), size);
        }
        assert!(surface.model_state.picker.target.is_none());
    }
    /// Off the window: the scrim, which covers the rest of the app.
    const OUTSIDE: egui::Pos2 = egui::pos2(20.0, 20.0);
    #[test]
    fn clicking_outside_the_picker_dismisses_it_without_an_action() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::new(View::default(), effects);
        surface.model_state.picker.open((0, 0));
        let size = egui::vec2(1000.0, 700.0);
        for _ in 0..3 {
            frame(&mut surface, &ctx, vec![], size);
        }
        frame(&mut surface, &ctx, click(OUTSIDE, true), size);
        let (_, response) = frame(&mut surface, &ctx, click(OUTSIDE, false), size);
        assert!(surface.model_state.picker.target.is_none());
        assert!(response.indicator_actions.is_empty());
    }
    /// A press on the scrim that is no click leaves the picker open, and the
    /// window has to stay above the scrim or the trader holds a modal nothing
    /// can reach.
    #[test]
    fn a_drag_on_the_scrim_leaves_the_window_reachable() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::new(View::default(), effects);
        surface.model_state.picker.open((0, 0));
        let size = egui::vec2(1000.0, 700.0);
        let mut output = frame(&mut surface, &ctx, vec![], size).0;
        for _ in 0..3 {
            output = frame(&mut surface, &ctx, vec![], size).0;
        }
        let row = text_rect(&output, "CVD pane").unwrap().center();
        let away = OUTSIDE + egui::vec2(60.0, 60.0);
        frame(&mut surface, &ctx, click(OUTSIDE, true), size);
        frame(
            &mut surface,
            &ctx,
            vec![egui::Event::PointerMoved(away)],
            size,
        );
        frame(&mut surface, &ctx, click(away, false), size);
        frame(&mut surface, &ctx, vec![], size);
        assert!(
            surface.model_state.picker.target.is_some(),
            "a drag is not a dismissal"
        );
        frame(&mut surface, &ctx, click(row, true), size);
        let (_, response) = frame(&mut surface, &ctx, click(row, false), size);
        assert_eq!(
            response.indicator_actions,
            vec![ToolbarAction::AddNative("native.cvd")]
        );
    }
    fn click(at: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn picker_active_controls_preserve_slot_actions_and_error_stale_status() {
        for (icon, expected) in [
            (icons::EYE_SLASH, ToolbarAction::ToggleIndicatorHidden(17)),
            (icons::GEAR, ToolbarAction::OpenIndicatorSettings(17)),
            (icons::TRASH, ToolbarAction::RemoveIndicator(17)),
        ] {
            let ctx = egui::Context::default();
            let mut surface = IndicatorPickerSurface::new(View::default(), effects);
            surface.model_state.picker.open((0, 0));
            surface.model_state.picker.category = Category::Active;
            let entries = [
                ActiveEntry {
                    slot: 17,
                    label: "Failed script".into(),
                    hidden: false,
                    errored: true,
                    stale: false,
                },
                ActiveEntry {
                    slot: 18,
                    label: "Stale script".into(),
                    hidden: false,
                    errored: false,
                    stale: true,
                },
            ];
            let size = egui::vec2(1000.0, 700.0);
            let mut output = frame_with_entries(&mut surface, &ctx, vec![], size, &entries).0;
            for _ in 0..3 {
                output = frame_with_entries(&mut surface, &ctx, vec![], size, &entries).0;
            }
            assert!(text_rect(&output, icons::WARNING_CIRCLE).is_some());
            assert!(text_rect(&output, icons::WARNING).is_some());
            let at = text_rect(&output, icon).unwrap().center();
            frame_with_entries(&mut surface, &ctx, click(at, true), size, &entries);
            let (_, response) =
                frame_with_entries(&mut surface, &ctx, click(at, false), size, &entries);
            assert_eq!(response.indicator_actions, vec![expected]);
        }
    }
}
