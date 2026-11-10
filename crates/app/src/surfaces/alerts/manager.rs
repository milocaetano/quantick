use super::*;
use egui_phosphor::regular as icons;
impl AlertSurface {
    pub(super) fn draw_manager(
        &mut self,
        ui: &mut egui::Ui,
        book: &AlertBook,
        env: &SurfaceEnv<'_>,
        response: &mut SurfaceResponse,
    ) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.history, false, format!("{} Alerts", icons::BELL));
            ui.selectable_value(&mut self.history, true, "History");
            if ui
                .add_enabled(
                    env.alert_context.is_some(),
                    egui::Button::new(format!("{} + Create", icons::BELL)),
                )
                .on_hover_text("Create an alert for the focused chart")
                .clicked()
                && let Some(context) = env.alert_context
            {
                self.create(context, None);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.text_edit_singleline(&mut self.search);
            ui.checkbox(&mut self.newest_first, "Newest first");
        });
        ui.separator();
        let body_height = (ui.ctx().screen_rect().height() - 180.0).max(40.0);
        egui::ScrollArea::vertical()
            .id_salt("alert_manager_body")
            .max_height(body_height)
            .show(ui, |ui| {
                if self.history {
                    self.draw_history(ui, book);
                } else {
                    self.draw_rows(ui, book, env, response);
                }
                if let Some(message) = &self.error {
                    error(ui, message);
                }
                super::editor::follow_focus(ui);
            });
    }
    fn draw_rows(
        &mut self,
        ui: &mut egui::Ui,
        book: &AlertBook,
        env: &SurfaceEnv<'_>,
        response: &mut SurfaceResponse,
    ) {
        let Ok(mut views) = book.views() else {
            error(ui, "Alert definitions are unavailable.");
            return;
        };
        let total = views.len();
        let needle = self.search.to_lowercase();
        views.retain(|view| {
            let spec = &view.spec;
            spec.name.to_lowercase().contains(&needle)
                || spec.market.symbol.to_lowercase().contains(&needle)
        });
        if self.newest_first {
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
                    ui.label(super::editor::summary(&view.spec, env.alert_context));
                    ui.label(status_label(view.status));
                    ui.horizontal_wrapped(|ui| {
                        if let Some(command) =
                            status_action(ui, view.status, view.spec.id, view.revision)
                        {
                            response.alerts = Some(Box::new(Request {
                                command: Some(command),
                                ..Default::default()
                            }));
                        }
                        if action(ui, icons::PENCIL_SIMPLE, "Edit").clicked() {
                            let mut context =
                                env.alert_context.cloned().unwrap_or_else(|| Context {
                                    market: view.spec.market.clone(),
                                    price: None,
                                    utc_ms: 0,
                                    display_offset_minutes: 0,
                                    drawings: Vec::new(),
                                });
                            if context.market != view.spec.market {
                                context.drawings.clear();
                                context.price = None;
                                context.market = view.spec.market.clone();
                            }
                            let (expiration_choice, custom_expiry) = match view.spec.expiration {
                                Expiration::Never => (0, context.utc_ms),
                                Expiration::At {
                                    utc_ms,
                                    display_offset_minutes,
                                } => {
                                    context.display_offset_minutes = display_offset_minutes;
                                    (4, utc_ms)
                                }
                            };
                            let tz = quantick_civil::TzOffset::new(context.display_offset_minutes);
                            let (_, _, _, hour, minute, _) =
                                quantick_civil::civil_utc(custom_expiry.saturating_add(
                                    i64::from(context.display_offset_minutes) * 60_000,
                                ));
                            let custom_date =
                                quantick_civil::CivilDate::from_ms(custom_expiry, tz).iso();
                            self.draft = Some(Draft {
                                spec: view.spec.clone(),
                                revision: Some(view.revision),
                                context,
                                subview: Subview::Conditions,
                                resolving: None,
                                expiration_error: None,
                                waiting: false,
                                confirming_delete: false,
                                focus_delete_cancel: false,
                                focus_name: true,
                                expiration_choice,
                                custom_expiry,
                                custom_date,
                                custom_hour: hour as u32,
                                custom_minute: minute as u32,
                            });
                        }
                    });
                });
            });
        }
    }
    fn draw_history(&self, ui: &mut egui::Ui, book: &AlertBook) {
        let Ok(mut events) = book.history(0, MAX_HISTORY) else {
            error(ui, "Alert history is unavailable.");
            return;
        };
        events.retain(|event| {
            event
                .spec
                .name
                .to_lowercase()
                .contains(&self.search.to_lowercase())
        });
        if self.newest_first {
            events.reverse();
        }
        if events.is_empty() {
            ui.label("No alert events yet.");
        }
        for event in events {
            ui.push_id(event.sequence, |ui| {
                ui.group(|ui| {
                    ui.strong(&event.spec.name);
                    ui.label(format!(
                        "{} · {} · Price {}",
                        event.spec.market.symbol, event.spec.market.aggregation, event.price
                    ));
                    ui.label(format!(
                        "{} UTC · {:?}",
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
