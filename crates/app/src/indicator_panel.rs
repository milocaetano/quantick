//! The indicator settings dialog, generated from [`InputSpec`]s.
//!
//! Every widget the user sees maps to exactly one spec variant — the crate
//! declares, the app renders, nothing is hand-tailored per indicator. A
//! numeric input that declares both `minval` and `maxval` renders as a
//! slider; dragging any widget previews the draft live through the worker
//! (construct anew → replace → replay, the same path Apply takes), so a
//! running indicator never observes a value changing mid-stream — and the
//! dialog stays open, because tuning is a nudge-and-look loop (audit M2).
//! Apply is still the only commit: a preview never touches the state file,
//! and Close reverts the chart to the last committed values.

use eframe::egui;
use quantick_indicators::{InputSpec, InputValue, Rgba8, SourceId};

use crate::indicator_worker::SlotId;
use crate::theme;

/// Drag sensitivity of a float input that declares no `step`.
const DEFAULT_FLOAT_DRAG_SPEED: f64 = 0.1;

/// Where the dialog first opens: beside the drawing inspector's own default
/// (`DRAWING_INSPECTOR_DEFAULT_POSITION`), clear of the toolbar and of the
/// INDICATORS menu that spawns it — a deliberate position instead of egui's
/// centre default landing under the still-open menu (audit M3/QW2).
const SETTINGS_DEFAULT_POSITION: egui::Pos2 = egui::pos2(120.0, 150.0);

/// An open settings dialog: which slot, and the in-flight draft values.
pub(crate) struct SettingsDialog {
    /// The slot being edited.
    pub slot: SlotId,
    /// Dialog title (the indicator's label at open time).
    pub title: String,
    /// One draft value per declared input, edited in place.
    pub draft: Vec<InputValue>,
    /// The values last committed through Apply (seeded from the running
    /// instance at open time) — what Close must put back on the chart.
    pub committed: Vec<InputValue>,
    /// Whether a preview has been sent since the last commit: the chart may
    /// be showing values the state file does not hold.
    pub previewed: bool,
}

/// What the dialog asked for this frame.
pub(crate) enum SettingsOutcome {
    /// Keep showing the dialog.
    Open,
    /// Close the dialog, dropping any un-applied edits.
    Close,
    /// Send the draft to the worker; the dialog stays open.
    Apply,
    /// A widget changed this frame: show the draft on the chart without
    /// committing it (no state-file write; Close still reverts).
    Preview,
}

/// Draw the dialog; the caller owns the state and executes the outcome.
pub(crate) fn draw(
    ctx: &egui::Context,
    dialog: &mut SettingsDialog,
    specs: &[InputSpec],
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::Open;
    let mut open = true;
    let previewed = dialog.previewed;
    egui::Window::new(format!("Settings — {}", dialog.title))
        .id(egui::Id::new(("indicator-settings", dialog.slot.0)))
        .default_pos(SETTINGS_DEFAULT_POSITION)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let mut changed = false;
            let SettingsDialog {
                draft, committed, ..
            } = dialog;
            egui::Grid::new("indicator-settings-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    for ((spec, value), applied) in
                        specs.iter().zip(draft.iter_mut()).zip(committed.iter())
                    {
                        changed |= input_row(ui, spec, value, applied);
                        ui.end_row();
                    }
                });
            if changed {
                outcome = SettingsOutcome::Preview;
            }
            if specs.is_empty() {
                ui.label(
                    egui::RichText::new("this indicator declares no settings")
                        .color(theme::TEXT_MUTED),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button("Apply")
                    .on_hover_text("apply and keep tuning - the dialog stays open")
                    .clicked()
                {
                    outcome = SettingsOutcome::Apply;
                }
                // While a preview is pending the same button destroys work,
                // and its name must say so (trader-ux review).
                let (close_label, close_hover) = if previewed {
                    (
                        "Discard",
                        "close and revert the chart to the applied values",
                    )
                } else {
                    ("Close", "close without applying the current edits")
                };
                if ui.button(close_label).on_hover_text(close_hover).clicked() {
                    outcome = SettingsOutcome::Close;
                }
                // Honesty about the gap between chart and disk: while a
                // preview is live, the chart shows values Apply has not
                // committed yet.
                if previewed {
                    ui.label(
                        egui::RichText::new("previewing — Apply keeps, Close reverts")
                            .color(theme::TEXT_MUTED),
                    );
                }
            });
        });
    if !open {
        outcome = SettingsOutcome::Close;
    }
    outcome
}

/// One label + widget row. The value enum always matches its spec variant
/// (both come from the same declaration); a mismatch draws nothing rather
/// than panicking a frame. Returns whether the value changed this frame —
/// the signal the caller turns into a live preview. Free text is the one
/// widget that never reports a change: its intermediate states ("E", "EM")
/// are not values anyone asked to run. `applied` is the last committed
/// value: double-clicking a slider snaps back to it — the per-parameter
/// undo a live preview owes the hand that slipped.
fn input_row(
    ui: &mut egui::Ui,
    spec: &InputSpec,
    value: &mut InputValue,
    applied: &InputValue,
) -> bool {
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
            // A declared range on both ends is the author saying "this axis
            // is worth sweeping" — render it as a slider. A half-open range
            // has no lower/upper anchor to draw, so it stays a DragValue.
            if let (Some(lo), Some(hi)) = (min, max) {
                let mut slider = egui::Slider::new(current, *lo..=*hi);
                if let Some(step) = step {
                    slider = slider.step_by(*step as f64);
                }
                let response = ui
                    .add(slider)
                    .on_hover_text("double-click: back to applied");
                if response.double_clicked()
                    && let InputValue::Int(applied) = applied
                {
                    *current = *applied;
                    return true;
                }
                response.changed()
            } else {
                let mut drag = egui::DragValue::new(current);
                if let Some(lo) = min {
                    drag = drag.range(*lo..=i64::MAX);
                } else if let Some(hi) = max {
                    drag = drag.range(i64::MIN..=*hi);
                }
                if let Some(step) = step {
                    drag = drag.speed(*step as f64);
                }
                ui.add(drag).changed()
            }
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
            if let (Some(lo), Some(hi)) = (min, max) {
                let mut slider = egui::Slider::new(current, *lo..=*hi);
                if let Some(step) = step {
                    slider = slider.step_by(*step);
                }
                let response = ui
                    .add(slider)
                    .on_hover_text("double-click: back to applied");
                if response.double_clicked()
                    && let InputValue::Float(applied) = applied
                {
                    *current = *applied;
                    return true;
                }
                response.changed()
            } else {
                let mut drag = egui::DragValue::new(current);
                if let Some(lo) = min {
                    drag = drag.range(*lo..=f64::MAX);
                } else if let Some(hi) = max {
                    drag = drag.range(f64::MIN..=*hi);
                }
                drag = drag.speed(step.unwrap_or(DEFAULT_FLOAT_DRAG_SPEED));
                ui.add(drag).changed()
            }
        }
        (InputSpec::Bool { title, .. }, InputValue::Bool(current)) => {
            ui.label(title);
            ui.checkbox(current, "").changed()
        }
        (InputSpec::Color { title, .. }, InputValue::Color(current)) => {
            ui.label(title);
            let mut rgba = [current.r, current.g, current.b, current.a];
            if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                *current = Rgba8::new(rgba[0], rgba[1], rgba[2], rgba[3]);
                true
            } else {
                false
            }
        }
        (InputSpec::Str { title, options, .. }, InputValue::Str(current)) => {
            ui.label(title);
            if options.is_empty() {
                ui.text_edit_singleline(current);
                false
            } else {
                let mut changed = false;
                egui::ComboBox::from_id_salt(("input-str", title))
                    .selected_text(current.clone())
                    .show_ui(ui, |ui| {
                        for option in options {
                            changed |= ui
                                .selectable_value(current, option.clone(), option)
                                .changed();
                        }
                    });
                changed
            }
        }
        (InputSpec::Source { title, .. }, InputValue::Source(current)) => {
            ui.label(title);
            let mut changed = false;
            egui::ComboBox::from_id_salt(("input-source", title))
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    for source in SourceId::ALL {
                        changed |= ui
                            .selectable_value(current, source, source.as_str())
                            .changed();
                    }
                });
            changed
        }
        // Spec/value variant mismatch: impossible through the worker's
        // plumbing; drawing nothing beats panicking the frame.
        _ => {
            ui.label(egui::RichText::new("(unrenderable input)").color(theme::TEXT_MUTED));
            ui.label("");
            false
        }
    }
}
