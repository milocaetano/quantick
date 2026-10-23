//! Centered indicator library and active-slot management.
use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::{
    theme,
    toolbar::{IndicatorMenuEntry, ToolbarAction},
};
use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_stores::indicator_picker::{Category, Choice, Picker};

#[derive(Default)]
pub(crate) struct IndicatorPickerSurface {
    pub state: Picker,
    keyboard: Option<(Vec<egui::Event>, std::collections::HashSet<egui::Key>)>,
}
impl IndicatorPickerSurface {
    /// Hold keyboard input away from the chrome until the modal paints it.
    pub fn capture_keyboard(&mut self, ctx: &egui::Context) {
        if self.state.target.is_some() {
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

impl Surface for IndicatorPickerSurface {
    fn id(&self) -> &'static str {
        "indicator-picker"
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) {
        if crate::hooks::captured::var("QUANTICK_INDICATOR_PICKER").is_some_and(|v| v == "1") {
            self.state.open((env.active_tab, env.picker_pane_id));
        }
    }
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        self.state
            .retain_target((env.active_tab, env.picker_pane_id));
        if self.state.target.is_none() {
            return SurfaceResponse::default();
        }
        if let Some((events, keys)) = self.keyboard.take() {
            ctx.input_mut(|input| {
                input.events.extend(events);
                input.keys_down = keys;
            });
        }
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut actions = Vec::new();
        let mut open = true;
        let available = ctx.screen_rect().size();
        let width = (available.x - 32.0).clamp(240.0, 760.0);
        let height = (available.y - 120.0).clamp(100.0, 440.0);
        // The foreground scrim consumes pointer input before it reaches the chart.
        egui::Area::new(egui::Id::new("indicator-picker-scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(ctx.screen_rect().min)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(90));
            });
        egui::Window::new(egui::RichText::new("Indicators").size(20.0).strong())
            .id(egui::Id::new(self.id()))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .resizable(false)
            .collapsible(false)
            .open(&mut open)
            .fixed_size(egui::vec2(width, height + 60.0))
            .frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                let search = ui.add_sized(
                    [ui.available_width(), 30.0],
                    egui::TextEdit::singleline(&mut self.state.query)
                        .hint_text("Search indicators")
                        .char_limit(256),
                );
                if self.state.take_focus() {
                    search.request_focus();
                }
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(if width < 460.0 { 110.0 } else { 160.0 });
                        for category in Category::ALL {
                            ui.selectable_value(
                                &mut self.state.category,
                                category,
                                category.label(),
                            );
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
                                if self.state.category == Category::Active {
                                    let entries: Vec<_> = env
                                        .picker_indicators
                                        .iter()
                                        .filter(|e| self.state.matches(&e.label))
                                        .cloned()
                                        .collect();
                                    if entries.is_empty() {
                                        ui.label("No indicators on this chart match your search.");
                                    }
                                    if draw_active(ui, &entries, &mut actions) {
                                        self.state.close();
                                    }
                                } else {
                                    let entries = self.state.entries(env.picker_scripts);
                                    if entries.is_empty() {
                                        ui.label("No indicators match your search.");
                                    }
                                    for entry in entries {
                                        if ui
                                            .allocate_ui_with_layout(
                                                egui::vec2(ui.available_width(), 32.0),
                                                egui::Layout::left_to_right(egui::Align::Center)
                                                    .with_main_justify(true),
                                                |ui| ui.selectable_label(false, &entry.name),
                                            )
                                            .inner
                                            .clicked()
                                        {
                                            actions.push(match entry.choice {
                                                Choice::Native(id) => ToolbarAction::AddNative(id),
                                                Choice::Script(index) => {
                                                    ToolbarAction::AddScriptIndicator(index)
                                                }
                                            });
                                            self.state.close();
                                        }
                                    }
                                }
                            });
                    });
                });
            });
        if !open || escape {
            self.state.close();
        }
        // The chart paints after surfaces; it must never see this modal's keys.
        ctx.input_mut(|input| {
            input.events.retain(|event| !keyboard_event(event));
            input.keys_down.clear();
        });
        SurfaceResponse {
            indicator_actions: actions,
            ..Default::default()
        }
    }
}

fn draw_active(
    ui: &mut egui::Ui,
    entries: &[IndicatorMenuEntry],
    actions: &mut Vec<ToolbarAction>,
) -> bool {
    let mut close = false;
    for entry in entries {
        ui.horizontal(|ui| {
            // Status dot: honest state at a glance — errored beats hidden.
            let (dot, color) = if entry.errored {
                (icons::WARNING_CIRCLE, theme::SELL)
            } else if entry.stale {
                // Running, but the file on disk has errors.
                (icons::WARNING, theme::ACCENT)
            } else if entry.hidden {
                (icons::EYE_SLASH, theme::TEXT_MUTED)
            } else {
                (icons::CIRCLE, theme::BUY)
            };
            ui.label(egui::RichText::new(dot).color(color));
            ui.label(&entry.label);
            if ui
                .small_button(if entry.hidden {
                    icons::EYE
                } else {
                    icons::EYE_SLASH
                })
                .on_hover_text("hide/show without removing (no recompute)")
                .clicked()
            {
                actions.push(ToolbarAction::ToggleIndicatorHidden(entry.slot));
            }
            if ui
                .small_button(icons::GEAR)
                .on_hover_text("settings (applying recomputes from scratch)")
                .clicked()
            {
                actions.push(ToolbarAction::OpenIndicatorSettings(entry.slot));
                // The menu closes so it cannot occlude the dialog it
                // just spawned (egui paints menus above windows) —
                // audit M3.
                close = true;
            }
            if ui
                .small_button(icons::TRASH)
                .on_hover_text("remove this indicator")
                .clicked()
            {
                actions.push(ToolbarAction::RemoveIndicator(entry.slot));
                // Closing beats redrawing the menu around a vanished
                // row. The eye deliberately keeps the menu open:
                // toggling several indicators is one errand.
                close = true;
            }
        });
    }
    close
}

crate::hooks::declare_hooks!["QUANTICK_INDICATOR_PICKER"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    fn frame(
        surface: &mut IndicatorPickerSurface,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        size: egui::Vec2,
    ) -> (egui::FullOutput, SurfaceResponse) {
        let mut response = SurfaceResponse::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                response = surface.draw(ctx, &SurfaceEnv::quiet(Instant::now()));
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
    fn picker_focuses_search_filters_and_escape_dismisses_at_compact_size() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::default();
        surface.state.open((0, 0));
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
            surface.state.query, "no-match",
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
        assert!(surface.state.target.is_none());
    }
    #[test]
    fn filtered_native_click_returns_original_toolbar_action() {
        let ctx = egui::Context::default();
        let mut surface = IndicatorPickerSurface::default();
        surface.state.open((0, 0));
        surface.state.query = "cVd".into();
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
        assert!(surface.state.target.is_none());
    }
}
