//! Paint the alert manager and editor. Every change returns as a model intent;
//! every mutation of the book returns through the persistent host.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;
use quantick_alerts::{AlertBook, AlertError};
pub(crate) use quantick_ui_model::alerts::{Context, DrawingChoice, Request};
use quantick_ui_model::alerts::{Draft, Effects, Intent, Model, State, Subview};
use quantick_ui_model::UpdateSession;
#[path = "alerts/editor.rs"]
mod editor;
#[path = "alerts/manager.rs"]
mod manager;
#[cfg(test)]
#[path = "alerts/tests.rs"]
mod tests;

/// The catalog's alert surface: manager panel and editor window.
pub(crate) type AlertSurface = super::Adapter<Model, View>;

#[derive(Default)]
pub(crate) struct View;

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "alerts"
    }
    fn paint_updated(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        let Some(env) = env else {
            return;
        };
        let ctx = target.context().clone();
        if !session.state().is_open() {
            return;
        }
        let Some(book) = env.alerts else {
            return;
        };
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(context) = ready_fixture(session.state(), env) {
            session.update(Intent::FixtureReady(context.clone()));
        }
        if session.state().has_pending_drawing() {
            session.update(Intent::ConsumePending(env.alert_context.cloned()));
        }
        // Consume Escape only after a nested combo has had an opportunity to close.
        if !ctx.memory(|memory| memory.any_popup_open())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            session.update(Intent::Escape);
            return;
        }
        let mut intents = Vec::new();
        paint_window(&ctx, session.state(), book, env, &mut intents);
        for intent in intents {
            session.update(intent);
        }
    }
    fn paint(
        &mut self,
        _state: &State,
        _target: &mut ViewContext<'_>,
        _env: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) -> Option<Intent> {
        let mode = crate::hooks::captured::var("QUANTICK_ALERTS")?;
        let subview = match mode.as_str() {
            "editor" => Some(Subview::Conditions),
            "editor-message" => Some(Subview::Message),
            "editor-notifications" => Some(Subview::Notifications),
            _ => None,
        };
        Some(Intent::Fixture {
            subview,
            history: mode == "history",
        })
    }
}

pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse {
        alerts: effects.request,
        test_alert: effects.preview,
        ..SurfaceResponse::default()
    }
}

/// The editor window over the chart, while a draft is open.
fn paint_window(
    ctx: &egui::Context,
    state: &State,
    book: &AlertBook,
    env: &SurfaceEnv<'_>,
    intents: &mut Vec<Intent>,
) {
    let Some(before) = state.draft() else {
        return;
    };
    let mut draft = before.clone();
    let width = 560.0_f32.min((ctx.screen_rect().width() - 32.0).max(100.0));
    let mut open = state.is_open();
    let title = if draft.revision.is_some() {
        "Edit alert"
    } else {
        "Create alert"
    };
    let mut actions = Vec::new();
    egui::Window::new(title)
        .id(egui::Id::new("alerts_surface"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(width)
        .max_width(width)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(width);
            if let Some(storage_error) = env.alert_error {
                show_storage_error(ui, storage_error);
                ui.label("Your open draft is retained until you close this window or restart.");
            } else {
                let failure = env.alert_failure;
                editor::draw_editor(ui, &mut draft, state.error(), book, failure, &mut actions);
            }
        });
    // The edit lands first, so a Save or Delete in the same frame sends it.
    if draft != *before {
        intents.push(Intent::Edit(Box::new(draft)));
    }
    intents.extend(actions);
    if !open {
        intents.push(Intent::Close);
    }
}

/// Reserve manager space after the permanent right rail, before chart layout.
pub(crate) fn draw_manager_panel(
    surface: &mut AlertSurface,
    ctx: &egui::Context,
    book: &AlertBook,
    storage_error: Option<&AlertError>,
    context: Option<&Context>,
) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    surface.with_updates(
        |effect| response.merge(effect),
        |_, session| {
            let state = session.state();
            if !state.is_open() || state.draft().is_some() {
                return;
            }
            let mut intents = Vec::new();
            let width = 340.0_f32
                .min(ctx.available_rect().width() * 0.55)
                .max(180.0);
            egui::SidePanel::right(egui::Id::new((ctx.viewport_id(), "alerts_manager_panel")))
                .resizable(true)
                .default_width(width)
                .width_range(180.0..=width)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Alerts");
                        if ui.button("Close").clicked() {
                            intents.push(Intent::Close);
                        }
                    });
                    if let Some(storage_error) = storage_error {
                        show_storage_error(ui, storage_error);
                    } else {
                        manager::draw_manager(ui, state, book, context, &mut intents);
                    }
                });
            for intent in intents {
                session.update(intent);
            }
        },
    );
    response
}

/// A surface outside the catalog, for the host tests that drive one alone.
#[cfg(test)]
pub(crate) fn new_surface() -> AlertSurface {
    AlertSurface::new(View, effects).in_slots(&[quantick_ui_model::Slot::Overlay])
}

/// A replay capture must open the same draft a trader sees after loading.
/// This delay belongs only to fixture activation; manual Create stays immediate.
#[cfg(any(feature = "scenario-harness", test))]
fn ready_fixture<'a>(state: &State, env: &SurfaceEnv<'a>) -> Option<&'a Context> {
    state.pending_fixture()?;
    let context = env.alert_context?;
    let replay_requested = crate::hooks::captured::var("QUANTICK_REPLAY_AUTOSTART")
        .is_some_and(|mode| mode == "1" || mode == "paused");
    let replay_ready = context.price.is_some()
        && matches!(
            &context.market.source,
            quantick_alerts::SourceContext::Replay { .. }
        );
    (!replay_requested || replay_ready).then_some(context)
}

fn action(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    ui.button(format!("{icon} {label}")).on_hover_text(label)
}
fn error(ui: &mut egui::Ui, message: &str) {
    ui.colored_label(ui.visuals().error_fg_color, message);
}
fn show_storage_error(ui: &mut egui::Ui, problem: &AlertError) {
    ui.heading("Alert storage is unavailable");
    error(ui, &error_text(problem));
    ui.label("Definitions and history cannot be confirmed. Failed saves do not replace the last saved definitions.");
    ui.label(
        "Correct the alert storage file or its permissions, then restart Quantick to reload it.",
    );
}
fn error_text(value: &AlertError) -> String {
    match value {
        AlertError::Invalid { field, reason } => format!("{field}: {reason}"),
        other => format!("{other:?}"),
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_ALERTS"];
