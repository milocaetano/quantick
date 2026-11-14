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

use quantick_ui_model::UpdateSession;
use quantick_ui_model::indicator_settings::{
    DEFAULT_PRESET, Intent, Model, SettingsOutcome, row_order,
};
pub(crate) use quantick_ui_model::indicator_settings::{
    DISPLAY_SECTION, SettingsTab, split_section,
};

/// Paint native widgets against the registered owner's readonly projection.
/// The caller settles the final outcome after all native precedence is known.
pub(crate) fn draw(
    ctx: &egui::Context,
    session: &mut UpdateSession<'_, Model>,
    specs: &[InputSpec],
    plots: &[PlotSpec],
    style: &mut StyleOverride,
    presets: Option<&[String]>,
) -> Intent {
    let palette = theme::get(ctx);
    let mut outcome = SettingsOutcome::Open;
    let mut open = true;
    let mut changed = false;
    let mut style_changed = false;
    let dialog = session.state().dialog().expect("resolved editor");
    let previewed = dialog.previewed;
    let slot_id = dialog.target.slot.0;
    let has_style = !plots.is_empty();
    let window = egui::Window::new(format!("Settings — {}", dialog.title));
    crate::popup::centred(window, egui::Id::new(("indicator-settings", slot_id)), ctx)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            if has_style {
                ui.horizontal(|ui| {
                    for tab in [SettingsTab::Inputs, SettingsTab::Style] {
                        let label = match tab {
                            SettingsTab::Inputs => "Inputs",
                            SettingsTab::Style => "Style",
                        };
                        if ui
                            .selectable_label(session.state().dialog().unwrap().tab == tab, label)
                            .clicked()
                        {
                            session.update(Intent::SelectTab(tab));
                        }
                    }
                });
                ui.separator();
            }
            if session.state().dialog().unwrap().tab == SettingsTab::Style {
                style_changed = draw_style(ui, plots, style);
                draw_buttons(ui, previewed, &mut outcome);
                return;
            }
            if let Some(names) = presets {
                let dialog = session.state().dialog().unwrap();
                let mut name = dialog.preset_name_draft.clone();
                preset_row(
                    ui,
                    slot_id,
                    names,
                    &dialog.preset_label,
                    &mut name,
                    &mut outcome,
                );
                if name != session.state().dialog().unwrap().preset_name_draft {
                    session.update(Intent::EditPresetName(name));
                }
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
                                    .color(palette.text_muted()),
                            );
                            ui.label("");
                            ui.end_row();
                        }
                        let dialog = session.state().dialog().unwrap();
                        let mut value = dialog.draft[index].clone();
                        changed |= input_row(ui, spec, &mut value, &dialog.committed[index], label);
                        if value != session.state().dialog().unwrap().draft[index] {
                            session.update(Intent::EditInput { index, value });
                        }
                        ui.end_row();
                    }
                });
            if specs.is_empty() {
                ui.label(
                    egui::RichText::new("this indicator declares no settings")
                        .color(palette.text_muted()),
                );
            }
            draw_buttons(ui, previewed, &mut outcome);
        });
    Intent::FinishPaint {
        changed,
        style_changed,
        outcome,
        window_closed: !open,
        // Read after fields have consumed Escape to cancel their own edits.
        escape: ctx.input(|input| input.key_pressed(egui::Key::Escape)),
    }
}

/// The button row, shared by both tabs so the way out of the dialog is in the
/// same place whichever half you are on.
fn draw_buttons(ui: &mut egui::Ui, previewed: bool, outcome: &mut SettingsOutcome) {
    let palette = theme::get(ui.ctx());
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
                    .color(palette.text_muted()),
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
                let resolved = style.resolve(index, spec, None);
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
                draw_plot_preview(ui, style.resolve(index, spec, None), visible);
                style.set(index, over);
                ui.end_row();
            }
        });
    // The way back to the defaults: the surface's ink for a plot (its colour as
    // it arrives here), else its declared colour. Disabled with nothing to undo.
    ui.add_space(4.0);
    if ui
        .add_enabled(!style.is_default(), egui::Button::new("Reset style"))
        .on_hover_text("back to the default colours (the theme's, else the script's) and widths")
        .on_disabled_hover_text("already showing the defaults")
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
    let palette = theme::get(ui.ctx());
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
            palette.text_muted().gamma_multiply(PREVIEW_OFF_FADE),
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
    let palette = theme::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Preset")
                .strong()
                .color(palette.text_muted()),
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
    let palette = theme::get(ui.ctx());
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
            ui.label(egui::RichText::new("(unrenderable input)").color(palette.text_muted()));
            ui.label("");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    type Editor = crate::surfaces::indicator_settings::IndicatorSettingsSurface;

    fn editor(slot: u64, title: &str, values: Vec<InputValue>) -> Editor {
        let mut editor = Editor::new(
            Default::default(),
            crate::surfaces::indicator_settings::effects,
        );
        editor.update(Intent::Open {
            target: quantick_chart::indicator_registry::TabSlot {
                workspace: quantick_workspace::lifetime::WorkspaceId(1),
                pane: 1,
                tab: 1,
                side: crate::pane::PaneSide::Flow,
                slot: crate::indicator_worker::SlotId(slot),
            },
            title: title.to_owned(),
            values,
        });
        editor
    }

    fn open_dialog() -> Editor {
        editor(0, "EMA(9)", vec![InputValue::Int(9)])
    }

    fn draw(
        ctx: &egui::Context,
        editor: &mut Editor,
        specs: &[InputSpec],
        plots: &[PlotSpec],
        style: &mut StyleOverride,
        presets: Option<&[String]>,
    ) -> SettingsOutcome {
        let mut outcome = SettingsOutcome::Open;
        editor.with_updates(
            |response| {
                for effect in response.model_effects {
                    if let crate::surfaces::HostEffect::IndicatorSettings(effects) = effect {
                        for change in effects.changes {
                            outcome = match change {
                                quantick_ui_model::indicator_settings::Change::Inputs {
                                    commit: true,
                                    ..
                                } => SettingsOutcome::Apply,
                                quantick_ui_model::indicator_settings::Change::Inputs {
                                    ..
                                } => SettingsOutcome::Preview,
                                quantick_ui_model::indicator_settings::Change::Style(_) => {
                                    SettingsOutcome::StyleChanged
                                }
                            };
                        }
                    }
                }
            },
            |_, session| {
                let label = session.state().dialog().unwrap().title.clone();
                session.update(Intent::ObserveTarget {
                    valid: true,
                    label,
                    has_style: !plots.is_empty(),
                });
                let intent = super::draw(ctx, session, specs, plots, style, presets);
                session.update(intent);
            },
        );
        if editor.model_state.dialog().is_none() {
            SettingsOutcome::Close
        } else {
            outcome
        }
    }

    fn painted_dialog(
        ctx: &egui::Context,
        dialog: &mut Editor,
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
        stale.update(Intent::SelectTab(SettingsTab::Style));
        let _ = painted_dialog(&ctx, &mut stale, &specs, &[], &mut style);
        assert_eq!(
            stale.model_state.dialog().unwrap().tab,
            SettingsTab::Inputs,
            "it fell back"
        );
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
        let resolved = style.resolve(0, &plot("ema"), None);
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
        let mut dialog = editor(0, "EMA(9)", vec![InputValue::Int(5)]);
        let mut style = StyleOverride::default();
        let text = painted_dialog(&ctx, &mut dialog, &specs, &[], &mut style);

        assert!(text.contains("Smoothing"), "the row is drawn: {text}");
        assert!(text.contains('5'), "showing the value bound now: {text}");
        assert!(
            !text.contains("unrenderable"),
            "the option branch was reached: {text}"
        );
    }

    /// The dialog opens in the centre, fully on screen.
    ///
    /// It used to land in the bottom-left corner to keep clear of the chart,
    /// which read as a window opening somewhere unrelated to the click; the
    /// trader asked for the centre (2026-11-08). A trader who wants the
    /// preview uncovered drags it once and it reopens there. Computed from
    /// the live window, because a fixed point is the centre of one size only.
    #[test]
    fn the_dialog_opens_centred_and_on_screen() {
        for size in [egui::vec2(1700.0, 950.0), egui::vec2(900.0, 560.0)] {
            let ctx = egui::Context::default();
            let mut dialog = open_dialog();
            let specs = [int_spec()];
            let plots = [plot("ema")];
            let mut style = StyleOverride::default();
            let id = egui::Id::new((
                "indicator-settings",
                dialog.model_state.target().unwrap().slot.0,
            ));
            // Enough frames for the window's size, and so its centring, to
            // settle.
            for _ in 0..6 {
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

            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            assert!(
                screen.contains_rect(rect),
                "{size:?}: the buttons must be on screen ({rect:?})"
            );
            assert!(
                rect.center().distance(screen.center()) < 2.0,
                "{size:?}: it opens centred, not in a corner ({rect:?})"
            );
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

    /// A stepped slider snaps a value that is not on its grid the moment it
    /// first draws, and reports that as `changed`. Without the settle frame
    /// the dialog opened already announcing a preview nobody asked for —
    /// this test fails with `Preview` on frame 0 if that regresses.
    #[test]
    fn first_frame_widget_normalization_is_not_a_preview() {
        let specs = vec![stepped_float("Advanced: min bar body (×ATR)")];
        let mut dialog = editor(7, "Copilot", vec![InputValue::Float(0.123)]);
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
        assert!(
            dialog.model_state.dialog().unwrap().settled,
            "the first frame settles the dialog"
        );
        assert_eq!(
            dialog.model_state.dialog().unwrap().draft,
            dialog.model_state.dialog().unwrap().committed,
            "whatever the widgets normalized became the baseline"
        );
    }
}
