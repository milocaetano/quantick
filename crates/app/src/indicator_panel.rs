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

/// Width of the preset-name field, in pixels — room for a MAX_NAME_LEN-ish
/// name without pushing the save button off the row.
const PRESET_NAME_FIELD_WIDTH_PX: f32 = 110.0;

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
    /// Whether the widgets have had their first frame. A slider with a
    /// `step` snaps a not-quite-aligned value (0.1 is not a binary multiple
    /// of 0.01) the moment it first draws, and that snap reports `changed` —
    /// widget normalization, not a user edit. The first frame adopts it as
    /// the baseline instead of announcing a preview nobody asked for.
    pub settled: bool,
    /// What the preset picker shows: a preset name, [`DEFAULT_PRESET`], or
    /// `None` for "custom" — any hand-edit diverges from whatever was
    /// picked, and claiming the name would be the picker lying.
    pub preset_label: Option<String>,
    /// The name typed into the save-preset field.
    pub preset_name_draft: String,
}

/// The built-in preset every indicator has: its declared defaults. Not
/// stored anywhere — the script is its own source of truth.
pub(crate) const DEFAULT_PRESET: &str = "Default";

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
    /// Replace the draft with a saved preset's values (`None` = the
    /// declared defaults) and preview them. Apply still commits.
    LoadPreset(Option<String>),
    /// Save the current draft under this name.
    SavePreset(String),
    /// Forget the preset of this name.
    DeletePreset(String),
}

/// Draw the dialog; the caller owns the state and executes the outcome.
/// `presets` is the saved-setup list for this indicator's kind — `None`
/// hides the picker entirely (a slot whose kind was never registered has
/// no shelf to save to).
pub(crate) fn draw(
    ctx: &egui::Context,
    dialog: &mut SettingsDialog,
    specs: &[InputSpec],
    presets: Option<&[String]>,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::Open;
    let mut open = true;
    let mut changed = false;
    let previewed = dialog.previewed;
    let slot_id = dialog.slot.0;
    egui::Window::new(format!("Settings — {}", dialog.title))
        .id(egui::Id::new(("indicator-settings", slot_id)))
        .default_pos(SETTINGS_DEFAULT_POSITION)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let SettingsDialog {
                draft,
                committed,
                preset_label,
                preset_name_draft,
                ..
            } = dialog;
            if let Some(names) = presets {
                preset_row(
                    ui,
                    slot_id,
                    names,
                    preset_label,
                    preset_name_draft,
                    &mut outcome,
                );
                ui.separator();
            }
            let order = row_order(specs);
            egui::Grid::new("indicator-settings-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let mut current_section: Option<&str> = None;
                    for &index in &order {
                        let spec = &specs[index];
                        let (section, label) = split_section(spec.title());
                        if !section.is_empty() && current_section != Some(section) {
                            current_section = Some(section);
                            ui.label(
                                egui::RichText::new(section)
                                    .strong()
                                    .color(theme::TEXT_MUTED),
                            );
                            ui.label("");
                            ui.end_row();
                        }
                        changed |= input_row(ui, spec, &mut draft[index], &committed[index], label);
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
                        egui::RichText::new("previewing — Apply keeps, Discard reverts")
                            .color(theme::TEXT_MUTED),
                    );
                }
            });
        });
    // First-frame settle: adopt widget normalization as the baseline (see
    // `SettingsDialog::settled`) instead of reporting it as an edit.
    if !dialog.settled {
        dialog.settled = true;
        if changed {
            dialog.committed = dialog.draft.clone();
            changed = false;
        }
    }
    if changed && matches!(outcome, SettingsOutcome::Open) {
        // A hand-edit diverges from whatever preset was picked: the picker
        // says "custom" from here on instead of wearing a stale name.
        dialog.preset_label = None;
        outcome = SettingsOutcome::Preview;
    }
    if !open {
        outcome = SettingsOutcome::Close;
    }
    outcome
}

/// The preset picker: "Default" (the declared defaults) plus every saved
/// setup for this indicator's kind, a name field and a save button.
/// Picking one is a *preview*, exactly like dragging a slider — Apply is
/// still the only commit, so browsing presets mid-session is free.
fn preset_row(
    ui: &mut egui::Ui,
    slot_id: u64,
    names: &[String],
    preset_label: &Option<String>,
    preset_name_draft: &mut String,
    outcome: &mut SettingsOutcome,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Preset")
                .strong()
                .color(theme::TEXT_MUTED),
        );
        egui::ComboBox::from_id_salt(("indicator-preset", slot_id))
            .selected_text(preset_label.as_deref().unwrap_or("custom"))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        preset_label.as_deref() == Some(DEFAULT_PRESET),
                        DEFAULT_PRESET,
                    )
                    .on_hover_text("the script's declared defaults")
                    .clicked()
                {
                    *outcome = SettingsOutcome::LoadPreset(None);
                }
                for name in names {
                    let row = ui
                        .selectable_label(preset_label.as_deref() == Some(name.as_str()), name)
                        .on_hover_text("click previews; right-click deletes");
                    if row.clicked() {
                        *outcome = SettingsOutcome::LoadPreset(Some(name.clone()));
                    }
                    if row.secondary_clicked() {
                        *outcome = SettingsOutcome::DeletePreset(name.clone());
                    }
                }
            });
        ui.add(
            egui::TextEdit::singleline(preset_name_draft)
                .desired_width(PRESET_NAME_FIELD_WIDTH_PX)
                .hint_text("preset name"),
        );
        let name_ok = !preset_name_draft.trim().is_empty();
        if ui
            .add_enabled(name_ok, egui::Button::new("save"))
            .on_hover_text("save the current values under this name")
            .clicked()
        {
            *outcome = SettingsOutcome::SavePreset(preset_name_draft.trim().to_owned());
        }
    });
}

/// The section whose rows are hoisted to the top of the dialog: the layer
/// switches a trader reaches for mid-session must not sit below "Advanced"
/// at the bottom of a long parameter list (trader-ux review). Scripts opt
/// in by titling an input `Display: <layer>`.
pub(crate) const DISPLAY_SECTION: &str = "Display";

/// A title's section grammar, the one the built-in scripts already use:
/// `"1 Context: window (bars)"` → section `"1 Context"`, label
/// `"window (bars)"`. A title without the separator has no section.
pub(crate) fn split_section(title: &str) -> (&str, &str) {
    title
        .split_once(": ")
        .map_or(("", title), |(section, label)| (section, label))
}

/// Render order for the dialog's rows: script order, except the
/// [`DISPLAY_SECTION`] rows, which move to the top as one block. Rendering
/// order only — the draft/committed vectors stay in declaration order, so
/// positional persistence never notices.
fn row_order(specs: &[InputSpec]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..specs.len()).collect();
    // Stable sort: within "display first, rest after", script order holds.
    order.sort_by_key(|&index| split_section(specs[index].title()).0 != DISPLAY_SECTION);
    order
}

/// One label + widget row. The value enum always matches its spec variant
/// (both come from the same declaration); a mismatch draws nothing rather
/// than panicking a frame. Returns whether the value changed this frame —
/// the signal the caller turns into a live preview. Free text is the one
/// widget that never reports a change: its intermediate states ("E", "EM")
/// are not values anyone asked to run. `applied` is the last committed
/// value: double-clicking a slider snaps back to it — the per-parameter
/// undo a live preview owes the hand that slipped. `label` is the title
/// with its section prefix stripped; widget ids keep using the full title,
/// which is the stable name.
fn input_row(
    ui: &mut egui::Ui,
    spec: &InputSpec,
    value: &mut InputValue,
    applied: &InputValue,
    label: &str,
) -> bool {
    match (spec, value) {
        (InputSpec::Int { min, max, step, .. }, InputValue::Int(current)) => {
            ui.label(label);
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
        (InputSpec::Float { min, max, step, .. }, InputValue::Float(current)) => {
            ui.label(label);
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
        (InputSpec::Bool { .. }, InputValue::Bool(current)) => {
            ui.label(label);
            ui.checkbox(current, "").changed()
        }
        (InputSpec::Color { .. }, InputValue::Color(current)) => {
            ui.label(label);
            let mut rgba = [current.r, current.g, current.b, current.a];
            if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                *current = Rgba8::new(rgba[0], rgba[1], rgba[2], rgba[3]);
                true
            } else {
                false
            }
        }
        (InputSpec::Str { title, options, .. }, InputValue::Str(current)) => {
            ui.label(label);
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
            ui.label(label);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_worker::SlotId;

    fn stepped_float(title: &str) -> InputSpec {
        InputSpec::Float {
            name: "x".to_owned(),
            title: title.to_owned(),
            default: 0.1,
            min: Some(0.0),
            max: Some(0.5),
            step: Some(0.05),
            options: Vec::new(),
        }
    }

    fn display_bool() -> InputSpec {
        InputSpec::Bool {
            name: "show".to_owned(),
            title: "Display: context semaphore".to_owned(),
            default: true,
        }
    }

    /// A stepped slider snaps a value that is not on its grid the moment it
    /// first draws, and reports that as `changed`. Without the settle frame
    /// the dialog opened already announcing a preview nobody asked for —
    /// this test fails with `Preview` on frame 0 if that regresses.
    #[test]
    fn first_frame_widget_normalization_is_not_a_preview() {
        let specs = vec![stepped_float("Advanced: min bar body (×ATR)")];
        let mut dialog = SettingsDialog {
            slot: SlotId(7),
            title: "Copilot".to_owned(),
            draft: vec![InputValue::Float(0.123)],
            committed: vec![InputValue::Float(0.123)],
            previewed: false,
            settled: false,
            preset_label: None,
            preset_name_draft: String::new(),
        };
        let ctx = egui::Context::default();
        for frame in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                let outcome = draw(ctx, &mut dialog, &specs, None);
                assert!(
                    !matches!(outcome, SettingsOutcome::Preview),
                    "frame {frame}: widget normalization must not announce a preview"
                );
            });
        }
        assert!(dialog.settled, "the first frame settles the dialog");
        assert_eq!(
            dialog.draft, dialog.committed,
            "whatever the widgets normalized became the baseline"
        );
    }

    /// Render order only: the Display rows move to the top as one block,
    /// everything else keeps script order — and the indices still point at
    /// the declaration positions the state file persists by.
    #[test]
    fn display_rows_hoist_to_the_top_in_render_order_only() {
        let specs = vec![
            stepped_float("1 Context: max height (×ATR)"),
            display_bool(),
            stepped_float("Advanced: min bar body (×ATR)"),
        ];
        assert_eq!(row_order(&specs), vec![1, 0, 2]);
    }

    #[test]
    fn titles_split_into_section_and_label() {
        assert_eq!(
            split_section("1 Context: window (bars)"),
            ("1 Context", "window (bars)")
        );
        assert_eq!(split_section("Length"), ("", "Length"));
    }
}
