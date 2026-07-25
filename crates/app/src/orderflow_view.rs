//! egui facade for the asynchronous order-flow heatmap.
//!
//! All book state (history, synchronization, projection) lives in
//! [`crate::orderflow_engine::BookEngine`] on the worker thread owned by
//! [`crate::orderflow_worker::BookWorker`]. This layer only forwards commands,
//! mirrors the published snapshot for the current frame and converts
//! normalized primitives into egui shapes. Nothing here can block the UI on a
//! dense book: drawing always uses the latest already-built frame.

use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use quantick_engine::{Bar, Trade};
use quantick_feed_binance::depth::DepthEvent;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::orderflow::{
    DisplayGrouping, HeatmapConfig, HeatmapTheme, IntensityMode, MAX_BUBBLE_MAX_RADIUS,
    MIN_BUBBLE_MAX_RADIUS,
};
use crate::orderflow_engine::{
    BookPublished, CaptureStatus, OrderflowHealth, PROJECTION_INTERVAL, ProjectionLayout,
    ProjectionRequest, VisibleOrderflow,
};
use crate::orderflow_render::{
    OrderflowRenderStyle, ProjectedLayout, RenderContext, draw_aggression_bubbles,
    draw_compact_legend, draw_heatmap_background, draw_liquidity_events, draw_preview,
};
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::viewport::Viewport;

fn status_color(status: &CaptureStatus) -> egui::Color32 {
    match status {
        CaptureStatus::Live { .. } => egui::Color32::from_rgb(45, 205, 145),
        CaptureStatus::Connecting | CaptureStatus::Buffering | CaptureStatus::SnapshotFetching => {
            egui::Color32::from_rgb(240, 185, 11)
        }
        CaptureStatus::Disabled => egui::Color32::from_rgb(145, 155, 170),
        CaptureStatus::Resyncing { .. }
        | CaptureStatus::Disconnected { .. }
        | CaptureStatus::Error => egui::Color32::from_rgb(255, 99, 71),
    }
}

/// Stateful UI/controller facade for the optional heatmap.
pub struct OrderflowView {
    symbol: String,
    /// UI mirror of the engine configuration. The engine owns
    /// `price_grouping` (auto-base can rewrite it); the mirror adopts engine
    /// changes through [`Self::sync_published`].
    config: HeatmapConfig,
    worker: BookWorker,
    published: BookPublished,
    /// Engine bucket last adopted into the mirror, to detect auto-base moves.
    last_seen_base: Decimal,
    /// Whether the docked L2 (liquidity map) settings panel is open.
    show_l2_panel: bool,
    /// Whether the docked aggression-bubble settings panel is open.
    show_bubbles_panel: bool,
    capture_grouping_draft: f64,
    pending_capture_grouping_previous: Option<Decimal>,
    last_requested_layout: Option<ProjectionLayout>,
    last_request_at: Option<Instant>,
}

impl OrderflowView {
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        let config = HeatmapConfig::default();
        let base_grouping = config.price_grouping;
        Self {
            worker: BookWorker::spawn(&symbol),
            symbol,
            config,
            published: BookPublished::initial(),
            last_seen_base: base_grouping,
            show_l2_panel: false,
            show_bubbles_panel: false,
            capture_grouping_draft: base_grouping.to_f64().unwrap_or(0.01),
            pending_capture_grouping_previous: None,
            last_requested_layout: None,
            last_request_at: None,
        }
    }

    /// Pull the newest worker snapshot into this frame's mirror. Cheap: one
    /// mutex lock and a small clone (frames are shared through `Arc`).
    fn sync_published(&mut self) {
        self.published = self.worker.published();
        let base = self.published.base_price_grouping;
        if base != self.last_seen_base {
            self.last_seen_base = base;
            // The engine auto-sized the capture bucket from live data; adopt
            // it unless the user has a competing change staged.
            if self.pending_capture_grouping_previous.is_none() {
                self.config.price_grouping = base;
                self.capture_grouping_draft = base.to_f64().unwrap_or(self.capture_grouping_draft);
            }
        }
    }

    /// Whether L2 depth capture is on. Says nothing about the bubble layer.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Whether the aggression layer is on. Independent of the depth map: it
    /// reads the trade stream the chart already consumes.
    #[must_use]
    pub fn bubbles_enabled(&self) -> bool {
        self.config.show_aggressions
    }

    /// Toggle the aggression layer without touching L2 capture. No feed
    /// command is needed — aggregate trades already flow for the candles.
    pub fn set_bubbles_enabled(&mut self, enabled: bool) {
        if self.config.show_aggressions == enabled {
            return;
        }
        let before = self.config.clone();
        self.config.show_aggressions = enabled;
        self.commit_config_changes(before);
    }

    /// Latest exchange timestamp for which live book state is known, while
    /// capture is active. Drives how wide the forming bar's live tail grows.
    #[must_use]
    pub fn live_end_ms(&mut self) -> Option<i64> {
        if !self.config.enabled {
            return None;
        }
        self.sync_published();
        self.published.live_end_ms
    }

    pub fn toggle_l2_panel(&mut self) {
        self.show_l2_panel = !self.show_l2_panel;
    }

    pub fn toggle_bubbles_panel(&mut self) {
        self.show_bubbles_panel = !self.show_bubbles_panel;
    }

    #[must_use]
    pub fn l2_button_label(&self) -> String {
        match self.config.display_grouping {
            DisplayGrouping::Adaptive { .. } => "⚙ L2 · auto".to_owned(),
            DisplayGrouping::Native => "⚙ L2 · 1×".to_owned(),
            DisplayGrouping::Multiple(multiple) => format!("⚙ L2 · {multiple}×"),
        }
    }

    #[must_use]
    pub fn bubbles_button_label(&self) -> String {
        format!(
            "⚙ bubbles · {}",
            cluster_label(self.config.bubble_cluster_ms)
        )
    }

    #[cfg(test)]
    pub(crate) fn stage_capture_grouping_for_test(&mut self, grouping: Decimal) -> bool {
        let before = self.config.clone();
        self.config.price_grouping = grouping;
        self.capture_grouping_draft = grouping.to_f64().unwrap_or(self.capture_grouping_draft);
        self.commit_config_changes(before)
    }

    #[cfg(test)]
    pub(crate) fn base_capture_grouping_for_test(&mut self) -> Decimal {
        self.flush_for_test();
        self.published.base_price_grouping
    }

    /// Wait until the worker has applied and published everything sent so
    /// far, then adopt the result. Makes the async pipeline deterministic in
    /// tests; production code never blocks on the worker.
    #[cfg(test)]
    pub(crate) fn flush_for_test(&mut self) {
        self.worker.flush();
        self.sync_published();
    }

    /// Reset market-specific history while preserving visual/retention
    /// settings. Capture deliberately returns to off; the app starts a fresh
    /// provider task only after the new feed handle is installed.
    pub fn reset_for_symbol(&mut self, symbol: impl Into<String>) {
        self.symbol = symbol.into();
        self.config.enabled = false;
        self.pending_capture_grouping_previous = None;
        self.last_requested_layout = None;
        self.last_request_at = None;
        self.published = BookPublished::initial();
        self.worker
            .send(BookCommand::ResetForSymbol(self.symbol.clone()));
    }

    /// Commit a capture toggle only after its feed command was accepted.
    pub fn set_enabled(&mut self, enabled: bool, generation_floor: u64) {
        if self.config.enabled == enabled {
            return;
        }
        self.config.enabled = enabled;
        if !enabled {
            // Drop the local frame immediately; the worker clears its own.
            self.published.frame = None;
        }
        self.worker.send(BookCommand::SetEnabled {
            enabled,
            generation_floor,
        });
    }

    /// Mark the existing generation discontinuous before the feed is restarted.
    pub fn prepare_restart(&mut self, generation_floor: u64, reason: &'static str) {
        self.worker.send(BookCommand::PrepareRestart {
            generation_floor,
            reason,
        });
    }

    /// Commit a staged base-grouping change only after the feed accepted its
    /// restart command. Until this point the old history and capture remain
    /// fully usable.
    pub fn accept_capture_grouping_restart(&mut self, generation_floor: u64) {
        match self.pending_capture_grouping_previous.take() {
            Some(_previous) => {
                self.last_seen_base = self.config.price_grouping;
                self.worker.send(BookCommand::AcceptGroupingRestart {
                    grouping: self.config.price_grouping,
                    generation_floor,
                });
            }
            None => self.prepare_restart(generation_floor, "configuration_restart"),
        }
    }

    /// Roll back a staged base-grouping change when the feed command could not
    /// be queued. The engine never saw the change, so only the mirror moves.
    pub fn reject_capture_grouping_restart(&mut self, reason: &'static str) {
        let Some(previous) = self.pending_capture_grouping_previous.take() else {
            return;
        };
        let requested = self.config.price_grouping;
        self.config.price_grouping = previous;
        self.capture_grouping_draft = previous.to_f64().unwrap_or(self.capture_grouping_draft);
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_GROUPING_ROLLED_BACK",
            symbol = self.symbol.as_str(),
            previous_grouping = %previous,
            requested_grouping = %requested,
            reason,
            action = "keep_existing_capture_and_history",
            "base grouping change was rolled back because capture restart was not queued"
        );
    }

    /// Record a factual aggregate trade for the aggression overlay.
    pub fn record_trade(&mut self, trade: &Trade) {
        if self.config.any_layer_enabled() {
            self.worker.send(BookCommand::Trade(trade.clone()));
        }
    }

    /// Forward one feed event to the book thread. Generation and symbol
    /// filtering happen engine-side.
    pub fn handle_depth_event(&mut self, event: DepthEvent) {
        self.worker.send(BookCommand::Depth(event));
    }

    /// Request projection of the visible bar slice and return the newest
    /// already-built frame. Never blocks: a heavy projection only delays the
    /// next frame swap, not the UI.
    pub fn project_visible(
        &mut self,
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        extend_live_end: bool,
        price_range: (f64, f64),
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        let request = ProjectionRequest {
            first_bar_index,
            closed: closed.to_vec(),
            partial: partial.cloned(),
            extend_live_end,
            price_range,
        };
        let layout = request.layout();
        let due = self
            .last_request_at
            .is_none_or(|at| at.elapsed() >= PROJECTION_INTERVAL);
        if self.last_requested_layout != Some(layout) || due {
            self.worker.send(BookCommand::Project(request));
            self.last_requested_layout = Some(layout);
            self.last_request_at = Some(Instant::now());
        }
        self.published.frame.clone()
    }

    /// Draw resting liquidity, coverage gaps and factual liquidity changes
    /// behind the candle layer.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_background(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
        canvas_background: egui::Color32,
        live_span: f32,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            live_span,
        );
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_heatmap_background(painter, &context);
        draw_liquidity_events(painter, &context);
    }

    /// Draw factual aggressive prints and the compact visual key over candles.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_aggressions(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
        canvas_background: egui::Color32,
        live_span: f32,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            live_span,
        );
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_aggression_bubbles(painter, &context);
        draw_compact_legend(painter, &context);
    }

    pub fn draw_status_badge(&self, painter: &egui::Painter, chart_rect: egui::Rect) {
        if !self.config.enabled {
            return;
        }
        let text = self.published.status.label();
        let color = status_color(&self.published.status);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), color);
        let pos = egui::pos2(
            chart_rect.right() - galley.size().x - 10.0,
            chart_rect.top() + 4.0,
        );
        let rect = egui::Rect::from_min_size(
            pos - egui::vec2(5.0, 3.0),
            galley.size() + egui::vec2(10.0, 6.0),
        );
        painter.rect_filled(
            rect,
            egui::Rounding::same(3.0),
            egui::Color32::from_black_alpha(165),
        );
        painter.galley(pos, galley, color);
    }

    pub fn health(&mut self) -> OrderflowHealth {
        self.sync_published();
        self.published.health.clone()
    }

    pub fn reset_summary_counters(&mut self) {
        self.worker.send(BookCommand::ResetSummaryCounters);
    }

    /// Draw the docked order-flow panels and return whether capture must
    /// restart because the base capture resolution changed.
    ///
    /// The L2 map and the aggression bubbles get one panel each, so neither
    /// configuration hides behind the other's switch.
    pub fn draw_panels(&mut self, ctx: &egui::Context) -> bool {
        if !self.show_l2_panel && !self.show_bubbles_panel {
            return false;
        }
        self.sync_published();
        let before = self.config.clone();
        self.draw_l2_panel(ctx);
        self.draw_bubbles_panel(ctx);
        self.commit_config_changes(before)
    }

    /// Left dock for everything the depth map owns.
    fn draw_l2_panel(&mut self, ctx: &egui::Context) {
        if !self.show_l2_panel {
            return;
        }
        let mut open = true;
        egui::SidePanel::left("orderflow_l2_panel")
            .resizable(true)
            .default_width(340.0)
            .width_range(300.0..=620.0)
            .show(ctx, |ui| {
                panel_header(ui, "L2 · LIQUIDITY MAP", &mut open);
                egui::ScrollArea::vertical()
                    .id_salt("orderflow_l2_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(self.published.status.label())
                        .small()
                        .color(status_color(&self.published.status)),
                );
                ui.small(
                    "Brightness is resting liquidity. Green/red bubbles are confirmed trades.",
                );
                ui.small(
                    "A bite means a compatible L2 reduction; a violet tail is an unattributed withdrawal.",
                );
                draw_preview(ui, &self.config).on_hover_text(
                    "Deterministic preview: persistent wall, aligned depletion, full withdrawal and clustered trades.",
                );
                ui.separator();

                ui.horizontal(|ui| {
                    ui.strong("liquidity ranges");
                    ui.add_space(8.0);
                    egui::ComboBox::from_id_salt("heatmap_theme")
                        .selected_text(theme_label(self.config.theme))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.theme,
                                HeatmapTheme::Bookmap,
                                "Bookmap",
                            );
                            ui.selectable_value(
                                &mut self.config.theme,
                                HeatmapTheme::HighContrast,
                                "High contrast",
                            );
                            ui.selectable_value(
                                &mut self.config.theme,
                                HeatmapTheme::ColorBlind,
                                "Color blind",
                            );
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("display range");
                    egui::ComboBox::from_id_salt("heatmap_display_grouping")
                        .selected_text(display_grouping_label(self.config.display_grouping))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.display_grouping,
                                DisplayGrouping::Adaptive { target_rows: 160 },
                                "Auto · follows zoom",
                            );
                            ui.selectable_value(
                                &mut self.config.display_grouping,
                                DisplayGrouping::Native,
                                "Native · 1×",
                            );
                            for multiple in [2, 5, 10, 25, 50] {
                                ui.selectable_value(
                                    &mut self.config.display_grouping,
                                    DisplayGrouping::Multiple(multiple),
                                    format!("Range · {multiple}×"),
                                );
                            }
                            ui.selectable_value(
                                &mut self.config.display_grouping,
                                DisplayGrouping::Multiple(3),
                                "Custom…",
                            );
                        });
                });
                match &mut self.config.display_grouping {
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
                    DisplayGrouping::Multiple(multiple)
                        if ![2_u32, 5, 10, 25, 50].contains(multiple) =>
                    {
                        ui.add(
                            egui::DragValue::new(multiple)
                                .range(1..=1_000_000)
                                .prefix("custom multiple "),
                        );
                    }
                    DisplayGrouping::Native | DisplayGrouping::Multiple(_) => {}
                }
                ui.small(
                    "Display grouping is instant and non-destructive; it never restarts L2 capture.",
                );
                ui.add(egui::Slider::new(&mut self.config.opacity, 0.05..=1.0).text("brightness"));
                ui.add(
                    egui::Slider::new(&mut self.config.gamma, 0.25..=2.0)
                        .text("quiet liquidity"),
                );

                ui.separator();
                ui.strong("liquidity response");
                ui.checkbox(
                    &mut self.config.show_liquidity_events,
                    "show bites and withdrawal tails",
                );
                ui.add_enabled(
                    self.config.show_liquidity_events,
                    egui::Slider::new(&mut self.config.liquidity_correlation_ms, 25..=1_000)
                        .text("matching window ms")
                        .logarithmic(true),
                )
                .on_hover_text(
                    "time/price window used to associate a factual trade with a factual L2 reduction",
                );
                ui.add_enabled(
                    self.config.show_liquidity_events,
                    egui::Slider::new(&mut self.config.min_unattributed_reduction, 0.0..=1.0)
                        .text("min unattributed pull"),
                )
                .on_hover_text(
                    "hide unattributed (depth-only) reductions smaller than this fraction of the level; aggression-aligned bites always show",
                );
                ui.add_enabled(
                    self.config.show_liquidity_events,
                    egui::Slider::new(&mut self.config.min_unattributed_pull_share, 0.0..=1.0)
                        .text("min pull vs walls"),
                )
                .on_hover_text(
                    "hide unattributed pulls smaller than this share of the visible full-intensity liquidity (P99); a deep pull of a tiny level is noise, of a wall it is the story",
                );
                ui.small(
                    "Association is evidence, not causality: depth updates can also contain pulls or replacements.",
                );

                ui.separator();
                ui.strong("scale & history");
                let mut retention_minutes = self.config.retention_ms as f64 / 60_000.0;
                ui.add(
                    egui::Slider::new(&mut retention_minutes, 1.0..=1_440.0)
                        .logarithmic(true)
                        .text("retention min"),
                );
                self.config.retention_ms = (retention_minutes * 60_000.0) as i64;
                let mut automatic = matches!(self.config.intensity_mode, IntensityMode::VisibleP99);
                ui.checkbox(&mut automatic, "auto intensity (visible P99)");
                if automatic {
                    self.config.intensity_mode = IntensityMode::VisibleP99;
                } else {
                    let mut maximum = match self.config.intensity_mode {
                        IntensityMode::Fixed(value) => value.to_f64().unwrap_or(1.0),
                        IntensityMode::VisibleP99 => 1.0,
                    };
                    ui.add(
                        egui::DragValue::new(&mut maximum)
                            .range(0.000_000_01..=1_000_000_000.0)
                            .speed(1.0)
                            .prefix("full qty "),
                    );
                    self.config.intensity_mode = IntensityMode::Fixed(
                        Decimal::from_f64(maximum.max(0.000_000_01)).unwrap_or(Decimal::ONE),
                    );
                }
                ui.checkbox(&mut self.config.show_legend, "show chart legend");

                ui.collapsing("advanced · capture resolution", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("base price bucket");
                        ui.add(
                            egui::DragValue::new(&mut self.capture_grouping_draft)
                                .range(0.000_000_01..=1_000_000.0)
                                .speed(0.01),
                        );
                    });
                    let candidate =
                        Decimal::from_f64(self.capture_grouping_draft.max(0.000_000_01))
                            .unwrap_or(Decimal::new(1, 2));
                    if ui
                        .add_enabled(
                            candidate != self.config.price_grouping,
                            egui::Button::new("apply base resolution & resync"),
                        )
                        .clicked()
                    {
                        self.config.price_grouping = candidate;
                    }
                    ui.small(
                        "Changing the capture bucket requires a fresh snapshot and clears retained L2 history.",
                    );
                });

                ui.separator();
                let health = &self.published.health;
                ui.label(format!(
                    "{} · {} bid / {} ask levels",
                    self.published.status.label(),
                    health.bid_levels,
                    health.ask_levels
                ));
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
                if ui.button("reset L2 visuals").clicked() {
                    let price_grouping = self.config.price_grouping;
                    self.capture_grouping_draft =
                        price_grouping.to_f64().unwrap_or(self.capture_grouping_draft);
                    self.config = HeatmapConfig {
                        enabled: self.config.enabled,
                        price_grouping,
                        // The bubble layer owns its own panel and its own reset.
                        show_aggressions: self.config.show_aggressions,
                        bubble_cluster_ms: self.config.bubble_cluster_ms,
                        bubble_opacity: self.config.bubble_opacity,
                        bubble_max_radius: self.config.bubble_max_radius,
                        ..HeatmapConfig::default()
                    };
                }
                    });
            });
        self.show_l2_panel = open;
    }

    /// Left dock for the aggression layer. Everything here is independent of
    /// L2 capture: bubbles are built from the aggregate-trade stream.
    fn draw_bubbles_panel(&mut self, ctx: &egui::Context) {
        if !self.show_bubbles_panel {
            return;
        }
        let mut open = true;
        egui::SidePanel::left("orderflow_bubbles_panel")
            .resizable(true)
            .default_width(300.0)
            .width_range(260.0..=520.0)
            .show(ctx, |ui| {
                panel_header(ui, "AGGRESSION BUBBLES", &mut open);
                egui::ScrollArea::vertical()
                    .id_salt("orderflow_bubbles_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.small(
                            "Confirmed executions from the trade stream. Colour is the aggressor side; area is quantity.",
                        );
                        ui.small(
                            "This layer is independent: it keeps drawing with L2 capture off.",
                        );
                        ui.separator();

                        ui.checkbox(&mut self.config.show_aggressions, "show aggression bubbles")
                            .on_hover_text(
                                "records and projects confirmed trades; does not start or stop L2 depth capture",
                            );
                        ui.add_enabled_ui(self.config.show_aggressions, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("cluster");
                                egui::ComboBox::from_id_salt("heatmap_bubble_cluster")
                                    .selected_text(cluster_label(self.config.bubble_cluster_ms))
                                    .show_ui(ui, |ui| {
                                        for (milliseconds, label) in [
                                            (0, "Raw"),
                                            (50, "50 ms"),
                                            (100, "100 ms"),
                                            (200, "200 ms"),
                                            (500, "500 ms"),
                                        ] {
                                            ui.selectable_value(
                                                &mut self.config.bubble_cluster_ms,
                                                milliseconds,
                                                label,
                                            );
                                        }
                                    });
                            })
                            .response
                            .on_hover_text(
                                "merge compatible prints inside this window into one bubble; Raw keeps one bubble per trade",
                            );
                            ui.add(
                                egui::Slider::new(&mut self.config.bubble_opacity, 0.05..=1.0)
                                    .text("bubble opacity"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut self.config.bubble_max_radius,
                                    MIN_BUBBLE_MAX_RADIUS..=MAX_BUBBLE_MAX_RADIUS,
                                )
                                .text("largest bubble px"),
                            )
                            .on_hover_text(
                                "area stays quantity-proportional; this only sets how big the biggest print draws",
                            );
                        });

                        ui.separator();
                        let health = &self.published.health;
                        ui.label(format!(
                            "{} bubbles projected · {} aggressions retained",
                            health.projection_aggressions, health.aggression_count
                        ));
                        if health.dropped_aggressions > 0 {
                            ui.small(format!(
                                "{} bubbles above the primitive cap were not drawn",
                                health.dropped_aggressions
                            ));
                        }
                        if ui.button("reset bubble visuals").clicked() {
                            let defaults = HeatmapConfig::default();
                            self.config.bubble_cluster_ms = defaults.bubble_cluster_ms;
                            self.config.bubble_opacity = defaults.bubble_opacity;
                            self.config.bubble_max_radius = defaults.bubble_max_radius;
                        }
                    });
            });
        self.show_bubbles_panel = open;
    }

    fn commit_config_changes(&mut self, before: HeatmapConfig) -> bool {
        self.config.sanitize();
        if self.config == before {
            return false;
        }
        let capture_grouping_changed = self.config.price_grouping != before.price_grouping;
        let restart_required = capture_grouping_changed && self.config.enabled;
        if restart_required {
            // Stage the bucket: visual changes apply now, the destructive
            // grouping reset only after the feed accepts the restart command.
            self.pending_capture_grouping_previous = Some(before.price_grouping);
            self.worker
                .send(BookCommand::ApplyVisualConfig(self.config.clone()));
        } else if capture_grouping_changed {
            self.last_seen_base = self.config.price_grouping;
            self.worker
                .send(BookCommand::ApplyVisualConfig(self.config.clone()));
            self.worker
                .send(BookCommand::ApplyGroupingNow(self.config.price_grouping));
        } else {
            self.worker
                .send(BookCommand::ApplyVisualConfig(self.config.clone()));
        }
        restart_required
    }
}

/// Title row shared by the docked panels, with the close affordance a floating
/// window used to provide.
fn panel_header(ui: &mut egui::Ui, title: &str, open: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .strong()
                .color(egui::Color32::from_rgb(255, 222, 92)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // `×` is Latin-1, so it renders in egui's default proportional
            // font — the fancier ✕/✖ glyphs fall back to an empty box.
            if ui
                .small_button("×")
                .on_hover_text("close this panel")
                .clicked()
            {
                *open = false;
            }
        });
    });
    ui.separator();
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

fn cluster_label(milliseconds: i64) -> String {
    if milliseconds == 0 {
        "Raw".to_owned()
    } else {
        format!("{milliseconds} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

    fn snapshot_event(generation: u64) -> DepthEvent {
        DepthEvent::Snapshot {
            symbol: "BTCUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1000,
                },
            ),
        }
    }

    fn bar(open_time: i64, close_time: i64) -> Bar {
        Bar {
            open_time,
            close_time,
            open: Decimal::from(100),
            high: Decimal::from(102),
            low: Decimal::from(98),
            close: Decimal::from(101),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    #[test]
    fn worker_round_trip_publishes_book_state_and_frame() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        // Advance book time so the open runs have visible width.
        view.handle_depth_event(DepthEvent::Update {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            event_time_ms: 1_050,
            delta: BookDelta::new(
                11,
                11,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(7)).unwrap()],
                Vec::new(),
            ),
        });
        view.flush_for_test();
        assert_eq!(view.health().status, "connecting");
        assert_eq!(view.health().active_levels, 2);

        let bars = [bar(900, 1_100)];
        // First call queues the projection; the frame appears after a flush.
        let first = view.project_visible(0, &bars, None, true, (98.0, 102.0));
        assert!(first.is_none());
        view.flush_for_test();
        let frame = view
            .project_visible(0, &bars, None, true, (98.0, 102.0))
            .expect("published frame");
        assert!(frame.projection.enabled);
        assert!(!frame.projection.cells.is_empty());
    }

    #[test]
    fn bubbles_project_while_book_capture_stays_off() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_bubbles_enabled(true);
        assert!(!view.enabled(), "the bubble layer must not start capture");

        view.record_trade(&Trade {
            agg_id: 1,
            timestamp_ms: 1_000,
            price: Decimal::new(1_005, 1),
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        });
        view.flush_for_test();
        assert_eq!(view.health().aggression_count, 1);

        let bars = [bar(900, 1_100)];
        view.project_visible(0, &bars, None, true, (98.0, 102.0));
        view.flush_for_test();
        let frame = view
            .project_visible(0, &bars, None, true, (98.0, 102.0))
            .expect("published frame");
        assert_eq!(frame.projection.aggressions.len(), 1);
        assert!(frame.projection.cells.is_empty(), "no map without capture");

        // Turning the bubbles off closes the pipeline again.
        view.set_bubbles_enabled(false);
        assert!(
            view.project_visible(0, &bars, None, true, (98.0, 102.0))
                .is_none()
        );
    }

    #[test]
    fn disabling_capture_drops_the_published_frame() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];
        view.project_visible(0, &bars, None, true, (98.0, 102.0));
        view.flush_for_test();
        assert!(
            view.project_visible(0, &bars, None, true, (98.0, 102.0))
                .is_some()
        );

        view.set_enabled(false, 11);
        assert!(
            view.project_visible(0, &bars, None, true, (98.0, 102.0))
                .is_none()
        );
        view.flush_for_test();
        assert!(view.published.frame.is_none());
    }

    #[test]
    fn auto_base_from_live_data_is_adopted_by_the_ui_mirror() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 1);
        view.handle_depth_event(DepthEvent::Snapshot {
            symbol: "BTCUSDT".to_owned(),
            generation: 1,
            observed_at_ms: 1_100,
            effective_at_ms: 1_000,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(64_999), Decimal::from(2)).unwrap()],
                vec![BookLevel::new(Decimal::from(65_001), Decimal::from(3)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
        view.flush_for_test();
        assert_eq!(view.config.price_grouping, Decimal::from(1));
        assert_eq!(view.capture_grouping_draft, 1.0);
    }

    #[test]
    fn staged_grouping_change_survives_until_accept_and_rolls_back_on_reject() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        view.flush_for_test();
        let original = view.published.base_price_grouping;

        let staged = Decimal::new(5, 1);
        assert!(view.stage_capture_grouping_for_test(staged));
        // Engine untouched while staged.
        assert_eq!(view.base_capture_grouping_for_test(), original);
        assert_eq!(view.health().active_levels, 2);

        view.reject_capture_grouping_restart("command_channel_full");
        assert_eq!(view.config.price_grouping, original);
        assert_eq!(view.base_capture_grouping_for_test(), original);

        // Stage again, accept: the engine resets to the new bucket.
        assert!(view.stage_capture_grouping_for_test(staged));
        view.accept_capture_grouping_restart(20);
        assert_eq!(view.base_capture_grouping_for_test(), staged);
        assert_eq!(view.health().active_levels, 0);
        assert_eq!(view.health().status, "connecting");
    }
}
