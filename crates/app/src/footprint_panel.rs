//! The footprint settings window — the Profitchart-style properties dialog,
//! opened from the layer menu's "configure footprint…" entry.
//!
//! It edits the **focused chart's** knobs, not the window's: a split layout
//! is two different readings of the same market (a 90-day context chart and
//! a 50-tick flow chart), and one set of thresholds cannot serve both. The
//! title says which chart is being edited, and a chart that has never been
//! configured simply follows the last setup the trader used, so the common
//! case — one chart, one taste — still behaves like a global setting.
//!
//! Named presets sit at the top: a reading stance (scalp / swing / numbers
//! off) is worth one click, not five drags. Every control carries a hover
//! explaining itself in full — this window is where a newcomer learns what a
//! POC or a diagonal imbalance is, without a manual.

use std::path::Path;

use eframe::egui;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::footprint_config::{
    DETAIL_SCALE_RANGE, FootprintConfig, FootprintStyle, PROFILE_ROW_PX_RANGE,
};
use crate::footprint_presets::{MAX_NAME_LEN, PresetStore};
use crate::theme;

/// The number a pinned imbalance floor starts from when the trader unticks
/// "auto" — the Profitchart reference's own default, a sane opening bid the
/// drag immediately tunes.
const DEFAULT_PINNED_MIN_QTY: f64 = 20.0;

/// Everything the window edits, borrowed for one frame.
pub struct PanelInput<'a> {
    pub open: &'a mut bool,
    /// The focused chart's effective config (its own, or the window default
    /// it still follows).
    pub config: &'a mut FootprintConfig,
    pub presets: &'a mut PresetStore,
    pub presets_path: &'a Path,
    /// The "save as" field's text, kept across frames by the app.
    pub name_draft: &'a mut String,
    /// Which chart these knobs belong to, for the title.
    pub target: &'a str,
    /// Whether this chart has already diverged from the window default.
    pub customized: bool,
}

/// What the frame asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelOutcome {
    /// Nothing moved.
    Untouched,
    /// A knob moved: `config` now holds this chart's setup.
    Changed,
    /// "follow the default again" — drop this chart's own copy.
    ResetToDefault,
}

/// Draw the window when `open`; returns what the frame asked for.
pub fn draw(ctx: &egui::Context, input: PanelInput<'_>) -> PanelOutcome {
    if !*input.open {
        return PanelOutcome::Untouched;
    }
    let PanelInput {
        open,
        config,
        presets,
        presets_path,
        name_draft,
        target,
        customized,
    } = input;
    let mut outcome = PanelOutcome::Untouched;
    let mut changed = false;
    let mut window_open = *open;
    egui::Window::new(format!("footprint settings — {target}"))
        .id(egui::Id::new("footprint_settings"))
        .open(&mut window_open)
        .default_width(340.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(if customized {
                    "these knobs are this chart's own"
                } else {
                    "this chart follows your last setup"
                })
                .size(10.5)
                .color(theme::TEXT_MUTED),
            );

            ui.separator();
            ui.heading("Presets");
            ui.horizontal_wrapped(|ui| {
                let names: Vec<String> = presets.names().map(str::to_owned).collect();
                if names.is_empty() {
                    ui.label(
                        egui::RichText::new("none saved yet")
                            .size(11.0)
                            .color(theme::TEXT_FAINT),
                    );
                }
                for name in names {
                    let selected = presets.get(&name) == Some(&*config);
                    let response = ui
                        .selectable_label(selected, &name)
                        .on_hover_text("click to load · right-click to delete");
                    if response.clicked()
                        && let Some(preset) = presets.get(&name)
                        && *config != *preset
                    {
                        *config = preset.clone();
                        changed = true;
                    }
                    // Right-click deletes: a preset row with its own delete
                    // button would double the width of every entry.
                    if response.secondary_clicked() && presets.remove(&name) {
                        presets.save(presets_path);
                    }
                }
            });
            ui.horizontal(|ui| {
                let full = presets.is_full();
                ui.add(
                    egui::TextEdit::singleline(name_draft)
                        .desired_width(150.0)
                        .char_limit(MAX_NAME_LEN)
                        .hint_text("name this setup"),
                );
                let can_save = !name_draft.trim().is_empty();
                let save = ui.add_enabled(can_save, egui::Button::new("save preset"));
                if save
                    .on_hover_text(
                        "store these knobs under that name; a name you already \
                         used is overwritten",
                    )
                    .clicked()
                {
                    if presets.insert(name_draft, config) {
                        presets.save(presets_path);
                        name_draft.clear();
                    } else if full {
                        // The store refuses to evict someone else's stance;
                        // say so rather than dropping the click silently.
                        ui.label(
                            egui::RichText::new("preset list is full")
                                .size(10.5)
                                .color(theme::WARN),
                        );
                    }
                }
            });

            ui.separator();
            ui.heading("Style");
            // The registry draws this row, never a list kept here: a style
            // that exists is a style the panel offers, with the same name and
            // the same explanation the TOML and the env hook answer to.
            ui.horizontal_wrapped(|ui| {
                for style in FootprintStyle::ALL {
                    if ui
                        .selectable_label(config.style == style, style.label())
                        .on_hover_text(style.hover())
                        .clicked()
                        && config.style != style
                    {
                        config.style = style;
                        changed = true;
                    }
                }
            });

            // Only where they mean something. A knob for a style that is not
            // selected is a knob whose effect the trader cannot see, and this
            // window is small on purpose.
            if config.style == FootprintStyle::Cluster {
                ui.indent("cluster_knobs", |ui| {
                    changed |= ui
                        .checkbox(&mut config.cluster_show_total, "row total column")
                        .on_hover_text(
                            "the third column, bid × ask × total — what makes this \
                             style read like the reference chart. It costs about a \
                             third more candle width before the numbers fit; off, \
                             the style arrives at the same zoom the sell|buy ladder \
                             does",
                        )
                        .changed();
                    changed |= ui
                        .checkbox(&mut config.cluster_bevel, "raised cells")
                        .on_hover_text(
                            "a light top edge and a dark base on every cell, at the \
                             deepest zoom only — the reference chart's relief. Never \
                             drawn at shallower zooms: relief on a four-pixel cell is \
                             dirt",
                        )
                        .changed();
                });
            }

            ui.separator();
            ui.heading("Rows");
            changed |= ui
                .add(
                    egui::Slider::new(&mut config.profile_row_px, PROFILE_ROW_PX_RANGE)
                        .text("thinnest band px")
                        .step_by(0.5),
                )
                .on_hover_text(
                    "how fine the price bands may get before neighbouring rows merge. \
                     Lower = more, thinner bands per candle; the legend always names \
                     the price width one band stands for",
                )
                .changed();

            ui.separator();
            ui.heading("Detail");
            changed |= ui
                .add(
                    egui::Slider::new(&mut config.detail_scale, DETAIL_SCALE_RANGE)
                        .text("zoom needed for detail")
                        .step_by(0.05),
                )
                .on_hover_text(
                    "how much candle width each level waits for. 1.0 is where the \
                     numbers exactly fit their candle; lower brings marks, profile \
                     and numbers in at narrower candles, at the cost of the digits \
                     crowding their neighbour. The legend says how much further to \
                     zoom whenever numbers are not showing yet",
                )
                .changed();

            ui.separator();
            ui.heading("Numbers");
            changed |= ui
                .checkbox(&mut config.show_numbers, "numbers inside the candles")
                .on_hover_text(
                    "the per-row quantities. Off keeps the bars, imbalance chips, \
                     POC and badges — the shape of the fight without the digits",
                )
                .changed();
            changed |= ui
                .checkbox(&mut config.show_delta_totals, "bar delta totals")
                .on_hover_text("the per-bar delta chips along the chart's bottom edge")
                .changed();

            ui.separator();
            ui.heading("Imbalance");
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("dominance ≥")
                        .size(11.0)
                        .color(theme::TEXT_MUTED),
                );
                let mut ratio = config.imbalance_ratio.to_f64().unwrap_or(3.0);
                if ui
                    .add(
                        egui::DragValue::new(&mut ratio)
                            .range(1.0..=10.0)
                            .speed(0.1)
                            .suffix("x"),
                    )
                    .on_hover_text(
                        "one side must be at least this many times its diagonal \
                         neighbour to highlight as an imbalance",
                    )
                    .changed()
                    && let Some(decimal) =
                        rust_decimal::Decimal::from_f64((ratio * 10.0).round() / 10.0)
                {
                    config.imbalance_ratio = decimal;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                let mut auto = config.imbalance_min_qty.is_none();
                if ui
                    .checkbox(&mut auto, "auto qty floor")
                    .on_hover_text(
                        "the minimum size difference an imbalance needs. Auto reads \
                         the tape itself (p60 of per-row volume over the newest \
                         closed bars, shown in the layer legend); untick to pin a \
                         number",
                    )
                    .changed()
                {
                    config.imbalance_min_qty = (!auto)
                        .then(|| rust_decimal::Decimal::from_f64(DEFAULT_PINNED_MIN_QTY))
                        .flatten();
                    changed = true;
                }
                if let Some(current) = config.imbalance_min_qty {
                    let mut qty = current.to_f64().unwrap_or(DEFAULT_PINNED_MIN_QTY);
                    if ui
                        .add(
                            egui::DragValue::new(&mut qty)
                                .range(0.0..=1_000_000.0)
                                .speed(1.0),
                        )
                        .changed()
                        && let Some(decimal) = rust_decimal::Decimal::from_f64(qty)
                    {
                        config.imbalance_min_qty = Some(decimal);
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("stacked run")
                        .size(11.0)
                        .color(theme::TEXT_MUTED),
                );
                let mut stacked = config.stacked_count as u64;
                if ui
                    .add(egui::DragValue::new(&mut stacked).range(2..=6))
                    .on_hover_text(
                        "consecutive same-side imbalances that make a zone worth \
                         remembering",
                    )
                    .changed()
                {
                    config.stacked_count = stacked as usize;
                    changed = true;
                }
            });

            ui.separator();
            ui.heading("Marks");
            changed |= ui
                .checkbox(&mut config.show_poc, "POC line")
                .on_hover_text(
                    "Point of Control — the yellow line at the price with the most \
                     volume in each bar",
                )
                .changed();
            changed |= ui
                .checkbox(&mut config.extreme_ratio_badge, "extreme ratio badge")
                .on_hover_text(
                    "the Nx aggression ratio beside a bar's high and low, at the \
                     detailed zoom — the classic exhaustion cue",
                )
                .changed();
            if config.extreme_ratio_badge {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("badge ≥")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    );
                    let mut floor = config.badge_min_ratio.to_f64().unwrap_or(2.0);
                    if ui
                        .add(
                            egui::DragValue::new(&mut floor)
                                .range(1.0..=10.0)
                                .speed(0.1)
                                .suffix("x"),
                        )
                        .on_hover_text(
                            "hide badges below this ratio — 1.0x is the absence of \
                             aggression, and a badge that always shows stops being \
                             a signal",
                        )
                        .changed()
                        && let Some(decimal) =
                            rust_decimal::Decimal::from_f64((floor * 10.0).round() / 10.0)
                    {
                        config.badge_min_ratio = decimal;
                        changed = true;
                    }
                });
            }

            ui.separator();
            if ui
                .add_enabled(customized, egui::Button::new("follow the default again"))
                .on_hover_text(
                    "drop this chart's own knobs and go back to following the last \
                     setup used anywhere",
                )
                .clicked()
            {
                outcome = PanelOutcome::ResetToDefault;
            }
            ui.small("Changes apply to this chart immediately and persist across restarts.");
        });
    *open = window_open;
    if outcome == PanelOutcome::Untouched && changed {
        outcome = PanelOutcome::Changed;
    }
    outcome
}
