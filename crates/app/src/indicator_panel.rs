//! The indicator settings dialog, generated from [`InputSpec`]s.
//!
//! Every widget the user sees maps to exactly one spec variant — the crate
//! declares, the app renders, nothing is hand-tailored per indicator. Apply
//! sends the draft to the worker (construct anew → replace → replay), so a
//! running indicator never observes a value changing mid-stream; Cancel
//! drops the draft.

use eframe::egui;
use quantick_indicators::{InputSpec, InputValue, Rgba8, SourceId};

use crate::indicator_worker::SlotId;
use crate::theme;

/// Drag sensitivity of a float input that declares no `step`.
const DEFAULT_FLOAT_DRAG_SPEED: f64 = 0.1;

/// An open settings dialog: which slot, and the in-flight draft values.
pub(crate) struct SettingsDialog {
    /// The slot being edited.
    pub slot: SlotId,
    /// Dialog title (the indicator's label at open time).
    pub title: String,
    /// One draft value per declared input, edited in place.
    pub draft: Vec<InputValue>,
}

/// What the dialog asked for this frame.
pub(crate) enum SettingsOutcome {
    /// Keep showing the dialog.
    Open,
    /// Discard the draft.
    Cancel,
    /// Send the draft to the worker and close.
    Apply,
}

/// Draw the dialog; the caller owns the state and executes the outcome.
pub(crate) fn draw(
    ctx: &egui::Context,
    dialog: &mut SettingsDialog,
    specs: &[InputSpec],
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::Open;
    let mut open = true;
    egui::Window::new(format!("Settings — {}", dialog.title))
        .id(egui::Id::new(("indicator-settings", dialog.slot.0)))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("indicator-settings-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    for (spec, value) in specs.iter().zip(dialog.draft.iter_mut()) {
                        input_row(ui, spec, value);
                        ui.end_row();
                    }
                });
            if specs.is_empty() {
                ui.label(
                    egui::RichText::new("this indicator declares no settings")
                        .color(theme::TEXT_MUTED),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    outcome = SettingsOutcome::Apply;
                }
                if ui.button("Cancel").clicked() {
                    outcome = SettingsOutcome::Cancel;
                }
            });
        });
    if !open {
        outcome = SettingsOutcome::Cancel;
    }
    outcome
}

/// One label + widget row. The value enum always matches its spec variant
/// (both come from the same declaration); a mismatch draws nothing rather
/// than panicking a frame.
fn input_row(ui: &mut egui::Ui, spec: &InputSpec, value: &mut InputValue) {
    match (spec, value) {
        (
            InputSpec::Int {
                title,
                min,
                max,
                step,
                ..
            },
            InputValue::Int(current),
        ) => {
            ui.label(title);
            let mut drag = egui::DragValue::new(current);
            if let (Some(lo), Some(hi)) = (min, max) {
                drag = drag.range(*lo..=*hi);
            } else if let Some(lo) = min {
                drag = drag.range(*lo..=i64::MAX);
            } else if let Some(hi) = max {
                drag = drag.range(i64::MIN..=*hi);
            }
            if let Some(step) = step {
                drag = drag.speed(*step as f64);
            }
            ui.add(drag);
        }
        (
            InputSpec::Float {
                title,
                min,
                max,
                step,
                ..
            },
            InputValue::Float(current),
        ) => {
            ui.label(title);
            let mut drag = egui::DragValue::new(current);
            if let (Some(lo), Some(hi)) = (min, max) {
                drag = drag.range(*lo..=*hi);
            } else if let Some(lo) = min {
                drag = drag.range(*lo..=f64::MAX);
            } else if let Some(hi) = max {
                drag = drag.range(f64::MIN..=*hi);
            }
            drag = drag.speed(step.unwrap_or(DEFAULT_FLOAT_DRAG_SPEED));
            ui.add(drag);
        }
        (InputSpec::Bool { title, .. }, InputValue::Bool(current)) => {
            ui.label(title);
            ui.checkbox(current, "");
        }
        (InputSpec::Color { title, .. }, InputValue::Color(current)) => {
            ui.label(title);
            let mut rgba = [current.r, current.g, current.b, current.a];
            if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                *current = Rgba8::new(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
        (InputSpec::Str { title, options, .. }, InputValue::Str(current)) => {
            ui.label(title);
            if options.is_empty() {
                ui.text_edit_singleline(current);
            } else {
                egui::ComboBox::from_id_salt(("input-str", title))
                    .selected_text(current.clone())
                    .show_ui(ui, |ui| {
                        for option in options {
                            ui.selectable_value(current, option.clone(), option);
                        }
                    });
            }
        }
        (InputSpec::Source { title, .. }, InputValue::Source(current)) => {
            ui.label(title);
            egui::ComboBox::from_id_salt(("input-source", title))
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    for source in SourceId::ALL {
                        ui.selectable_value(current, source, source.as_str());
                    }
                });
        }
        // Spec/value variant mismatch: impossible through the worker's
        // plumbing; drawing nothing beats panicking the frame.
        _ => {
            ui.label(egui::RichText::new("(unrenderable input)").color(theme::TEXT_MUTED));
            ui.label("");
        }
    }
}
