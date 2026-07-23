//! egui adapter for the pure order-flow history and projection modules.
//!
//! Feed synchronization, deterministic book mutation and renderer-independent
//! geometry remain outside this file. This layer owns only UI configuration,
//! status presentation and conversion of normalized primitives into egui
//! shapes, keeping the existing candle path isolated.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use quantick_engine::{Bar, Side, Trade};
use quantick_feed_binance::depth::{DepthEvent, DepthResyncReason, DepthStatus};
use quantick_orderbook::BookSide;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::orderflow::{
    BarTimeline, HeatmapConfig, HeatmapProjection, HistoryStatus, IntensityMode, LiquidityHistory,
    PriceWindow, project,
};
use crate::viewport::Viewport;

const BID_LOW: [u8; 3] = [14, 40, 82];
const BID_MID: [u8; 3] = [0, 184, 224];
const ASK_LOW: [u8; 3] = [78, 20, 42];
const ASK_MID: [u8; 3] = [255, 84, 42];
const HEAT_HIGH: [u8; 3] = [255, 222, 72];
const BUY_PRINT: egui::Color32 = egui::Color32::from_rgb(55, 226, 176);
const SELL_PRINT: egui::Color32 = egui::Color32::from_rgb(255, 91, 105);
const GAP_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(55, 63, 78, 34);
const GAP_BOUNDARY: egui::Color32 = egui::Color32::from_rgba_premultiplied(135, 145, 160, 90);
const PROJECTION_INTERVAL: Duration = Duration::from_millis(100);

/// One visible, already-projected order-flow frame.
#[derive(Clone)]
pub struct VisibleOrderflow {
    projection: Arc<HeatmapProjection>,
    first_bar_index: usize,
    slot_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectionLayout {
    first_bar_index: usize,
    slot_count: usize,
    extend_live_end: bool,
}

struct ProjectionCache {
    built_at: Instant,
    layout: ProjectionLayout,
    frame: Arc<VisibleOrderflow>,
}

/// Health data consumed by the periodic AI-first application summary.
#[derive(Debug, Clone)]
pub struct OrderflowHealth {
    pub enabled: bool,
    pub status: &'static str,
    pub generation: Option<u64>,
    pub last_update_id: Option<u64>,
    pub last_event_ms: Option<i64>,
    pub bid_levels: usize,
    pub ask_levels: usize,
    pub active_levels: usize,
    pub archived_runs: usize,
    pub aggression_count: usize,
    pub history_bytes: usize,
    pub projection_cells: usize,
    pub projection_aggressions: usize,
    pub dropped_cells: usize,
    pub dropped_aggressions: usize,
    pub projection_ms: f32,
    pub projection_builds: u64,
    pub projection_cache_hits: u64,
    pub config_revision: u64,
    pub last_snapshot_observed_ms: Option<i64>,
    pub depth_updates: u64,
    pub depth_updates_since_summary: u64,
    pub snapshots: u64,
    pub gaps: u64,
}

#[derive(Debug, Clone)]
enum CaptureStatus {
    Disabled,
    Connecting,
    Buffering,
    SnapshotFetching,
    Live {
        generation: u64,
        last_update_id: u64,
    },
    Resyncing {
        reason: &'static str,
    },
    Disconnected {
        error_class: &'static str,
    },
    Error,
}

impl CaptureStatus {
    fn label(&self) -> String {
        match self {
            Self::Disabled => "book off".to_owned(),
            Self::Connecting => "book connecting".to_owned(),
            Self::Buffering => "book buffering".to_owned(),
            Self::SnapshotFetching => "book syncing".to_owned(),
            Self::Live {
                generation,
                last_update_id,
            } => format!("book live · gen {generation} · #{last_update_id}"),
            Self::Resyncing { reason } => format!("book resync · {reason}"),
            Self::Disconnected { error_class } => format!("book down · {error_class}"),
            Self::Error => "book error".to_owned(),
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Connecting => "connecting",
            Self::Buffering => "buffering",
            Self::SnapshotFetching => "snapshot_fetching",
            Self::Live { .. } => "live",
            Self::Resyncing { .. } => "resyncing",
            Self::Disconnected { .. } => "disconnected",
            Self::Error => "error",
        }
    }

    fn color(&self) -> egui::Color32 {
        match self {
            Self::Live { .. } => egui::Color32::from_rgb(45, 205, 145),
            Self::Connecting | Self::Buffering | Self::SnapshotFetching => {
                egui::Color32::from_rgb(240, 185, 11)
            }
            Self::Disabled => egui::Color32::from_rgb(145, 155, 170),
            Self::Resyncing { .. } | Self::Disconnected { .. } | Self::Error => {
                egui::Color32::from_rgb(255, 99, 71)
            }
        }
    }
}

/// Stateful UI/controller facade for the optional heatmap.
pub struct OrderflowView {
    symbol: String,
    config: HeatmapConfig,
    history: LiquidityHistory,
    show_settings: bool,
    status: CaptureStatus,
    generation_floor: u64,
    latest_generation: Option<u64>,
    last_snapshot_observed_ms: Option<i64>,
    depth_updates: u64,
    depth_updates_since_summary: u64,
    config_revision: u64,
    last_projection_cells: usize,
    last_projection_aggressions: usize,
    last_dropped_cells: usize,
    last_dropped_aggressions: usize,
    last_projection_ms: f32,
    projection_builds: u64,
    projection_cache_hits: u64,
    projection_cache: Option<ProjectionCache>,
}

impl OrderflowView {
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        let config = HeatmapConfig::default();
        Self {
            symbol: symbol.into(),
            history: LiquidityHistory::new(config.clone()),
            config,
            show_settings: false,
            status: CaptureStatus::Disabled,
            generation_floor: 0,
            latest_generation: None,
            last_snapshot_observed_ms: None,
            depth_updates: 0,
            depth_updates_since_summary: 0,
            config_revision: 0,
            last_projection_cells: 0,
            last_projection_aggressions: 0,
            last_dropped_cells: 0,
            last_dropped_aggressions: 0,
            last_projection_ms: 0.0,
            projection_builds: 0,
            projection_cache_hits: 0,
            projection_cache: None,
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn toggle_settings(&mut self) {
        self.show_settings = !self.show_settings;
    }

    /// Reset market-specific history while preserving visual/retention
    /// settings. Capture deliberately returns to off; the app starts a fresh
    /// provider task only after the new feed handle is installed.
    pub fn reset_for_symbol(&mut self, symbol: impl Into<String>) {
        self.symbol = symbol.into();
        self.config.enabled = false;
        self.history = LiquidityHistory::new(self.config.clone());
        self.status = CaptureStatus::Disabled;
        self.generation_floor = 0;
        self.latest_generation = None;
        self.last_snapshot_observed_ms = None;
        self.depth_updates = 0;
        self.depth_updates_since_summary = 0;
        self.last_projection_cells = 0;
        self.last_projection_aggressions = 0;
        self.last_dropped_cells = 0;
        self.last_dropped_aggressions = 0;
        self.last_projection_ms = 0.0;
        self.projection_builds = 0;
        self.projection_cache_hits = 0;
        self.projection_cache = None;
    }

    /// Commit a capture toggle only after its feed command was accepted.
    pub fn set_enabled(&mut self, enabled: bool, generation_floor: u64) {
        if self.config.enabled == enabled {
            return;
        }
        if !enabled {
            self.mark_gap_at_latest("capture_disabled");
        }
        self.config.enabled = enabled;
        self.generation_floor = generation_floor;
        self.invalidate_projection();
        self.status = if enabled {
            CaptureStatus::Connecting
        } else {
            CaptureStatus::Disabled
        };
        self.apply_non_grouping_config();
        self.config_revision = self.config_revision.saturating_add(1);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_CAPTURE_CHANGED",
            symbol = self.symbol.as_str(),
            enabled,
            generation_floor,
            config_revision = self.config_revision,
            action = if enabled { "start_capture" } else { "stop_capture" },
            "book heatmap capture changed"
        );
    }

    /// Mark the existing generation discontinuous before the feed is restarted.
    pub fn prepare_restart(&mut self, generation_floor: u64, reason: &'static str) {
        self.mark_gap_at_latest(reason);
        self.generation_floor = generation_floor;
        self.status = CaptureStatus::Connecting;
        self.invalidate_projection();
    }

    /// Record a factual aggregate trade for the aggression overlay.
    pub fn record_trade(&mut self, trade: &Trade) {
        if self.config.enabled {
            self.history.record_aggression(trade);
        }
    }

    /// Apply one already-synchronized feed event.
    pub fn handle_depth_event(&mut self, event: DepthEvent) {
        let (symbol, generation) = match &event {
            DepthEvent::Status {
                symbol, generation, ..
            }
            | DepthEvent::Snapshot {
                symbol, generation, ..
            }
            | DepthEvent::Update {
                symbol, generation, ..
            } => (symbol, *generation),
        };
        if !self.config.enabled {
            return;
        }
        if !symbol.eq_ignore_ascii_case(&self.symbol) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_SYMBOL_MISMATCH",
                expected_symbol = self.symbol.as_str(),
                got_symbol = symbol.as_str(),
                generation,
                action = "ignore_event",
                "ignored order-book event for another symbol"
            );
            return;
        }
        let minimum_generation = self
            .latest_generation
            .unwrap_or(self.generation_floor)
            .max(self.generation_floor);
        if generation < minimum_generation {
            tracing::debug!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_STALE_GENERATION",
                symbol = self.symbol.as_str(),
                generation,
                generation_floor = self.generation_floor,
                latest_generation = self.latest_generation,
                minimum_generation,
                action = "ignore_event",
                "ignored queued event from a stopped depth task"
            );
            return;
        }
        self.latest_generation = Some(generation);

        match event {
            DepthEvent::Status { status, .. } => self.handle_status(generation, status),
            DepthEvent::Snapshot {
                observed_at_ms,
                effective_at_ms,
                snapshot,
                ..
            } => {
                self.invalidate_projection();
                self.last_snapshot_observed_ms = Some(observed_at_ms);
                if let Err(error) =
                    self.history
                        .install_snapshot(effective_at_ms, generation, snapshot)
                {
                    self.log_history_error("snapshot", generation, &error);
                    self.mark_gap_at_latest("snapshot_rejected");
                    self.status = CaptureStatus::Error;
                } else {
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "HEATMAP_SNAPSHOT_APPLIED",
                        symbol = self.symbol.as_str(),
                        generation,
                        observed_at_ms,
                        effective_at_ms,
                        bid_levels = self.history.book().bid_count(),
                        ask_levels = self.history.book().ask_count(),
                        coverage = ?self.history.book().coverage(),
                        action = "open_coverage",
                        "book snapshot installed in heatmap history"
                    );
                }
            }
            DepthEvent::Update {
                event_time_ms,
                delta,
                ..
            } => match self.history.apply_delta(event_time_ms, &delta) {
                Ok(_) => {
                    self.depth_updates = self.depth_updates.saturating_add(1);
                    self.depth_updates_since_summary =
                        self.depth_updates_since_summary.saturating_add(1);
                }
                Err(error) => {
                    self.log_history_error("delta", generation, &error);
                    self.mark_gap_at_latest("delta_rejected");
                    self.status = CaptureStatus::Error;
                }
            },
        }
    }

    fn handle_status(&mut self, generation: u64, status: DepthStatus) {
        self.status = match status {
            DepthStatus::Connecting => CaptureStatus::Connecting,
            DepthStatus::Buffering { .. } => CaptureStatus::Buffering,
            DepthStatus::SnapshotFetching { .. } => CaptureStatus::SnapshotFetching,
            DepthStatus::Synchronized { last_update_id, .. } => CaptureStatus::Live {
                generation,
                last_update_id,
            },
            DepthStatus::Resyncing { reason } => {
                let reason = resync_reason_code(&reason);
                self.mark_gap_at_latest(reason);
                CaptureStatus::Resyncing { reason }
            }
            DepthStatus::Disconnected { error_class } => {
                self.mark_gap_at_latest(error_class);
                CaptureStatus::Disconnected { error_class }
            }
            DepthStatus::Stopped => CaptureStatus::Disabled,
        };
    }

    fn mark_gap_at_latest(&mut self, reason: &'static str) {
        let Some(timestamp_ms) = self.history.latest_book_ms() else {
            return;
        };
        if matches!(self.history.status(), HistoryStatus::Gap { .. }) {
            return;
        }
        if let Err(error) = self.history.mark_gap(timestamp_ms, reason) {
            self.log_history_error("gap", self.latest_generation.unwrap_or(0), &error);
        } else {
            self.invalidate_projection();
        }
    }

    fn log_history_error(
        &self,
        operation: &'static str,
        generation: u64,
        error: &dyn std::fmt::Display,
    ) {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_HISTORY_REJECTED",
            symbol = self.symbol.as_str(),
            generation,
            operation,
            error = %error,
            action = "wait_for_fresh_snapshot",
            "order-flow history rejected an event"
        );
    }

    /// Build renderer-independent primitives for the visible bar slice.
    pub fn project_visible(
        &mut self,
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        extend_live_end: bool,
        price_range: (f64, f64),
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.enabled {
            return None;
        }
        let layout = ProjectionLayout {
            first_bar_index,
            slot_count: closed.len() + usize::from(partial.is_some()),
            extend_live_end,
        };
        let now = Instant::now();
        if let Some(cache) = &self.projection_cache
            && cache.layout == layout
            && now.saturating_duration_since(cache.built_at) < PROJECTION_INTERVAL
        {
            self.projection_cache_hits = self.projection_cache_hits.saturating_add(1);
            return Some(Arc::clone(&cache.frame));
        }

        let low = Decimal::from_f64(price_range.0)?;
        let high = Decimal::from_f64(price_range.1)?;
        let prices = PriceWindow::new(low, high)?;
        let timeline = BarTimeline::from_bars(
            first_bar_index,
            closed,
            partial,
            if extend_live_end {
                self.history.latest_book_ms()
            } else {
                None
            },
        );
        if timeline.is_empty() {
            return None;
        }
        let started = now;
        let projection = project(&self.history, &timeline, prices);
        self.last_projection_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.last_projection_cells = projection.cells.len();
        self.last_projection_aggressions = projection.aggressions.len();
        self.last_dropped_cells = projection.dropped_cells;
        self.last_dropped_aggressions = projection.dropped_aggressions;
        self.projection_builds = self.projection_builds.saturating_add(1);
        let frame = Arc::new(VisibleOrderflow {
            projection: Arc::new(projection),
            first_bar_index,
            slot_count: timeline.len(),
        });
        self.projection_cache = Some(ProjectionCache {
            built_at: now,
            layout,
            frame: Arc::clone(&frame),
        });
        Some(frame)
    }

    fn invalidate_projection(&mut self) {
        self.projection_cache = None;
    }

    /// Draw resting liquidity and explicit coverage gaps behind candles.
    pub fn draw_background(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
    ) {
        let clip = painter.with_clip_rect(chart_rect);
        let mut mesh = egui::Mesh::default();
        mesh.vertices
            .reserve(frame.projection.cells.len().saturating_mul(4));
        mesh.indices
            .reserve(frame.projection.cells.len().saturating_mul(6));

        for cell in &frame.projection.cells {
            let rect = projected_rect(
                chart_rect,
                viewport,
                total_bars,
                frame.first_bar_index,
                frame.slot_count,
                cell.x0,
                cell.x1,
                cell.y0,
                cell.y1,
            );
            let color = heat_color(cell.side, cell.intensity, cell.alpha);
            if rect.is_positive() {
                mesh.add_colored_rect(rect, color);
            }
        }
        if !mesh.is_empty() {
            clip.add(egui::Shape::mesh(mesh));
        }

        for gap in &frame.projection.gaps {
            let x0 = projected_x(
                chart_rect,
                viewport,
                total_bars,
                frame.first_bar_index,
                frame.slot_count,
                gap.x0,
            );
            let x1 = projected_x(
                chart_rect,
                viewport,
                total_bars,
                frame.first_bar_index,
                frame.slot_count,
                gap.x1,
            );
            let rect = egui::Rect::from_min_max(
                egui::pos2(x0.min(x1), chart_rect.top()),
                egui::pos2(x0.max(x1), chart_rect.bottom()),
            )
            .intersect(chart_rect);
            if !rect.is_positive() {
                continue;
            }
            clip.rect_filled(rect, egui::Rounding::ZERO, GAP_FILL);
            draw_dashed_vertical(&clip, rect.left(), rect);
            draw_dashed_vertical(&clip, rect.right(), rect);
            if rect.width() >= 100.0 {
                clip.text(
                    rect.center_top() + egui::vec2(0.0, 18.0),
                    egui::Align2::CENTER_TOP,
                    gap_label(&gap.reason),
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_gray(175),
                );
            }
        }
    }

    /// Draw factual aggressive prints over candles.
    pub fn draw_aggressions(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
    ) {
        let clip = painter.with_clip_rect(chart_rect);
        for trade in &frame.projection.aggressions {
            let x = projected_x(
                chart_rect,
                viewport,
                total_bars,
                frame.first_bar_index,
                frame.slot_count,
                trade.x,
            );
            let y = chart_rect.top() + trade.y as f32 * chart_rect.height();
            if !chart_rect.contains(egui::pos2(x, y)) {
                continue;
            }
            let radius = 2.5 + trade.size * 8.0;
            let color = match trade.side {
                Side::Buy => BUY_PRINT,
                Side::Sell => SELL_PRINT,
            };
            clip.circle_filled(egui::pos2(x, y), radius, color.gamma_multiply(0.72));
            clip.circle_stroke(egui::pos2(x, y), radius, egui::Stroke::new(1.0_f32, color));
        }
    }

    pub fn draw_status_badge(&self, painter: &egui::Painter, chart_rect: egui::Rect) {
        if !self.config.enabled {
            return;
        }
        let text = self.status.label();
        let color = self.status.color();
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

    /// Draw the floating settings window and return whether capture must
    /// restart because the exact price grouping changed.
    pub fn draw_settings(&mut self, ctx: &egui::Context) -> bool {
        if !self.show_settings {
            return false;
        }
        let before = self.config.clone();
        let mut open = self.show_settings;
        egui::Window::new("order flow layers")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("History begins at the first live depth sync.");
                ui.small("Binance does not provide historical L2 backfill.");
                ui.separator();

                let mut grouping = self.config.price_grouping.to_f64().unwrap_or(0.01);
                ui.horizontal(|ui| {
                    ui.label("price bucket");
                    ui.add(
                        egui::DragValue::new(&mut grouping)
                            .range(0.000_000_01..=1_000_000.0)
                            .speed(0.01),
                    );
                });
                self.config.price_grouping =
                    Decimal::from_f64(grouping.max(0.000_000_01)).unwrap_or(Decimal::new(1, 2));

                let mut retention_minutes = self.config.retention_ms as f64 / 60_000.0;
                ui.add(
                    egui::Slider::new(&mut retention_minutes, 1.0..=1_440.0)
                        .logarithmic(true)
                        .text("retention min"),
                );
                self.config.retention_ms = (retention_minutes * 60_000.0) as i64;
                ui.add(egui::Slider::new(&mut self.config.opacity, 0.0..=1.0).text("opacity"));
                ui.add(egui::Slider::new(&mut self.config.gamma, 0.1..=3.0).text("gamma"));
                ui.checkbox(&mut self.config.show_aggressions, "aggression bubbles")
                    .on_hover_text(
                        "confirmed aggTrades; bubbles never mutate resting book quantities",
                    );

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

                ui.separator();
                ui.label(format!(
                    "{} · {} bid / {} ask levels",
                    self.status.label(),
                    self.history.book().bid_count(),
                    self.history.book().ask_count()
                ));
                ui.label(format!(
                    "{} runs · {:.1} MiB retained",
                    self.history.archived_run_count() + self.history.active_level_count(),
                    self.history.approximate_history_bytes() as f64 / (1024.0 * 1024.0)
                ));
                if ui.button("reset visual defaults").clicked() {
                    let enabled = self.config.enabled;
                    self.config = HeatmapConfig {
                        enabled,
                        ..HeatmapConfig::default()
                    };
                }
            });
        self.show_settings = open;

        self.config.sanitize();
        if self.config == before {
            return false;
        }
        self.invalidate_projection();

        let grouping_changed = self.config.price_grouping != before.price_grouping;
        if grouping_changed {
            match self
                .history
                .reset_price_grouping(self.config.price_grouping)
            {
                Ok(reset) => tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_GROUPING_RESET",
                    symbol = self.symbol.as_str(),
                    previous_grouping = %reset.previous,
                    current_grouping = %reset.current,
                    dropped_runs = reset.dropped_runs,
                    dropped_aggressions = reset.dropped_aggressions,
                    action = "restart_capture",
                    "price grouping changed; historical runs were reset honestly"
                ),
                Err(error) => self.log_history_error(
                    "grouping_reset",
                    self.latest_generation.unwrap_or(0),
                    &error,
                ),
            }
        }
        self.apply_non_grouping_config();
        self.config_revision = self.config_revision.saturating_add(1);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_CONFIG_CHANGED",
            symbol = self.symbol.as_str(),
            config_revision = self.config_revision,
            enabled = self.config.enabled,
            retention_ms = self.config.retention_ms,
            price_grouping = %self.config.price_grouping,
            opacity = self.config.opacity,
            gamma = self.config.gamma,
            show_aggressions = self.config.show_aggressions,
            grouping_changed,
            action = if grouping_changed { "restart_capture" } else { "reproject" },
            "heatmap configuration changed"
        );
        grouping_changed && self.config.enabled
    }

    fn apply_non_grouping_config(&mut self) {
        if let Err(error) = self.history.update_config(self.config.clone()) {
            self.log_history_error("config_update", self.latest_generation.unwrap_or(0), &error);
        }
    }

    pub fn health(&self) -> OrderflowHealth {
        let (generation, last_update_id, last_event_ms) = match self.history.status() {
            HistoryStatus::Synced {
                generation,
                last_update_id,
                last_event_ms,
            } => (Some(generation), last_update_id, Some(last_event_ms)),
            HistoryStatus::Gap {
                from_generation, ..
            } => (
                from_generation,
                self.history.book().last_update_id(),
                self.history.latest_book_ms(),
            ),
            HistoryStatus::Empty => (None, None, None),
        };
        let counters = self.history.counters();
        OrderflowHealth {
            enabled: self.config.enabled,
            status: self.status.code(),
            generation,
            last_update_id,
            last_event_ms,
            bid_levels: self.history.book().bid_count(),
            ask_levels: self.history.book().ask_count(),
            active_levels: self.history.active_level_count(),
            archived_runs: self.history.archived_run_count(),
            aggression_count: self.history.aggressions().count(),
            history_bytes: self.history.approximate_history_bytes(),
            projection_cells: self.last_projection_cells,
            projection_aggressions: self.last_projection_aggressions,
            dropped_cells: self.last_dropped_cells,
            dropped_aggressions: self.last_dropped_aggressions,
            projection_ms: self.last_projection_ms,
            projection_builds: self.projection_builds,
            projection_cache_hits: self.projection_cache_hits,
            config_revision: self.config_revision,
            last_snapshot_observed_ms: self.last_snapshot_observed_ms,
            depth_updates: self.depth_updates,
            depth_updates_since_summary: self.depth_updates_since_summary,
            snapshots: counters.snapshots,
            gaps: counters.gaps,
        }
    }

    pub fn reset_summary_counters(&mut self) {
        self.depth_updates_since_summary = 0;
    }
}

fn draw_dashed_vertical(painter: &egui::Painter, x: f32, rect: egui::Rect) {
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(x, y),
                egui::pos2(x, (y + 4.0).min(rect.bottom())),
            ],
            egui::Stroke::new(1.0_f32, GAP_BOUNDARY),
        );
        y += 9.0;
    }
}

fn resync_reason_code(reason: &DepthResyncReason) -> &'static str {
    match reason {
        DepthResyncReason::SnapshotTooOld { .. } => "snapshot_too_old",
        DepthResyncReason::SequenceGap { .. } => "sequence_gap",
        DepthResyncReason::SnapshotAttemptsExhausted { .. } => "snapshot_attempts_exhausted",
    }
}

fn gap_label(reason: &str) -> &'static str {
    match reason {
        "book_unavailable_before_capture" => "book unavailable before capture",
        "capture_disabled" => "book capture disabled",
        "sequence_gap" => "book gap · resynchronizing",
        _ => "book continuity unavailable",
    }
}

fn projected_x(
    chart_rect: egui::Rect,
    viewport: &Viewport,
    total_bars: usize,
    first_bar_index: usize,
    slot_count: usize,
    normalized: f64,
) -> f32 {
    let position = first_bar_index as f32 - 0.5 + normalized as f32 * slot_count as f32;
    viewport.x_at_bar_position(position, chart_rect.right(), total_bars)
}

#[allow(clippy::too_many_arguments)]
fn projected_rect(
    chart_rect: egui::Rect,
    viewport: &Viewport,
    total_bars: usize,
    first_bar_index: usize,
    slot_count: usize,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
) -> egui::Rect {
    let left = projected_x(
        chart_rect,
        viewport,
        total_bars,
        first_bar_index,
        slot_count,
        x0,
    );
    let right = projected_x(
        chart_rect,
        viewport,
        total_bars,
        first_bar_index,
        slot_count,
        x1,
    );
    let top = chart_rect.top() + y0 as f32 * chart_rect.height();
    let bottom = chart_rect.top() + y1 as f32 * chart_rect.height();
    let mut rect = egui::Rect::from_min_max(
        egui::pos2(left.min(right), top.min(bottom)),
        egui::pos2(left.max(right), top.max(bottom)),
    );
    if rect.height() < 1.0 {
        rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width().max(0.5), 1.0));
    }
    rect.intersect(chart_rect)
}

fn heat_color(side: BookSide, intensity: f32, alpha: f32) -> egui::Color32 {
    let t = intensity.clamp(0.0, 1.0);
    let (low, mid) = match side {
        BookSide::Bid => (BID_LOW, BID_MID),
        BookSide::Ask => (ASK_LOW, ASK_MID),
    };
    let rgb = if t < 0.7 {
        lerp_rgb(low, mid, t / 0.7)
    } else {
        lerp_rgb(mid, HEAT_HIGH, (t - 0.7) / 0.3)
    };
    egui::Color32::from_rgba_unmultiplied(
        rgb[0],
        rgb[1],
        rgb[2],
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (f32::from(a[0]) + (f32::from(b[0]) - f32::from(a[0])) * t).round() as u8,
        (f32::from(a[1]) + (f32::from(b[1]) - f32::from(a[1])) * t).round() as u8,
        (f32::from(a[2]) + (f32::from(b[2]) - f32::from(a[2])) * t).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_feed_binance::depth::DepthStatus;
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
    fn disabled_view_ignores_book_and_trade_events() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.handle_depth_event(snapshot_event(1));
        view.record_trade(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert!(!view.history.book().is_initialized());
        assert_eq!(view.history.aggressions().count(), 0);
    }

    #[test]
    fn effective_snapshot_time_allows_earlier_buffered_observation() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 100);
        view.handle_depth_event(snapshot_event(100));
        assert_eq!(view.history.latest_book_ms(), Some(999));
        assert_eq!(view.last_snapshot_observed_ms, Some(1_100));
    }

    #[test]
    fn projection_is_cached_at_depth_cadence_and_gap_invalidates_it() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];

        let first = view
            .project_visible(0, &bars, None, true, (98.0, 102.0))
            .unwrap();
        let second = view
            .project_visible(0, &bars, None, true, (98.0, 102.0))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(view.projection_builds, 1);
        assert_eq!(view.projection_cache_hits, 1);

        view.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            status: DepthStatus::Resyncing {
                reason: DepthResyncReason::SequenceGap {
                    expected_update_id: 11,
                    first_update_id: 12,
                    final_update_id: 13,
                },
            },
        });
        let after_gap = view
            .project_visible(0, &bars, None, true, (98.0, 102.0))
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &after_gap));
        assert_eq!(view.projection_builds, 2);
    }

    #[test]
    fn events_below_capture_generation_are_ignored() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 500);
        view.handle_depth_event(snapshot_event(499));
        assert!(!view.history.book().is_initialized());
    }

    #[test]
    fn delayed_event_from_an_older_reconnect_generation_is_ignored() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 12,
            status: DepthStatus::Connecting,
        });
        view.handle_depth_event(snapshot_event(11));
        assert!(!view.history.book().is_initialized());
        assert_eq!(view.latest_generation, Some(12));
    }

    #[test]
    fn rejected_delta_closes_coverage_immediately() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        view.handle_depth_event(DepthEvent::Update {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            event_time_ms: 998,
            delta: BookDelta::new(11, 11, Vec::new(), Vec::new()),
        });

        assert!(matches!(view.history.status(), HistoryStatus::Gap { .. }));
        assert!(!view.history.book().is_initialized());
        assert_eq!(view.history.coverage_gaps().count(), 1);
    }

    #[test]
    fn resync_status_opens_an_explicit_gap() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        view.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            status: DepthStatus::Resyncing {
                reason: DepthResyncReason::SequenceGap {
                    expected_update_id: 11,
                    first_update_id: 12,
                    final_update_id: 13,
                },
            },
        });
        assert!(matches!(view.history.status(), HistoryStatus::Gap { .. }));
        assert_eq!(view.history.coverage_gaps().count(), 1);
    }

    #[test]
    fn fractional_projection_uses_bar_slot_edges() {
        let viewport = Viewport::new();
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let x0 = projected_x(rect, &viewport, 10, 8, 2, 0.0);
        let x1 = projected_x(rect, &viewport, 10, 8, 2, 1.0);
        assert!((x1 - x0 - 16.0).abs() < 0.001);
    }

    #[test]
    fn heat_palette_is_side_specific_and_bounded() {
        let bid = heat_color(BookSide::Bid, 0.5, 0.7);
        let ask = heat_color(BookSide::Ask, 0.5, 0.7);
        assert_ne!(bid, ask);
        assert_eq!(bid.a(), 179);
        assert_eq!(ask.a(), 179);
    }
}
