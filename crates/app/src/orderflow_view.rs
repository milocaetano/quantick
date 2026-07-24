//! egui adapter for the pure order-flow history and projection modules.
//!
//! Feed synchronization, deterministic book mutation and renderer-independent
//! geometry remain outside this file. This layer owns only UI configuration,
//! status presentation and conversion of normalized primitives into egui
//! shapes, keeping the existing candle path isolated.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use quantick_engine::{Bar, Trade};
use quantick_feed_binance::depth::{DepthEvent, DepthResyncReason, DepthStatus};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::orderflow::{
    BarTimeline, DisplayGrouping, HeatmapConfig, HeatmapProjection, HeatmapTheme, HistoryStatus,
    IntensityMode, LiquidityHistory, PriceWindow, project,
};
use crate::orderflow_render::{
    OrderflowRenderStyle, ProjectedLayout, RenderContext, draw_aggression_bubbles,
    draw_compact_legend, draw_heatmap_background, draw_liquidity_events, draw_preview,
};
use crate::viewport::Viewport;

const PROJECTION_INTERVAL: Duration = Duration::from_millis(220);

/// Quantize the price window before it keys the projection cache, so a
/// sub-pixel wiggle of the auto-fit range (which happens almost every frame on a
/// live book) does not force a full re-projection. The window is snapped to
/// ~1/500 of its own span; a cached frame is at most that far off vertically.
fn quantize_price(value: f64, span: f64) -> u64 {
    let step = (span / 500.0).max(f64::MIN_POSITIVE);
    ((value / step).round() * step).to_bits()
}

/// Reference price of a snapshot (best bid/ask mid), for sizing the capture
/// bucket. Any level works — only the magnitude matters.
fn snapshot_reference_price(snapshot: &quantick_orderbook::BookSnapshot) -> Option<f64> {
    let best_bid = snapshot.bids().iter().map(|level| level.price()).max();
    let best_ask = snapshot.asks().iter().map(|level| level.price()).min();
    let price = match (best_bid, best_ask) {
        (Some(bid), Some(ask)) => (bid + ask) / Decimal::from(2),
        (Some(price), None) | (None, Some(price)) => price,
        (None, None) => return None,
    };
    price.to_f64()
}

/// A capture price bucket proportional to the asset's price, snapped to a
/// 1 / 2 / 5 · 10^k step near `price / 65_000` — so BTC (~65k) lands near $1, an
/// index (~130k) near $2, a ~5k contract near $0.1, and FX stays sub-cent. A
/// dense book at the 0.01 default explodes into tens of thousands of RLE runs;
/// this keeps the count affordable without per-asset tuning. Falls back to the
/// fine default when the price is unusable.
fn adaptive_base(price: f64) -> Decimal {
    if !price.is_finite() || price <= 0.0 {
        return Decimal::new(1, 2);
    }
    let target = (price / 65_000.0).clamp(1e-9, 1e6);
    let pow = 10f64.powf(target.log10().floor());
    let mantissa = target / pow; // in [1, 10)
    let nice = if mantissa < 1.5 {
        1.0
    } else if mantissa < 3.5 {
        2.0
    } else if mantissa < 7.5 {
        5.0
    } else {
        10.0
    };
    Decimal::from_f64(nice * pow).unwrap_or(Decimal::new(1, 2))
}

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
    // Cell y-positions are normalized against the price window at build
    // time, so a cached frame is only valid for the exact same window
    // (bit-compared: any pan/zoom or auto-fit shift must rebuild).
    price_low_bits: u64,
    price_high_bits: u64,
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
    pub projection_liquidity_events: usize,
    pub dropped_cells: usize,
    pub dropped_aggressions: usize,
    pub dropped_liquidity_events: usize,
    pub effective_grouping: Decimal,
    pub effective_grouping_multiple: u32,
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
    /// Size the capture bucket from the asset price on the first snapshot.
    /// Cleared once the user sets the base resolution by hand.
    auto_base: bool,
    show_settings: bool,
    capture_grouping_draft: f64,
    pending_capture_grouping_previous: Option<Decimal>,
    status: CaptureStatus,
    generation_floor: u64,
    latest_generation: Option<u64>,
    last_snapshot_observed_ms: Option<i64>,
    depth_updates: u64,
    depth_updates_since_summary: u64,
    config_revision: u64,
    last_projection_cells: usize,
    last_projection_aggressions: usize,
    last_projection_liquidity_events: usize,
    last_dropped_cells: usize,
    last_dropped_aggressions: usize,
    last_dropped_liquidity_events: usize,
    last_effective_grouping: Decimal,
    last_effective_grouping_multiple: u32,
    last_projection_ms: f32,
    projection_builds: u64,
    projection_cache_hits: u64,
    projection_cache: Option<ProjectionCache>,
}

impl OrderflowView {
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        let config = HeatmapConfig::default();
        let base_grouping = config.price_grouping;
        Self {
            symbol: symbol.into(),
            history: LiquidityHistory::new(config.clone()),
            config,
            auto_base: true,
            show_settings: false,
            capture_grouping_draft: base_grouping.to_f64().unwrap_or(0.01),
            pending_capture_grouping_previous: None,
            status: CaptureStatus::Disabled,
            generation_floor: 0,
            latest_generation: None,
            last_snapshot_observed_ms: None,
            depth_updates: 0,
            depth_updates_since_summary: 0,
            config_revision: 0,
            last_projection_cells: 0,
            last_projection_aggressions: 0,
            last_projection_liquidity_events: 0,
            last_dropped_cells: 0,
            last_dropped_aggressions: 0,
            last_dropped_liquidity_events: 0,
            last_effective_grouping: base_grouping,
            last_effective_grouping_multiple: 1,
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

    /// Latest exchange timestamp for which live book state is known, while
    /// capture is active. Drives how wide the forming bar's live tail grows.
    #[must_use]
    pub fn live_end_ms(&self) -> Option<i64> {
        if self.config.enabled {
            self.history.latest_book_ms()
        } else {
            None
        }
    }

    pub fn toggle_settings(&mut self) {
        self.show_settings = !self.show_settings;
    }

    #[must_use]
    pub fn settings_button_label(&self) -> String {
        match self.config.display_grouping {
            DisplayGrouping::Adaptive { .. } => "⚙ L2 · auto".to_owned(),
            DisplayGrouping::Native => "⚙ L2 · 1×".to_owned(),
            DisplayGrouping::Multiple(multiple) => format!("⚙ L2 · {multiple}×"),
        }
    }

    #[cfg(test)]
    pub(crate) fn stage_capture_grouping_for_test(&mut self, grouping: Decimal) -> bool {
        let before = self.config.clone();
        self.config.price_grouping = grouping;
        self.capture_grouping_draft = grouping.to_f64().unwrap_or(self.capture_grouping_draft);
        self.commit_config_changes(before)
    }

    #[cfg(test)]
    pub(crate) fn base_capture_grouping_for_test(&self) -> Decimal {
        self.config.price_grouping
    }

    /// Reset market-specific history while preserving visual/retention
    /// settings. Capture deliberately returns to off; the app starts a fresh
    /// provider task only after the new feed handle is installed.
    pub fn reset_for_symbol(&mut self, symbol: impl Into<String>) {
        self.symbol = symbol.into();
        self.config.enabled = false;
        // A new market has a new price scale; re-derive the capture bucket.
        self.auto_base = true;
        self.history = LiquidityHistory::new(self.config.clone());
        self.capture_grouping_draft = self.config.price_grouping.to_f64().unwrap_or(0.01);
        self.pending_capture_grouping_previous = None;
        self.status = CaptureStatus::Disabled;
        self.generation_floor = 0;
        self.latest_generation = None;
        self.last_snapshot_observed_ms = None;
        self.depth_updates = 0;
        self.depth_updates_since_summary = 0;
        self.last_projection_cells = 0;
        self.last_projection_aggressions = 0;
        self.last_projection_liquidity_events = 0;
        self.last_dropped_cells = 0;
        self.last_dropped_aggressions = 0;
        self.last_dropped_liquidity_events = 0;
        self.last_effective_grouping = self.config.price_grouping;
        self.last_effective_grouping_multiple = 1;
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

    /// Commit a staged base-grouping change only after the feed accepted its
    /// restart command. Until this point the old history and capture remain
    /// fully usable.
    pub fn accept_capture_grouping_restart(&mut self, generation_floor: u64) {
        let Some(previous) = self.pending_capture_grouping_previous.take() else {
            self.prepare_restart(generation_floor, "configuration_restart");
            return;
        };
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
                "accepted capture restart; base grouping and retained L2 history were reset"
            ),
            Err(error) => {
                self.config.price_grouping = previous;
                self.capture_grouping_draft =
                    previous.to_f64().unwrap_or(self.capture_grouping_draft);
                self.log_history_error(
                    "grouping_reset",
                    self.latest_generation.unwrap_or(0),
                    &error,
                );
            }
        }
        self.apply_non_grouping_config();
        self.prepare_restart(generation_floor, "configuration_restart");
    }

    /// Roll back a staged base-grouping change when the feed command could not
    /// be queued. No retained history has been touched at this point.
    pub fn reject_capture_grouping_restart(&mut self, reason: &'static str) {
        let Some(previous) = self.pending_capture_grouping_previous.take() else {
            return;
        };
        let requested = self.config.price_grouping;
        self.config.price_grouping = previous;
        self.capture_grouping_draft = previous.to_f64().unwrap_or(self.capture_grouping_draft);
        self.apply_non_grouping_config();
        self.invalidate_projection();
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
                // Size the capture bucket from the asset price before the first
                // snapshot of a capture, so a dense book (BTC etc.) doesn't
                // explode into runs. Skipped once the user picks a base by hand.
                if self.auto_base
                    && matches!(self.history.status(), HistoryStatus::Empty)
                    && let Some(price) = snapshot_reference_price(&snapshot)
                {
                    self.apply_auto_base(adaptive_base(price));
                }
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
        let price_span = price_range.1 - price_range.0;
        let layout = ProjectionLayout {
            first_bar_index,
            slot_count: closed.len() + usize::from(partial.is_some()),
            extend_live_end,
            price_low_bits: quantize_price(price_range.0, price_span),
            price_high_bits: quantize_price(price_range.1, price_span),
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
        self.last_projection_liquidity_events = projection.liquidity_events.len();
        self.last_dropped_cells = projection.dropped_cells;
        self.last_dropped_aggressions = projection.dropped_aggressions;
        self.last_dropped_liquidity_events = projection.dropped_liquidity_events;
        self.last_effective_grouping = projection.effective_grouping.bucket_width;
        self.last_effective_grouping_multiple = projection.effective_grouping.multiple;
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
    /// restart because the base capture resolution changed.
    pub fn draw_settings(&mut self, ctx: &egui::Context) -> bool {
        if !self.show_settings {
            return false;
        }
        let before = self.config.clone();
        let mut open = self.show_settings;
        egui::Window::new("order flow · liquidity map")
            .open(&mut open)
            .default_width(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("orderflow_settings_scroll")
                    .max_height(560.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("BOOKMAP VIEW")
                            .strong()
                            .color(egui::Color32::from_rgb(255, 222, 92)),
                    );
                    ui.label(
                        egui::RichText::new(self.status.label())
                            .small()
                            .color(self.status.color()),
                    );
                });
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
                ui.strong("aggression bubbles");
                ui.checkbox(&mut self.config.show_aggressions, "show confirmed executions")
                    .on_hover_text("confirmed Binance aggTrades; color is the aggressor side");
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
                                    (250, "250 ms"),
                                    (500, "500 ms"),
                                ] {
                                    ui.selectable_value(
                                        &mut self.config.bubble_cluster_ms,
                                        milliseconds,
                                        label,
                                    );
                                }
                            });
                    });
                });

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
                ui.label(format!(
                    "effective range {} · {}× base",
                    self.last_effective_grouping, self.last_effective_grouping_multiple
                ));
                if ui.button("reset Bookmap visuals").clicked() {
                    let enabled = self.config.enabled;
                    let price_grouping = self.config.price_grouping;
                    self.capture_grouping_draft =
                        price_grouping.to_f64().unwrap_or(self.capture_grouping_draft);
                    self.config = HeatmapConfig {
                        enabled,
                        price_grouping,
                        ..HeatmapConfig::default()
                    };
                }
                    });
            });
        self.show_settings = open;

        self.commit_config_changes(before)
    }

    fn commit_config_changes(&mut self, before: HeatmapConfig) -> bool {
        self.config.sanitize();
        if self.config == before {
            return false;
        }
        self.invalidate_projection();

        let capture_grouping_changed = self.config.price_grouping != before.price_grouping;
        if capture_grouping_changed {
            // The user picked a base resolution by hand; stop auto-sizing it.
            self.auto_base = false;
        }
        let restart_required = capture_grouping_changed && self.config.enabled;
        if restart_required {
            self.pending_capture_grouping_previous = Some(before.price_grouping);
            let mut compatible = self.config.clone();
            compatible.price_grouping = before.price_grouping;
            self.apply_history_config(compatible);
        } else if capture_grouping_changed {
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
                    action = "apply_while_capture_disabled",
                    "base price grouping changed; historical runs were reset honestly"
                ),
                Err(error) => self.log_history_error(
                    "grouping_reset",
                    self.latest_generation.unwrap_or(0),
                    &error,
                ),
            }
            self.apply_non_grouping_config();
        } else {
            self.apply_non_grouping_config();
        }
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
            display_grouping = ?self.config.display_grouping,
            opacity = self.config.opacity,
            gamma = self.config.gamma,
            show_aggressions = self.config.show_aggressions,
            bubble_cluster_ms = self.config.bubble_cluster_ms,
            show_liquidity_events = self.config.show_liquidity_events,
            liquidity_correlation_ms = self.config.liquidity_correlation_ms,
            theme = ?self.config.theme,
            capture_grouping_changed,
            action = if restart_required { "request_capture_restart" } else { "reproject" },
            "heatmap configuration changed"
        );
        restart_required
    }

    fn apply_non_grouping_config(&mut self) {
        self.apply_history_config(self.config.clone());
    }

    fn apply_history_config(&mut self, config: HeatmapConfig) {
        if let Err(error) = self.history.update_config(config) {
            self.log_history_error("config_update", self.latest_generation.unwrap_or(0), &error);
        }
    }

    /// Adopt a price-derived capture bucket while history is still empty (so no
    /// data is discarded), keeping config, draft and history in sync.
    fn apply_auto_base(&mut self, base: Decimal) {
        if base <= Decimal::ZERO || base == self.config.price_grouping {
            return;
        }
        match self.history.reset_price_grouping(base) {
            Ok(_) => {
                self.config.price_grouping = base;
                self.capture_grouping_draft = base.to_f64().unwrap_or(self.capture_grouping_draft);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_AUTO_BASE",
                    symbol = self.symbol.as_str(),
                    base = %base,
                    action = "size_capture_bucket_from_price",
                    "auto-sized L2 capture bucket from asset price"
                );
            }
            Err(error) => {
                self.log_history_error("auto_base", self.latest_generation.unwrap_or(0), &error);
            }
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
            projection_liquidity_events: self.last_projection_liquidity_events,
            dropped_cells: self.last_dropped_cells,
            dropped_aggressions: self.last_dropped_aggressions,
            dropped_liquidity_events: self.last_dropped_liquidity_events,
            effective_grouping: self.last_effective_grouping,
            effective_grouping_multiple: self.last_effective_grouping_multiple,
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

fn resync_reason_code(reason: &DepthResyncReason) -> &'static str {
    match reason {
        DepthResyncReason::SnapshotTooOld { .. } => "snapshot_too_old",
        DepthResyncReason::SequenceGap { .. } => "sequence_gap",
        DepthResyncReason::SnapshotAttemptsExhausted { .. } => "snapshot_attempts_exhausted",
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
    use quantick_engine::Side;
    use quantick_feed_binance::depth::DepthStatus;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

    #[test]
    fn adaptive_base_scales_with_price_and_stays_round() {
        // BTC ~ $1, an index ~ $2, a ~5k contract ~ $0.1, FX stays sub-cent.
        assert_eq!(adaptive_base(65_000.0), Decimal::from(1));
        assert_eq!(adaptive_base(118_000.0), Decimal::from(2));
        assert_eq!(adaptive_base(5_000.0), Decimal::new(1, 1)); // 0.1
        assert!(adaptive_base(1.1) < Decimal::new(1, 2)); // < 0.01
        // Every result is a positive 1/2/5 * 10^k step.
        for price in [0.5, 12.0, 300.0, 4_200.0, 65_000.0, 250_000.0] {
            let base = adaptive_base(price);
            assert!(base > Decimal::ZERO, "price {price} -> {base}");
        }
        // Degenerate prices fall back to the fine default.
        assert_eq!(adaptive_base(0.0), Decimal::new(1, 2));
        assert_eq!(adaptive_base(f64::NAN), Decimal::new(1, 2));
    }

    #[test]
    fn auto_base_sizes_the_capture_bucket_from_the_first_snapshot() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 1);
        // A ~65k book: the 0.01 default should auto-size up to ~$1.
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
        assert_eq!(view.base_capture_grouping_for_test(), Decimal::from(1));
        assert!(view.history.book().is_initialized());
    }

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

        // A price-window change (pan/zoom, auto-fit shift) must rebuild:
        // cached cell positions are normalized against the old window.
        let panned = view
            .project_visible(0, &bars, None, true, (97.0, 103.0))
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &panned));
        assert_eq!(view.projection_builds, 2);

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
        assert_eq!(view.projection_builds, 3);
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
}
