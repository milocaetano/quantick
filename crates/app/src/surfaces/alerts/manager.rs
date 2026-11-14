use super::*;
use egui_phosphor::regular as icons;
use quantick_alerts::{AlertCommand, AlertId, AlertStatus, MAX_HISTORY, Unavailable};

pub(super) fn draw_manager(
    ui: &mut egui::Ui,
    state: &State,
    book: &AlertBook,
    context: Option<&Context>,
    intents: &mut Vec<Intent>,
) {
    ui.horizontal_wrapped(|ui| {
        let mut history = state.history();
        ui.selectable_value(&mut history, false, format!("{} Alerts", icons::BELL));
        ui.selectable_value(&mut history, true, "History");
        if history != state.history() {
            intents.push(Intent::ShowHistory(history));
        }
        if ui
            .add_enabled(
                context.is_some(),
                egui::Button::new(format!("{} + Create", icons::BELL)),
            )
            .on_hover_text("Create an alert for the focused chart")
            .clicked()
            && let Some(context) = context
        {
            intents.push(Intent::Create {
                context: context.clone(),
                target: None,
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label("Search");
        let mut search = state.search().to_owned();
        let edit = egui::TextEdit::singleline(&mut search).desired_width(ui.available_width());
        if ui.add(edit).changed() {
            intents.push(Intent::Search(search));
        }
    });
    let mut newest_first = state.newest_first();
    if ui.checkbox(&mut newest_first, "Newest first").changed() {
        intents.push(Intent::NewestFirst(newest_first));
    }
    ui.separator();
    let body_height = (ui.ctx().screen_rect().height() - 180.0).max(40.0);
    egui::ScrollArea::vertical()
        .id_salt("alert_manager_body")
        .max_height(body_height)
        .show(ui, |ui| {
            if state.history() {
                draw_history(ui, state, book);
            } else {
                draw_rows(ui, state, book, context, intents);
            }
            if let Some(message) = state.error() {
                error(ui, message);
            }
            super::editor::follow_focus(ui);
        });
}
fn draw_rows(
    ui: &mut egui::Ui,
    state: &State,
    book: &AlertBook,
    context: Option<&Context>,
    intents: &mut Vec<Intent>,
) {
    let Ok(mut views) = book.views() else {
        error(ui, "Alert definitions are unavailable.");
        return;
    };
    let total = views.len();
    let needle = state.search().to_lowercase();
    views.retain(|view| {
        let spec = &view.spec;
        spec.name.to_lowercase().contains(&needle)
            || spec.market.symbol.to_lowercase().contains(&needle)
    });
    if state.newest_first() {
        views.reverse();
    }
    if views.is_empty() {
        ui.label(match total {
            0 => "No alerts. Create one for a price or drawing.",
            _ => "No alerts match the search.",
        });
    }
    for view in views {
        ui.push_id(view.spec.id.0, |ui| {
            ui.group(|ui| {
                ui.strong(&view.spec.name);
                super::editor::market_label(ui, &view.spec.market);
                ui.label(super::editor::summary(&view.spec, context));
                ui.label(status_label(view.status));
                ui.horizontal_wrapped(|ui| {
                    if let Some(command) =
                        status_action(ui, view.status, view.spec.id, view.revision)
                    {
                        intents.push(Intent::Command(command));
                    }
                    let edit = action(ui, icons::PENCIL_SIMPLE, "Edit").clicked();
                    let delete = action(ui, icons::TRASH, "Delete").clicked();
                    if edit || delete {
                        intents.push(Intent::EditExisting {
                            spec: Box::new(view.spec.clone()),
                            revision: view.revision,
                            context: context.cloned(),
                            delete,
                        });
                    }
                });
            });
        });
    }
}
fn draw_history(ui: &mut egui::Ui, state: &State, book: &AlertBook) {
    let Ok(mut events) = book.history(0, MAX_HISTORY) else {
        error(ui, "Alert history is unavailable.");
        return;
    };
    let needle = state.search().to_lowercase();
    events.retain(|event| event.spec.name.to_lowercase().contains(&needle));
    if state.newest_first() {
        events.reverse();
    }
    if events.is_empty() {
        ui.label("No alert events yet.");
    }
    for event in events {
        ui.push_id(event.sequence, |ui| {
            ui.group(|ui| {
                ui.strong(&event.spec.name);
                super::editor::market_label(ui, &event.spec.market);
                ui.label(format!(
                    "{} · {} · Price {}",
                    event.spec.market.symbol, event.spec.market.aggregation, event.price
                ));
                ui.label(format!(
                    "Received {} UTC · {:?}",
                    quantick_civil::fmt_offset_minute(
                        event.utc_ms,
                        quantick_civil::TzOffset::new(0)
                    ),
                    event.delivery
                ));
                ui.label(&event.spec.message);
                ui.label(format!(
                    "Definition revision {} · Event {}",
                    event.definition_revision, event.sequence
                ));
            });
        });
    }
}
pub(super) fn status_action(
    ui: &mut egui::Ui,
    status: AlertStatus,
    id: AlertId,
    expected_revision: u64,
) -> Option<AlertCommand> {
    let (icon, label, command) = match status {
        AlertStatus::Paused => (
            icons::PLAY,
            "Resume",
            AlertCommand::Resume {
                id,
                expected_revision,
            },
        ),
        AlertStatus::Completed => (
            icons::PLAY,
            "Rearm",
            AlertCommand::Rearm {
                id,
                expected_revision,
            },
        ),
        AlertStatus::Expired => {
            ui.add_enabled(false, egui::Button::new("Expired"))
                .on_disabled_hover_text("Edit the expiration before monitoring again.");
            return None;
        }
        _ => (
            icons::PAUSE,
            "Pause",
            AlertCommand::Pause {
                id,
                expected_revision,
            },
        ),
    };
    action(ui, icon, label).clicked().then_some(command)
}

fn status_label(status: AlertStatus) -> &'static str {
    match status {
        AlertStatus::Monitoring => "Monitoring",
        AlertStatus::Paused => "Paused",
        AlertStatus::Completed => "Completed",
        AlertStatus::Expired => "Expired",
        AlertStatus::Suspended(reason) => match reason {
            Unavailable::SourceInactive => "Suspended: source is inactive",
            Unavailable::MissingDrawing => "Suspended: drawing is missing",
            Unavailable::AmbiguousDrawing => "Suspended: drawing identity is ambiguous",
            Unavailable::LegacyIdentity => "Suspended: drawing identity must be saved",
            Unavailable::UnsupportedDrawing => "Suspended: drawing type is unsupported",
            Unavailable::OutsideTimeDomain => {
                "Suspended: price is outside the drawing's time range"
            }
            Unavailable::InvalidGeometry => "Suspended: drawing geometry is invalid",
            Unavailable::Hydrating => "Suspended: loading history",
            Unavailable::ReplayPaused => "Suspended: replay is paused",
            Unavailable::Scaffold => "Suspended: monitoring is unavailable",
        },
    }
}
