use super::*;
use egui_phosphor::regular as icons;

impl AlertSurface {
    pub(super) fn draw_editor(
        &mut self,
        ui: &mut egui::Ui,
        book: &AlertBook,
        env: &SurfaceEnv<'_>,
        response: &mut SurfaceResponse,
    ) {
        let draft = self.draft.as_mut().expect("editor is open");
        market_label(ui, &draft.spec.market);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut draft.subview, Subview::Conditions, "Conditions");
            ui.selectable_value(&mut draft.subview, Subview::Message, "Message");
            ui.selectable_value(&mut draft.subview, Subview::Notifications, "Notifications");
        });
        ui.separator();
        // Reserve the viewport margin, window title and fixed footer after the
        // measured header, including wrapped source labels on short windows.
        let header_height = (ui.cursor().top() - ui.min_rect().top()).max(0.0);
        let body_height = (ui.ctx().screen_rect().height() - header_height - 112.0).max(40.0);
        egui::ScrollArea::vertical()
            .id_salt("alert_editor_body")
            .max_height(body_height)
            .show(ui, |ui| {
                ui.label("Name");
                let name = ui.text_edit_singleline(&mut draft.spec.name);
                if draft.focus_name {
                    name.request_focus();
                    draft.focus_name = false;
                }
                match draft.subview {
                    Subview::Conditions => conditions(ui, draft, book, response),
                    Subview::Message => {
                        ui.label("Message shown when this alert fires");
                        ui.add(
                            egui::TextEdit::multiline(&mut draft.spec.message)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    Subview::Notifications => notifications(ui, draft, env, response),
                }
                if let Ok(view) = book.editor(draft.spec.clone()) {
                    ui.separator();
                    ui.label("Summary");
                    ui.label(view.summary);
                    for issue in &view.errors {
                        error(ui, &error_text(issue));
                    }
                }
                if let Some(message) = &self.error {
                    error(ui, message);
                }
            });
        ui.separator();
        let mut cancel = false;
        ui.horizontal(|ui| {
            if draft.confirming_delete {
                ui.label("Delete this alert? History is kept.");
                let keep = ui.button("Keep alert");
                if draft.focus_delete_cancel {
                    keep.request_focus();
                    draft.focus_delete_cancel = false;
                }
                if keep.clicked() {
                    draft.confirming_delete = false;
                }
                if ui.button("Confirm delete").clicked() {
                    response.alerts = Some(Box::new(Request {
                        command: Some(AlertCommand::Delete {
                            id: draft.spec.id,
                            expected_revision: draft.revision.expect("only saved alerts delete"),
                        }),
                        ..Default::default()
                    }));
                    draft.waiting = true;
                }
                return;
            }
            if draft.revision.is_some() && action(ui, icons::TRASH, "Delete").clicked() {
                draft.confirming_delete = true;
                draft.focus_delete_cancel = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
            let valid = book.validate(&draft.spec).is_ok()
                && !draft.waiting
                && draft.resolving.is_none()
                && draft.expiration_error.is_none();
            if ui
                .add_enabled(
                    valid,
                    egui::Button::new(if draft.revision.is_some() {
                        "Save changes"
                    } else {
                        "Create alert"
                    }),
                )
                .clicked()
            {
                let command = draft.revision.map_or_else(
                    || AlertCommand::Create(draft.spec.clone()),
                    |expected_revision| AlertCommand::Edit {
                        expected_revision,
                        spec: draft.spec.clone(),
                    },
                );
                response.alerts = Some(Box::new(Request {
                    command: Some(command),
                    ..Default::default()
                }));
                draft.waiting = true;
            }
            if draft.waiting {
                ui.label("Saving…");
            }
        });
        if cancel {
            self.draft = None;
            self.error = None;
        }
    }
}
pub(super) fn market_label(ui: &mut egui::Ui, market: &MarketRef) {
    let (label, identity) = match &market.source {
        SourceContext::Live { feed } => (feed.clone(), feed),
        SourceContext::Replay { recording } => (
            format!(
                "Replay · {}",
                recording.rsplit(['/', '\\']).next().unwrap_or(recording)
            ),
            recording,
        ),
    };
    ui.label(format!(
        "{label} · {} · {}",
        market.symbol, market.aggregation
    ))
    .on_hover_text(identity);
}
fn operator_label(kind: Option<TargetKind>, item: &OperatorDescriptor) -> &'static str {
    match (kind, item.operator) {
        (Some(TargetKind::Rectangle), Operator::GreaterThan) => "Above upper boundary",
        (Some(TargetKind::Rectangle), Operator::LessThan) => "Below lower boundary",
        _ => item.label,
    }
}
fn conditions(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    book: &AlertBook,
    response: &mut SurfaceResponse,
) {
    ui.strong("All conditions");
    ui.label("Every condition must be true at the same observation.");
    let mut remove = None;
    for (index, predicate) in draft.spec.conditions.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Price");
                    let kind = match &predicate.target {
                        TargetRef::Value(_) => Some(TargetKind::Value),
                        TargetRef::Drawing(target) => draft
                            .context
                            .drawings
                            .iter()
                            .find(|choice| choice.target.as_ref() == Some(target))
                            .map(|choice| choice.kind),
                    };
                    let label = quantick_alerts::operators::OPERATORS
                        .iter()
                        .find(|item| item.operator == predicate.operator)
                        .map_or("Select operator", |item| operator_label(kind, item));
                    egui::ComboBox::from_id_salt("operator")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for item in
                                quantick_alerts::operators::OPERATORS.iter().filter(|item| {
                                    kind.is_some_and(|kind| item.targets.contains(&kind))
                                })
                            {
                                let label = operator_label(kind, item);
                                ui.selectable_value(&mut predicate.operator, item.operator, label);
                            }
                        });
                    if action(ui, icons::X, "Remove condition").clicked() {
                        remove = Some(index);
                    }
                });
                let target_name = match &predicate.target {
                    TargetRef::Value(_) => "Value".to_owned(),
                    TargetRef::Drawing(target) => draft
                        .context
                        .drawings
                        .iter()
                        .find(|choice| choice.target.as_ref() == Some(target))
                        .map_or_else(
                            || format!("Drawing {} (unavailable)", target.drawing),
                            |choice| choice.name.clone(),
                        ),
                };
                egui::ComboBox::from_id_salt("target")
                    .selected_text(target_name)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                matches!(predicate.target, TargetRef::Value(_)),
                                "Value",
                            )
                            .clicked()
                        {
                            predicate.target = TargetRef::Value(draft.context.price.unwrap_or(0.0));
                            predicate.operator = quantick_alerts::operators::for_target(
                                predicate.operator,
                                TargetKind::Value,
                            );
                        }
                        for choice in &draft.context.drawings {
                            if ui
                                .selectable_label(
                                    choice.target.as_ref().is_some_and(|target| {
                                        predicate.target == TargetRef::Drawing(target.clone())
                                    }),
                                    &choice.name,
                                )
                                .clicked()
                            {
                                draft.resolving = Some((index, choice.selector));
                                response.alerts = Some(Box::new(Request {
                                    resolve: Some(choice.selector),
                                    ..Default::default()
                                }));
                            }
                        }
                    });
                match &mut predicate.target {
                    TargetRef::Value(value) => {
                        ui.add(
                            egui::DragValue::new(value)
                                .speed(0.1)
                                .custom_formatter(|value, _| value.to_string())
                                .prefix("Value "),
                        );
                    }
                    TargetRef::Drawing(target) => {
                        if action(ui, icons::CROSSHAIR, "Locate drawing").clicked() {
                            response.alerts = Some(Box::new(Request {
                                locate: Some(target.clone()),
                                ..Default::default()
                            }));
                        }
                    }
                }
            });
        });
    }
    if let Some(index) = remove {
        draft.spec.conditions.remove(index);
    }
    if ui
        .add_enabled(
            draft.spec.conditions.len() < MAX_CONDITIONS,
            egui::Button::new("Add condition"),
        )
        .clicked()
    {
        draft.spec.conditions.push(Predicate {
            operator: Operator::Crossing,
            target: TargetRef::Value(draft.context.price.unwrap_or(0.0)),
        });
    }
    ui.separator();
    ui.label("Trigger");
    egui::ComboBox::from_id_salt("frequency")
        .selected_text(frequency_label(draft.spec.frequency))
        .show_ui(ui, |ui| {
            for frequency in [
                Frequency::Once,
                Frequency::PerBar,
                Frequency::PerBarClose,
                Frequency::PerMinute,
            ] {
                ui.selectable_value(
                    &mut draft.spec.frequency,
                    frequency,
                    frequency_label(frequency),
                );
            }
        });
    ui.label(match draft.spec.frequency {
        Frequency::Once => "Completes after the first trigger.",
        Frequency::PerBar => "At most one trigger in each bar.",
        Frequency::PerBarClose => "Evaluates each committed bar close.",
        Frequency::PerMinute => "At least one minute between triggers.",
    });
    expiration(ui, draft, book);
}
fn frequency_label(value: Frequency) -> &'static str {
    match value {
        Frequency::Once => "Only once",
        Frequency::PerBar => "Once per bar",
        Frequency::PerBarClose => "Once per bar close",
        Frequency::PerMinute => "Once per minute",
    }
}
fn expiration(ui: &mut egui::Ui, draft: &mut Draft, book: &AlertBook) {
    ui.label("Expiration");
    let labels = [
        "Never",
        "End of day",
        "One week",
        "One calendar month",
        "Custom",
    ];
    let before = draft.expiration_choice;
    egui::ComboBox::from_id_salt("expiration")
        .selected_text(labels[draft.expiration_choice])
        .show_ui(ui, |ui| {
            for (index, label) in labels.iter().enumerate() {
                ui.selectable_value(&mut draft.expiration_choice, index, *label);
            }
        });
    let mut changed = false;
    if draft.expiration_choice == 4 {
        ui.horizontal_wrapped(|ui| {
            ui.label("Date (YYYY-MM-DD)");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut draft.custom_date).desired_width(100.0))
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut draft.custom_hour)
                        .range(0..=23)
                        .prefix("Hour "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut draft.custom_minute)
                        .range(0..=59)
                        .prefix("Minute "),
                )
                .changed();
        });
        ui.label(quantick_civil::TzOffset::new(draft.context.display_offset_minutes).label());
        if let Some(date) = quantick_civil::parse_iso_date(&draft.custom_date) {
            draft.custom_expiry = date.start_ms(quantick_civil::TzOffset::new(
                draft.context.display_offset_minutes,
            )) + i64::from(draft.custom_hour) * 3_600_000
                + i64::from(draft.custom_minute) * 60_000;
        } else {
            draft.expiration_error = Some("Enter a valid date as YYYY-MM-DD.".into());
            error(ui, "Enter a valid date as YYYY-MM-DD.");
            return;
        }
    }
    if before != draft.expiration_choice || changed {
        let choice = match draft.expiration_choice {
            0 => ExpirationChoice::Never,
            1 => ExpirationChoice::EndOfDay,
            2 => ExpirationChoice::Week,
            3 => ExpirationChoice::CalendarMonth,
            _ => ExpirationChoice::Custom {
                utc_ms: draft.custom_expiry,
            },
        };
        match book.resolve_expiration(
            choice,
            draft.context.utc_ms,
            draft.context.display_offset_minutes,
        ) {
            Ok(expiration) => {
                draft.spec.expiration = expiration;
                draft.expiration_error = None;
            }
            Err(issue) => draft.expiration_error = Some(error_text(&issue)),
        }
    }
    if let Some(issue) = &draft.expiration_error {
        error(ui, issue);
    }
    if let Expiration::At {
        utc_ms,
        display_offset_minutes,
    } = draft.spec.expiration
    {
        let tz = quantick_civil::TzOffset::new(display_offset_minutes);
        ui.label(format!(
            "{} {}",
            quantick_civil::fmt_offset_minute(utc_ms, tz),
            tz.label()
        ));
    }
}
fn notifications(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    env: &SurfaceEnv<'_>,
    response: &mut SurfaceResponse,
) {
    ui.checkbox(&mut draft.spec.notification.popup, "Local popup");
    ui.checkbox(&mut draft.spec.notification.toast, "Local toast");
    ui.label("Sound");
    let selected = draft
        .spec
        .notification
        .sound
        .as_deref()
        .and_then(sound::AlertSound::from_token);
    egui::ComboBox::from_id_salt("sound")
        .selected_text(selected.map_or("No sound", sound::AlertSound::label))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut draft.spec.notification.sound, None, "No sound");
            for category in sound::SoundCategory::ALL {
                ui.strong(category.label());
                for sound in sound::AlertSound::in_category(category) {
                    ui.selectable_value(
                        &mut draft.spec.notification.sound,
                        Some(sound.token().to_owned()),
                        sound.label(),
                    );
                }
            }
        });
    if let Some(sound) = selected {
        if action(ui, icons::SPEAKER_HIGH, "Preview sound").clicked() {
            response.test_alert = Some(crate::audio::Cue::new(
                sound,
                draft.spec.notification.sound_seconds,
            ));
        }
        if sound.can_be_cut() {
            let mut cut = draft.spec.notification.sound_seconds.is_some();
            if ui.checkbox(&mut cut, "Limit duration").changed() {
                draft.spec.notification.sound_seconds = cut.then_some(5);
            }
            if let Some(seconds) = &mut draft.spec.notification.sound_seconds {
                ui.add(
                    egui::DragValue::new(seconds)
                        .range(1..=3600)
                        .suffix(" seconds"),
                );
            }
        }
    }
    if let Some(failure) = env.alert_failure {
        error(ui, failure);
    }
    ui.label("Alerts are monitored while this workspace is open.");
}
