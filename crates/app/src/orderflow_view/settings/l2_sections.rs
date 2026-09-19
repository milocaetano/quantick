//! The L2 dock tab's sections, one view component each.
//!
//! Every section borrows exactly the settings it edits and draws them in one
//! `show`; the tab composes them in order and commits whatever changed once.

use eframe::egui;
use quantick_orderbook::BookLevel;
use quantick_orderflow::engine::BookPublished;
use quantick_orderflow::{DisplayGrouping, HeatmapConfig, HeatmapTheme, IntensityMode};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

/// Colour theme, display grouping, brightness and quiet-liquidity gamma.
pub(super) struct LiquidityRangesSection<'a> {
    pub(super) config: &'a mut HeatmapConfig,
}

impl LiquidityRangesSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let config = self.config;
        ui.horizontal(|ui| {
            ui.strong("liquidity ranges");
            ui.add_space(8.0);
            egui::ComboBox::from_id_salt("heatmap_theme")
                .selected_text(theme_label(config.theme))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut config.theme, HeatmapTheme::Bookmap, "Bookmap");
                    ui.selectable_value(
                        &mut config.theme,
                        HeatmapTheme::HighContrast,
                        "High contrast",
                    );
                    ui.selectable_value(&mut config.theme, HeatmapTheme::ColorBlind, "Color blind");
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
        ui.strong("visual layers");
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
        ui.strong("liquidity response");
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
        ui.strong("scale & history");
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
        let published = self.published;
        let health = &published.health;
        ui.label(format!(
            "{} · {} bid / {} ask levels",
            published.status.label(),
            health.bid_levels,
            health.ask_levels
        ));
        if let Some(ladder) = &published.ladder {
            let side = |level: Option<BookLevel>| match level {
                Some(level) => format!("{} × {}", level.price(), level.quantity()),
                None => "—".to_owned(),
            };
            let spread = match (ladder.best_bid, ladder.best_ask) {
                (Some(bid), Some(ask)) => (ask.price() - bid.price()).to_string(),
                _ => "—".to_owned(),
            };
            ui.label(format!(
                "book now: bid {} · ask {} · spread {}",
                side(ladder.best_bid),
                side(ladder.best_ask),
                spread
            ))
            .on_hover_text(
                "Best resting bid and ask of the live book, read from the published ladder.",
            );
            ui.small(format!(
                "ladder holds {} bids / {} asks in view",
                ladder.bids.len(),
                ladder.asks.len()
            ));
        }
        ui.label(format!(
            "{} runs · {:.1} MiB retained · projection {:.1} ms",
            health.archived_runs + health.active_levels,
            health.history_bytes as f64 / (1024.0 * 1024.0),
            health.projection_ms
        ));
        ui.label(format!(
            "effective range {} · {}× base",
            health.effective_grouping, health.effective_grouping_multiple
        ));
    }
}

fn theme_label(theme: HeatmapTheme) -> &'static str {
    match theme {
        HeatmapTheme::Bookmap => "Bookmap",
        HeatmapTheme::HighContrast => "High contrast",
        HeatmapTheme::ColorBlind => "Color blind",
    }
}

fn display_grouping_label(grouping: DisplayGrouping) -> String {
    match grouping {
        DisplayGrouping::Native => "Native · 1×".to_owned(),
        DisplayGrouping::Multiple(multiple) => format!("Range · {multiple}×"),
        DisplayGrouping::Adaptive { target_rows } => format!("Auto · {target_rows} rows"),
    }
}
