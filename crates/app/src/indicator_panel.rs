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
use quantick_indicators::{InputSpec, InputValue, PlotSpec, Rgba8, SourceId};

use crate::indicator_style::{MAX_PLOT_WIDTH_PX, MIN_PLOT_WIDTH_PX, ResolvedPlot, StyleOverride};
use crate::indicator_worker::SlotId;
use crate::theme;

/// Drag sensitivity of a float input that declares no `step`.
const DEFAULT_FLOAT_DRAG_SPEED: f64 = 0.1;
/// Width of a numeric input with no declared range, in pixels. Wide enough to
/// look like a box you can type into and to hold a five-figure value.
const NUMBER_FIELD_WIDTH_PX: f32 = 72.0;
/// Size of the line preview at the end of a Style row, in pixels: long enough
/// to read a width from, one text line tall so the row does not grow.
const PLOT_PREVIEW_SIZE: egui::Vec2 = egui::vec2(44.0, 16.0);
/// How much of its colour a switched-off plot's preview keeps — enough to
/// still be a line, faded enough to read as off, matching the legend dot.
const PREVIEW_OFF_FADE: f32 = 0.4;
/// Width the preview draws a switched-off plot at: a hairline, because the
/// number beside it describes a line nobody is drawing.
const PREVIEW_OFF_WIDTH_PX: f32 = 1.0;
/// Drag sensitivity of a plot's width, in pixels per pointer pixel. The whole
/// usable range is eight pixels wide, so a 1:1 drag would cross it in a flick.
const PLOT_WIDTH_DRAG_SPEED: f64 = 0.02;

/// Width of the preset-name field, in pixels — room for a MAX_NAME_LEN-ish
/// name without pushing the save button off the row.
const PRESET_NAME_FIELD_WIDTH_PX: f32 = 110.0;

/// Which half of the dialog is showing.
///
/// The two are mutually exclusive on purpose, and that is load-bearing: an
/// input edit and a style edit can never happen in the same frame, so the
/// dialog's single outcome can speak for either without ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    /// The declared inputs — periods, sources, thresholds.
    #[default]
    Inputs,
    /// Per-plot colour, width and visibility.
    Style,
}

/// An open settings dialog: which slot, and the in-flight draft values.
pub(crate) struct SettingsDialog {
    /// The slot being edited.
    pub slot: SlotId,
    /// Dialog title (the indicator's label at open time).
    pub title: String,
    /// Which tab is showing.
    pub tab: SettingsTab,
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
    /// A plot's style moved on the Style tab. Render-side and already on the
    /// chart — all the caller owes is the state file.
    ///
    /// Its own outcome rather than a flag beside `Preview`, because the tabs
    /// are mutually exclusive: a frame edits an input or a plot's style, never
    /// both, so one outcome can carry either without lying about the other.
    StyleChanged,
}

/// Draw the dialog; the caller owns the state and executes the outcome.
/// `presets` is the saved-setup list for this indicator's kind — `None`
/// hides the picker entirely (a slot whose kind was never registered has
/// no shelf to save to).
pub(crate) fn draw(
    ctx: &egui::Context,
    dialog: &mut SettingsDialog,
    specs: &[InputSpec],
    plots: &[PlotSpec],
    style: &mut StyleOverride,
    presets: Option<&[String]>,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::Open;
    let mut open = true;
    let mut changed = false;
    let mut style_changed = false;
    let previewed = dialog.previewed;
    let slot_id = dialog.slot.0;
    // An indicator that declares no plots gets no Style tab rather than an
    // empty one — a tab that opens onto nothing is a promise the dialog cannot
    // keep.
    let has_style = !plots.is_empty();
    if dialog.tab == SettingsTab::Style && !has_style {
        dialog.tab = SettingsTab::Inputs;
    }
    egui::Window::new(format!("Settings — {}", dialog.title))
        .id(egui::Id::new(("indicator-settings", slot_id)))
        .default_pos(crate::popup::position(ctx))
        .pivot(crate::popup::anchor())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            if has_style {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut dialog.tab, SettingsTab::Inputs, "Inputs");
                    ui.selectable_value(&mut dialog.tab, SettingsTab::Style, "Style");
                });
                ui.separator();
            }
            if dialog.tab == SettingsTab::Style {
                style_changed = draw_style(ui, plots, style);
                draw_buttons(ui, previewed, &mut outcome);
                return;
            }
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
            draw_buttons(ui, previewed, &mut outcome);
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
    if style_changed && matches!(outcome, SettingsOutcome::Open) {
        outcome = SettingsOutcome::StyleChanged;
    }
    if !open {
        outcome = SettingsOutcome::Close;
    }
    // Escape gets out, the way every other dialog in the app lets you out.
    //
    // This is a trading window, and it can now be opened by a double click
    // that was meant for the chart; while one of its fields holds the keyboard
    // the app's trading shortcuts are suppressed by design
    // (`wants_keyboard_input`). One key back to the chart is the difference
    // between an interruption and a trap. Escape is Close, which reverts a
    // pending preview — the same thing the button of that name does.
    //
    // Read after the window has drawn, so an Escape a field consumed to cancel
    // its own edit is spent there first: one press leaves the field, the next
    // leaves the dialog.
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        outcome = SettingsOutcome::Close;
    }
    outcome
}

/// The button row, shared by both tabs so the way out of the dialog is in the
/// same place whichever half you are on.
fn draw_buttons(ui: &mut egui::Ui, previewed: bool, outcome: &mut SettingsOutcome) {
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .button("Apply")
            .on_hover_text("apply and keep tuning - the dialog stays open")
            .clicked()
        {
            *outcome = SettingsOutcome::Apply;
        }
        // While a preview is pending the same button destroys work, and its
        // name must say so (trader-ux review).
        let (close_label, close_hover) = if previewed {
            (
                "Discard",
                "close and revert the chart to the applied values",
            )
        } else {
            ("Close", "close without applying the current edits")
        };
        if ui.button(close_label).on_hover_text(close_hover).clicked() {
            *outcome = SettingsOutcome::Close;
        }
        // Honesty about the gap between chart and disk: while a preview is
        // live, the chart shows values Apply has not committed yet.
        if previewed {
            ui.label(
                egui::RichText::new("previewing — Apply keeps, Discard reverts")
                    .color(theme::TEXT_MUTED),
            );
        }
    });
}

/// The Style tab: one row per declared plot — draw it, its colour, its width,
/// and a preview of the line those two produce.
///
/// Writes straight through to the app-side [`StyleOverride`] rather than into
/// a draft. Style needs no draft and no Apply: it is render-side, it costs a
/// repaint rather than a rebuild and a replay, and the chart already shows the
/// answer. Returns whether anything moved, which is all the caller needs to
/// know to write the state file.
fn draw_style(ui: &mut egui::Ui, plots: &[PlotSpec], style: &mut StyleOverride) -> bool {
    let mut changed = false;
    egui::Grid::new("indicator-style-grid")
        .num_columns(4)
        .spacing([10.0, 8.0])
        .show(ui, |ui| {
            for (index, spec) in plots.iter().enumerate() {
                let resolved = style.resolve(index, spec);
                let mut over = style.get(index);

                let mut visible = resolved.visible;
                if ui.checkbox(&mut visible, &spec.title).changed() {
                    // Recorded only when it differs from the declaration, so
                    // ticking a box back on leaves no override behind.
                    over.visible = (!visible).then_some(false);
                    changed = true;
                }

                let mut rgba = [
                    resolved.color.r,
                    resolved.color.g,
                    resolved.color.b,
                    resolved.color.a,
                ];
                if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                    over.color = Some(Rgba8::new(rgba[0], rgba[1], rgba[2], rgba[3]));
                    changed = true;
                }

                let mut width = resolved.width;
                if ui
                    .add(
                        egui::DragValue::new(&mut width)
                            .range(MIN_PLOT_WIDTH_PX..=MAX_PLOT_WIDTH_PX)
                            .speed(PLOT_WIDTH_DRAG_SPEED)
                            .suffix(" px"),
                    )
                    .on_hover_text("line width")
                    .changed()
                {
                    over.width = Some(width);
                    changed = true;
                }

                // What the two controls beside it add up to. A swatch alone
                // says nothing about width, and a number alone says nothing
                // about how thin 0.5 px looks over candles.
                draw_plot_preview(ui, style.resolve(index, spec), visible);
                style.set(index, over);
                ui.end_row();
            }
        });
    // The way back. The Inputs tab has one — the `Default` preset restores the
    // declared values — and without this the Style tab had none: a trader who
    // tuned a palette into something unreadable could only guess their way back
    // to the colours the indicator's author chose. Disabled when there is
    // nothing to undo, so it never offers to change what it would not.
    ui.add_space(4.0);
    if ui
        .add_enabled(!style.is_default(), egui::Button::new("Reset style"))
        .on_hover_text("back to the colours and widths the indicator declares")
        .on_disabled_hover_text("already showing what the indicator declares")
        .clicked()
    {
        style.clear();
        changed = true;
    }
    changed
}

/// A short line in the plot's resolved colour and width — greyed to a hairline
/// when the plot is switched off, because a preview of a line nobody is drawing
/// has to look like one.
fn draw_plot_preview(ui: &mut egui::Ui, resolved: ResolvedPlot, visible: bool) {
    let (rect, _) = ui.allocate_exact_size(PLOT_PREVIEW_SIZE, egui::Sense::hover());
    let color = egui::Color32::from_rgba_unmultiplied(
        resolved.color.r,
        resolved.color.g,
        resolved.color.b,
        resolved.color.a,
    );
    let (color, width) = if visible {
        (color, resolved.width)
    } else {
        (
            theme::TEXT_MUTED.gamma_multiply(PREVIEW_OFF_FADE),
            PREVIEW_OFF_WIDTH_PX,
        )
    };
    let y = rect.center().y;
    ui.painter().line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        egui::Stroke::new(width, color),
    );
}

/// A numeric input with no declared range, drawn as a *field* rather than a
/// bare number.
///
/// egui's `DragValue` has always been typeable — click it and it becomes a text
/// box — but nothing said so, so it read as a number you could only nudge, and
/// nudging a period from 9 to 200 is not a thing anyone will do. A fixed width
/// gives it the shape of a field and the hover says the rest, bounds included:
/// a trader who types 500 into a field capped at 100 deserves to know that
/// before the value silently clamps. (A spec with *both* bounds is a slider
/// instead, and carries its own hover.)
fn number_field(
    ui: &mut egui::Ui,
    drag: egui::DragValue<'_>,
    bounds: Option<String>,
) -> egui::Response {
    let response = ui.add_sized([NUMBER_FIELD_WIDTH_PX, ui.spacing().interact_size.y], drag);
    match bounds {
        Some(bounds) => response.on_hover_text(format!("click to type, or drag  ({bounds})")),
        None => response.on_hover_text("click to type, or drag"),
    }
}

/// The half-open bound a numeric spec declares, spelled for a tooltip. `None`
/// when it declares neither — an empty parenthesis is chrome saying nothing.
fn range_hint<T: Copy>(
    min: Option<T>,
    max: Option<T>,
    show: impl Fn(T) -> String,
) -> Option<String> {
    match (min, max) {
        (Some(lo), Some(hi)) => Some(format!("{} to {}", show(lo), show(hi))),
        (Some(lo), None) => Some(format!("{} or more", show(lo))),
        (None, Some(hi)) => Some(format!("up to {}", show(hi))),
        (None, None) => None,
    }
}

/// A numeric input the spec closed to a fixed set: a dropdown of exactly those
/// values, which is the only control that cannot produce one outside it.
fn number_options<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    salt: (&str, &str),
    current: &mut T,
    options: &[T],
    show: impl Fn(T) -> String,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(show(*current))
        .show_ui(ui, |ui| {
            for option in options {
                changed |= ui
                    .selectable_value(current, *option, show(*option))
                    .clicked();
            }
        });
    changed
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
        (
            InputSpec::Int {
                min,
                max,
                step,
                options,
                title,
                ..
            },
            InputValue::Int(current),
        ) => {
            ui.label(label);
            // A declared option list is a *closed* set. Drawing a free field or
            // a slider over one let the dialog produce a number the indicator
            // does not take, which the build then silently replaced with
            // something else — a value shown as bound that never was.
            if !options.is_empty() {
                return number_options(ui, ("input-int", title), current, options, |v: i64| {
                    v.to_string()
                });
            }
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
                number_field(ui, drag, range_hint(*min, *max, |v: i64| v.to_string())).changed()
            }
        }
        (
            InputSpec::Float {
                min,
                max,
                step,
                options,
                title,
                ..
            },
            InputValue::Float(current),
        ) => {
            ui.label(label);
            if !options.is_empty() {
                return number_options(ui, ("input-float", title), current, options, |v: f64| {
                    format!("{v}")
                });
            }
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
                number_field(ui, drag, range_hint(*min, *max, |v: f64| format!("{v}"))).changed()
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

    fn plot(title: &str) -> PlotSpec {
        PlotSpec {
            id: quantick_indicators::PlotId::new(0),
            title: title.to_owned(),
            style: quantick_indicators::PlotStyle::Line,
            base_color: Rgba8::opaque(0x26, 0xA6, 0x9A),
            width: 1.5,
            offset: 0,
            marker: None,
        }
    }

    fn open_dialog() -> SettingsDialog {
        SettingsDialog {
            slot: SlotId(0),
            title: "EMA(9)".to_owned(),
            tab: SettingsTab::default(),
            draft: vec![InputValue::Int(9)],
            committed: vec![InputValue::Int(9)],
            previewed: false,
            settled: false,
            preset_label: None,
            preset_name_draft: String::new(),
        }
    }

    fn painted_dialog(
        ctx: &egui::Context,
        dialog: &mut SettingsDialog,
        specs: &[InputSpec],
        plots: &[PlotSpec],
        style: &mut StyleOverride,
    ) -> String {
        let mut text = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                draw(ctx, dialog, specs, plots, style, None);
            });
            text.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                    text.push(' ');
                }
            }
        }
        text
    }

    /// Criterion 2: the Style tab exists when there are plots to style, and
    /// does not exist when there are none — a tab that opens onto nothing is a
    /// promise the dialog cannot keep.
    #[test]
    fn the_style_tab_appears_only_for_an_indicator_that_declares_plots() {
        let ctx = egui::Context::default();
        let specs = [int_spec()];

        let mut with_plots = open_dialog();
        let mut style = StyleOverride::default();
        let text = painted_dialog(&ctx, &mut with_plots, &specs, &[plot("ema")], &mut style);
        assert!(text.contains("Inputs") && text.contains("Style"), "{text}");

        let mut without = open_dialog();
        let text = painted_dialog(&ctx, &mut without, &specs, &[], &mut style);
        assert!(!text.contains("Style"), "no plots, no Style tab: {text}");

        // And a tab left selected when the plots vanish falls back rather than
        // showing an empty half.
        let mut stale = open_dialog();
        stale.tab = SettingsTab::Style;
        let _ = painted_dialog(&ctx, &mut stale, &specs, &[], &mut style);
        assert_eq!(stale.tab, SettingsTab::Inputs, "it fell back");
    }

    /// A style edit reports itself so the caller can write the state file, and
    /// it is render-side: it never claims to be a preview, which would put the
    /// dialog into the un-applied state and offer to Discard something there is
    /// nothing to discard.
    #[test]
    fn a_style_edit_is_its_own_outcome_and_lands_on_the_layer() {
        let mut style = StyleOverride::default();
        style.set(
            0,
            crate::indicator_style::PlotOverride {
                color: Some(Rgba8::opaque(255, 0, 0)),
                ..crate::indicator_style::PlotOverride::default()
            },
        );
        let resolved = style.resolve(0, &plot("ema"));
        assert_eq!(resolved.color, Rgba8::opaque(255, 0, 0));
        assert!(
            (resolved.width - 1.5).abs() < f32::EPSILON,
            "an untouched field still falls through to the declaration"
        );
    }

    /// A spec that declares a closed set of values gets a control that cannot
    /// produce one outside it.
    ///
    /// The slider the dialog drew over an option list let a trader land on a
    /// number the indicator does not take; the build then quietly swapped in
    /// something else and the dialog went on showing the value that was never
    /// bound — the "inferred data, silently patched" the honesty rule forbids.
    #[test]
    fn an_input_with_a_closed_option_set_renders_as_a_choice() {
        let ctx = egui::Context::default();
        let specs = [InputSpec::Int {
            name: "smoothing".to_owned(),
            title: "Smoothing".to_owned(),
            default: 3,
            min: Some(1),
            max: Some(9),
            step: None,
            options: vec![1, 3, 5],
        }];
        let mut dialog = open_dialog();
        dialog.draft = vec![InputValue::Int(5)];
        dialog.committed = vec![InputValue::Int(5)];
        let mut style = StyleOverride::default();
        let text = painted_dialog(&ctx, &mut dialog, &specs, &[], &mut style);

        assert!(text.contains("Smoothing"), "the row is drawn: {text}");
        assert!(text.contains('5'), "showing the value bound now: {text}");
        assert!(
            !text.contains("unrenderable"),
            "the option branch was reached: {text}"
        );
    }

    /// The dialog opens clear of the chart it is about.
    ///
    /// It previews live, so the one thing it must not do is cover the drawing,
    /// profile or curve you opened it to judge. It lands in the bottom-left —
    /// the corner the legend, the HUD, the order-flow key, the book badge and
    /// the price axis all leave alone — placed by its own bottom-left corner so
    /// it grows upward instead of pushing its buttons off the window. Computed
    /// from the live window, because a fixed `y` is the bottom of one size only.
    #[test]
    fn the_dialog_opens_in_the_corner_that_hides_the_least_chart() {
        for size in [egui::vec2(1700.0, 950.0), egui::vec2(900.0, 560.0)] {
            let ctx = egui::Context::default();
            let mut dialog = open_dialog();
            let specs = [int_spec()];
            let plots = [plot("ema")];
            let mut style = StyleOverride::default();
            let id = egui::Id::new(("indicator-settings", dialog.slot.0));
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
                    ..egui::RawInput::default()
                };
                let _ = ctx.run(input, |ctx| {
                    draw(ctx, &mut dialog, &specs, &plots, &mut style, None);
                });
            }
            // The window's own rect, not the painted shapes: egui draws a drop
            // shadow whose bounding box spills past the frame.
            let rect = ctx
                .memory(|memory| memory.area_rect(id))
                .expect("the dialog was laid out");

            assert!(
                rect.bottom() <= size.y,
                "{size:?}: the buttons must be on screen, not below it ({rect:?})"
            );
            assert!(
                rect.bottom() > size.y / 2.0,
                "{size:?}: it opens in the bottom half ({rect:?})"
            );
            assert!(
                rect.left() < size.x / 2.0,
                "{size:?}: and on the left ({rect:?})"
            );
            assert!(rect.top() >= 0.0, "{size:?}: and not above it ({rect:?})");
        }
    }

    /// Escape gets out — a trader UX finding, not a nicety.
    ///
    /// The dialog can now be opened by a double click aimed at the chart, and
    /// while one of its fields holds the keyboard the app's trading shortcuts
    /// are suppressed by design. Without a key that closes it, the way out is
    /// to find and hit a button, which is the wrong thing to be doing when the
    /// reason you want out is the tape. Escape is Close, so it also reverts a
    /// pending preview.
    #[test]
    fn escape_closes_the_dialog() {
        let ctx = egui::Context::default();
        let mut dialog = open_dialog();
        let specs = [int_spec()];
        let mut style = StyleOverride::default();

        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let outcome = draw(ctx, &mut dialog, &specs, &[], &mut style, None);
            assert!(matches!(outcome, SettingsOutcome::Open), "nothing pressed");
        });

        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let mut closed = false;
        let _ = ctx.run(input, |ctx| {
            closed = matches!(
                draw(ctx, &mut dialog, &specs, &[], &mut style, None),
                SettingsOutcome::Close
            );
        });
        assert!(closed, "Escape is the way out");
    }

    fn int_spec() -> InputSpec {
        InputSpec::Int {
            name: "length".to_owned(),
            title: "Length".to_owned(),
            default: 9,
            min: Some(1),
            max: None,
            step: None,
            options: Vec::new(),
        }
    }

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
            tab: SettingsTab::default(),
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
                let outcome = draw(
                    ctx,
                    &mut dialog,
                    &specs,
                    &[],
                    &mut StyleOverride::default(),
                    None,
                );
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
