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
    BubbleRenderMode, BubbleSizeReference, ConsumptionMark, DisplayGrouping, HeatmapConfig,
    HeatmapTheme, IntensityMode, LANE_WINDOW_PRESETS_MS, LaneWindow, MAX_BUBBLE_MAX_RADIUS,
    MAX_BUBBLE_MIN_RADIUS, MAX_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_SHARE,
    MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_MAX_RADIUS, MIN_LIVE_LANE_RADIUS_SCALE,
    MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_ZOOM, format_window_ms,
    lane_window_label, reserved_span_ms, same_lane_window,
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

/// Borrowed chart timeline handed to one order-flow projection request.
///
/// Keeping the boundary revision beside the exact bar slice prevents callers
/// from accidentally pairing a new timeline with an old cache identity.
#[derive(Clone, Copy)]
pub(crate) struct VisibleBarTimeline<'a> {
    revision: u64,
    first_bar_index: usize,
    closed: &'a [Bar],
    partial: Option<&'a Bar>,
}

impl<'a> VisibleBarTimeline<'a> {
    #[must_use]
    pub(crate) fn new(
        revision: u64,
        first_bar_index: usize,
        closed: &'a [Bar],
        partial: Option<&'a Bar>,
    ) -> Self {
        Self {
            revision,
            first_bar_index,
            closed,
            partial,
        }
    }
}

/// The live lane's band and the instant it runs to, read together.
///
/// A pane draws inside this band in tape time, so it needs both numbers from
/// the same frame's published book — see [`OrderflowView::live_lane`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveLane {
    /// Width of the band, in pixels, taken off the chart's right edge.
    pub width_px: f32,
    /// Exchange timestamp at the band's right edge: the live edge.
    pub end_ms: i64,
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
    /// Scripted tape starvation: prints stop reaching the tape this many
    /// milliseconds after the first one, while the book keeps arriving.
    /// `None` — always, outside a capture run — feeds the tape every print.
    starve_tape_after_ms: Option<i64>,
    /// Instant of the first print this view ever saw, the starvation clock's
    /// zero. Read only when the hook above is set.
    first_print_ms: Option<i64>,
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
            starve_tape_after_ms: None,
            first_print_ms: None,
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

    /// The capture bucket the book engine derived for this instrument — the
    /// declared `price_step` where the feed reports one, else the auto-sized
    /// base. The footprint adopts it as its row grid, so the two ladders can
    /// never disagree about what one row of price means.
    #[must_use]
    pub fn base_capture_grouping(&self) -> Decimal {
        self.published.base_price_grouping
    }

    /// Whether L2 depth capture is recording. Says nothing about whether the
    /// map is on screen — see [`depth_visible`](Self::depth_visible).
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Whether the depth map is drawn on the candles: recording *and* not
    /// hidden.
    #[must_use]
    pub fn depth_visible(&self) -> bool {
        self.config.depth_visible()
    }

    /// Show or hide the depth map over the candles, without touching L2
    /// capture.
    ///
    /// No feed command is involved: the recorder keeps running, so turning the
    /// map back on repaints the retained past instead of opening a gap in it.
    ///
    /// **The tape is not touched, in either direction.** This is the toolbar's
    /// switch and it governs the candles; the tape holds a value of its own
    /// ([`Self::set_lane_depth_visible`]), reached by right-clicking it.
    pub fn set_depth_visible(&mut self, visible: bool) {
        if self.config.show_depth == visible {
            return;
        }
        let before = self.config.clone();
        self.config.show_depth = visible;
        if !self.config.depth_visible_anywhere() {
            // Drop the local frame immediately; the worker clears its own.
            self.published.frame = None;
        }
        self.commit_config_changes(before);
    }

    /// Whether the aggression layer is on over the candles. Independent of the
    /// depth map: it reads the trade stream the chart already consumes.
    #[must_use]
    pub fn bubbles_enabled(&self) -> bool {
        self.config.show_aggressions
    }

    /// Toggle the aggression layer over the candles, without touching L2
    /// capture. No feed command is needed — aggregate trades already flow for
    /// the candles. The tape is not touched in either direction, as with the
    /// depth map.
    pub fn set_bubbles_enabled(&mut self, enabled: bool) {
        if self.config.show_aggressions == enabled {
            return;
        }
        let before = self.config.clone();
        self.config.show_aggressions = enabled;
        self.commit_config_changes(before);
    }

    /// Squeeze the frame's bubble budget, through the same field the
    /// projection reads.
    ///
    /// The scripted way to reach a folded frame. The fold is the one bubble
    /// state a capture cannot otherwise arrange: it needs a tape dense enough
    /// to exhaust the budget, which is a market condition rather than a
    /// setting. One path, never two — the projection reads this field whoever
    /// wrote it.
    pub fn set_primitive_budget(&mut self, budget: usize) {
        if budget == 0 || self.config.max_aggression_primitives == budget {
            return;
        }
        let before = self.config.clone();
        self.config.max_aggression_primitives = budget;
        self.commit_config_changes(before);
    }

    /// Whether the tape is on the canvas at all.
    #[must_use]
    pub fn lane_enabled(&self) -> bool {
        self.config.lane_enabled()
    }

    /// Put the tape on the canvas, or take it off.
    ///
    /// Off, the lane reserves no width and asks for no projection; the candles
    /// take the whole canvas. Its two layer switches are left exactly as they
    /// were, so switching the tape back on returns the tape that was switched
    /// off rather than a fresh one.
    pub fn set_lane_enabled(&mut self, enabled: bool) {
        if self.config.live_lane.enabled == enabled {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.enabled = enabled;
        if !self.config.depth_visible_anywhere() {
            // Drop the local frame immediately; the worker clears its own.
            self.published.frame = None;
        }
        self.commit_config_changes(before);
    }

    /// The band's width on a canvas this wide — zero when the tape is off.
    ///
    /// The one number the tape switch reaches the canvas through, and the one
    /// [`Self::live_lane`] reports when there is a live edge to anchor the band
    /// on. Asking for it directly is how a caller — or a test — knows whether a
    /// band is reserved at all, without needing a market to have printed yet.
    #[must_use]
    pub fn lane_width_px(&self, chart_width: f32) -> f32 {
        self.config.live_lane.resolved_width_px(chart_width)
    }

    /// Whether the depth map is switched on for the tape. Says nothing about
    /// whether there is a tape — see [`Self::lane_enabled`].
    #[must_use]
    pub fn lane_depth_visible(&self) -> bool {
        self.config.lane_depth_visible()
    }

    /// The candles' depth switch alone, whatever capture lets through it.
    ///
    /// [`Self::depth_visible`] answers "is it drawn", which is what a renderer
    /// needs; this answers "did anyone ask for it", which is what persistence
    /// needs. On a source with no book the map is undrawn however the switch
    /// stands, and writing that down as the trader's answer would turn a
    /// capability into a choice they never made — and one that then outranks
    /// the shipped default on every market, including the ones with a book.
    /// The same rule [`Self::set_depth_visible`] already compares against.
    #[must_use]
    pub fn depth_switched_on(&self) -> bool {
        self.config.show_depth
    }

    /// The tape's depth switch alone. Twin of [`Self::depth_switched_on`],
    /// same rule and the same reason.
    #[must_use]
    pub fn lane_depth_switched_on(&self) -> bool {
        self.config.live_lane.show_depth
    }

    /// Show or hide the depth map on the tape alone. Capture is untouched, and
    /// so are the candles.
    pub fn set_lane_depth_visible(&mut self, visible: bool) {
        // Compared against the switch, not against what capture allows through
        // it — same rule as the candles' `set_depth_visible`. A source with no
        // book would otherwise swallow "off" and spring the layer back the
        // moment a source with one arrived.
        if self.config.live_lane.show_depth == visible {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.show_depth = visible;
        if !self.config.depth_visible_anywhere() {
            self.published.frame = None;
        }
        self.commit_config_changes(before);
    }

    /// Whether aggression bubbles are switched on for the tape.
    #[must_use]
    pub fn lane_bubbles_enabled(&self) -> bool {
        self.config.lane_aggressions_visible()
    }

    /// Toggle the aggression bubbles on the tape alone.
    pub fn set_lane_bubbles_enabled(&mut self, enabled: bool) {
        if self.config.lane_aggressions_visible() == enabled {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.show_aggressions = enabled;
        self.commit_config_changes(before);
    }

    /// State that a surface other than the bubbles is reading the aggression
    /// clusters this frame — the live strip beside the price axis.
    ///
    /// The pane says this every frame from the layer it owns, so the two can
    /// never drift apart. With the bubbles hidden and the strip shown, this is
    /// what keeps prints being retained and projected: the strip draws the
    /// same clusters, from the same engine path, and stopping the pipeline
    /// under it would blank a live surface nobody switched off.
    pub fn set_projection_demand(&mut self, wanted: bool) {
        if self.config.projection_demand == wanted {
            return;
        }
        let before = self.config.clone();
        self.config.projection_demand = wanted;
        self.commit_config_changes(before);
    }

    /// Whether the live lane's boundary and live-edge lines are drawn.
    #[must_use]
    pub fn lane_marks_visible(&self) -> bool {
        self.config.live_lane.show_marks
    }

    /// Show or hide those marks. The very field the dock's checkbox writes, so
    /// the two entry points can never disagree — and, like every other lane
    /// setting, it is saved with the order-flow preset rather than on its own.
    pub fn set_lane_marks_visible(&mut self, visible: bool) {
        if self.config.live_lane.show_marks == visible {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.show_marks = visible;
        self.commit_config_changes(before);
    }

    /// Whether the canvas's compact visual key is drawn.
    #[must_use]
    pub fn legend_visible(&self) -> bool {
        self.config.show_legend
    }

    /// Show or hide it — the very field the L2 panel's "show chart legend"
    /// checkbox writes, so the canvas's right-click menu and the panel can
    /// never disagree about it. Chrome only: every layer it names keeps
    /// drawing while the key is hidden.
    pub fn set_legend_visible(&mut self, visible: bool) {
        if self.config.show_legend == visible {
            return;
        }
        let before = self.config.clone();
        self.config.show_legend = visible;
        self.commit_config_changes(before);
    }

    /// Whether the book's status badge is drawn on the canvas.
    #[must_use]
    pub fn status_badge_visible(&self) -> bool {
        self.config.show_status_badge
    }

    /// Show or hide it. The recorder is not part of this question: capture,
    /// generation and the ladder carry on, and the L2 panel still states them
    /// — this silences a label on the canvas, nothing else.
    pub fn set_status_badge_visible(&mut self, visible: bool) {
        if self.config.show_status_badge == visible {
            return;
        }
        let before = self.config.clone();
        self.config.show_status_badge = visible;
        self.commit_config_changes(before);
    }

    /// Whether intervals with no depth coverage are marked out.
    #[must_use]
    pub fn gaps_visible(&self) -> bool {
        self.config.show_gaps
    }

    /// Show or hide the gap boundaries — the same field as the dock's "L2 gap"
    /// checkbox. This one hides a *statement about missing data* rather than
    /// data itself, which is why the layer menu's entry spells out that an
    /// unrecorded stretch will then look like a recorded one.
    pub fn set_gaps_visible(&mut self, visible: bool) {
        if self.config.show_gaps == visible {
            return;
        }
        let before = self.config.clone();
        self.config.show_gaps = visible;
        self.commit_config_changes(before);
    }

    /// Latest exchange timestamp for which live book state is known, while the
    /// map is on screen. Marks the live edge inside the forming bar's lane.
    #[must_use]
    pub fn live_end_ms(&mut self) -> Option<i64> {
        // Any flow layer, not the depth map: this instant is the tape's
        // anchor, and asking the *map* for it made the tape a hostage of L2.
        // Switching both maps off used to delete the band, its bubbles, its
        // strip and the menu that configures it — and a feed that streams no
        // book never had a tape at all, in any configuration. Each pane
        // answers for its own canvas; the tape answers for its own existence.
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        self.published.live_end_ms
    }

    /// The live lane as the chart needs it: how wide its band is, and the
    /// instant its right edge stands for.
    ///
    /// A pane, not a slot: the candles own everything left of the band and the
    /// lane owns it whatever they do. Panning or zooming them changes how many
    /// bars fit beside the tape and never the tape itself, which is what keeps
    /// the newest prints on screen through every chart movement.
    ///
    /// One call because it is one look at the published book. Reading the two
    /// separately sends the render thread back through the worker's mutex for
    /// a number the first read already had — a lock per frame for nothing, on
    /// the one thread that must never wait.
    ///
    /// `None` when there is no live edge to run to, which is the same thing as
    /// "this chart has no lane".
    #[must_use]
    pub fn live_lane(&mut self, chart_width: f32) -> Option<LiveLane> {
        let end_ms = self.live_end_ms()?;
        Some(LiveLane {
            width_px: self.lane_width_px(chart_width),
            end_ms,
        })
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
    ///
    /// The gesture speaks whichever language the window is in — the zoom while
    /// it follows the bars, the milliseconds while it is pinned — and never
    /// changes which one that is. Dragging a pinned tape gives a different
    /// pinned tape, not a silent return to automatic.
    pub fn zoom_live_lane(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.window.zoom_by(factor);
        self.commit_config_changes(before);
    }

    /// How much market time the tape shows, and in which language it was
    /// asked for.
    #[must_use]
    pub fn live_lane_window(&self) -> LaneWindow {
        self.config.live_lane.window
    }

    /// Choose how the tape's window is decided: a preset, a custom duration,
    /// or back to following the bars.
    pub fn set_live_lane_window(&mut self, window: LaneWindow) {
        if self.config.live_lane.window == window {
            return;
        }
        let before = self.config.clone();
        self.config.live_lane.window = window;
        self.config.live_lane.window.sanitize();
        self.commit_config_changes(before);
    }

    /// Market time the lane is showing right now, in milliseconds — the label
    /// under it, and the only readout of what the zoom is worth.
    #[must_use]
    pub fn live_lane_window_ms(&self, closed: &[Bar]) -> i64 {
        self.config.live_lane.window_ms(reserved_span_ms(closed))
    }

    /// How old the newest aggression on the tape is, against the instant the
    /// lane's right edge stands for.
    ///
    /// The lane's edge follows the newer of the book clock and the print
    /// clock, and only the print clock places bubbles. When the book runs
    /// ahead — a quiet stretch with a busy book, or prints held up between the
    /// venue and this process — every bubble is drawn this far left of the
    /// edge, and past the lane's own window none is drawn on the tape at all.
    /// The axis under the tape says so rather than letting an empty tape read
    /// as a market that stopped trading.
    ///
    /// `None` also when no pane draws the bubbles: an empty tape the trader
    /// emptied themselves needs no explanation.
    #[must_use]
    pub fn tape_age(&self) -> Option<crate::orderflow::TapeAge> {
        // Asked only when a pane still draws the bubbles. The depth map and
        // the aggression layer switch apart, so a tape showing liquidity with
        // its bubbles deliberately off has no missing marks to explain —
        // warning about them there is the caption inventing a problem.
        if !self.config.aggressions_visible_anywhere() {
            return None;
        }
        self.published.health.tape_age
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
        // The starvation clock is per market, like the history it starves.
        // Carrying the old symbol's zero across would open the new one on a
        // tape that is already dead — the capture hook would be photographing
        // its own leftovers instead of the state it was asked for.
        self.first_print_ms = None;
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
        if !self.config.any_layer_enabled() || self.starved_at(trade.timestamp_ms) {
            return;
        }
        self.worker.send(BookCommand::Trade(trade.clone()));
    }

    /// Whether the scripted starvation hook is holding this print back.
    ///
    /// Off by default and free when off: the option is `None` outside a
    /// capture run, so a live tape pays one `is_some` per print.
    fn starved_at(&mut self, timestamp_ms: i64) -> bool {
        let Some(after_ms) = self.starve_tape_after_ms else {
            return false;
        };
        let first = *self.first_print_ms.get_or_insert(timestamp_ms);
        timestamp_ms.saturating_sub(first) > after_ms
    }

    /// Stop feeding the tape once the session is `after_ms` old, leaving the
    /// book running.
    ///
    /// The scripted way to reach a starved tape. A tape whose newest mark has
    /// drifted off the lane is a *market* state — a book that keeps changing
    /// while nothing prints — so no setting produces it and no capture can
    /// wait for one to happen. This withholds prints from the tape through the
    /// same call the feed uses, rather than forging a number into the caption:
    /// the axis then reports the age it genuinely observes, and a screenshot
    /// shows what the trader's own chart would show.
    ///
    /// The bars, the indicators and the simulator are untouched — they are fed
    /// upstream of here — which is exactly right: the candles keep their
    /// prints, the tape loses them, and that contrast is the thing under test.
    pub fn set_starve_tape_after_ms(&mut self, after_ms: i64) {
        self.starve_tape_after_ms = Some(after_ms.max(0));
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
        timeline: VisibleBarTimeline<'_>,
        lane: bool,
        on_newest_bar: bool,
        lane_reference_ms: Option<i64>,
        price_range: (f64, f64),
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        let request = ProjectionRequest {
            timeline_revision: timeline.revision,
            first_bar_index: timeline.first_bar_index,
            closed: timeline.closed.to_vec(),
            partial: timeline.partial.cloned(),
            lane,
            on_newest_bar,
            lane_reference_ms,
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
    /// behind the candle layer. `inverted` is the candles' own orientation,
    /// so the map turns over with the bars it sits behind.
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
        inverted: bool,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        )
        .with_inverted(inverted);
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_heatmap_background(painter, &context);
        draw_live_lane_marks(painter, &context);
        draw_liquidity_events(painter, &context);
    }

    /// Draw factual aggressive prints over the candles. The canvas's key is
    /// not part of this pass — see [`draw_legend`](Self::draw_legend).
    /// `inverted` is the candles' own orientation, as in
    /// [`draw_background`](Self::draw_background).
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
        inverted: bool,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        )
        .with_inverted(inverted);
        let style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_aggression_bubbles(painter, &context);
    }

    /// Draw the canvas's compact visual key.
    ///
    /// Its own pass, not a tail of the bubbles: the legend is chrome about
    /// what the canvas is showing — it names the depth layers too — so hiding
    /// the bubbles must not take it down, and hiding it must not take the
    /// bubbles down. The trader switches it from the canvas's right-click
    /// menu (`ChartLayer::FlowLegend`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_legend(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        frame: &VisibleOrderflow,
        canvas_background: egui::Color32,
        lane_width_px: f32,
        top_inset_px: f32,
    ) {
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        );
        let mut style = OrderflowRenderStyle::from_config(&self.config, canvas_background);
        style.legend_top_inset = top_inset_px;
        let context = RenderContext::new(&frame.projection, layout, &style);
        draw_compact_legend(painter, &context);
    }

    /// `right_inset_px` is the room another piece of chrome has already claimed
    /// in this corner — the tape switch, which sits on the corner itself. The
    /// badge steps left of it rather than under it: two labels on one pixel is
    /// how a status message becomes unreadable exactly when it matters.
    pub fn draw_status_badge(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        right_inset_px: f32,
    ) {
        // Tied to the map, not to the recorder: a badge reporting on a book
        // nobody asked to see is just chrome. The trader can silence it on its
        // own too, from the canvas's right-click menu — but only while the
        // book is working. A failure re-asserts the badge: it is the one
        // real-time statement that the depth on screen has stopped being the
        // book, and hiding chrome may never hide *that* (data honesty).
        let failing = self.published.status.is_failure();
        if !self.config.depth_visible() || (!self.config.show_status_badge && !failing) {
            return;
        }
        let text = self.published.status.label();
        let color = status_color(&self.published.status);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), color);
        let pos = egui::pos2(
            chart_rect.right() - galley.size().x - 10.0 - right_inset_px.max(0.0),
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
            (Some(frame), Some(open_ms)) => live_strip::aggression_rows(
                &frame.projection.aggressions,
                open_ms,
                frame.projection.summarized,
                frame.projection.effective_grouping.bucket_width,
            ),
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
                // A plain row is one bucket tall; a regional mark declares the
                // whole region it covers. Zero-span marks (older projections)
                // fall back to the bucket width.
                let row_span = if row.price_span > Decimal::ZERO {
                    row.price_span
                } else {
                    bucket_width
                };
                // Ordered on screen, not by price: on an inverted scale the
                // row's higher edge is the lower pixel.
                let a = scale.y((row.price_bucket + row_span).to_f64().unwrap_or(f64::NAN));
                let b = scale.y(row.price_bucket.to_f64().unwrap_or(f64::NAN));
                if !a.is_finite() || !b.is_finite() {
                    continue;
                }
                let (top, bottom) = (a.min(b), a.max(b));
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

    /// Timestamp of the newest accepted book event, from the frame's mirror.
    ///
    /// Read-only twin of [`Self::health`] for callers that only need this one
    /// figure and hold `&self` — the status bar's tape-age readout, which
    /// runs while the frame is already borrowing the app immutably.
    #[must_use]
    pub fn last_event_ms(&self) -> Option<i64> {
        self.published.health.last_event_ms
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
                    ui.label("window");
                    egui::ComboBox::from_id_salt("bubble_live_lane_window")
                        .selected_text(lane_window_label(lane.window, None))
                        .show_ui(ui, |ui| {
                            let mut choose = |ui: &mut egui::Ui, option: LaneWindow| {
                                // Selecting the mode the lane is already in must
                                // not overwrite the number it is carrying, so
                                // the entry compares modes and assigns whole
                                // values only when the mode actually changes.
                                let selected = same_lane_window(lane.window, option);
                                if ui
                                    .selectable_label(selected, lane_window_label(option, None))
                                    .clicked()
                                    && !selected
                                {
                                    lane.window = option;
                                }
                            };
                            choose(ui, LaneWindow::default());
                            for ms in LANE_WINDOW_PRESETS_MS {
                                choose(ui, LaneWindow::Fixed { ms });
                            }
                        });
                })
                .response
                .on_hover_text(
                    "how much market time fits in the tape. Following the bars keeps roughly one bar's worth of flow in the band whatever the instrument; a fixed window shows that much time however fast the bars are closing, which is what a burst calls for. The clustering window follows either way, so a crowded tape gathers into fewer, bigger bubbles instead of a smear",
                );
                // One row, whichever language the window is in: the zoom while
                // it follows the bars, the duration while it is pinned.
                match &mut lane.window {
                    LaneWindow::Auto { zoom } => {
                        ui.horizontal(|ui| {
                            ui.label("zoom");
                            ui.add(
                                egui::Slider::new(zoom, MIN_LIVE_LANE_ZOOM..=MAX_LIVE_LANE_ZOOM)
                                    .logarithmic(true)
                                    .suffix("×"),
                            );
                        })
                        .response
                        .on_hover_text(
                            "the recent bars' typical duration, scaled. Zoom in and prints run across the tape faster and further apart; zoom out and more time crowds in. Also set by dragging the time strip under the tape",
                        );
                    }
                    LaneWindow::Fixed { ms } => {
                        ui.horizontal(|ui| {
                            ui.label("duration");
                            ui.add(
                                egui::Slider::new(
                                    ms,
                                    MIN_LIVE_LANE_WINDOW_MS..=MAX_LIVE_LANE_WINDOW_MS,
                                )
                                .logarithmic(true)
                                .custom_formatter(|value, _| format_window_ms(value as i64)),
                            );
                        })
                        .response
                        .on_hover_text(
                            "market time pinned in the tape, whatever the bars do. Also set by dragging the time strip under the tape",
                        );
                    }
                }
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
                                "Auto · session P99",
                            );
                            ui.selectable_value(
                                &mut bubbles.size_reference,
                                BubbleSizeReference::VisibleMax,
                                "Auto · largest in session",
                            );
                            ui.selectable_value(
                                &mut bubbles.size_reference,
                                BubbleSizeReference::Fixed,
                                "Fixed quantity",
                            );
                        })
                        .response
                        .on_hover_text(
                            "the automatic modes measure the recorded session, so zooming never \
                             resizes a print. P99 keeps one outlier sweep from shrinking \
                             everything else, but the top 1% all render at the maximum radius. \
                             'Largest in session' restores a strict order; a fixed quantity \
                             makes bubble size mean the same thing across sessions.",
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
                ui.horizontal(|ui| {
                    ui.label("mark");
                    egui::ComboBox::from_id_salt("bubble_consumption_mark")
                        .selected_text(consumption_mark_label(bubbles.consumption_mark))
                        .show_ui(ui, |ui| {
                            for mark in [
                                ConsumptionMark::Crown,
                                ConsumptionMark::Front,
                                ConsumptionMark::None,
                            ] {
                                ui.selectable_value(
                                    &mut bubbles.consumption_mark,
                                    mark,
                                    consumption_mark_label(mark),
                                );
                            }
                        })
                        .response
                        .on_hover_text(
                            "the crown is an open arc just outside the rim, on the side of the \
                             book the print ate — its length grows with how much of the print \
                             matched, and it never crosses the disc whose area is the quantity. \
                             The front is the older vertical line through the bubble.",
                        );
                });
                if bubbles.consumption_mark.is_front() {
                    ui.add(
                        egui::Slider::new(&mut bubbles.front_width, 0.5..=10.0)
                            .text("front width px"),
                    );
                    ui.add(
                        egui::Slider::new(&mut bubbles.front_length_scale, 0.5..=6.0)
                            .text("front length × radius"),
                    );
                }
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
                        bubble_region_rows: self.config.bubble_region_rows,
                        bubble_region_ms: self.config.bubble_region_ms,
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
                            ui.horizontal(|ui| {
                                ui.label("region height");
                                egui::ComboBox::from_id_salt("heatmap_bubble_region")
                                    .selected_text(region_label(self.config.bubble_region_rows))
                                    .show_ui(ui, |ui| {
                                        for rows in [1, 2, 3, 4, 6, 8, 12] {
                                            ui.selectable_value(
                                                &mut self.config.bubble_region_rows,
                                                rows,
                                                region_label(rows),
                                            );
                                        }
                                    });
                                if self.config.bubble_region_rows > 1 {
                                    ui.label("window");
                                    egui::ComboBox::from_id_salt("heatmap_bubble_region_ms")
                                        .selected_text(region_window_label(
                                            self.config.bubble_region_ms,
                                        ))
                                        .show_ui(ui, |ui| {
                                            for milliseconds in [500, 1_000, 1_500, 2_000, 3_000, 5_000]
                                            {
                                                ui.selectable_value(
                                                    &mut self.config.bubble_region_ms,
                                                    milliseconds,
                                                    region_window_label(milliseconds),
                                                );
                                            }
                                        });
                                }
                            })
                            .response
                            .on_hover_text(
                                "fold same-side bubbles landing in a price region this many rows tall into one bubble at their volume-weighted price — aggression read per zone, the Bookmap way, instead of one mark per row. Quantities, ids and matched evidence are summed exactly; buy and sell regions stay separate marks",
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
                        // The projection carries clusters whether or not the
                        // bubble layer draws them — the live strip reads the
                        // same ones. Calling them "bubbles" while none is on
                        // screen would report a layer that is off.
                        let noun = if self.config.show_aggressions {
                            "bubbles"
                        } else {
                            "clusters (bubble layer off)"
                        };
                        ui.label(format!(
                            "{} {noun} projected · {} aggressions retained",
                            health.projection_aggressions, health.aggression_count
                        ));
                        if health.floored_quantity > Decimal::ZERO {
                            ui.small(format!(
                                "{} contracts below your display floor are not drawn",
                                health.floored_quantity
                            ))
                            .on_hover_text(concat!(
                                "the minimum-quantity setting under bubble visuals. It is the ",
                                "only thing left that keeps contracts off the canvas, so it says ",
                                "how many - in contracts, not in dots, because what matters is ",
                                "the size of what is missing. Set it to zero to draw everything",
                            ));
                        }
                        if health.folded_aggressions > 0 {
                            ui.small(format!(
                                "{} marks merged into a neighbour to fit the frame",
                                health.folded_aggressions
                            ))
                            .on_hover_text(concat!(
                                "the frame draws a bounded number of bubbles, split between ",
                                "the candles and the tape so neither can crowd the other out. ",
                                "Over that budget the marks merge - the candles fold their ",
                                "smallest together, the tape folds its oldest - and a merged ",
                                "bubble carries the exact summed quantity and says how many ",
                                "marks it stands for. A fold never crosses a side, a pane or a ",
                                "bar, so a frame with more of those than it has budget draws ",
                                "the extra marks instead. Nothing is discarded",
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
                            self.config.bubble_region_rows = defaults.bubble_region_rows;
                            self.config.bubble_region_ms = defaults.bubble_region_ms;
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

fn region_label(rows: u32) -> String {
    if rows <= 1 {
        "Off · one mark per row".to_owned()
    } else {
        format!("{rows} rows")
    }
}

fn region_window_label(milliseconds: i64) -> String {
    if milliseconds % 1_000 == 0 {
        format!("{} s", milliseconds / 1_000)
    } else {
        format!("{milliseconds} ms")
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
        BubbleSizeReference::VisibleP99 => "Auto · session P99",
        BubbleSizeReference::VisibleMax => "Auto · largest in session",
        BubbleSizeReference::Fixed => "Fixed quantity",
    }
}

const fn render_mode_label(mode: BubbleRenderMode) -> &'static str {
    match mode {
        BubbleRenderMode::Flat => "Flat · 2D disc",
        BubbleRenderMode::Sphere => "Sphere · 3D shaded",
    }
}

const fn consumption_mark_label(mark: ConsumptionMark) -> &'static str {
    match mark {
        ConsumptionMark::Crown => "Crown · arc outside the rim",
        ConsumptionMark::Front => "Front · line through the bubble",
        ConsumptionMark::None => "None",
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

    /// The ask, in one test: the toolbar governs the candles and nothing else.
    /// Every one of the four movements — each layer switched off *and* back on
    /// — has to leave the tape exactly where the trader left it.
    #[test]
    fn the_toolbar_switches_move_the_candles_and_never_the_tape() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.config.enabled = true;
        view.config.show_depth = true;
        view.config.show_aggressions = true;
        // The tape opens with both layers, whatever the candles are doing.
        assert!(view.lane_enabled(), "and with a band to draw them on");
        assert!(view.lane_depth_visible() && view.lane_bubbles_enabled());

        // Movement 1 and 2: both layers off on the candles.
        view.set_depth_visible(false);
        view.set_bubbles_enabled(false);
        assert!(!view.depth_visible() && !view.bubbles_enabled());
        assert!(
            view.lane_depth_visible(),
            "the tape still has the book — the whole point"
        );
        assert!(view.lane_bubbles_enabled(), "and the prints");
        assert!(
            view.config.depth_visible_anywhere(),
            "a frame the tape is still reading may not be dropped under it"
        );

        // Movements 3 and 4: both back on. This is the direction that used to
        // drag the tape along, because an inheriting lane had no answer of its
        // own to keep.
        view.set_lane_depth_visible(false);
        view.set_lane_bubbles_enabled(false);
        view.set_depth_visible(true);
        view.set_bubbles_enabled(true);
        assert!(view.depth_visible() && view.bubbles_enabled());
        assert!(
            !view.lane_depth_visible() && !view.lane_bubbles_enabled(),
            "the candles come back alone: the toolbar is not the tape's switch"
        );

        // And a tape whose layers were never touched is still not the
        // toolbar's to move — the case a fresh launch is in.
        let mut fresh = OrderflowView::new("BTCUSDT");
        fresh.config.enabled = true;
        assert!(!fresh.bubbles_enabled(), "the candles open without them");
        assert!(fresh.lane_bubbles_enabled(), "the tape opens with them");
        fresh.set_bubbles_enabled(true);
        fresh.set_bubbles_enabled(false);
        assert!(
            fresh.lane_bubbles_enabled(),
            "there and back again, and the tape never moved"
        );
    }

    /// The tape's own switch: one click takes the band off the canvas, another
    /// puts back the tape that was taken away.
    #[test]
    fn the_tape_switch_takes_the_band_away_and_gives_it_back_unchanged() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.config.enabled = true;
        view.set_lane_depth_visible(false);
        assert!(view.lane_enabled() && view.lane_bubbles_enabled());

        view.set_lane_enabled(false);
        assert!(!view.lane_enabled());
        assert_eq!(
            view.config.live_lane.resolved_width_px(1_000.0),
            0.0,
            "no band is reserved, so the candles take the whole canvas"
        );
        assert!(
            !view.config.aggressions_visible_anywhere(),
            "and nothing is projected for a tape that is not there"
        );
        assert!(
            view.lane_bubbles_enabled() && !view.lane_depth_visible(),
            "the tape's own layer switches are not touched"
        );

        view.set_lane_enabled(true);
        assert!(view.lane_enabled());
        assert!(
            view.lane_bubbles_enabled() && !view.lane_depth_visible(),
            "the tape that comes back is the tape that went away"
        );
    }

    /// Hiding the map over the candles must not delete the tape. The lane is
    /// anchored on the live instant, and that instant used to be answered from
    /// the candles' switch alone — so clearing them took the band, its bubbles
    /// and its strip with it, which is the whole defect this split exists to
    /// remove.
    #[test]
    fn clearing_the_candles_does_not_delete_the_tape_itself() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.config.enabled = true;
        view.config.show_depth = true;
        assert!(view.config.depth_visible_anywhere());

        // Candles clear, tape keeps the map: the anchor survives, so there is
        // still a lane to draw on.
        view.set_depth_visible(false);
        assert!(!view.config.depth_visible());
        assert!(
            view.config.depth_visible_anywhere(),
            "the tape still draws the map, so the lane still has an anchor"
        );

        // Both maps clear, and the tape is still a tape. This is the line the
        // old assertion had backwards: it demanded `None` here, which is a
        // trader switching two map layers off and watching the whole band —
        // bubbles, time axis and the menu that configures it — disappear.
        view.set_lane_depth_visible(false);
        assert!(!view.config.depth_visible_anywhere());
        view.config.live_lane.show_aggressions = true;
        assert!(
            view.config.lane_aggressions_drawn(),
            "the tape draws its bubbles off the trade stream, map or no map"
        );
        assert!(
            view.config.any_layer_enabled(),
            "the tape is still reading, so the live edge may not be gated shut              (that the edge itself comes from prints is proven in `history` and              `orderflow_engine`)"
        );

        // Only when nothing at all is on does the lane stand down.
        view.config.live_lane.show_aggressions = false;
        view.config.show_aggressions = false;
        view.config.projection_demand = false;
        assert!(!view.config.any_layer_enabled());
        assert_eq!(view.live_end_ms(), None);
    }

    /// The tape's window is one field, reachable from the menu and the dock,
    /// and a gesture edits whichever language it is in.
    #[test]
    fn the_tape_window_is_one_field_whichever_door_it_is_set_from() {
        let mut view = OrderflowView::new("BTCUSDT");
        assert_eq!(view.live_lane_window(), LaneWindow::default());

        view.set_live_lane_window(LaneWindow::Fixed { ms: 120_000 });
        assert_eq!(view.live_lane_window(), LaneWindow::Fixed { ms: 120_000 });
        assert_eq!(
            view.config.live_lane.window,
            LaneWindow::Fixed { ms: 120_000 },
            "the menu and the dock read the same field"
        );
        // A pinned tape shows that much market time whatever the bars did.
        assert_eq!(view.live_lane_window_ms(&[]), 120_000);

        // The gesture edits the pinned duration and does not fall back to
        // following the bars.
        view.zoom_live_lane(2.0);
        assert_eq!(view.live_lane_window(), LaneWindow::Fixed { ms: 60_000 });

        // An out-of-range choice is clamped rather than stored to be drawn.
        view.set_live_lane_window(LaneWindow::Fixed { ms: i64::MAX });
        assert_eq!(
            view.live_lane_window(),
            LaneWindow::Fixed {
                ms: MAX_LIVE_LANE_WINDOW_MS
            }
        );

        view.set_live_lane_window(LaneWindow::default());
        assert_eq!(view.live_lane_window(), LaneWindow::default());
    }

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
        view.config.bubbles.consumption_mark = ConsumptionMark::Front;
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
            region_rows: 3,
            region_ms: 2_000,
            bubbles: BubbleStyle {
                max_radius: 42.0,
                side_offset: 8.0,
                ..BubbleStyle::default()
            },
            live_lane: LiveLaneStyle {
                width_share: 0.5,
                window: LaneWindow::Auto { zoom: 2.0 },
                cluster_ms: Some(50),
                radius_scale: 1.6,
                show_marks: true,
                enabled: true,
                show_depth: true,
                show_aggressions: true,
            },
        });
        assert!(view.apply_preset("wide"), "a stored name applies");
        assert_eq!(view.config.bubbles.max_radius, 42.0);
        assert_eq!(view.config.bubble_cluster_ms, 100);
        assert_eq!(view.config.bubble_dust_merge_ms, 3_000);
        assert!(view.config.bubble_candle_summary);
        assert_eq!(view.config.live_lane.width_share, 0.5);
        assert_eq!(view.config.live_lane.window, LaneWindow::Auto { zoom: 2.0 });
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

    fn visible_timeline(bars: &[Bar]) -> VisibleBarTimeline<'_> {
        VisibleBarTimeline::new(0, 0, bars, None)
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
        let first = view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));
        assert!(first.is_none());
        view.flush_for_test();
        let frame = view
            .project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
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
        assert_eq!(
            view.config.live_lane.window,
            LaneWindow::Auto {
                zoom: MAX_LIVE_LANE_ZOOM
            }
        );
        view.zoom_live_lane(1e-6);
        assert_eq!(
            view.config.live_lane.window,
            LaneWindow::Auto {
                zoom: MIN_LIVE_LANE_ZOOM
            }
        );

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
        view.project_visible(visible_timeline(&bars), true, true, None, (100.0, 102.0));
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

    /// The scripted starved tape produces the real thing, not a caption.
    ///
    /// The state this hook exists for — bubbles trailing the lane's right edge
    /// and, past its window, gone from it — is a market condition: a book that
    /// keeps changing while nothing prints. A capture cannot wait for one, and
    /// forging the number into the axis would photograph a claim rather than a
    /// chart. So the hook withholds prints from the tape through the feed's
    /// own call, and the age the chart reports is one it genuinely observed.
    #[test]
    fn the_scripted_starved_tape_ages_for_real() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.set_bubbles_enabled(true);
        view.handle_depth_event(snapshot_event(10));
        // Prints stop reaching the tape two seconds after the first one.
        view.set_starve_tape_after_ms(2_000);

        let print_at = |view: &mut OrderflowView, agg_id: u64, timestamp_ms: i64| {
            view.record_trade(&Trade {
                agg_id,
                timestamp_ms,
                price: Decimal::from(100),
                quantity: Decimal::ONE,
                side: quantick_engine::Side::Buy,
            });
        };
        print_at(&mut view, 1, 10_000);
        print_at(&mut view, 2, 12_000);
        // Past the window: withheld, exactly as a market that stopped printing
        // would have withheld it.
        print_at(&mut view, 3, 14_000);
        print_at(&mut view, 4, 20_000);
        view.flush_for_test();
        assert_eq!(
            view.health().aggression_count,
            2,
            "the tape keeps what arrived before the hook's cutoff"
        );

        // The book carries on, which is the half that makes the gap visible.
        view.handle_depth_event(DepthEvent::Update {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            event_time_ms: 26_000,
            delta: BookDelta::new(
                11,
                11,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(7)).unwrap()],
                Vec::new(),
            ),
        });
        view.flush_for_test();
        assert_eq!(
            view.tape_age(),
            Some(crate::orderflow::TapeAge::Behind(14_000)),
            "the chart reports the age it observed: 26 s of book, 12 s of tape"
        );

        // And with the hook unset the same view feeds every print, so nothing
        // a capture run does can leak into an ordinary session.
        let mut ordinary = OrderflowView::new("BTCUSDT");
        ordinary.set_enabled(true, 10);
        ordinary.set_bubbles_enabled(true);
        ordinary.handle_depth_event(snapshot_event(10));
        for (agg_id, timestamp_ms) in [(1, 10_000), (2, 12_000), (3, 14_000), (4, 20_000)] {
            print_at(&mut ordinary, agg_id, timestamp_ms);
        }
        ordinary.flush_for_test();
        assert_eq!(ordinary.health().aggression_count, 4);
        assert_eq!(
            ordinary.tape_age(),
            None,
            "the tape is ahead of the book: nothing to declare"
        );

        // And with the bubbles switched off on every pane the question is not
        // asked at all: a tape the trader emptied has no missing marks to
        // explain, and a warn-coloured caption there invents a problem.
        let mut depth_only = OrderflowView::new("BTCUSDT");
        depth_only.set_enabled(true, 10);
        depth_only.set_bubbles_enabled(true);
        depth_only.handle_depth_event(snapshot_event(10));
        print_at(&mut depth_only, 1, 6_000);
        depth_only.handle_depth_event(DepthEvent::Update {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            event_time_ms: 20_000,
            delta: BookDelta::new(
                11,
                11,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(7)).unwrap()],
                Vec::new(),
            ),
        });
        depth_only.flush_for_test();
        assert!(
            depth_only.tape_age().is_some(),
            "with the bubbles on the gap is worth declaring"
        );
        depth_only.set_bubbles_enabled(false);
        depth_only.set_lane_bubbles_enabled(false);
        assert_eq!(
            depth_only.tape_age(),
            None,
            "no pane draws the bubbles, so nothing explains their absence"
        );
    }

    /// The starvation clock belongs to the market it is starving.
    ///
    /// A capture run that switches symbol would otherwise open the new one on
    /// a tape that is already dead — the hook photographing the leftovers of
    /// the market before it rather than the state it was asked for.
    #[test]
    fn switching_symbol_restarts_the_scripted_starvation() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_bubbles_enabled(true);
        view.set_starve_tape_after_ms(2_000);
        let print_at = |view: &mut OrderflowView, agg_id: u64, timestamp_ms: i64| {
            view.record_trade(&Trade {
                agg_id,
                timestamp_ms,
                price: Decimal::from(100),
                quantity: Decimal::ONE,
                side: quantick_engine::Side::Buy,
            });
        };
        print_at(&mut view, 1, 10_000);
        print_at(&mut view, 2, 20_000);
        view.flush_for_test();
        assert_eq!(view.health().aggression_count, 1, "the cutoff bit");

        view.reset_for_symbol("WINV26");
        view.set_bubbles_enabled(true);
        // The same instants that were past the old cutoff are the new tape's
        // first seconds, and they arrive.
        print_at(&mut view, 3, 20_000);
        print_at(&mut view, 4, 21_500);
        view.flush_for_test();
        assert_eq!(
            view.health().aggression_count,
            2,
            "the new market's tape opened starved"
        );
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
        view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));
        view.flush_for_test();
        let frame = view
            .project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
            .expect("published frame");
        // One print, two marks, and both are meant: the tape draws it where it
        // landed, and the bar it belongs to counts it into the running summary
        // pie the active preset asks for. The tape exists here at all only
        // because the live edge now comes from prints — with capture off it
        // used to come from the book, so there was no tape and no mark on it.
        assert_eq!(
            frame
                .projection
                .aggressions
                .iter()
                .filter(|mark| mark.live)
                .count(),
            1,
            "the tape draws the print"
        );
        assert_eq!(
            frame
                .projection
                .aggressions
                .iter()
                .filter(|mark| !mark.live)
                .count(),
            1,
            "and its bar counts it into the summary"
        );
        assert!(frame.projection.cells.is_empty(), "no map without capture");

        // Turning the bubbles off over the candles does *not* close the
        // pipeline: the tape draws them too, and it was never asked to stop.
        // This is the split's whole point — the projection now answers to both
        // panes, so it stands down only when neither is reading it.
        view.set_bubbles_enabled(false);
        assert!(view.lane_bubbles_enabled(), "the tape kept them");
        assert!(
            view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
                .is_some(),
            "a tape nobody switched off may not lose the frame that feeds it"
        );

        // Switching the tape's own copy off too leaves nobody reading, and the
        // pipeline closes exactly as it always did.
        view.set_lane_bubbles_enabled(false);
        view.flush_for_test();
        assert!(
            view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
                .is_none()
        );
    }

    /// Hiding the badge silences chrome, never a dead feed. The badge is the
    /// only real-time statement that the depth on screen has stopped being the
    /// book — the loading overlay covers the waiting states only, and the dock
    /// strip carries no status at all — so a failure re-asserts it whatever
    /// the switch says.
    #[test]
    fn a_failing_book_says_so_even_with_the_badge_switched_off() {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        view.flush_for_test();

        let badge_text = |view: &mut OrderflowView| {
            view.sync_published();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.draw_status_badge(ui.painter(), rect, 0.0);
                });
            });
            let mut text = String::new();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                }
            }
            text
        };

        assert!(badge_text(&mut view).contains("book"), "a healthy badge");
        view.set_status_badge_visible(false);
        assert!(
            badge_text(&mut view).is_empty(),
            "a healthy book stays quiet once the trader silences it"
        );

        // The feed drops. The switch has not moved, and the badge is back.
        view.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            status: quantick_orderbook::DepthStatus::Disconnected {
                error_class: "websocket",
            },
        });
        view.flush_for_test();
        assert!(!view.status_badge_visible(), "the switch did not move");
        let failing = badge_text(&mut view);
        assert!(
            failing.contains("book down"),
            "a dead book may never be hidden chrome: {failing:?}"
        );
    }

    /// The canvas's key is chrome about the canvas, not a tail of the
    /// bubbles: it names the depth layers too. It draws in a pass of its own,
    /// so hiding the bubbles leaves it standing, and the trader can silence it
    /// from the canvas's right-click menu without touching a single layer.
    #[test]
    fn the_legend_draws_on_its_own_pass_and_the_trader_can_silence_it() {
        let ctx = egui::Context::default();
        let viewport = Viewport::new();
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        view.flush_for_test();
        let bars = [bar(900, 1_100)];
        view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));
        view.flush_for_test();
        let frame = view
            .project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
            .expect("published frame");

        let text_of = |view: &OrderflowView, legend: bool| {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if legend {
                        view.draw_legend(
                            ui.painter(),
                            rect,
                            &viewport,
                            1,
                            &frame,
                            egui::Color32::BLACK,
                            0.0,
                            crate::orderflow_render::LEGEND_HEADER_CLEARANCE_PX,
                        );
                    } else {
                        view.draw_aggressions(
                            ui.painter(),
                            rect,
                            &viewport,
                            1,
                            &frame,
                            egui::Color32::BLACK,
                            0.0,
                            false,
                        );
                    }
                });
            });
            let mut text = String::new();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                    text.push(' ');
                }
            }
            text
        };

        // The bubble pass writes no key…
        assert!(
            !text_of(&view, false).contains("liquidity"),
            "the bubbles must not carry the legend on their back"
        );
        // …the key's own pass does, with the bubble layer off.
        view.set_bubbles_enabled(false);
        assert!(
            text_of(&view, true).contains("liquidity"),
            "the key stands with the bubbles hidden"
        );
        // And the right-click switch silences it outright.
        view.set_legend_visible(false);
        assert!(!view.legend_visible());
        assert!(
            text_of(&view, true).is_empty(),
            "a silenced key draws no text at all"
        );
    }

    /// The live strip is a consumer in its own right: it draws the forming
    /// bar's clusters beside the price axis, from the same engine path the
    /// bubbles use. Hiding the bubbles must not blank it — "essa parte deve
    /// permanecer calculando … mesmo desabilitando a bolha".
    #[test]
    fn the_live_strip_alone_keeps_the_aggression_pipeline_running() {
        let mut view = OrderflowView::new("BTCUSDT");
        // No depth capture, no bubbles anywhere — the tape's included, so the
        // strip really is the only surface asking.
        view.set_lane_enabled(false);
        view.set_projection_demand(true);
        assert!(!view.enabled(), "a strip must not start book capture");
        assert!(!view.bubbles_enabled(), "and it draws no bubbles");

        view.record_trade(&Trade {
            agg_id: 1,
            timestamp_ms: 1_000,
            price: Decimal::new(1_005, 1),
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        });
        view.flush_for_test();
        assert_eq!(view.health().aggression_count, 1, "the print was retained");

        let bars = [bar(900, 1_100)];
        view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));
        view.flush_for_test();
        let frame = view
            .project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
            .expect("the strip's own frame");
        // The one print reaches the frame twice on purpose: once on the tape,
        // which exists here because the live edge comes from prints rather than
        // from a book nobody is capturing, and once inside the running summary
        // pie of the bar it landed in. What matters to this test is that the
        // clusters exist at all while the bubble layer is off — the strip is
        // reading them.
        assert_eq!(
            frame.projection.aggressions.len(),
            2,
            "the strip reads the clusters the hidden bubbles would have drawn"
        );
        assert!(
            !live_strip::aggression_rows(
                &frame.projection.aggressions,
                900,
                frame.projection.summarized,
                frame.projection.effective_grouping.bucket_width,
            )
            .is_empty(),
            "and they become histogram rows"
        );

        // Drop the demand and the pipeline closes, exactly as before: nothing
        // keeps running for a surface nobody is showing.
        view.set_projection_demand(false);
        assert!(
            view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
                .is_none()
        );
    }

    #[test]
    fn disabling_capture_drops_the_published_frame() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];
        view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));
        view.flush_for_test();
        assert!(
            view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
                .is_some()
        );

        // Capture stops. The tape is still drawing prints, and prints do not
        // come from the book — so the frame it reads must survive, carrying
        // aggressions and no depth. Blanking it here would take a live surface
        // down with a recorder nobody was watching.
        view.set_enabled(false, 11);
        view.flush_for_test();
        assert!(
            view.lane_bubbles_enabled(),
            "the tape was never asked to stop"
        );
        assert!(
            view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
                .is_some()
        );

        // With the tape off as well nobody is reading, and the frame goes.
        view.set_lane_enabled(false);
        assert!(
            view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0))
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
