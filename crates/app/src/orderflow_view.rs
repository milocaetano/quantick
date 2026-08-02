//! egui facade for the asynchronous order-flow heatmap.
//!
//! All book state (history, synchronization, projection) lives in
//! [`crate::orderflow_engine::BookEngine`] on the worker thread owned by
//! [`crate::orderflow_worker::BookWorker`]. This layer only forwards commands,
//! mirrors the published snapshot for the current frame and converts
//! normalized primitives into egui shapes. Nothing here can block the UI on a
//! dense book: drawing always uses the latest already-built frame.

use std::sync::Arc;

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::{Bar, Trade};
use quantick_orderbook::{BookLevel, DepthEvent};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::bubble_presets::{self, BubblePreset, BubblePresetFile, PresetSource};
use crate::chart::PriceScale;
use crate::live_strip;
use crate::orderflow::projection::normalized_area_size;
use crate::orderflow::{
    BubbleRenderMode, BubbleSizeReference, DisplayGrouping, HeatmapConfig, HeatmapTheme,
    IntensityMode, MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_MIN_RADIUS, MAX_LIVE_LANE_RADIUS_SCALE,
    MAX_LIVE_LANE_SHARE, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_MAX_RADIUS, MIN_LIVE_LANE_RADIUS_SCALE,
    MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_ZOOM, reserved_span_ms,
};
use crate::orderflow_engine::{
    BookPublished, CaptureStatus, OrderflowHealth, ProjectionRequest, VisibleOrderflow,
};
use crate::orderflow_render::{
    OrderflowRenderStyle, ProjectedLayout, RenderContext, draw_aggression_bubbles,
    draw_compact_legend, draw_heatmap_background, draw_liquidity_events, draw_live_lane_marks,
    draw_preview, theme_bubble_rgb,
};
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::viewport::Viewport;

fn status_color(status: &CaptureStatus) -> egui::Color32 {
    match status {
        CaptureStatus::Live { .. } => crate::theme::BUY,
        CaptureStatus::Connecting | CaptureStatus::Buffering | CaptureStatus::SnapshotFetching => {
            crate::theme::AMBER
        }
        CaptureStatus::Disabled => crate::theme::TEXT_MUTED,
        CaptureStatus::Resyncing { .. }
        | CaptureStatus::Disconnected { .. }
        | CaptureStatus::Error => crate::theme::WARN,
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
    capture_grouping_draft: f64,
    pending_capture_grouping_previous: Option<Decimal>,
    /// Named bubble looks, loaded from the versionable presets file.
    presets: BubblePresetFile,
    /// Where those presets came from, shown in the panel.
    presets_source: PresetSource,
    /// Name being typed for the next save.
    preset_name_draft: String,
    /// Last preset action (or failure), shown verbatim in the panel.
    preset_status: Option<String>,
}

impl OrderflowView {
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        let mut config = HeatmapConfig::default();
        // The presets file is the record of how the tape should look, so the
        // chart opens on its active preset instead of the compiled defaults.
        let (presets, presets_source, load_error) = bubble_presets::load();
        let mut preset_status = None;
        if let Some(message) = load_error {
            tracing::error!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "BUBBLE_PRESETS_UNREADABLE",
                error = message.as_str(),
                action = "using_built_in_presets",
                "bubble presets file could not be read; built-in presets are in use"
            );
            preset_status = Some(format!("presets not loaded — {message}"));
        }
        if let Some(active) = presets.get(&presets.active) {
            active.apply_to(&mut config);
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BUBBLE_PRESETS_LOADED",
            source = %presets_source,
            stored = presets.presets.len(),
            active = presets.active.as_str(),
            "bubble presets resolved"
        );
        let preset_name_draft = presets.active.clone();
        let base_grouping = config.price_grouping;
        Self {
            worker: BookWorker::spawn(&symbol),
            symbol,
            config,
            published: BookPublished::initial(),
            last_seen_base: base_grouping,
            capture_grouping_draft: base_grouping.to_f64().unwrap_or(0.01),
            pending_capture_grouping_previous: None,
            presets,
            presets_source,
            preset_name_draft,
            preset_status,
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

    /// Whether L2 depth capture is recording. Says nothing about whether the
    /// map is on screen — see [`depth_visible`](Self::depth_visible).
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Whether the depth map is drawn: recording *and* not hidden.
    #[must_use]
    pub fn depth_visible(&self) -> bool {
        self.config.depth_visible()
    }

    /// Show or hide the depth map without touching L2 capture.
    ///
    /// No feed command is involved: the recorder keeps running, so turning the
    /// map back on repaints the retained past instead of opening a gap in it.
    pub fn set_depth_visible(&mut self, visible: bool) {
        if self.config.show_depth == visible {
            return;
        }
        let before = self.config.clone();
        self.config.show_depth = visible;
        if !visible {
            // Drop the local frame immediately; the worker clears its own.
            self.published.frame = None;
        }
        self.commit_config_changes(before);
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

    /// Latest exchange timestamp for which live book state is known, while the
    /// map is on screen. Marks the live edge inside the forming bar's lane.
    #[must_use]
    pub fn live_end_ms(&mut self) -> Option<i64> {
        if !self.config.depth_visible() {
            return None;
        }
        self.sync_published();
        self.published.live_end_ms
    }

    /// Width in pixels of the live lane's pane, taken off the right edge of a
    /// chart this wide.
    ///
    /// A pane, not a slot: the candles own everything left of it and the lane
    /// owns this band whatever they do. Panning or zooming them changes how
    /// many bars fit beside the tape and never the tape itself, which is what
    /// keeps the most recent prints on screen through every chart movement.
    /// `None` when there is no live edge to run to.
    #[must_use]
    pub fn live_lane_width_px(&mut self, chart_width: f32) -> Option<f32> {
        self.live_end_ms()?;
        Some(self.config.live_lane.resolved_width_px(chart_width))
    }

    /// Widen or narrow the lane by a pixel drag on its divider. Dragging left
    /// (negative `delta_px`) gives the tape more room, at the expense of the
    /// history beside it.
    pub fn resize_live_lane(&mut self, delta_px: f32, chart_width: f32) {
        if !delta_px.is_finite() || !chart_width.is_finite() || chart_width <= 0.0 {
            return;
        }
        let before = self.config.clone();
        let width = self.config.live_lane.resolved_width_px(chart_width) - delta_px;
        self.config.live_lane.width_share = width / chart_width;
        self.commit_config_changes(before);
    }

    /// Zoom the lane's time window by a multiplicative factor: `> 1` shows
    /// less market time in the same band (prints run faster and further
    /// apart), `< 1` shows more (prints crowd together and cluster).
    pub fn zoom_live_lane(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.time_zoom *= factor;
        self.commit_config_changes(before);
    }

    /// Market time the lane is showing right now, in milliseconds — the label
    /// under it, and the only readout of what the zoom is worth.
    #[must_use]
    pub fn live_lane_window_ms(&self, closed: &[Bar]) -> i64 {
        self.config.live_lane.window_ms(reserved_span_ms(closed))
    }

    /// Name of the preset the panel currently wears.
    #[cfg(test)]
    pub(crate) fn active_preset_for_test(&self) -> &str {
        &self.presets.active
    }

    /// Read-only view of the heatmap config, for app-level assertions.
    #[cfg(test)]
    pub(crate) fn config_for_test(&self) -> &HeatmapConfig {
        &self.config
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

    /// Forward one feed event and its UI observation time to the book thread.
    /// Generation and symbol filtering happen engine-side; only an accepted
    /// timestamped event becomes a latency observation.
    pub fn handle_depth_event_at(&mut self, event: DepthEvent, received_at_ms: i64) {
        self.worker.send(BookCommand::Depth {
            event,
            received_at_ms,
        });
    }

    /// Deterministic shorthand for tests that do not inspect arrival latency.
    #[cfg(test)]
    pub fn handle_depth_event(&mut self, event: DepthEvent) {
        let received_at_ms = match &event {
            DepthEvent::Snapshot { observed_at_ms, .. } => *observed_at_ms,
            DepthEvent::Update { event_time_ms, .. } => *event_time_ms,
            DepthEvent::Status { .. } => 0,
        };
        self.handle_depth_event_at(event, received_at_ms);
    }

    /// Request projection of the visible bar slice and return the newest
    /// already-built frame. Never blocks: a heavy projection only delays the
    /// next frame swap, not the UI.
    pub fn project_visible(
        &mut self,
        timeline_revision: u64,
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        lane: bool,
        on_newest_bar: bool,
        price_range: (f64, f64),
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        let request = ProjectionRequest {
            timeline_revision,
            first_bar_index,
            closed: closed.to_vec(),
            partial: partial.cloned(),
            lane,
            on_newest_bar,
            price_range,
        };
        // Every frame, with no gate of its own. The worker coalesces requests
        // latest-wins and decides for itself what is worth rebuilding, so the
        // only thing a gate here could add is a bar snapshot older than the
        // prints it is supposed to place — which is how a fresh print ends up
        // outside the timeline and drawn nowhere.
        self.worker.send(BookCommand::Project(request));
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
        lane_width_px: f32,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        );
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_heatmap_background(painter, &context);
        draw_live_lane_marks(painter, &context);
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
        lane_width_px: f32,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        );
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_aggression_bubbles(painter, &context);
        draw_compact_legend(painter, &context);
    }

    pub fn draw_status_badge(&self, painter: &egui::Painter, chart_rect: egui::Rect) {
        // Tied to the map, not to the recorder: a badge reporting on a book
        // nobody asked to see is just chrome.
        if !self.config.depth_visible() {
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

    /// Draw the live strip: the forming bar's aggression histogram, buys
    /// growing rightward from the centre and sells leftward, on the bubbles'
    /// square-root area rule normalized by the bar itself — it resets on bar
    /// close because the new bar has no clusters yet. The published ladder
    /// contributes only the best bid/ask touch lines (the real spread); the
    /// depth silhouette was retired after live use — it repeated the
    /// heatmap's right edge and buried the histogram. `bar_open_ms` is the
    /// forming bar's open time (`None` hides the histogram); `scale` is the
    /// chart's own price scale, so everything lines up 1:1 with the chart.
    pub fn draw_live_strip(
        &mut self,
        painter: &egui::Painter,
        strip: egui::Rect,
        scale: &PriceScale,
        canvas_background: egui::Color32,
        bar_open_ms: Option<i64>,
    ) {
        self.sync_published();
        painter.rect_filled(strip, egui::Rounding::ZERO, canvas_background);
        painter.line_segment(
            [strip.left_top(), strip.left_bottom()],
            egui::Stroke::new(
                1.0_f32,
                crate::theme::TEXT_MUTED.gamma_multiply(live_strip::STRIP_BORDER_ALPHA),
            ),
        );

        let ladder = self.published.ladder.clone();
        let frame = self.published.frame.clone();
        let colors = theme_bubble_rgb(self.config.theme);
        let clip = painter.with_clip_rect(strip);
        let rows_left = strip.left() + live_strip::STRIP_ROW_INSET_PX;

        // The forming bar's mirrored aggression histogram, from the same
        // projection clusters the bubbles draw — one engine, one aggregation
        // path. Empty whenever those layers publish nothing.
        let histogram = match (frame.as_deref(), bar_open_ms) {
            (Some(frame), Some(open_ms)) => {
                live_strip::aggression_rows(&frame.projection.aggressions, open_ms)
            }
            _ => Vec::new(),
        };
        if !histogram.is_empty() {
            let bucket_width = frame
                .as_deref()
                .map(|frame| frame.projection.effective_grouping.bucket_width)
                .unwrap_or(self.published.base_price_grouping);
            let reference = live_strip::histogram_reference(&histogram);
            let centre_x = f32::midpoint(rows_left, strip.right());
            let half_width =
                (strip.right() - rows_left) / 2.0 * live_strip::HISTOGRAM_MAX_HALF_FRAC;
            let buy_fill = egui::Color32::from_rgb(colors.buy[0], colors.buy[1], colors.buy[2])
                .gamma_multiply(live_strip::HISTOGRAM_ALPHA);
            let sell_fill = egui::Color32::from_rgb(colors.sell[0], colors.sell[1], colors.sell[2])
                .gamma_multiply(live_strip::HISTOGRAM_ALPHA);
            for row in &histogram {
                let top = scale.y((row.price_bucket + bucket_width)
                    .to_f64()
                    .unwrap_or(f64::NAN));
                let bottom = scale.y(row.price_bucket.to_f64().unwrap_or(f64::NAN));
                if !top.is_finite() || !bottom.is_finite() {
                    continue;
                }
                let buy_extent = normalized_area_size(row.buy, reference) * half_width;
                if buy_extent > 0.0 {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(centre_x, top),
                        egui::pos2(centre_x + buy_extent, bottom),
                    )
                    .intersect(strip);
                    if rect.is_positive() {
                        clip.rect_filled(rect, egui::Rounding::ZERO, buy_fill);
                    }
                }
                let sell_extent = normalized_area_size(row.sell, reference) * half_width;
                if sell_extent > 0.0 {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(centre_x - sell_extent, top),
                        egui::pos2(centre_x, bottom),
                    )
                    .intersect(strip);
                    if rect.is_positive() {
                        clip.rect_filled(rect, egui::Rounding::ZERO, sell_fill);
                    }
                }
            }
        }

        // Touch markers last, readable over both layers.
        if let Some(ladder) = &ladder {
            let mark = |price: Option<Decimal>, rgb: [u8; 3]| {
                let Some(price) = price.and_then(|price| price.to_f64()) else {
                    return;
                };
                let y = scale.y(price);
                if !y.is_finite() {
                    return;
                }
                clip.line_segment(
                    [egui::pos2(rows_left, y), egui::pos2(strip.right(), y)],
                    egui::Stroke::new(
                        live_strip::TOUCH_MARKER_STROKE_PX,
                        egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    ),
                );
            };
            mark(ladder.best_ask.map(BookLevel::price), colors.sell);
            mark(ladder.best_bid.map(BookLevel::price), colors.buy);
        }

        // Honest empty state: the strip is on, no data source has anything —
        // no book (capture off or no snapshot) and no forming-bar aggression.
        if ladder.is_none() && histogram.is_empty() {
            painter.text(
                egui::pos2(strip.center().x, strip.top() + 10.0),
                egui::Align2::CENTER_CENTER,
                "no book",
                egui::FontId::proportional(10.0),
                crate::theme::TEXT_MUTED,
            );
        }
    }

    pub fn health(&mut self) -> OrderflowHealth {
        self.sync_published();
        self.published.health.clone()
    }

    /// Whether the map is open but not yet (or no longer) backed by a live
    /// book. What the app's loading overlay mirrors. Reads the frame's mirror,
    /// refreshed by the panel/projection calls the frame already made; which
    /// statuses count as a wait is [`CaptureStatus::is_syncing`]'s call.
    ///
    /// Visibility-gated: the recorder synchronizing in the background is not
    /// something to hold a loading overlay up for.
    #[must_use]
    pub fn is_syncing(&self) -> bool {
        self.config.depth_visible() && self.published.status.is_syncing()
    }

    pub fn reset_summary_counters(&mut self) {
        self.worker.send(BookCommand::ResetSummaryCounters);
    }

    /// Picker, save and reload for the named bubble looks.
    ///
    /// Saving writes the whole presets file, so what the panel shows and what
    /// the repository holds never drift apart.
    fn draw_bubble_presets(&mut self, ui: &mut egui::Ui) {
        // The picker reads the stored presets while the closure below wants to
        // mutate them, so it hands back an index and the name is read after.
        // Cloning every name each frame would be the other way out, and this
        // runs on the render thread.
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("preset");
            let selected = if self.presets.active.is_empty() {
                "— custom —"
            } else {
                self.presets.active.as_str()
            };
            egui::ComboBox::from_id_salt("bubble_preset")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (index, preset) in self.presets.presets.iter().enumerate() {
                        if ui
                            .selectable_label(self.presets.active == preset.name, &preset.name)
                            .clicked()
                        {
                            chosen = Some(index);
                        }
                    }
                });
            if ui
                .small_button(icons::ARROW_CLOCKWISE)
                .on_hover_text("reload the presets file from disk, discarding unsaved tweaks")
                .clicked()
            {
                self.reload_presets();
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_name_draft)
                    .hint_text("preset name")
                    .desired_width(150.0),
            );
            if ui
                .button("save")
                .on_hover_text("write the current bubble settings to the presets file")
                .clicked()
            {
                self.save_preset();
            }
            let removable = !self.preset_name_draft.trim().is_empty()
                && self.presets.get(self.preset_name_draft.trim()).is_some();
            if ui
                .add_enabled(removable, egui::Button::new("delete"))
                .on_hover_text("remove this preset from the file")
                .clicked()
            {
                let name = self.preset_name_draft.trim().to_owned();
                self.presets.remove(&name);
                self.persist_presets(format!("preset '{name}' removed"));
            }
        });
        if let Some(index) = chosen
            && let Some(name) = self
                .presets
                .presets
                .get(index)
                .map(|preset| preset.name.clone())
        {
            self.apply_preset(&name);
        }
        ui.small(format!("presets · {}", self.presets_source));
        if let Some(status) = &self.preset_status {
            ui.small(status.clone());
        }
    }

    /// Apply the stored preset called `name`, reporting whether it exists.
    ///
    /// The panel's picker and a feed's declared preset both land here, so a
    /// preset applies identically no matter who asked. An unknown name changes
    /// nothing and returns `false`; the caller decides how loudly to say so.
    pub(crate) fn apply_preset(&mut self, name: &str) -> bool {
        let Some(preset) = self.presets.get(name).cloned() else {
            return false;
        };
        preset.apply_to(&mut self.config);
        self.presets.active = preset.name.clone();
        self.preset_name_draft = preset.name.clone();
        self.preset_status = Some(format!("'{}' applied", preset.name));
        true
    }

    fn save_preset(&mut self) {
        let name = self.preset_name_draft.trim().to_owned();
        if name.is_empty() {
            self.preset_status = Some("name the preset before saving".to_owned());
            return;
        }
        self.presets
            .upsert(BubblePreset::capture(&name, &self.config));
        self.presets.active = name.clone();
        self.persist_presets(format!("'{name}' saved"));
    }

    fn persist_presets(&mut self, success: String) {
        match bubble_presets::save(&self.presets) {
            Ok(path) => {
                self.presets_source = PresetSource::WorkingDir(path.clone());
                self.preset_status = Some(format!("{success} → {}", path.display()));
            }
            Err(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_NOT_SAVED",
                    error = message.as_str(),
                    action = "keep_settings_in_memory_only",
                    "bubble presets could not be written; the current look is in memory only"
                );
                self.preset_status = Some(format!("not saved — {message}"));
            }
        }
    }

    fn reload_presets(&mut self) {
        let (presets, source, error) = bubble_presets::load();
        self.presets = presets;
        self.presets_source = source;
        match error {
            Some(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_UNREADABLE",
                    error = message.as_str(),
                    action = "using_built_in_presets",
                    "bubble presets file could not be read; built-in presets are in use"
                );
                self.preset_status = Some(format!("presets not loaded — {message}"));
            }
            None => {
                let active = self.presets.active.clone();
                if active.is_empty() {
                    self.preset_status = Some("presets reloaded".to_owned());
                } else {
                    self.apply_preset(&active);
                    self.preset_status = Some(format!("reloaded · '{active}' applied"));
                }
            }
        }
    }

    /// Every visual choice for the bubbles themselves, grouped so the section
    /// stays readable: size and placement, the marks a consuming print leaves,
    /// labels, and colour.
    /// The live lane's own settings: the reserved band right of the forming
    /// bar, which has room the compressed history does not.
    fn draw_live_lane_controls(&mut self, ui: &mut egui::Ui) {
        let inherited = self.config.bubble_cluster_ms;
        let lane = &mut self.config.live_lane;

        egui::CollapsingHeader::new("live lane")
            .id_salt("bubble_live_lane_section")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("width");
                    ui.add(
                        egui::Slider::new(
                            &mut lane.width_share,
                            MIN_LIVE_LANE_SHARE..=MAX_LIVE_LANE_SHARE,
                        )
                        .custom_formatter(|value, _| format!("{:.0}% of the chart", value * 100.0)),
                    );
                })
                .response
                .on_hover_text(
                    "how much of the chart the rolling tape takes, up to half of it. Also set by dragging the divider on the chart; measured against the chart, not the candle, so zooming the time axis changes how many bars fit beside the tape and never how much room it gets",
                );
                ui.horizontal(|ui| {
                    ui.label("time zoom");
                    ui.add(
                        egui::Slider::new(
                            &mut lane.time_zoom,
                            MIN_LIVE_LANE_ZOOM..=MAX_LIVE_LANE_ZOOM,
                        )
                        .logarithmic(true)
                        .suffix("×"),
                    );
                })
                .response
                .on_hover_text(
                    "how much market time fits in the tape, against the recent bars' typical duration. Zoom in and prints run across it faster and further apart; zoom out and more time crowds in — the clustering window follows, so they gather into fewer, bigger bubbles. Also set by dragging the time strip under the tape",
                );
                ui.horizontal(|ui| {
                    ui.label("cluster");
                    egui::ComboBox::from_id_salt("bubble_live_lane_cluster")
                        .selected_text(lane_cluster_label(lane.cluster_ms, inherited))
                        .show_ui(ui, |ui| {
                            for window in [None, Some(0), Some(50), Some(100), Some(200), Some(500)]
                            {
                                ui.selectable_value(
                                    &mut lane.cluster_ms,
                                    window,
                                    lane_cluster_label(window, inherited),
                                );
                            }
                        });
                })
                .response
                .on_hover_text(
                    "clustering window for prints on the tape. A shorter one than history's buys detail where there is room for it; \"same as history\" keeps the two regions identical",
                );
                ui.horizontal(|ui| {
                    ui.label("bubble size");
                    ui.add(
                        egui::Slider::new(
                            &mut lane.radius_scale,
                            MIN_LIVE_LANE_RADIUS_SCALE..=MAX_LIVE_LANE_RADIUS_SCALE,
                        )
                        .suffix("×"),
                    );
                })
                .response
                .on_hover_text(
                    "multiplies both bubble radii inside the lane only. The lane is the one region with room to spare, so a wider range reads as detail here and as overlap anywhere else",
                );
                ui.checkbox(&mut lane.show_marks, "boundary and live-edge line")
                    .on_hover_text(
                        "the dashed line where the bar slots end and the tape begins, and the line on the live edge itself at its right end",
                    );
            });
    }

    fn draw_bubble_controls(&mut self, ui: &mut egui::Ui) {
        let theme_rgb = theme_bubble_rgb(self.config.theme);
        let bubbles = &mut self.config.bubbles;

        egui::CollapsingHeader::new("size & placement")
            .id_salt("bubble_size_section")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("render style");
                    egui::ComboBox::from_id_salt("bubble_render_mode")
                        .selected_text(render_mode_label(bubbles.render_mode))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut bubbles.render_mode,
                                BubbleRenderMode::Flat,
                                "Flat · 2D disc",
                            );
                            ui.selectable_value(
                                &mut bubbles.render_mode,
                                BubbleRenderMode::Sphere,
                                "Sphere · 3D shaded",
                            );
                        })
                        .response
                        .on_hover_text(
                            "flat is the classic solid disc. Sphere shades every bubble like a \
                             ball lit from the upper left (the Bookmap look): on a dense tape \
                             each darkened rim keeps overlapping prints readable as separate \
                             bubbles instead of one merged blob. Purely visual — clustering and \
                             liquidity association do not change.",
                        );
                });
                if bubbles.render_mode == BubbleRenderMode::Sphere {
                    ui.add(
                        egui::Slider::new(&mut bubbles.sphere_shading, 0.0..=1.0)
                            .text("depth shading"),
                    )
                    .on_hover_text(
                        "how much the rim darkens toward the edge; higher separates \
                         overlapping bubbles harder, zero reads flat again",
                    );
                    ui.add(
                        egui::Slider::new(&mut bubbles.sphere_highlight, 0.0..=1.0)
                            .text("highlight"),
                    )
                    .on_hover_text("strength of the light spot that gives the ball its volume");
                    ui.small(
                        "Sphere shading applies from the 'detail from px' radius up; smaller \
                         prints stay cheap dots.",
                    );
                }
                ui.add(
                    egui::Slider::new(&mut bubbles.min_radius, 0.5..=MAX_BUBBLE_MIN_RADIUS)
                        .text("smallest print px"),
                )
                .on_hover_text("floor radius, so the quietest print is still a visible dot");
                ui.add(
                    egui::Slider::new(
                        &mut bubbles.max_radius,
                        MIN_BUBBLE_MAX_RADIUS..=MAX_BUBBLE_MAX_RADIUS,
                    )
                    .text("biggest print px"),
                )
                .on_hover_text(
                    "radius of a full-size print; bubble *area* stays proportional to \
                         quantity between the two limits",
                );
                if bubbles.max_radius < bubbles.min_radius {
                    bubbles.max_radius = bubbles.min_radius;
                }
                ui.horizontal(|ui| {
                    ui.label("full size at");
                    egui::ComboBox::from_id_salt("bubble_size_reference")
                        .selected_text(size_reference_label(bubbles.size_reference))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut bubbles.size_reference,
                                BubbleSizeReference::VisibleP99,
                                "Auto · visible P99",
                            );
                            ui.selectable_value(
                                &mut bubbles.size_reference,
                                BubbleSizeReference::VisibleMax,
                                "Auto · largest visible",
                            );
                            ui.selectable_value(
                                &mut bubbles.size_reference,
                                BubbleSizeReference::Fixed,
                                "Fixed quantity",
                            );
                        })
                        .response
                        .on_hover_text(
                            "P99 keeps one outlier sweep from shrinking everything else, but \
                             the top 1% all render at the maximum radius. 'Largest visible' \
                             restores a strict order; a fixed quantity makes bubble size mean \
                             the same thing across sessions.",
                        );
                });
                if bubbles.size_reference == BubbleSizeReference::Fixed {
                    ui.add(
                        egui::DragValue::new(&mut bubbles.size_reference_quantity)
                            .range(0.000_001..=1_000_000_000.0)
                            .speed(1.0)
                            .prefix("full size qty "),
                    )
                    .on_hover_text("quantity drawn at the maximum radius, in the symbol's units");
                }
                ui.add(
                    egui::DragValue::new(&mut bubbles.min_quantity)
                        .range(0.0..=1_000_000_000.0)
                        .speed(0.5)
                        .prefix("hide below qty "),
                )
                .on_hover_text(
                    "display-only floor: smaller prints are not drawn. Applied after liquidity \
                     association, so a hidden print still counts as the evidence behind a \
                     consumption mark. Zero draws everything.",
                );
                ui.add(
                    egui::Slider::new(&mut bubbles.side_offset, 0.0..=20.0)
                        .text("side separation px"),
                )
                .on_hover_text(
                    "buy bubbles are nudged up, sell bubbles down: a buy lifts the ask, a sell \
                     hits the bid, so they are not on the same row. With a one-tick spread they \
                     would otherwise stack into an unreadable line. Zero pins both to the exact \
                     price.",
                );
                ui.add(egui::Slider::new(&mut bubbles.opacity, 0.05..=1.0).text("fill opacity"));
                ui.add(
                    egui::Slider::new(&mut bubbles.outline_width, 0.0..=4.0).text("rim width px"),
                )
                .on_hover_text("zero draws no rim");
                ui.add(egui::Slider::new(&mut bubbles.halo_strength, 0.0..=0.6).text("halo"))
                    .on_hover_text("soft glow behind the fill; opens up a little with size");
                ui.add(
                    egui::Slider::new(&mut bubbles.detail_min_radius, 0.0..=20.0)
                        .text("detail from px"),
                )
                .on_hover_text(
                    "bubbles smaller than this are plain dots (no halo, rim or impact ring). \
                     Raising it buys frame time on a fast tape.",
                );
                ui.add(
                    egui::Slider::new(&mut bubbles.readable_min_radius, 0.0..=24.0)
                        .text("readable from px"),
                )
                .on_hover_text(
                    "the size at which a bubble stops being readable on its own. Prints below \
                     it are what \"fold dust\" merges, and what the ring below marks. Raise it \
                     for fewer, larger bubbles; zero disables both.",
                );
                ui.checkbox(&mut bubbles.hollow_small_buys, "hollow small buys")
                    .on_hover_text(
                        "draw buy prints below the readable radius as an open ring instead of \
                         a solid dot. At that size a green speck and a red speck read the same; \
                         a ring and a disc do not. Larger bubbles keep their fill.",
                    );
            });

        egui::CollapsingHeader::new("consumption marks")
            .id_salt("bubble_consumption_section")
            .default_open(true)
            .show(ui, |ui| {
                ui.small(
                    "Drawn only when a print aligned with a factual L2 reduction — the bubble \
                     ate resting liquidity.",
                );
                ui.checkbox(
                    &mut bubbles.show_consumption_front,
                    "vertical front through the bubble",
                );
                ui.add_enabled(
                    bubbles.show_consumption_front,
                    egui::Slider::new(&mut bubbles.front_width, 0.5..=10.0).text("front width px"),
                );
                ui.add_enabled(
                    bubbles.show_consumption_front,
                    egui::Slider::new(&mut bubbles.front_length_scale, 0.5..=6.0)
                        .text("front length × radius"),
                );
                ui.checkbox(&mut bubbles.show_impact_ring, "impact ring on the rim");
                ui.add_enabled(
                    bubbles.show_impact_ring,
                    egui::Slider::new(&mut bubbles.impact_ring_width, 0.5..=6.0)
                        .text("ring width px"),
                )
                .on_hover_text("brightness of the ring also tracks how much of the print matched");
                ui.add(
                    egui::Slider::new(&mut bubbles.trail_length, 0.0..=80.0)
                        .text("trail length px"),
                )
                .on_hover_text(
                    "the glow leaking into the consumed side, marking where the wall ended; \
                     zero draws no trail",
                );
                ui.add_enabled(
                    bubbles.trail_length > 0.0,
                    egui::Slider::new(&mut bubbles.trail_opacity, 0.0..=1.0).text("trail opacity"),
                );
            });

        egui::CollapsingHeader::new("labels")
            .id_salt("bubble_label_section")
            .show(ui, |ui| {
                ui.checkbox(
                    &mut bubbles.show_quantity_labels,
                    "quantity inside the bubble",
                );
                ui.checkbox(&mut bubbles.show_trade_count, "×N clustered prints");
                ui.add(
                    egui::Slider::new(&mut bubbles.label_min_radius, 4.0..=48.0)
                        .text("label from px"),
                )
                .on_hover_text(
                    "only bubbles this big get a label, and only when the text fits inside",
                );
            });

        egui::CollapsingHeader::new("colours")
            .id_salt("bubble_colour_section")
            .show(ui, |ui| {
                let front_fallback = bubbles.front_color.unwrap_or(theme_rgb.front);
                color_override(ui, "buy", &mut bubbles.buy_color, theme_rgb.buy);
                color_override(ui, "sell", &mut bubbles.sell_color, theme_rgb.sell);
                color_override(
                    ui,
                    "consumption front",
                    &mut bubbles.front_color,
                    theme_rgb.front,
                );
                color_override(ui, "trail", &mut bubbles.trail_color, front_fallback);
                color_override(ui, "label", &mut bubbles.label_color, theme_rgb.text);
                ui.small("Unset colours follow the chart theme.");
            });
    }

    /// The L2 dock tab's body: everything the depth map owns. Returns
    /// whether capture must restart because the base capture resolution
    /// changed.
    ///
    /// The layer's *toggle* lives in the toolbar; this tab is settings only —
    /// opening it never starts capture (looking is not enabling).
    pub fn draw_l2_tab(&mut self, ui: &mut egui::Ui) -> bool {
        self.sync_published();
        let before = self.config.clone();
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
                ui.strong("visual layers");
                ui.small(
                    "One switch per legend entry. Display-only: capture keeps running and \
                     history keeps accumulating, so a layer switched back on repaints the \
                     past it kept recording.",
                );
                ui.checkbox(&mut self.config.show_liquidity, "liquidity")
                    .on_hover_text("the resting-liquidity heat cells");
                ui.checkbox(
                    &mut self.config.show_buy_aggressions,
                    "buy aggression",
                )
                .on_hover_text(
                    "buy-side bubbles; needs the aggression layer on (toolbar). Hiding one \
                     side never rescales the other",
                );
                ui.checkbox(
                    &mut self.config.show_sell_aggressions,
                    "sell aggression",
                )
                .on_hover_text(
                    "sell-side bubbles; needs the aggression layer on (toolbar). Hiding one \
                     side never rescales the other",
                );
                ui.checkbox(
                    &mut self.config.show_aligned_depletion,
                    "aggression-aligned depletion",
                )
                .on_hover_text(
                    "depletion markers where a factual trade matches a factual L2 reduction",
                );
                ui.checkbox(
                    &mut self.config.show_unattributed_reductions,
                    "L2 reduction (unattributed)",
                )
                .on_hover_text(
                    "depth-only reductions and their fading withdrawal tails; with both \
                     depletion layers off, bubbles also lose their consumption marks",
                );
                ui.checkbox(&mut self.config.show_gaps, "L2 gap")
                    .on_hover_text(
                        "dashed boundaries around intervals with no depth coverage; the stretch \
                         older than this session's capture is marked by its boundary alone",
                    );

                ui.separator();
                ui.strong("liquidity response");
                ui.add_enabled(
                    self.config.liquidity_events_enabled(),
                    egui::Slider::new(&mut self.config.liquidity_correlation_ms, 25..=1_000)
                        .text("matching window ms")
                        .logarithmic(true),
                )
                .on_hover_text(
                    "time/price window used to associate a factual trade with a factual L2 reduction",
                );
                ui.add_enabled(
                    self.config.show_unattributed_reductions,
                    egui::Slider::new(&mut self.config.min_unattributed_reduction, 0.0..=1.0)
                        .text("min unattributed pull"),
                )
                .on_hover_text(
                    "hide unattributed (depth-only) reductions smaller than this fraction of the level; aggression-aligned bites always show",
                );
                ui.add_enabled(
                    self.config.show_unattributed_reductions,
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
                if let Some(ladder) = &self.published.ladder {
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
                if ui.button("reset L2 visuals").clicked() {
                    let price_grouping = self.config.price_grouping;
                    self.capture_grouping_draft =
                        price_grouping.to_f64().unwrap_or(self.capture_grouping_draft);
                    self.config = HeatmapConfig {
                        enabled: self.config.enabled,
                        price_grouping,
                        // The bubble layer owns its own panel and its own reset,
                        // so its whole look (and the preset it came from) stays.
                        show_aggressions: self.config.show_aggressions,
                        bubble_cluster_ms: self.config.bubble_cluster_ms,
                        bubble_dust_merge_ms: self.config.bubble_dust_merge_ms,
                        bubble_candle_summary: self.config.bubble_candle_summary,
                        bubbles: self.config.bubbles.clone(),
                        live_lane: self.config.live_lane.clone(),
                        ..HeatmapConfig::default()
                    };
                }
            });
        self.commit_config_changes(before)
    }

    /// The Bubbles dock tab's body: the aggression layer's settings.
    /// Everything here is independent of L2 capture — bubbles are built from
    /// the aggregate-trade stream. Same return contract as
    /// [`Self::draw_l2_tab`].
    pub fn draw_bubbles_tab(&mut self, ui: &mut egui::Ui) -> bool {
        self.sync_published();
        let before = self.config.clone();
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
                        draw_preview(ui, &self.config).on_hover_text(
                            "Deterministic preview: every control below shows its effect here, without waiting for a trade.",
                        );
                        ui.separator();

                        self.draw_bubble_presets(ui);
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
                                            (0, "Raw · one bubble per print"),
                                            (50, "50 ms"),
                                            (100, "100 ms"),
                                            (200, "200 ms"),
                                            (500, "500 ms"),
                                            (1_000, "1 s"),
                                            (2_000, "2 s"),
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
                                "merge compatible prints (same side, same price range) inside this window into one bubble; quantities are summed exactly",
                            );
                            ui.horizontal(|ui| {
                                ui.label("fold dust");
                                egui::ComboBox::from_id_salt("heatmap_bubble_dust")
                                    .selected_text(dust_label(self.config.bubble_dust_merge_ms))
                                    .show_ui(ui, |ui| {
                                        for milliseconds in [0, 500, 1_500, 3_000, 10_000] {
                                            ui.selectable_value(
                                                &mut self.config.bubble_dust_merge_ms,
                                                milliseconds,
                                                dust_label(milliseconds),
                                            );
                                        }
                                    });
                            })
                            .response
                            .on_hover_text(
                                "a second pass over the prints too small to read on their own: inside this window they fold into one bubble per price range. The threshold follows \"readable from px\" — quantities and trade counts are summed exactly",
                            );
                            ui.checkbox(
                                &mut self.config.bubble_candle_summary,
                                "summarize closed bars",
                            )
                            .on_hover_text(
                                "fold every print of a bar and price range into one bubble carrying both sides, drawn as a pie whose sectors are the buy/sell proportion. The forming bar included: its pie is a running total that grows with each order, so the compressed left side reports what is happening now instead of only what already happened. Quantities, ids and matched evidence are summed exactly, and the tape still shows those same prints one by one",
                            );
                            self.draw_live_lane_controls(ui);
                            self.draw_bubble_controls(ui);
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
                        if ui
                            .button("reset bubble visuals")
                            .on_hover_text("only this tab; L2 and history settings stay as they are")
                            .clicked()
                        {
                            let defaults = HeatmapConfig::default();
                            self.config.bubble_cluster_ms = defaults.bubble_cluster_ms;
                            self.config.bubble_dust_merge_ms = defaults.bubble_dust_merge_ms;
                            self.config.bubble_candle_summary = defaults.bubble_candle_summary;
                            self.config.bubbles = defaults.bubbles;
                            self.config.live_lane = defaults.live_lane;
                            // No stored preset is on screen any more, so the
                            // picker must not keep claiming one.
                            self.presets.active.clear();
                            self.preset_status =
                                Some("bubble defaults restored (not saved)".to_owned());
                        }
                    });
        self.commit_config_changes(before)
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

fn lane_cluster_label(window: Option<i64>, inherited: i64) -> String {
    match window {
        None => format!("Same as history · {}", dust_label(inherited)),
        Some(0) => "Raw · one bubble per print".to_owned(),
        Some(milliseconds) => dust_label(milliseconds),
    }
}

fn dust_label(milliseconds: i64) -> String {
    if milliseconds == 0 {
        "Off · draw every print".to_owned()
    } else if milliseconds % 1_000 == 0 {
        format!("{} s", milliseconds / 1_000)
    } else {
        format!("{milliseconds} ms")
    }
}

const fn size_reference_label(reference: BubbleSizeReference) -> &'static str {
    match reference {
        BubbleSizeReference::VisibleP99 => "Auto · visible P99",
        BubbleSizeReference::VisibleMax => "Auto · largest visible",
        BubbleSizeReference::Fixed => "Fixed quantity",
    }
}

const fn render_mode_label(mode: BubbleRenderMode) -> &'static str {
    match mode {
        BubbleRenderMode::Flat => "Flat · 2D disc",
        BubbleRenderMode::Sphere => "Sphere · 3D shaded",
    }
}

/// One optional colour: a swatch that adopts the override the moment it is
/// touched, and a way back to the theme.
fn color_override(ui: &mut egui::Ui, label: &str, value: &mut Option<[u8; 3]>, fallback: [u8; 3]) {
    ui.horizontal(|ui| {
        let mut rgb = value.unwrap_or(fallback);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *value = Some(rgb);
        }
        ui.label(label);
        if value.is_some() {
            if ui
                .small_button("theme")
                .on_hover_text("follow the chart theme again")
                .clicked()
            {
                *value = None;
            }
        } else {
            ui.weak("(theme)");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderflow::{BubbleSizeReference, BubbleStyle};
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

    /// Lay the dock tabs out for real, off-screen, so a broken nested layout
    /// or a duplicated widget id fails here instead of on the chart. Both tabs
    /// are drawn in the same frame: their widget ids must not collide.
    #[test]
    fn the_bubble_tab_lays_out_every_control() {
        let ctx = egui::Context::default();
        let mut view = OrderflowView::new("BTCUSDT");
        let frame = |view: &mut OrderflowView| {
            ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.draw_bubbles_tab(ui);
                    view.draw_l2_tab(ui);
                });
            })
        };
        // Two frames: the second one re-uses the ids the first allocated.
        frame(&mut view);
        frame(&mut view);

        // Now the conditional branches: fixed size reference (extra field),
        // marks turned off, no trail, and a custom colour instead of the theme.
        view.config.bubbles.size_reference = BubbleSizeReference::Fixed;
        view.config.bubbles.show_consumption_front = false;
        view.config.bubbles.show_impact_ring = false;
        view.config.bubbles.trail_length = 0.0;
        view.config.bubbles.buy_color = Some([1, 2, 3]);
        view.config.bubbles.min_quantity = 5.0;
        frame(&mut view);

        // And with bubbles switched off the controls are disabled, not gone.
        view.config.show_aggressions = false;
        let output = frame(&mut view);
        assert!(
            !output.shapes.is_empty(),
            "the tab must still paint when bubbles are hidden"
        );
    }

    #[test]
    fn presets_apply_only_the_bubble_section() {
        use crate::orderflow::LiveLaneStyle;
        let mut view = OrderflowView::new("BTCUSDT");
        let before = view.config.clone();
        view.presets.upsert(BubblePreset {
            name: "wide".to_owned(),
            cluster_ms: 100,
            dust_merge_ms: 3_000,
            candle_summary: true,
            bubbles: BubbleStyle {
                max_radius: 42.0,
                side_offset: 8.0,
                ..BubbleStyle::default()
            },
            live_lane: LiveLaneStyle {
                width_share: 0.5,
                time_zoom: 2.0,
                cluster_ms: Some(50),
                radius_scale: 1.6,
                show_marks: true,
            },
        });
        assert!(view.apply_preset("wide"), "a stored name applies");
        assert_eq!(view.config.bubbles.max_radius, 42.0);
        assert_eq!(view.config.bubble_cluster_ms, 100);
        assert_eq!(view.config.bubble_dust_merge_ms, 3_000);
        assert!(view.config.bubble_candle_summary);
        assert_eq!(view.config.live_lane.width_share, 0.5);
        assert_eq!(view.config.live_lane.time_zoom, 2.0);
        assert_eq!(view.config.live_lane.cluster_ms, Some(50));
        assert_eq!(view.presets.active, "wide");
        assert_eq!(view.preset_name_draft, "wide");
        // Untouched: the layer switch, retention, grouping, gamma, capture bucket.
        assert_eq!(view.config.show_aggressions, before.show_aggressions);
        assert_eq!(view.config.retention_ms, before.retention_ms);
        assert_eq!(view.config.display_grouping, before.display_grouping);
        assert_eq!(view.config.gamma, before.gamma);
        assert_eq!(view.config.price_grouping, before.price_grouping);

        // An unknown name changes nothing at all, and says so.
        let after = view.config.clone();
        assert!(!view.apply_preset("nope"));
        assert_eq!(view.config, after);
        assert_eq!(view.presets.active, "wide");
    }

    fn snapshot_event(generation: u64) -> DepthEvent {
        DepthEvent::Snapshot {
            symbol: "BTCUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            price_step: None,
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
    fn capture_reads_as_syncing_only_while_enabled_and_settling() {
        let mut view = OrderflowView::new("BTCUSDT");
        assert!(!view.is_syncing(), "disabled capture is not a wait");

        view.set_enabled(true, 10);
        view.flush_for_test();
        assert!(view.is_syncing(), "connecting reads as a wait");

        view.set_enabled(false, 20);
        view.flush_for_test();
        assert!(!view.is_syncing(), "turning capture off ends the wait");
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
        let first = view.project_visible(0, 0, &bars, None, true, true, (98.0, 102.0));
        assert!(first.is_none());
        view.flush_for_test();
        let frame = view
            .project_visible(0, 0, &bars, None, true, true, (98.0, 102.0))
            .expect("published frame");
        assert!(frame.projection.enabled);
        assert!(!frame.projection.cells.is_empty());
    }

    /// Dragging the divider is the width slider by another route, and it stops
    /// at half the chart: past that the history has no room to be read in.
    #[test]
    fn dragging_the_divider_resizes_the_lane_up_to_half_the_chart() {
        let mut view = OrderflowView::new("BTCUSDT");
        let chart = 1_000.0;
        let before = view.config.live_lane.resolved_width_px(chart);

        // Drag left → a wider tape, pixel for pixel.
        view.resize_live_lane(-60.0, chart);
        assert!((view.config.live_lane.resolved_width_px(chart) - (before + 60.0)).abs() < 0.01);
        // Drag right → a narrower one.
        view.resize_live_lane(60.0, chart);
        assert!((view.config.live_lane.resolved_width_px(chart) - before).abs() < 0.01);

        // Half the chart is the ceiling, a twentieth the floor, whatever the
        // drag asked for.
        view.resize_live_lane(-10_000.0, chart);
        assert_eq!(view.config.live_lane.width_share, MAX_LIVE_LANE_SHARE);
        view.resize_live_lane(10_000.0, chart);
        assert_eq!(view.config.live_lane.width_share, MIN_LIVE_LANE_SHARE);

        // A degenerate chart or a lost pointer changes nothing at all.
        let steady = view.config.live_lane.clone();
        view.resize_live_lane(f32::NAN, chart);
        view.resize_live_lane(-20.0, 0.0);
        assert_eq!(view.config.live_lane, steady);
    }

    /// The tape's own zoom, and the bounds that keep it a tape.
    #[test]
    fn zooming_the_lane_scales_its_window_and_stops_at_the_bounds() {
        let mut view = OrderflowView::new("BTCUSDT");
        let bars = [bar(0, 8_000), bar(8_000, 16_000)];
        let unzoomed = view.live_lane_window_ms(&bars);
        assert_eq!(unzoomed, 8_000, "one typical bar of market time");

        view.zoom_live_lane(2.0);
        assert_eq!(view.live_lane_window_ms(&bars), 4_000, "half the time");
        view.zoom_live_lane(0.5);
        assert_eq!(view.live_lane_window_ms(&bars), unzoomed);

        view.zoom_live_lane(1_000.0);
        assert_eq!(view.config.live_lane.time_zoom, MAX_LIVE_LANE_ZOOM);
        view.zoom_live_lane(1e-6);
        assert_eq!(view.config.live_lane.time_zoom, MIN_LIVE_LANE_ZOOM);

        let steady = view.config.live_lane.clone();
        view.zoom_live_lane(0.0);
        view.zoom_live_lane(f32::NAN);
        assert_eq!(view.config.live_lane, steady);
    }

    #[test]
    fn the_projection_request_window_clips_the_published_ladder() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        // One 99 bid and one 101 ask (see `snapshot_event`).
        view.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];
        view.project_visible(0, 0, &bars, None, true, true, (100.0, 102.0));
        view.flush_for_test();

        let ladder = view.published.ladder.as_ref().expect("published ladder");
        assert!(
            ladder.bids.is_empty(),
            "the 99 bid sits outside the 100-102 window"
        );
        assert_eq!(prices_of(&ladder.asks), vec![Decimal::from(101)]);
        // The raw touch survives the clip on both sides.
        assert_eq!(
            ladder.best_bid.expect("best bid").price(),
            Decimal::from(99)
        );
        assert_eq!(
            ladder.best_ask.expect("best ask").price(),
            Decimal::from(101)
        );
    }

    fn prices_of(levels: &[BookLevel]) -> Vec<Decimal> {
        levels.iter().map(|level| level.price()).collect()
    }

    /// Paint the strip for real, off-screen, on both of its paths: with a
    /// published ladder (depth rows + spread gap) and without one (the honest
    /// "no book" state). A panicking painter or a broken price mapping fails
    /// here instead of on the chart.
    #[test]
    fn the_live_strip_paints_on_both_of_its_paths() {
        let ctx = egui::Context::default();
        let mut view = OrderflowView::new("BTCUSDT");
        let strip = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(84.0, 400.0));
        let scale = PriceScale::from_range(95.0, 105.0, strip.top(), strip.bottom());

        let paint = |view: &mut OrderflowView| {
            ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.draw_live_strip(
                        ui.painter(),
                        strip,
                        &scale,
                        egui::Color32::BLACK,
                        Some(0),
                    );
                });
            })
        };

        // Capture off: no ladder, the strip must say so rather than draw an
        // empty column that could be mistaken for "no liquidity".
        let empty = paint(&mut view);
        assert!(!empty.shapes.is_empty());

        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        view.flush_for_test();
        let live = paint(&mut view);
        assert!(!live.shapes.is_empty());
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
        view.project_visible(0, 0, &bars, None, true, true, (98.0, 102.0));
        view.flush_for_test();
        let frame = view
            .project_visible(0, 0, &bars, None, true, true, (98.0, 102.0))
            .expect("published frame");
        assert_eq!(frame.projection.aggressions.len(), 1);
        assert!(frame.projection.cells.is_empty(), "no map without capture");

        // Turning the bubbles off closes the pipeline again.
        view.set_bubbles_enabled(false);
        assert!(
            view.project_visible(0, 0, &bars, None, true, true, (98.0, 102.0))
                .is_none()
        );
    }

    #[test]
    fn disabling_capture_drops_the_published_frame() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];
        view.project_visible(0, 0, &bars, None, true, true, (98.0, 102.0));
        view.flush_for_test();
        assert!(
            view.project_visible(0, 0, &bars, None, true, true, (98.0, 102.0))
                .is_some()
        );

        view.set_enabled(false, 11);
        assert!(
            view.project_visible(0, 0, &bars, None, true, true, (98.0, 102.0))
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
            price_step: None,
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
