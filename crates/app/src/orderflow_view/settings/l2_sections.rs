//! The L2 dock tab's sections, one view component each.
//!
//! Every section borrows exactly the settings it edits and draws them in one
//! `show`; the tab composes them in order and commits whatever changed once.

use eframe::egui;
use quantick_orderflow::config::theme::PaletteChoice;
use quantick_orderflow::engine::BookPublished;
use quantick_orderflow::health_words::{BOOK_NOW_HOVER, BookHealthWords};
use quantick_orderflow::{DisplayGrouping, HeatmapConfig, IntensityMode};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::panel_chrome;

/// Colour theme, display grouping, brightness and quiet-liquidity gamma.
pub(super) struct LiquidityRangesSection<'a> {
    pub(super) config: &'a mut HeatmapConfig,
}

impl LiquidityRangesSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let config = self.config;
        panel_chrome::caption(ui, "liquidity ranges");
        ui.horizontal(|ui| {
            ui.label("theme");
            egui::ComboBox::from_id_salt("heatmap_theme")
                .selected_text(config.palette.label())
                .show_ui(ui, |ui| {
                    for choice in PaletteChoice::SELECTABLE {
                        ui.selectable_value(&mut config.palette, choice, choice.label());
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("display range");
            egui::ComboBox::from_id_salt("heatmap_display_grouping")
                .selected_text(display_grouping_label(config.display_grouping))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut config.display_grouping,
                        DisplayGrouping::Adaptive { target_rows: 160 },
                        "Auto · follows zoom",
                    );
                    ui.selectable_value(
                        &mut config.display_grouping,
                        DisplayGrouping::Native,
                        "Native · 1×",
                    );
                    for multiple in [2, 5, 10, 25, 50] {
                        ui.selectable_value(
                            &mut config.display_grouping,
                            DisplayGrouping::Multiple(multiple),
                            format!("Range · {multiple}×"),
                        );
                    }
                    ui.selectable_value(
                        &mut config.display_grouping,
                        DisplayGrouping::Multiple(3),
                        "Custom…",
                    );
                });
        });
        match &mut config.display_grouping {
            DisplayGrouping::Adaptive { target_rows } => {
                ui.add(
                    egui::Slider::new(target_rows, 40..=400)
                        .text("target screen rows")
                        .logarithmic(true),
                )
                .on_hover_text(
                    "automatically widens price ranges as you zoom out; history stays intact",
                );
            }
            DisplayGrouping::Multiple(multiple) if ![2_u32, 5, 10, 25, 50].contains(multiple) => {
                ui.add(
                    egui::DragValue::new(multiple)
                        .range(1..=1_000_000)
                        .prefix("custom multiple "),
                );
            }
            DisplayGrouping::Native | DisplayGrouping::Multiple(_) => {}
        }
        ui.small("Display grouping is instant and non-destructive; it never restarts L2 capture.");
        ui.add(egui::Slider::new(&mut config.opacity, 0.05..=1.0).text("brightness"));
        ui.add(egui::Slider::new(&mut config.gamma, 0.25..=2.0).text("quiet liquidity"));
    }
}

/// One switch per legend entry; display-only.
pub(super) struct VisualLayersSection<'a> {
    pub(super) config: &'a mut HeatmapConfig,
}

impl VisualLayersSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let config = self.config;
        panel_chrome::caption(ui, "visual layers");
        ui.small(
            "One switch per legend entry. Display-only: capture keeps running and \
             history keeps accumulating, so a layer switched back on repaints the \
             past it kept recording.",
        );
        ui.checkbox(&mut config.show_liquidity, "liquidity")
            .on_hover_text("the resting-liquidity heat cells");
        ui.checkbox(&mut config.show_buy_aggressions, "buy aggression")
            .on_hover_text(
                "buy-side bubbles; needs the aggression layer on (toolbar). Hiding one \
                 side never rescales the other",
            );
        ui.checkbox(&mut config.show_sell_aggressions, "sell aggression")
            .on_hover_text(
                "sell-side bubbles; needs the aggression layer on (toolbar). Hiding one \
                 side never rescales the other",
            );
        ui.checkbox(
            &mut config.show_aligned_depletion,
            "aggression-aligned depletion",
        )
        .on_hover_text("depletion markers where a factual trade matches a factual L2 reduction");
        ui.checkbox(
            &mut config.show_unattributed_reductions,
            "L2 reduction (unattributed)",
        )
        .on_hover_text(
            "depth-only reductions and their fading withdrawal tails; with both \
             depletion layers off, bubbles also lose their consumption marks",
        );
        ui.checkbox(&mut config.show_gaps, "L2 gap").on_hover_text(
            "dashed boundaries around intervals with no depth coverage; the stretch \
             older than this session's capture is marked by its boundary alone",
        );
    }
}

/// How a trade is matched to a reduction, and which pulls are noise.
pub(super) struct LiquidityResponseSection<'a> {
    pub(super) config: &'a mut HeatmapConfig,
}

impl LiquidityResponseSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let config = self.config;
        panel_chrome::caption(ui, "liquidity response");
        ui.add_enabled(
            config.liquidity_events_enabled(),
            egui::Slider::new(&mut config.liquidity_correlation_ms, 25..=1_000)
                .text("matching window ms")
                .logarithmic(true),
        )
        .on_hover_text(
            "time/price window used to associate a factual trade with a factual L2 reduction",
        );
        ui.add_enabled(
            config.show_unattributed_reductions,
            egui::Slider::new(&mut config.min_unattributed_reduction, 0.0..=1.0)
                .text("min unattributed pull"),
        )
        .on_hover_text(
            "hide unattributed (depth-only) reductions smaller than this fraction of the level; aggression-aligned bites always show",
        );
        ui.add_enabled(
            config.show_unattributed_reductions,
            egui::Slider::new(&mut config.min_unattributed_pull_share, 0.0..=1.0)
                .text("min pull vs walls"),
        )
        .on_hover_text(
            "hide unattributed pulls smaller than this share of the visible full-intensity liquidity (P99); a deep pull of a tiny level is noise, of a wall it is the story",
        );
        ui.small(
            "Association is evidence, not causality: depth updates can also contain pulls or replacements.",
        );
    }
}

/// Retention, intensity scale, the legend switch and the capture bucket.
///
/// The capture bucket is edited as a draft and applied only by its button,
/// because applying it restarts capture.
pub(super) struct ScaleHistorySection<'a> {
    pub(super) config: &'a mut HeatmapConfig,
    pub(super) capture_grouping_draft: &'a mut f64,
}

impl ScaleHistorySection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let config = self.config;
        panel_chrome::caption(ui, "scale & history");
        let mut retention_minutes = config.retention_ms as f64 / 60_000.0;
        ui.add(
            egui::Slider::new(&mut retention_minutes, 1.0..=1_440.0)
                .logarithmic(true)
                .text("retention min"),
        );
        config.retention_ms = (retention_minutes * 60_000.0) as i64;
        let mut automatic = matches!(config.intensity_mode, IntensityMode::VisibleP99);
        ui.checkbox(&mut automatic, "auto intensity (visible P99)");
        if automatic {
            config.intensity_mode = IntensityMode::VisibleP99;
        } else {
            let mut maximum = match config.intensity_mode {
                IntensityMode::Fixed(value) => value.to_f64().unwrap_or(1.0),
                IntensityMode::VisibleP99 => 1.0,
            };
            ui.add(
                egui::DragValue::new(&mut maximum)
                    .range(0.000_000_01..=1_000_000_000.0)
                    .speed(1.0)
                    .prefix("full qty "),
            );
            config.intensity_mode = IntensityMode::Fixed(
                Decimal::from_f64(maximum.max(0.000_000_01)).unwrap_or(Decimal::ONE),
            );
        }
        ui.checkbox(&mut config.show_legend, "show chart legend");

        let draft = self.capture_grouping_draft;
        ui.collapsing("advanced · capture resolution", |ui| {
            ui.horizontal(|ui| {
                ui.label("base price bucket");
                ui.add(
                    egui::DragValue::new(&mut *draft)
                        .range(0.000_000_01..=1_000_000.0)
                        .speed(0.01),
                );
            });
            let candidate = Decimal::from_f64(draft.max(0.000_000_01))
                .unwrap_or(Decimal::new(1, 2));
            if ui
                .add_enabled(
                    candidate != config.price_grouping,
                    egui::Button::new("apply base resolution & resync"),
                )
                .clicked()
            {
                config.price_grouping = candidate;
            }
            ui.small(
                "Changing the capture bucket requires a fresh snapshot and clears retained L2 history.",
            );
        });
    }
}

/// Read-only: what the live book and the capture look like right now.
pub(super) struct BookHealthSection<'a> {
    pub(super) published: &'a BookPublished,
}

impl BookHealthSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let words = BookHealthWords::of(self.published);
        ui.label(words.levels);
        if let Some(now) = words.book_now {
            ui.label(now).on_hover_text(BOOK_NOW_HOVER);
        }
        if let Some(ladder) = words.ladder {
            ui.small(ladder);
        }
        ui.label(words.retained);
        ui.label(words.range);
    }
}

fn display_grouping_label(grouping: DisplayGrouping) -> String {
    match grouping {
        DisplayGrouping::Native => "Native · 1×".to_owned(),
        DisplayGrouping::Multiple(multiple) => format!("Range · {multiple}×"),
        DisplayGrouping::Adaptive { target_rows } => format!("Auto · {target_rows} rows"),
    }
}
