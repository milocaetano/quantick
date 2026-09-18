//! What the application says about its own health.
//!
//! [`super::QuantickApp::maybe_emit_summary`] is the `APP_HEALTH_SUMMARY`
//! line a validation run reads to decide whether a frame budget held, and
//! `status_model` is the same set of facts shaped for the status bar. They
//! sit together because they answer one question from one set of counters,
//! and because the log line and the bar must never be able to disagree.

use std::time::Instant;

use eframe::egui;

use crate::metrics;
use crate::metrics::FrameStats;
use crate::statusbar;
use crate::style::CandlePreset;
use crate::window_scale;
use quantick_orderflow::engine::OrderflowHealth;

use super::{QuantickApp, fmt_progress};

/// What the window measures about itself between perf summaries.
///
/// Owned by this module, the only place the numbers are read together.
/// `frame` records into them each pass, `menu_bar` toggles the status-bar
/// readout and `control_host` publishes them.
pub(super) struct HealthCounters {
    pub(super) frames: FrameStats,

    /// CPU time per frame (update + tessellation + paint, no vsync wait), from
    /// eframe. Separates "we are slow" from "we are waiting for the display".
    pub(super) cpu_frames: FrameStats,
    pub(super) last_frame: Option<Instant>,

    /// Live trades taken in since the last perf summary, across every tab —
    /// what the window is ingesting, not what one market prints.
    pub(super) trades_since_summary: u64,
    /// What the last summary said about the live envelope, so its warnings
    /// speak once per event.
    pub(super) envelope: envelope::EnvelopeWatch,
    pub(super) last_summary: Instant,
    // Whether the status bar shows the perf readings (View → perf readings).
    pub(super) show_perf: bool,
}

impl HealthCounters {
    /// A window that has measured nothing yet, perf readings shown.
    pub(super) fn new() -> Self {
        Self {
            show_perf: true,
            frames: FrameStats::new(120),
            cpu_frames: FrameStats::new(120),
            last_frame: None,
            trades_since_summary: 0,
            envelope: envelope::EnvelopeWatch::default(),
            last_summary: Instant::now(),
        }
    }
}

impl QuantickApp {
    /// Periodically log a perf summary and warn on threshold breaches.
    ///
    /// The clock and the context are read here; what the line says is
    /// [`HealthSummary`]'s, built from those readings and the counters.
    pub(super) fn maybe_emit_summary(&mut self, now: Instant, ctx: &egui::Context) {
        let elapsed = now - self.health.last_summary;
        if !worker_diagnostics::emit_if_due(&self.tabs, elapsed) {
            return;
        }
        let geometry = WindowGeometry::read(ctx, self.chrome.surface.as_ref());
        let book = self.active_tab_mut().tape_mut().health();
        let summary = HealthSummary::build(self, elapsed.as_secs_f64(), geometry, book);
        summary.log();
        summary.warn_breaches();
        let envelope = summary.envelope;

        self.health.envelope.warn(&envelope);
        self.health.trades_since_summary = 0;
        self.active_tab_mut().tape_mut().reset_summary_counters();
        self.health.last_summary = now;
    }

    /// Everything the status bar reports this frame.
    ///
    /// Provenance (venue, symbol, transport, side honesty) is the market's and
    /// reads from the window; the content section — spec, bar counts, forming
    /// bar, whether the view follows live — is the *focused pane's* (§11), so
    /// the bar always describes the chart the user is working in.
    pub(super) fn status_model(&self) -> statusbar::StatusModel {
        let pane = self.focused_pane();
        let bars = pane.state.bars();
        let (backfilled, live) = match pane.state.backfill_boundary() {
            Some(boundary) => (boundary, bars.len().saturating_sub(boundary)),
            None => (0, bars.len()),
        };
        let venue_bars = pane.history_prefix.len();
        let note = self.active_tab().side_note(&self.config);
        statusbar::StatusModel {
            venue: if self.active_tab().replay.is_some() {
                "recording".to_owned()
            } else {
                self.active_tab().feed_display_name(&self.config).to_owned()
            },
            symbol: self.active_tab().symbol.clone(),
            replay: self
                .active_tab()
                .replay
                .as_ref()
                .map(|link| statusbar::ReplayFigures {
                    speed: link.status.speed(),
                    progress: link.status.progress(),
                }),
            connection: self.active_tab().feed_connection,
            feed_arrival_ms: self.active_tab().trade_arrival_ms(),
            feed_latency: self.active_tab().feed_latency(),
            tape_age_ms: self.active_tab().tape_age_at(metrics::wall_clock_ms()),
            spec_summary: pane.state.spec().summary(),
            bar_progress: pane
                .state
                .progress()
                .map(|(progress, unit)| fmt_progress(&progress, unit)),
            deal_recording: self.active_tab().deal_status_cell(),
            venue_bars,
            backfilled_bars: backfilled,
            live_bars: live,
            side_note: note.clone().map(|(label, _)| label),
            side_detail: note.and_then(|(_, detail)| detail),
            // Provenance follows the active tab (§11), and so does the
            // simulated P&L: the cell speaks for the market on screen, never
            // for a background tab's position.
            sim_pnl: self.active_tab().paper.status_cell(),
            follows_live: pane.viewport.follows_live(),
            price_auto: pane.price_view.is_auto(),
            live_trades: self.active_tab().live_trades,
            fps: self.health.frames.fps(),
            frame_avg_ms: self.health.frames.avg_ms(),
            frame_cpu_ms: self.health.cpu_frames.avg_ms(),
            show_perf: self.health.show_perf,
        }
    }
}

/// The window's own geometry, because a chart that lays out wider than the
/// surface it is painted on loses its right edge — the toolbar's layer
/// group, the price axis, the live strip and the dock all live there — and
/// nothing else in the summary would say so.
///
/// `client` is the platform's *own* answer, not `screen * scale`: that
/// product is algebraically the same number as `screen` beside it and could
/// never contradict anything, which would make the whole block decoration.
/// Two independent readings, so the line can be read for whether they agree
/// — and by the time it is written a correction may already have restored
/// that agreement, which `WINDOW_SCALE_CORRECTED` is the record of.
struct WindowGeometry {
    screen: egui::Vec2,
    client: Option<egui::Vec2>,
    scale: f32,
    native_scale: Option<f32>,
    zoom: f32,
}

impl WindowGeometry {
    fn read(ctx: &egui::Context, surface: Option<&window_scale::SurfaceProbe>) -> Self {
        // Read outside the `input` closure: egui holds one lock for the whole
        // of it, and reaching back into the context from inside deadlocks.
        let zoom = ctx.zoom_factor();
        let client = surface.and_then(window_scale::SurfaceProbe::client_size_px);
        let (screen, scale, native_scale) = ctx.input(|input| {
            (
                input.screen_rect.size(),
                input.pixels_per_point(),
                input.viewport().native_pixels_per_point,
            )
        });
        Self {
            screen,
            client,
            scale,
            native_scale,
            zoom,
        }
    }
}

/// One `APP_HEALTH_SUMMARY`, and the warnings its figures call for.
///
/// Frames and the trade rate are the window's; every market figure is the
/// *active* tab's, which is what is on screen.
struct HealthSummary<'a> {
    tab_count: usize,
    tab_id: u64,
    tab: &'a crate::tab::Tab,
    health: &'a HealthCounters,
    style: &'a crate::style::ChartStyle,
    style_revision: u64,
    candle_preset: &'static str,
    geometry: WindowGeometry,
    book: OrderflowHealth,
    envelope: envelope::EnvelopeReading,
    /// Live trades per second across every tab since the last summary.
    trades_per_s: f64,
    /// Depth updates per second on the active tab since the last summary.
    book_updates_per_s: f64,
    frame_avg_ms: f32,
    feed_arrival_ms: Option<i64>,
}

impl<'a> HealthSummary<'a> {
    /// Everything the line reports, read off the window over the `seconds`
    /// since the last summary.
    fn build(
        app: &'a QuantickApp,
        seconds: f64,
        geometry: WindowGeometry,
        book: OrderflowHealth,
    ) -> Self {
        let trades_per_s = app.health.trades_since_summary as f64 / seconds;
        let book_updates_per_s = book.depth_updates_since_summary as f64 / seconds;
        let tab = app.active_tab();
        Self {
            tab_count: app.tabs.len(),
            tab_id: app.tabs.active_id(),
            tab,
            health: &app.health,
            style: &app.style,
            style_revision: app.style_revision,
            candle_preset: CandlePreset::detect(&app.style.candles)
                .map_or("custom", CandlePreset::log_value),
            geometry,
            envelope: envelope::observe(&app.tabs, trades_per_s, book_updates_per_s),
            book,
            trades_per_s,
            book_updates_per_s,
            frame_avg_ms: app.health.frames.avg_ms().unwrap_or(0.0),
            feed_arrival_ms: tab.trade_arrival_ms(),
        }
    }

    /// The `APP_HEALTH_SUMMARY` line itself.
    fn log(&self) {
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "APP_HEALTH_SUMMARY",
            // Frames and the trade rate are the window's; every market figure
            // below is the *active* tab's, which is what is on screen.
            tabs = self.tab_count,
            tab = self.tab_id,
            fps = self.health.frames.fps().unwrap_or(0.0) as i64,
            frame_avg_ms = self.frame_avg_ms,
            frame_cpu_ms = self.health.cpu_frames.avg_ms().unwrap_or(0.0),
            frame_worst_ms = self.health.frames.worst_ms().unwrap_or(0.0),
            feed_arrival_ms = self.feed_arrival_ms,
            trades_per_s = self.trades_per_s,
            live_trades = self.tab.live_trades,
            worker_backlog = self.envelope.backlog,
            worker_parked = self.envelope.parked,
            worker_deferred = self.envelope.deferred,
            worker_coalesced = self.envelope.coalesced,
            worker_output_blocked = self.envelope.output_blocked,
            retained_trades = self.envelope.retained_trades,
            live_rate = self.envelope.live_rate,
            bar_spec = self.tab.flow_pane.state.spec().summary(),
            // Both facts the tape states about its own prices — the grid they
            // land on and the magnitude they land at — and the row width the
            // ladder ended up drawing from them. A validation run reads all
            // three: the first two are the sizing rule's whole input, so
            // without them a run can see that rows are 1.00 and not why, and
            // cannot tell "the chart has not been told a tick yet" from "the
            // chart is drawing rows finer than the instrument can trade at".
            tape_price_step = self
                .tab
                .flow_pane
                .state
                .tape_price_step()
                .map_or_else(|| "unknown".to_owned(), |step| step.to_string()),
            tape_reference_price = self
                .tab
                .flow_pane
                .state
                .tape_reference_price()
                .map_or_else(|| "unknown".to_owned(), |price| price.to_string()),
            footprint_rows = %self.tab.flow_pane.state.footprint_group(),
            canvas_layout = ?self.tab.layout,
            screen_pt_w = self.geometry.screen.x,
            screen_pt_h = self.geometry.screen.y,
            client_px_w = self.geometry.client.map(|size| size.x),
            client_px_h = self.geometry.client.map(|size| size.y),
            scale = self.geometry.scale,
            native_scale = self.geometry.native_scale,
            zoom_factor = self.geometry.zoom,
            time_pane_spec = self.tab.time_pane().map(|pane| pane.state.spec().summary()),
            time_pane_count = self.tab.time_panes.len(),
            // Drawings are a per-frame, O(objects) paint cost, and the shared
            // ones are additionally reprojected on every other pane of the
            // tab. Counting them here is what lets a frame-cost reading be
            // attributed instead of guessed — and it is the only way a
            // headless run can prove the drawing overlay is populated at all.
            drawings = self
                .tab
                .panes()
                .map(|(pane, _)| pane.drawings.items().len())
                .sum::<usize>(),
            shared_drawings = self
                .tab
                .panes()
                .map(|(pane, _)| pane.drawings.shared_count())
                .sum::<usize>(),
            book_enabled = self.book.enabled,
            book_status = self.book.status,
            book_generation = self.book.generation,
            book_last_update_id = self.book.last_update_id,
            book_last_event_ms = self.book.last_event_ms,
            book_snapshot_observed_ms = self.book.last_snapshot_observed_ms,
            book_arrival_ms = self.book.arrival_latency_ms,
            // How far the newest print sits behind the instant the lane calls
            // now. It is the pixel gap between the last bubble and the tape's
            // right edge, in milliseconds: a number, so "the bubbles are
            // trailing" can be measured rather than argued about.
            //
            // A distance between two *venue* clocks, not staleness against
            // this machine's. A dead session stops both, so this figure
            // freezes rather than growing — `feed_arrival_ms` above is the one
            // that answers "is anything still arriving".
            tape_age_ms = self.book.tape_age.map(|age| match age {
                quantick_orderflow::TapeAge::Behind(ms) | quantick_orderflow::TapeAge::NothingYet(ms) => ms,
            }),
            tape_age_kind = self.book.tape_age.map(|age| match age {
                quantick_orderflow::TapeAge::Behind(_) => "behind",
                quantick_orderflow::TapeAge::NothingYet(_) => "nothing_yet",
            }),
            book_updates_per_s = self.book_updates_per_s,
            book_updates_total = self.book.depth_updates,
            book_queue_len = self.tab.book_events.len(),
            book_channel_closed = self.tab.book_channel_closed_reported,
            book_bid_levels = self.book.bid_levels,
            book_ask_levels = self.book.ask_levels,
            heatmap_active_levels = self.book.active_levels,
            heatmap_archived_runs = self.book.archived_runs,
            aggression_count = self.book.aggression_count,
            heatmap_history_bytes = self.book.history_bytes,
            heatmap_cells = self.book.projection_cells,
            heatmap_aggressions = self.book.projection_aggressions,
            heatmap_liquidity_events = self.book.projection_liquidity_events,
            heatmap_effective_grouping = %self.book.effective_grouping,
            heatmap_effective_grouping_multiple = self.book.effective_grouping_multiple,
            heatmap_dropped_cells = self.book.dropped_cells,
            heatmap_folded_aggressions = self.book.folded_aggressions,
            heatmap_dropped_liquidity_events = self.book.dropped_liquidity_events,
            heatmap_projection_ms = self.book.projection_ms,
            heatmap_live_ms = self.book.live_ms,
            heatmap_projection_builds = self.book.projection_builds,
            heatmap_projection_cache_hits = self.book.projection_cache_hits,
            heatmap_config_revision = self.book.config_revision,
            heatmap_snapshots = self.book.snapshots,
            heatmap_gaps = self.book.gaps,
            heatmap_aggressions_evicted = self.book.aggressions_evicted,
            heatmap_runs_evicted = self.book.runs_evicted,
            candle_style_revision = self.style_revision,
            candle_preset = self.candle_preset,
            candle_body_mode = ?self.style.candles.body_mode,
            candle_fill_opacity = self.style.candles.fill_opacity,
            candle_outline_opacity = self.style.candles.outline_opacity,
            candle_outline_width_px = self.style.candles.outline_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            replay_active = self.tab.replay.is_some(),
            replay_speed = self.tab.replay.as_ref().map(|r| r.status.speed()),
            replay_playing = self.tab.replay.as_ref().map(|r| r.status.is_playing()),
            replay_progress = self.tab.replay.as_ref().map(|r| r.status.progress()),
            replay_played = self.tab.replay.as_ref().map(|r| r.status.played()),
            replay_total = self.tab.replay.as_ref().map(|r| r.status.total()),
            action = "observe",
            "application health summary"
        );
    }

    /// The warnings the figures call for, each only past its threshold.
    fn warn_breaches(&self) {
        if self.frame_avg_ms > metrics::SLOW_FRAME_MS {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_SLOW_FRAMES",
                frame_avg_ms = self.frame_avg_ms,
                threshold_ms = metrics::SLOW_FRAME_MS,
                heatmap_enabled = self.book.enabled,
                heatmap_projection_ms = self.book.projection_ms,
                heatmap_cells = self.book.projection_cells,
                action = "inspect_render_budget",
                "slow frames: the chart is not keeping up"
            );
        }
        if let Some(l) = self.feed_arrival_ms
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_HIGH_TRADE_LAG",
                feed_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                action = "inspect_trade_connection",
                "high feed lag: trades are arriving well behind their timestamps"
            );
        }
        if let Some(l) = self.book.arrival_latency_ms
            && self.book.enabled
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_HIGH_ARRIVAL",
                symbol = self.tab.symbol.as_str(),
                book_arrival_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                book_status = self.book.status,
                action = "inspect_depth_connection",
                // Arrival, not age: this is how late the newest accepted
                // depth event was when it reached us, an observation frozen
                // at that moment. A book that stops updating keeps its last
                // figure — the tape-age readout is what catches that.
                "order-book events are arriving late"
            );
        }
        // Losses only. Folding is the expected steady state on a busy tape and
        // loses nothing — warning about it would tell an operator (and the
        // planned assistant reading these events) to go fix something that is
        // not broken. The fold count still rides in the info summary above.
        if self.book.dropped_cells > 0 || self.book.dropped_liquidity_events > 0 {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROJECTION_CAPPED",
                symbol = self.tab.symbol.as_str(),
                dropped_cells = self.book.dropped_cells,
                dropped_liquidity_events = self.book.dropped_liquidity_events,
                // Not "group harder". Grouping is exactly what the trader is
                // complaining about when marks read as one blob, and the
                // aggression budget no longer discards anything to begin with —
                // it folds, and says how much it folded. What is worth widening
                // is the budget or the pane, so that is what this names.
                action = "increase_grouping_or_reduce_retention",
                "heatmap depth primitive cap dropped items"
            );
        }
    }
}

mod envelope;
mod worker_diagnostics;

pub(super) fn emit_style_changed(
    style: &crate::style::ChartStyle,
    revision: u64,
    applied_preset: Option<CandlePreset>,
) {
    let candles = &style.candles;
    let preset = applied_preset
        .or_else(|| CandlePreset::detect(candles))
        .map_or("custom", CandlePreset::log_value);
    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "CANDLE_STYLE_CHANGED",
        revision = revision,
        preset,
        body_mode = ?candles.body_mode,
        fill_opacity = candles.fill_opacity,
        outline_opacity = candles.outline_opacity,
        outline_width_px = candles.outline_width,
        body_width_fraction = candles.body_width_frac,
        wick_mode = ?candles.wick_color_mode,
        wick_width_px = candles.wick_width,
        chart_background_enabled = style.canvas.background_enabled,
        chart_grid_enabled = style.canvas.grid_enabled,
        action = "redraw_only",
        "candle appearance changed"
    );
}
