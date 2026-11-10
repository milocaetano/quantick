//! Alert manager and editor. All mutations return through the persistent host.
use super::{Surface, SurfaceEnv, SurfaceResponse};
use eframe::egui;
use quantick_alerts::*;
use quantick_control_schema::alerts::DrawingSelector;
mod editor;
mod manager;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub market: MarketRef,
    pub price: Option<f64>,
    pub utc_ms: i64,
    pub display_offset_minutes: i32,
    pub drawings: Vec<DrawingChoice>,
}
#[derive(Debug, Clone)]
pub(crate) struct DrawingChoice {
    pub selector: DrawingSelector,
    pub target: Option<DurableDrawingRef>,
    pub kind: TargetKind,
    pub name: String,
}
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Request {
    pub open: bool,
    pub drawing: Option<DurableDrawingRef>,
    pub command: Option<AlertCommand>,
    pub resolve: Option<DrawingSelector>,
    pub locate: Option<DurableDrawingRef>,
}
#[derive(Default)]
pub(crate) struct AlertSurface {
    open: bool,
    pending_drawing: Option<Option<DurableDrawingRef>>,
    #[cfg(any(feature = "scenario-harness", test))]
    pending_fixture: Option<Subview>,
    draft: Option<Draft>,
    history: bool,
    pub(crate) search: String,
    newest_first: bool,
    error: Option<String>,
}
struct Draft {
    spec: AlertSpec,
    revision: Option<u64>,
    context: Context,
    subview: Subview,
    resolving: Option<(usize, DrawingSelector)>,
    expiration_error: Option<String>,
    waiting: bool,
    confirming_delete: bool,
    focus_delete_cancel: bool,
    focus_name: bool,
    expiration_choice: usize,
    custom_expiry: i64,
    custom_date: String,
    custom_hour: u32,
    custom_minute: u32,
}
#[derive(Clone, Copy, PartialEq)]
enum Subview {
    Conditions,
    Message,
    Notifications,
}
impl AlertSurface {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }
    pub(crate) fn open_drawing(&mut self, target: Option<DurableDrawingRef>) {
        self.open = true;
        self.pending_drawing = Some(target);
    }
    pub(crate) fn apply(
        &mut self,
        _book: &mut AlertBook,
        request: Request,
    ) -> Result<(), AlertError> {
        if request.command.is_some() || request.resolve.is_some() || request.locate.is_some() {
            return Err(AlertError::Unavailable);
        }
        self.open |= request.open;
        if let Some(target) = request.drawing {
            self.open_drawing(Some(target));
        }
        Ok(())
    }
    pub(crate) fn settle_command(&mut self, outcome: Result<(), String>) {
        match outcome {
            Ok(()) => {
                self.draft = None;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error);
                if let Some(draft) = &mut self.draft {
                    draft.waiting = false;
                }
            }
        }
    }
    pub(crate) fn settle_target(&mut self, outcome: Result<DurableDrawingRef, String>) {
        if let Some(draft) = &mut self.draft
            && let Some((index, selector)) = draft.resolving.take()
        {
            match outcome {
                Ok(target) => {
                    if let Some(choice) = draft
                        .context
                        .drawings
                        .iter_mut()
                        .find(|choice| choice.selector == selector)
                    {
                        choice.target = Some(target.clone());
                        draft.spec.conditions[index].operator =
                            quantick_alerts::operators::for_target(
                                draft.spec.conditions[index].operator,
                                choice.kind,
                            );
                    }
                    draft.spec.conditions[index].target = TargetRef::Drawing(target);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
    fn create(&mut self, context: &Context, target: Option<DurableDrawingRef>) {
        let kind = target
            .as_ref()
            .and_then(|target| {
                context
                    .drawings
                    .iter()
                    .find(|choice| choice.target.as_ref() == Some(target))
            })
            .map_or(TargetKind::Value, |choice| choice.kind);
        let operator = quantick_alerts::operators::for_target(Operator::Crossing, kind);
        self.draft = Some(Draft {
            spec: AlertSpec {
                version: FORMAT_VERSION,
                id: AlertId(0),
                name: format!("{} price alert", context.market.symbol),
                market: context.market.clone(),
                conditions: vec![Predicate {
                    operator,
                    target: target.map_or(
                        TargetRef::Value(context.price.unwrap_or(0.0)),
                        TargetRef::Drawing,
                    ),
                }],
                frequency: Frequency::Once,
                expiration: Expiration::Never,
                message: String::new(),
                notification: Notification {
                    popup: false,
                    toast: true,
                    sound: None,
                    sound_seconds: None,
                },
            },
            revision: None,
            context: context.clone(),
            subview: Subview::Conditions,
            resolving: None,
            expiration_error: None,
            waiting: false,
            confirming_delete: false,
            focus_delete_cancel: false,
            focus_name: true,
            expiration_choice: 0,
            custom_expiry: context.utc_ms,
            custom_date: quantick_civil::CivilDate::from_ms(
                context.utc_ms,
                quantick_civil::TzOffset::new(context.display_offset_minutes),
            )
            .iso(),
            custom_hour: 23,
            custom_minute: 59,
        });
        self.error = None;
    }
}
impl Surface for AlertSurface {
    fn id(&self) -> &'static str {
        "alerts"
    }
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let mut response = SurfaceResponse::default();
        if !self.open {
            return response;
        }
        let Some(book) = env.alerts else {
            return response;
        };
        #[cfg(any(feature = "scenario-harness", test))]
        self.open_ready_fixture(env);
        if let Some(target) = self.pending_drawing.take() {
            if let Some(context) = env.alert_context {
                self.create(context, target);
            } else {
                self.error = Some("Open a chart before creating an alert.".into());
            }
        }
        // Consume Escape only after a nested combo has had an opportunity to close.
        if !ctx.memory(|memory| memory.any_popup_open())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if let Some(draft) = &mut self.draft
                && draft.confirming_delete
            {
                draft.confirming_delete = false;
            } else if self.draft.is_some() {
                self.draft = None;
            } else {
                self.open = false;
            }
            return response;
        }
        let width = 560.0_f32.min((ctx.screen_rect().width() - 32.0).max(100.0));
        let mut open = self.open;
        let title = match self.draft.as_ref() {
            Some(draft) if draft.revision.is_some() => "Edit alert",
            Some(_) => "Create alert",
            None => "Alerts",
        };
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
                    ui.heading("Alert storage is unavailable");
                    error(ui, &error_text(storage_error));
                    ui.label("Definitions and history cannot be confirmed. Failed saves do not replace the last saved definitions.");
                    ui.label("Correct the alert storage file or its permissions, then restart Quantick to reload it. Your open draft is retained until you close this window or restart.");
                } else if self.draft.is_some() {
                    self.draw_editor(ui, book, env, &mut response);
                } else {
                    self.draw_manager(ui, book, env, &mut response);
                }
            });
        self.open = open;
        response
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        if let Some(mode) = crate::hooks::captured::var("QUANTICK_ALERTS") {
            self.open = true;
            self.pending_fixture = match mode.as_str() {
                "editor" => Some(Subview::Conditions),
                "editor-message" => Some(Subview::Message),
                "editor-notifications" => Some(Subview::Notifications),
                _ => None,
            };
            self.history = mode == "history";
        }
    }
}
#[cfg(any(feature = "scenario-harness", test))]
impl AlertSurface {
    /// A replay capture must open the same draft a trader sees after loading.
    /// This delay belongs only to fixture activation; manual Create stays immediate.
    fn open_ready_fixture(&mut self, env: &SurfaceEnv<'_>) {
        let Some(subview) = self.pending_fixture else {
            return;
        };
        let Some(context) = env.alert_context else {
            return;
        };
        let replay_requested = crate::hooks::captured::var("QUANTICK_REPLAY_AUTOSTART")
            .is_some_and(|mode| mode == "1" || mode == "paused");
        if replay_requested
            && (context.price.is_none()
                || !matches!(&context.market.source, SourceContext::Replay { .. }))
        {
            return;
        }
        self.create(context, None);
        if let Some(draft) = &mut self.draft {
            draft.subview = subview;
        }
        self.pending_fixture = None;
    }
}

fn action(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    ui.button(format!("{icon} {label}")).on_hover_text(label)
}
fn error(ui: &mut egui::Ui, message: &str) {
    ui.colored_label(ui.visuals().error_fg_color, message);
}
fn error_text(value: &AlertError) -> String {
    match value {
        AlertError::Invalid { field, reason } => format!("{field}: {reason}"),
        other => format!("{other:?}"),
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_ALERTS"];
