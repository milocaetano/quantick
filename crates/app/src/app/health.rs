//! What the application says about its own health.
//!
//! [`super::QuantickApp::maybe_emit_summary`] is the `APP_HEALTH_SUMMARY`
//! line a validation run reads to decide whether a frame budget held, and
//! `status_model` is the same set of facts shaped for the status bar. They
//! sit together because they answer one question from one set of counters,
//! and because the log line and the bar must never be able to disagree.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::metrics;
use crate::statusbar;
use crate::style::CandlePreset;
use crate::window_scale;

use super::{QuantickApp, fmt_progress};

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);

impl QuantickApp {
    /// Periodically log a perf summary and warn on threshold breaches.
    pub(super) fn maybe_emit_summary(&mut self, now: Instant, ctx: &egui::Context) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        // The window's own geometry, because a chart that lays out wider than
        // the surface it is painted on loses its right edge — the toolbar's
        // layer group, the price axis, the live strip and the dock all live
        // there — and nothing else in this line would say so.
        //
        // `client_px` is the platform's *own* answer, not `screen * scale`:
        // that product is algebraically the same number as `screen_pt` beside
        // it and could never contradict anything, which would make the whole
        // block decoration. Two independent readings, so the line can be read
        // for whether they agree — and by the time it is written a correction
        // may already have restored that agreement, which `WINDOW_SCALE_CORRECTED`
        // is the record of.
        // Read outside the `input` closure: egui holds one lock for the whole
        // of it, and reaching back into the context from inside deadlocks.
        let zoom = ctx.zoom_factor();
        let client = self
            .surface
            .as_ref()
            .and_then(window_scale::SurfaceProbe::client_size_px);
        let (screen, scale, native_scale) = ctx.input(|input| {
            (
                input.screen_rect.size(),
                input.pixels_per_point(),
                input.viewport().native_pixels_per_point,
            )
        });
        let rate = self.trades_since_summary as f64 / elapsed.as_secs_f64();
        let lag = self.active_tab().trade_arrival_ms();
        let avg = self.frames.avg_ms().unwrap_or(0.0);
        let cpu_avg = self.cpu_frames.avg_ms().unwrap_or(0.0);
        let worst = self.frames.worst_ms().unwrap_or(0.0);
        let fps = self.frames.fps().unwrap_or(0.0);
        let book = self.active_tab_mut().tape_mut().health();
        let book_lag = book.arrival_latency_ms;
        let book_rate = book.depth_updates_since_summary as f64 / elapsed.as_secs_f64();
        let book_queue_len = self.active_tab().book_events.len();
        let candle_preset =
            CandlePreset::detect(&self.style.candles).map_or("custom", CandlePreset::log_value);

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "APP_HEALTH_SUMMARY",
            // Frames and the trade rate are the window's; every market figure
            // below is the *active* tab's, which is what is on screen.
            tabs = self.tabs.len(),
            tab = self.active_tab().id,
            fps = fps as i64,
            frame_avg_ms = avg,
            frame_cpu_ms = cpu_avg,
            frame_worst_ms = worst,
            feed_arrival_ms = lag,
            trades_per_s = rate,
            live_trades = self.active_tab().live_trades,
            bar_spec = self.active_tab().flow_pane.state.spec().summary(),
            // Both facts the tape states about its own prices — the grid they
            // land on and the magnitude they land at — and the row width the
            // ladder ended up drawing from them. A validation run reads all
            // three: the first two are the sizing rule's whole input, so
            // without them a run can see that rows are 1.00 and not why, and
            // cannot tell "the chart has not been told a tick yet" from "the
            // chart is drawing rows finer than the instrument can trade at".
            tape_price_step = self
                .active_tab()
                .flow_pane
                .state
                .tape_price_step()
                .map_or_else(|| "unknown".to_owned(), |step| step.to_string()),
            tape_reference_price = self
                .active_tab()
                .flow_pane
                .state
                .tape_reference_price()
                .map_or_else(|| "unknown".to_owned(), |price| price.to_string()),
            footprint_rows = %self.active_tab().flow_pane.state.footprint_group(),
            canvas_layout = ?self.active_tab().layout,
            screen_pt_w = screen.x,
            screen_pt_h = screen.y,
            client_px_w = client.map(|size| size.x),
            client_px_h = client.map(|size| size.y),
            scale = scale,
            native_scale = native_scale,
            zoom_factor = zoom,
            time_pane_spec = self.active_tab().time_pane().map(|pane| pane.state.spec().summary()),
            time_pane_count = self.active_tab().time_panes.len(),
            // Drawings are a per-frame, O(objects) paint cost, and the shared
            // ones are additionally reprojected on every other pane of the
            // tab. Counting them here is what lets a frame-cost reading be
            // attributed instead of guessed — and it is the only way a
            // headless run can prove the drawing overlay is populated at all.
            drawings = self
                .active_tab()
                .panes()
                .map(|(pane, _)| pane.drawings.items().len())
                .sum::<usize>(),
            shared_drawings = self
                .active_tab()
                .panes()
                .map(|(pane, _)| pane.drawings.shared_count())
                .sum::<usize>(),
            book_enabled = book.enabled,
            book_status = book.status,
            book_generation = book.generation,
            book_last_update_id = book.last_update_id,
            book_last_event_ms = book.last_event_ms,
            book_snapshot_observed_ms = book.last_snapshot_observed_ms,
            book_arrival_ms = book_lag,
            // How far the newest print sits behind the instant the lane calls
            // now. It is the pixel gap between the last bubble and the tape's
            // right edge, in milliseconds: a number, so "the bubbles are
            // trailing" can be measured rather than argued about.
            //
            // A distance between two *venue* clocks, not staleness against
            // this machine's. A dead session stops both, so this figure
            // freezes rather than growing — `feed_arrival_ms` above is the one
            // that answers "is anything still arriving".
            tape_age_ms = book.tape_age.map(|age| match age {
                quantick_orderflow::TapeAge::Behind(ms) | quantick_orderflow::TapeAge::NothingYet(ms) => ms,
            }),
            tape_age_kind = book.tape_age.map(|age| match age {
                quantick_orderflow::TapeAge::Behind(_) => "behind",
                quantick_orderflow::TapeAge::NothingYet(_) => "nothing_yet",
            }),
            book_updates_per_s = book_rate,
            book_updates_total = book.depth_updates,
            book_queue_len,
            book_channel_closed = self.active_tab().book_channel_closed_reported,
            book_bid_levels = book.bid_levels,
            book_ask_levels = book.ask_levels,
            heatmap_active_levels = book.active_levels,
            heatmap_archived_runs = book.archived_runs,
            aggression_count = book.aggression_count,
            heatmap_history_bytes = book.history_bytes,
            heatmap_cells = book.projection_cells,
            heatmap_aggressions = book.projection_aggressions,
            heatmap_liquidity_events = book.projection_liquidity_events,
            heatmap_effective_grouping = %book.effective_grouping,
            heatmap_effective_grouping_multiple = book.effective_grouping_multiple,
            heatmap_dropped_cells = book.dropped_cells,
            heatmap_folded_aggressions = book.folded_aggressions,
            heatmap_dropped_liquidity_events = book.dropped_liquidity_events,
            heatmap_projection_ms = book.projection_ms,
            heatmap_live_ms = book.live_ms,
            heatmap_projection_builds = book.projection_builds,
            heatmap_projection_cache_hits = book.projection_cache_hits,
            heatmap_config_revision = book.config_revision,
            heatmap_snapshots = book.snapshots,
            heatmap_gaps = book.gaps,
            candle_style_revision = self.style_revision,
            candle_preset,
            candle_body_mode = ?self.style.candles.body_mode,
            candle_fill_opacity = self.style.candles.fill_opacity,
            candle_outline_opacity = self.style.candles.outline_opacity,
            candle_outline_width_px = self.style.candles.outline_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            replay_active = self.active_tab().replay.is_some(),
            replay_speed = self.active_tab().replay.as_ref().map(|r| r.status.speed()),
            replay_playing = self.active_tab().replay.as_ref().map(|r| r.status.is_playing()),
            replay_progress = self.active_tab().replay.as_ref().map(|r| r.status.progress()),
            replay_played = self.active_tab().replay.as_ref().map(|r| r.status.played()),
            replay_total = self.active_tab().replay.as_ref().map(|r| r.status.total()),
            action = "observe",
            "application health summary"
        );
        if avg > metrics::SLOW_FRAME_MS {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_SLOW_FRAMES",
                frame_avg_ms = avg,
                threshold_ms = metrics::SLOW_FRAME_MS,
                heatmap_enabled = book.enabled,
                heatmap_projection_ms = book.projection_ms,
                heatmap_cells = book.projection_cells,
                action = "inspect_render_budget",
                "slow frames: the chart is not keeping up"
            );
        }
        if let Some(l) = lag
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
        if let Some(l) = book_lag
            && book.enabled
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_HIGH_ARRIVAL",
                symbol = self.active_tab().symbol.as_str(),
                book_arrival_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                book_status = book.status,
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
        if book.dropped_cells > 0 || book.dropped_liquidity_events > 0 {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROJECTION_CAPPED",
                symbol = self.active_tab().symbol.as_str(),
                dropped_cells = book.dropped_cells,
                dropped_liquidity_events = book.dropped_liquidity_events,
                // Not "group harder". Grouping is exactly what the trader is
                // complaining about when marks read as one blob, and the
                // aggression budget no longer discards anything to begin with —
                // it folds, and says how much it folded. What is worth widening
                // is the budget or the pane, so that is what this names.
                action = "increase_grouping_or_reduce_retention",
                "heatmap depth primitive cap dropped items"
            );
        }

        self.trades_since_summary = 0;
        self.active_tab_mut().tape_mut().reset_summary_counters();
        self.last_summary = now;
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
            fps: self.frames.fps(),
            frame_avg_ms: self.frames.avg_ms(),
            frame_cpu_ms: self.cpu_frames.avg_ms(),
            show_perf: self.show_perf,
        }
    }
}
