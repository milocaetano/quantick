//! The footprint settings window — the Profitchart-style properties dialog,
//! opened from the layer menu's "configure footprint…" entry.
//!
//! One window per app (the config is the window's: one set of thresholds,
//! every pane), editing [`FootprintConfig`] live — the next frame draws with
//! whatever moved — and reporting `true` so the app persists the change to
//! the settings file. Every control carries a hover explaining itself in
//! full: this window is where a newcomer learns what a POC or an imbalance
//! is, without a manual.

use eframe::egui;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::footprint_config::{FootprintConfig, FootprintStyle, PROFILE_ROW_PX_RANGE};
use crate::theme;

/// The number a pinned imbalance floor starts from when the trader unticks
/// "auto" — the Profitchart reference's own default, a sane opening bid the
/// drag immediately tunes.
const DEFAULT_PINNED_MIN_QTY: f64 = 20.0;

/// Draw the window when `open`; returns whether any knob changed.
pub fn draw(ctx: &egui::Context, open: &mut bool, config: &mut FootprintConfig) -> bool {
    if !*open {
        return false;
    }
    let mut changed = false;
    let mut window_open = *open;
    egui::Window::new("footprint settings")
        .open(&mut window_open)
        .default_width(330.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading("Style");
            ui.horizontal(|ui| {
                for (style, label, hover) in [
                    (
                        FootprintStyle::Split,
                        "profile",
                        "volume profile on the right, the winning side's delta bar \
                         on the left — the reference look, and the default",
                    ),
                    (
                        FootprintStyle::Ladder,
                        "sell|buy",
                        "the classic footprint ladder: sell and buy quantities per row",
                    ),
                ] {
                    if ui
                        .selectable_label(config.style == style, label)
                        .on_hover_text(hover)
                        .clicked()
                        && config.style != style
                    {
                        config.style = style;
                        changed = true;
                    }
                }
            });

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

            ui.small("Changes apply immediately and persist across restarts.");
        });
    *open = window_open;
    changed
}
