//! Paint the quick bar switch; engine-backed decisions live in ui-model.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use crate::theme;
use eframe::egui;
pub(crate) use quantick_ui_model::bar_switch::BarSwitchRequest;
use quantick_ui_model::bar_switch::{Intent, Model, State};

const TOP_MARGIN_PX: f32 = 24.0;
const WIDTH_PX: f32 = 260.0;

#[derive(Default)]
pub(crate) struct View;
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "bar_switch"
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
        session: &mut quantick_ui_model::UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        let Some(env) = env else {
            return;
        };
        let ctx = target.context();
        let palette = theme::get(ctx);
        let id = self.id();
        let query_id = egui::Id::new(id).with("query");
        let text: String = if session.state().open().is_none()
            && !ctx.memory(|memory| memory.focused().is_some())
        {
            ctx.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| {
                        if let egui::Event::Text(text) = event {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
        } else {
            String::new()
        };
        let address_changed = session
            .state()
            .open()
            .is_some_and(|open| open.tab != env.active_tab || open.side != env.focused_side);
        if address_changed || (session.state().open().is_none() && !text.is_empty()) {
            session.update(Intent::Observe {
                tab: env.active_tab,
                side: env.focused_side,
                text,
                widget_focused: ctx.memory(|memory| memory.focused().is_some()),
                modified: ctx.input(|input| {
                    input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt
                }),
            });
        }
        if session.state().open().is_none() {
            if address_changed {
                ctx.memory_mut(|memory| memory.surrender_focus(query_id));
            }
            return;
        }
        let typing = ctx.memory(|memory| memory.has_focus(query_id));
        let (escape, enter, down, up) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::Escape),
                typing && input.key_pressed(egui::Key::Enter),
                typing && input.key_pressed(egui::Key::ArrowDown),
                typing && input.key_pressed(egui::Key::ArrowUp),
            )
        });
        let mut clicked = None;
        let mut window = egui::Window::new("Bars")
            .id(egui::Id::new(id))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(WIDTH_PX, 0.0))
            .order(egui::Order::Foreground);
        window = match env.focused_chart_area {
            Some(area) => {
                window.fixed_pos(area.center_top() + egui::vec2(-WIDTH_PX / 2.0, TOP_MARGIN_PX))
            }
            None => window.anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 96.0)),
        };

        let shown = window.show(ctx, |ui| {
            let open = session.state().open().expect("open switch");
            let mut query = open.query.clone();
            let focus_pending = open.focus_pending;
            let edit = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .id(query_id)
                    .hint_text("bar size")
                    .desired_width(f32::INFINITY),
            );
            if focus_pending {
                edit.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ctx, edit.id) {
                    let end = egui::text::CCursor::new(query.chars().count());
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ctx, edit.id);
                }
            }
            // Up and Down walk the list; left to egui they would move focus
            // onto a row, and Enter would then press that row instead. egui
            // honours the filter from the frame after focus arrives.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    edit.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
            if query != open.query || down || up {
                session.update(Intent::Edit { query, down, up });
            }
            if focus_pending {
                session.update(Intent::FocusRequested);
            }
            let open = session.state().open().expect("editing keeps switch open");
            let candidates = &open.candidates;
            if candidates.is_empty() {
                ui.label(egui::RichText::new("no bar at this size").color(palette.text_support()));
            }
            for (index, config) in candidates.iter().enumerate() {
                let row = ui.selectable_label(index == open.selected, config.quick_label());
                if row.clicked() {
                    clicked = Some(index);
                }
            }
        });
        let over_list = shown.is_some_and(|shown| shown.response.contains_pointer());
        let open = session.state().open().expect("painting keeps switch open");
        let abandoned =
            !open.focus_pending && !ctx.memory(|memory| memory.has_focus(query_id)) && !over_list;
        if clicked.is_some() || enter || escape || abandoned || open.query.is_empty() {
            session.update(Intent::Finish {
                clicked,
                enter,
                escape,
                abandoned,
            });
        }
        if session.state().open().is_none() {
            ctx.memory_mut(|memory| memory.surrender_focus(query_id));
        }
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) -> Option<Intent> {
        crate::hooks::captured::var("QUANTICK_BAR_SWITCH")
            .filter(|query| !query.is_empty())
            .map(|query| Intent::Open {
                tab: env.active_tab,
                side: env.focused_side,
                query,
                follows_focus: true,
            })
    }
}

pub(crate) fn effects(request: Option<BarSwitchRequest>) -> SurfaceResponse {
    SurfaceResponse {
        bar_switch: request,
        ..SurfaceResponse::default()
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_BAR_SWITCH"];

#[cfg(test)]
#[path = "tests/bar_switch.rs"]
mod bar_switch_tests;
