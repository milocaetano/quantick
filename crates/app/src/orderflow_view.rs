//! egui facade for the asynchronous order-flow heatmap.
//!
//! All book state lives in [`quantick_orderflow::engine::BookEngine`] on the worker thread owned by
//! [`crate::orderflow_worker::BookWorker`]. This layer forwards commands, mirrors the published
//! snapshot and converts normalized primitives into egui shapes, always drawing the latest
//! already-built frame so a dense book never blocks the UI.

use eframe::egui;
use quantick_chart::{connection_state::Link, state::ChartState};
use quantick_engine::{Bar, Trade};
use quantick_orderbook::DepthEvent;
pub(crate) use quantick_orderflow::FlowCellHit;
use quantick_orderflow::engine::{BookLadder, BookPublished, CaptureStatus, OrderflowHealth};
use quantick_orderflow::pane_tape::PaneTape;
use quantick_orderflow::projection::GapPrimitive;
use quantick_orderflow::{
    BubbleScaleTarget, HeatmapConfig, LaneWindow, LiveLaneStyle, lane_window_pick, reserved_span_ms,
};
use quantick_stores::bubble_asset_store::AssetBinding;
use quantick_stores::bubble_asset_store::SaveSwitch;
use quantick_stores::bubble_assets::AssetBubbles;
use quantick_stores::bubble_look::{BubbleLook, LoadedPresets};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::bubble_presets;
use crate::orderflow_render::{OrderflowRenderStyle, RenderContext};
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::viewport::Viewport;

#[cfg(test)]
#[path = "orderflow_view/tests/asset_look_tests.rs"]
mod asset_look_tests;
mod constants;
pub(crate) mod flow_execution;
mod frame;
pub(crate) mod layers;
#[cfg(test)]
#[path = "orderflow_view/tests/native_split_tests.rs"]
mod native_split_tests;
#[cfg(test)]
#[path = "orderflow_view/tests/pending_tests.rs"]
mod pending_tests;
mod settings;
#[cfg(test)]
#[path = "orderflow_view/tests/tape_frame_tests.rs"]
mod tape_frame_tests;

/// Borrowed chart timeline for one order-flow projection request; the boundary revision sits beside
/// the exact bar slice so a new timeline never pairs with an old cache identity.
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

/// The live lane's band and the instant it runs to, read together from one frame's published book -
/// see [`OrderflowView::live_lane`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveLane {
    /// Width of the band, in pixels, taken off the chart's right edge.
    pub width_px: f32,
    /// Exchange timestamp at the band's right edge: the live edge.
    pub end_ms: i64,
}

/// Stateful UI/controller facade for the optional heatmap.
pub struct OrderflowView {
    /// The panel that edits this view's settings, attached to its flow pane.
    pub(crate) settings_panel: quantick_workspace::orderflow_panel::OrderFlowPanel,
    flow_execution: flow_execution::FlowExecutionView,
    flow_paint: std::cell::RefCell<quantick_chart::flow_execution::FlowPaintState>,
    symbol: String,
    /// UI mirror of the engine configuration. The engine owns
    /// `price_grouping` (auto-base can rewrite it); the mirror adopts engine
    /// changes through [`Self::sync_published`].
    config: HeatmapConfig,
    /// The book thread: commands in, publications out.
    worker: BookWorker,
    published: BookPublished,
    /// Engine bucket last adopted into the mirror, to detect auto-base moves.
    last_seen_base: Decimal,
    /// The `(step, reference_price)` pair last sent to the engine, so the per-trade path sends one
    /// command per change. Both halves are in the key: keying on the step alone would swallow the
    /// first magnitude a chart learns whenever the grid settled first.
    last_tape_price_grid: Option<(Decimal, Option<Decimal>)>,
    capture_grouping_draft: f64,
    pending_capture_grouping_previous: Option<Decimal>,
    /// The bubble look on screen: its presets, the asset it is kept for and
    /// what launch hooks hold over it.
    look: BubbleLook,
    /// Scripted tape starvation: prints stop reaching the tape this many
    /// milliseconds after the first one, while the book keeps arriving.
    /// `None` — always, outside a capture run — feeds the tape every print.
    starve_tape_after_ms: Option<i64>,
    /// Instant of the first print this view ever saw, the starvation clock's
    /// zero. Read only when the hook above is set.
    first_print_ms: Option<i64>,
    /// The volume-dot rungs in use, held through an autoscale wobble.
    dot_rungs: quantick_orderflow::DotRungMemory,
    /// The lane's market clock and the prints drawn before the worker
    /// publishes them.
    pane_tape: PaneTape,
    tape_dots: std::cell::RefCell<quantick_orderflow::projection::TapeDotMemory>,
    /// The tape's reconciliations too large for a frame, run beside it.
    tape_rebuilds: std::cell::RefCell<crate::orderflow_render::PaneTapeRebuilds>,
    /// Where the tape's right edge is held, and the frozen past it draws.
    tape_end: quantick_orderflow::tape_view::TapeEnd,
    past_dots: std::cell::RefCell<quantick_orderflow::projection::PastTapeMemory>,
    past_heat: std::cell::RefCell<quantick_orderflow::projection::PlacedPastHeat>,
    /// The lane divider's drag in progress, from press to release.
    lane_drag: Option<quantick_orderflow::LaneDividerDrag>,
    /// The live strip's width left of the divider, as the pane's last frame carved it.
    pub(crate) trail_strip_px: f32,
}

impl OrderflowView {
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        let mut config = HeatmapConfig::default();
        // The presets file is the record of how the tape should look, so the
        // chart opens on its active preset instead of the compiled defaults.
        let look = BubbleLook::open(bubble_presets::load(), &mut config);
        let base_grouping = config.price_grouping;
        Self {
            flow_execution: Default::default(),
            flow_paint: Default::default(),
            worker: BookWorker::spawn(&symbol),
            symbol,
            config,
            settings_panel: Default::default(),
            published: BookPublished::initial(),
            last_seen_base: base_grouping,
            last_tape_price_grid: None,
            capture_grouping_draft: base_grouping.to_f64().unwrap_or(0.01),
            pending_capture_grouping_previous: None,
            look,
            starve_tape_after_ms: None,
            first_print_ms: None,
            dot_rungs: Default::default(),
            pane_tape: PaneTape::default(),
            tape_dots: Default::default(),
            tape_rebuilds: Default::default(),
            tape_end: Default::default(),
            past_dots: Default::default(),
            past_heat: Default::default(),
            lane_drag: None,
            trail_strip_px: 0.0,
        }
    }

    /// The health mirror already used by the last application frame. Unlike [`Self::health`] it
    /// does not synchronize with the worker, so a control capture reads the frame the user saw and
    /// one scope cannot advance another underneath it.
    #[must_use]
    pub(crate) fn cached_health(&self) -> &OrderflowHealth {
        &self.published.health
    }

    /// The book frame the last application frame published, for a control capture. Same no-sync
    /// contract as [`Self::cached_health`]. The ladder is `None` while capture is off or the book
    /// has no snapshot yet: an honest absence, never an empty book.
    #[must_use]
    pub(crate) fn cached_book(
        &self,
    ) -> (
        &CaptureStatus,
        Option<&BookLadder>,
        Decimal,
        &[GapPrimitive],
    ) {
        (
            &self.published.status,
            self.published.ladder.as_deref(),
            self.published.base_price_grouping,
            self.pane_tape
                .drawn(self.published.frame.as_ref())
                .map_or(&[], |frame| frame.projection.gaps.as_slice()),
        )
    }

    /// The live lane's right edge as the last application frame published it, under the same
    /// no-sync contract as [`Self::cached_health`]. `None` when no flow layer is drawn: a chart
    /// with no lane has no edge to report.
    #[must_use]
    pub(crate) fn cached_live_end_ms(&self) -> Option<i64> {
        let cap = self.pane_tape.window_cap_ms().unwrap_or(i64::MAX);
        self.lane_now_ms()
            .or(self.published.live_end_ms.map(|end| end.min(cap)))
    }

    /// Snapshot only published tape state; capturing never wakes the worker.
    pub(crate) fn bubbles_snapshot(
        &self,
    ) -> quantick_control_schema::orderflow::BubblesStateSnapshot {
        quantick_control_schema::bubble_preset::snapshot(
            self.cached_config(),
            self.cached_health().floored_quantity,
            self.dot_scale(),
            &self.recorded_opening_bursts(),
            &self.look,
        )
    }

    /// The rungs and scales of the last published volume-dots frame.
    pub(crate) fn dot_scale(&self) -> Option<&quantick_orderflow::DotScale> {
        self.pane_tape
            .frame()
            .or(self.published.frame.as_ref())?
            .volume_dots
            .as_ref()
    }

    /// Where the last published tape traded, `(low, high)`.
    #[must_use]
    pub(crate) fn tape_price_range(&self) -> Option<(f64, f64)> {
        if let (Some(_), Some(past)) = (self.tape_end.past_ms(), &self.published.past_tape) {
            return past.price_range();
        }
        if self.config.immediate_tape() {
            let window_ms = self
                .config
                .lane_window_ms(quantick_orderflow::engine::PENDING_LANE_REFERENCE_MS);
            let retained = self.lane_now_ms().and_then(|now| {
                self.tape_rebuilds
                    .borrow()
                    .price_range(&self.tape_dots.borrow(), now, window_ms)
            });
            return self
                .pane_tape
                .pending()
                .price_range(
                    self.published
                        .frame
                        .as_ref()
                        .map(|frame| frame.projection.as_ref()),
                    self.lane_now_ms(),
                    window_ms,
                )
                .into_iter()
                .chain(retained)
                .reduce(|(low, high), (next_low, next_high)| {
                    (low.min(next_low), high.max(next_high))
                });
        }
        let frame = self.published.frame.as_deref()?;
        quantick_orderflow::projection::tape_price_range(&frame.projection.aggressions)
    }

    /// The heatmap setup this chart is drawing with, for a control capture.
    #[must_use]
    pub(crate) fn cached_config(&self) -> &HeatmapConfig {
        &self.config
    }

    /// Resolve the topmost displayed L2 heat cell under `position` from the frame the chart
    /// most recently painted ([`PaneTape::drawn`]), the candles ending at `history_right` and
    /// the tape right of them and of the strip, as painted. Projection and hit testing share
    /// [`Self::projected`]; the lookup is O(visible cells), only for a cursor snapshot.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn control_flow_cell_at(
        &self,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        history_right: f32,
        inverted: bool,
        position: egui::Pos2,
    ) -> Option<FlowCellHit> {
        let frame = &**self.pane_tape.drawn(self.published.frame.as_ref())?;
        let lane_px = (chart_rect.right() - history_right - self.trail_strip_px).max(0.0);
        let layout = self.projected((chart_rect, viewport, total_bars), frame, lane_px);
        let layout = layout.with_inverted(inverted);
        let in_lane = layout
            .lane_left_x()
            .is_some_and(|divider| position.x >= divider);
        let layer_visible = if in_lane {
            self.config.lane_depth_drawn()
        } else {
            self.config.depth_visible()
        };
        if !layer_visible || !chart_rect.contains(position) {
            return None;
        }

        let style =
            OrderflowRenderStyle::from_config(&self.config, egui::Color32::TRANSPARENT.to_array());
        // Over a held tape the cursor reads the book painted there.
        let held = if in_lane { self.held_heat(frame) } else { None };
        let (projection, layout) = match &held {
            Some(heat) => (heat, layout.with_lane_only()),
            None => (&frame.projection, layout),
        };
        let layout = RenderContext::new(projection, layout, &style).layout;
        let preview = |cell: &quantick_orderflow::HeatmapCell| {
            quantick_orderflow::projection::current_book_preview(
                cell,
                quantick_orderflow::projection::forward_book_preview(
                    &self.config.live_lane,
                    frame.current_book_live,
                ),
            ) && layout
                .current_book_preview_rect(cell.y0, cell.y1)
                .contains(position)
        };
        let cell = projection.cells.iter().rev().find(|cell| {
            layout
                .heat_cell_rect(cell, style.min_cell_height, frame.current_book_live)
                .contains(position)
                || preview(cell)
        })?;

        FlowCellHit::from_cell(
            cell,
            projection.effective_grouping.bucket_width,
            quantick_orderflow::projection::CellHitContext {
                first_bar_index: frame.first_bar_index,
                slot_count: frame.slot_count,
                has_lane: frame.projection.live_now_x.is_some(),
                held_tape_end_ms: held.as_ref().and(self.tape_end.past_ms()),
                held: held.is_some(),
                current_book_preview: preview(cell),
                current_book_source: projection.current_book,
            },
        )
    }

    /// Pull the newest worker snapshot into this frame's mirror. Cheap: one
    /// mutex lock and a small clone (frames are shared through `Arc`).
    fn sync_published(&mut self) {
        let mut publication = self.worker.publication();
        if let Some(receipt) = publication.tape_receipt
            && receipt.epoch == self.pane_tape.pending().epoch()
        {
            self.pane_tape
                .pending_mut()
                .acknowledge(receipt.through_ordinal);
        } else {
            publication.book.frame = None;
            publication.book.past_tape = None;
        }
        self.published = publication.book;
        let base = self.published.base_price_grouping;
        self.adopt_base(base);
    }

    /// Take an engine-chosen capture bucket into the UI mirror. Split out of
    /// [`Self::sync_published`] because the footprint's row width needs the bucket without the rest
    /// of the published state.
    fn adopt_base(&mut self, base: Decimal) {
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

    /// The capture bucket, taking whatever the engine has published since the last look.
    /// The UI mirror can lag while its consumers are hidden. Reading through here keeps
    /// profile and footprint rows from depending on a diagnostics log having run.
    pub fn capture_grouping_now(&mut self) -> Decimal {
        let base = self.worker.published_base_grouping();
        // The mirror adopts the same base as every drawing consumer.
        self.published.base_price_grouping = base;
        self.adopt_base(base);
        base
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

    /// Show or hide the depth map over the candles, without touching L2 capture. No feed command is
    /// involved: the recorder keeps running, so turning the map back on repaints the retained past.
    ///
    /// **The tape is not touched, in either direction.** This is the toolbar's switch and it
    /// governs the candles; the tape holds a value of its own ([`Self::set_lane_depth_visible`]).
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

    /// Toggle the aggression layer over the candles, without touching L2 capture (aggregate trades
    /// already flow for the candles). The tape is untouched in either direction, as with the depth
    /// map.
    pub fn set_bubbles_enabled(&mut self, enabled: bool) {
        if self.config.show_aggressions == enabled {
            return;
        }
        let before = self.config.clone();
        self.config.show_aggressions = enabled;
        self.commit_config_changes(before);
    }

    /// Squeeze the frame's bubble budget through the field the projection reads. The scripted way
    /// to reach a folded frame, which needs a tape dense enough to exhaust the budget and so cannot
    /// otherwise be arranged.
    #[cfg(any(feature = "scenario-harness", test))]
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

    /// Put the tape on the canvas, or take it off. Off, the lane reserves no width and asks for no
    /// projection; its two layer switches stay as they were, so switching it back on returns the
    /// same tape, beside the candles at the default width ([`quantick_orderflow::LiveLaneStyle::set_enabled`]).
    pub fn set_lane_enabled(&mut self, enabled: bool) {
        self.edit_lane(|lane| lane.set_enabled(enabled));
    }

    /// One write to the lane's switches, committed if it moved anything.
    pub(crate) fn edit_lane(&mut self, edit: impl FnOnce(&mut LiveLaneStyle) -> bool) -> bool {
        let before = self.config.clone();
        let changed = edit(&mut self.config.live_lane);
        if changed {
            self.commit_lane_switch(before);
        }
        changed
    }

    /// Show `view`, from any view, through the one write every door shares
    /// (the header's switches, their keys, the named call). Answers whether it changed.
    pub fn set_lane_view(&mut self, view: quantick_orderflow::LaneView) -> bool {
        self.edit_lane(|lane| lane.set_view(view))
    }

    /// The view by the side it closes, with the market's hold standing down: the
    /// rail's click and `orderflow.tape.fold.set`. Answers whether it changed.
    pub fn set_lane_fold(&mut self, fold: quantick_orderflow::LaneFold) -> bool {
        self.edit_lane(|lane| lane.set_fold(fold))
    }

    /// One step of the market-closed fold: only a hold that moves is news.
    pub(crate) fn observe_market(
        &mut self,
        feed: quantick_chart::connection_state::ConnectionState,
    ) {
        let next = quantick_chart::market_fold::next_for(&self.config.live_lane, feed);
        let before = (next != self.config.live_lane.market_hold).then(|| self.config.clone());
        self.config.live_lane.market_hold = next;
        before.map(|before| self.apply_config(before));
    }

    /// Stop the tape's window short of the clock while nothing is coming.
    pub(crate) fn hold_tape_window(&mut self, spoken: Option<Link>, state: &ChartState) {
        let hold = quantick_chart::market_fold::tape_hold(spoken, state, || {
            self.live_lane_window_ms(state.bars())
        });
        self.pane_tape.set_hold(hold);
    }

    /// Where a hold has stopped this pane's heat while its book goes on arriving.
    pub(crate) fn book_held_at_ms(&self) -> Option<i64> {
        use quantick_chart::{connection_state::Book, market_fold::book_held_at};
        let drawn = self.config.depth_visible_anywhere();
        let book = Book::when_drawn(drawn, &self.published.status);
        book_held_at(
            self.pane_tape.window_cap_ms(),
            self.published.live_end_ms,
            book,
        )
    }

    fn commit_lane_switch(&mut self, before: HeatmapConfig) {
        if !self.config.depth_visible_anywhere() {
            // Drop the local frame immediately; the worker clears its own.
            self.published.frame = None;
        }
        self.commit_config_changes(before);
    }

    /// The band's width on a canvas this wide, zero when the tape is off. The number the tape
    /// switch reaches the canvas through, and the one [`Self::live_lane`] reports when there is a
    /// live edge to anchor the band on.
    #[must_use]
    pub fn lane_width_px(&self, chart_width: f32) -> f32 {
        self.config.live_lane.resolved_width_px(chart_width)
    }

    /// The candles' depth switch alone, whatever capture lets through it. [`Self::depth_visible`]
    /// answers "is it drawn" (renderer); this answers "did anyone ask for it" (persistence). On a
    /// source with no book the map is undrawn however the switch stands, and saving that as the
    /// trader's answer would outrank the shipped default on every market. The rule
    /// [`Self::set_depth_visible`] uses.
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
        // Compared against the switch, not what capture allows through it (as in the candles'
        // `set_depth_visible`), or a source with no book would swallow "off" and spring the layer
        // back when one arrived.
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

    /// State that a surface other than the bubbles (the live strip beside the price axis) reads the
    /// aggression clusters this frame. The pane says it every frame from the layer it owns; with
    /// the bubbles hidden and the strip shown it keeps prints retained and projected, so a live
    /// surface nobody switched off does not blank.
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

    /// Show or hide the chart legend: the field the L2 panel's checkbox writes, so the right-click
    /// menu and the panel never disagree. Chrome only: every layer it names keeps drawing.
    pub fn set_legend_visible(&mut self, visible: bool) {
        if self.config.show_legend == visible {
            return;
        }
        let before = self.config.clone();
        self.config.show_legend = visible;
        self.commit_config_changes(before);
    }

    /// Whether intervals with no depth coverage are marked out.
    #[must_use]
    pub fn gaps_visible(&self) -> bool {
        self.config.show_gaps
    }

    /// Show or hide the gap boundaries, the field behind the dock's "L2 gap" checkbox. It hides a
    /// statement about missing data rather than data, so the menu entry warns that an unrecorded
    /// stretch will then look recorded.
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
        // Any flow layer, not the depth map: this instant anchors the tape, and asking the map made
        // the tape a hostage of L2 (both maps off deleted the band; a feed with no book never had
        // one). Each pane answers for its own canvas; the tape for its own existence.
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        self.cached_live_end_ms()
    }

    /// The live lane as the chart needs it: how wide its band is, and the instant its right edge
    /// stands for. The candles own everything left of the band and the lane owns it whatever they
    /// do, so panning or zooming never moves the tape and the newest prints stay on screen.
    ///
    /// One call because it is one look at the published book; two reads would send the render
    /// thread back through the worker's mutex for a number it already had. `None` when there is no
    /// live edge, i.e. this chart has no lane.
    #[must_use]
    pub fn live_lane(&mut self, chart_width: f32) -> Option<LiveLane> {
        let end_ms = self.tape_end.end_ms(self.live_end_ms()?);
        Some(LiveLane {
            width_px: self.lane_width_px(chart_width),
            end_ms,
        })
    }

    /// Widen or narrow the lane by a pixel drag on its divider. Dragging left
    /// (negative `delta_px`) gives the tape more room, at the expense of the
    /// history beside it; near either edge that side closes
    /// ([`HeatmapConfig::drag_lane_divider`]).
    pub fn resize_live_lane(&mut self, delta_px: f32, chart_width: f32) {
        let before = self.config.clone();
        let moved = self
            .config
            .drag_lane_divider(&mut self.lane_drag, delta_px, chart_width);
        // The width is navigation; a view is a setting.
        if moved {
            self.commit_lane_switch(before);
        } else {
            self.apply_config(before);
        }
    }

    /// The divider let go, or taken again: a close only shown is written, once.
    pub fn release_lane_drag(&mut self) {
        if self.lane_drag.take().is_some() {
            self.edit_lane(LiveLaneStyle::release_drag_close);
        }
    }

    /// Zoom the lane's time window by a multiplicative factor: `> 1` shows less market time in the
    /// same band (prints run faster and further apart), `< 1` shows more. The gesture speaks
    /// whichever language the window is in (zoom while following the bars, milliseconds while
    /// pinned) and never switches it: dragging a pinned tape gives a different pinned tape.
    pub fn zoom_live_lane(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.config.clone();
        self.config.zoom_lane_window(factor);
        self.apply_config(before);
    }

    /// How much market time the tape shows, and in which language it was
    /// asked for.
    #[must_use]
    pub fn live_lane_window(&self) -> LaneWindow {
        self.config.live_lane.window
    }

    /// Choose how the tape's window is decided: a preset, a custom duration,
    /// or back to following the bars. A setting of the asset on screen, even
    /// when the wheel already shows that window.
    pub fn set_live_lane_window(&mut self, window: LaneWindow) {
        self.navigate_live_lane_window(window);
        self.look.note_lane_set(&self.config);
    }

    /// The tape's window as the asset files it ([`BubbleLook::lane_window_setting`]).
    #[must_use]
    pub fn lane_window_setting(&self) -> LaneWindow {
        self.look.lane_window_setting()
    }

    /// The tape's window back to its setting: navigation undone, the asset untouched.
    pub fn reset_live_lane_window(&mut self) {
        self.navigate_live_lane_window(self.lane_window_setting());
    }

    /// The window a call names: filed as the asset's under the selector's own rule
    /// ([`lane_window_pick`]), or navigation, as the wheel's.
    pub fn move_live_lane_window(&mut self, window: LaneWindow, file: bool) {
        if !file {
            return self.navigate_live_lane_window(window);
        }
        let shown = self.live_lane_window();
        if let Some(window) = lane_window_pick(shown, self.lane_window_setting(), window) {
            self.set_live_lane_window(window);
        }
    }

    /// Move the tape's window as the wheel does: the view, not the asset.
    pub fn navigate_live_lane_window(&mut self, window: LaneWindow) {
        let before = self.config.clone();
        self.config.live_lane.window = window;
        self.config.live_lane.window.sanitize();
        self.apply_config(before);
    }

    /// Hold what a launch hook asks for this run ([`RunHold`]); never filed.
    #[cfg(any(feature = "scenario-harness", test))]
    pub fn hold_for_run(
        &mut self,
        hold: impl FnOnce(&mut quantick_stores::bubble_asset_store::RunHold),
    ) {
        let before = self.config.clone();
        self.look.hold_for_run(hold, &mut self.config);
        self.apply_config(before);
    }

    /// Market time the lane is showing right now, in milliseconds — the label
    /// under it, and the only readout of what the zoom is worth.
    #[must_use]
    pub fn live_lane_window_ms(&self, closed: &[Bar]) -> i64 {
        self.config.lane_window_ms(reserved_span_ms(closed))
    }

    /// How old the newest aggression on the tape is, against the instant the lane's right edge
    /// stands for. The edge follows the newer of the book clock and the print clock, but only the
    /// print clock places bubbles, so when the book runs ahead every bubble is drawn this far left
    /// of the edge and past the lane's window none is drawn. The axis says so rather than letting
    /// an empty tape read as a market that stopped trading. `None` also when no pane draws the
    /// bubbles.
    #[must_use]
    pub fn tape_age(&self) -> Option<quantick_orderflow::TapeAge> {
        // Asked only when a pane still draws the bubbles: a tape showing liquidity with its bubbles
        // deliberately off has no missing marks to explain.
        if !self.config.aggressions_visible_anywhere() {
            return None;
        }
        self.published.health.tape_age
    }

    /// Reset market-specific history while preserving visual/retention
    /// settings. Capture deliberately returns to off; the app starts a fresh
    /// provider task only after the new feed handle is installed.
    pub fn reset_for_symbol(&mut self, symbol: impl Into<String>) {
        self.reset_flow_executions();
        self.symbol = symbol.into();
        self.pane_tape.reset_clock();
        self.config.enabled = false;
        // A hold and a hand's stand-down are about the market being left. Here and not
        // in a preset's `apply_to`, which keeps them: a look chosen mid-spell is no
        // new market. Through the door, as the worker lays its frames out by it.
        let before = self.config.clone();
        self.config.live_lane.market_hold = Default::default();
        self.apply_config(before);
        self.pending_capture_grouping_previous = None;
        // The send-once cache is keyed on what was sent, so a new market printing on the same grid
        // would be suppressed, and a grid the engine refused after a manual bucket would never be
        // re-offered.
        self.last_tape_price_grid = None;
        self.published = BookPublished::initial();
        self.pane_tape.pending_mut().clear_opening_bursts();
        // The starvation clock is per market, like the history it starves; carrying the old zero
        // across would open the new symbol on an already-dead tape.
        self.first_print_ms = None;
        self.worker
            .send(BookCommand::ResetForSymbol(self.symbol.clone()));
        self.reset_pending_tape();
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
        self.pane_tape
            .pending_mut()
            .observe_opening_burst(trade.timestamp_ms);
        if self.config.immediate_tape() {
            // Same-frame tape facts; heavy book history stays on the worker.
            let print = self.pane_tape.record(trade, &self.config);
            if let Some(epoch) = print.opens {
                self.worker.send(BookCommand::ApplyVisualConfig(Box::new(
                    self.config.clone(),
                )));
                self.worker.send(BookCommand::TapeEpoch(epoch));
            }
            self.worker.send(BookCommand::TapeTrade {
                ordinal: print.ordinal,
                trade: trade.clone(),
            });
        } else {
            self.worker.send(BookCommand::Trade(trade.clone()));
        }
    }

    /// Tell the engine the price grid the tape prints on, and the magnitude it prints at. Both,
    /// because a tick alone does not size a row (BTCUSDT's tick is a cent, its rows are dollars); a
    /// chart with no L2 otherwise fell back to raw cents.
    ///
    /// Sent only when the answer changes, which a running GCD makes rare: it only narrows, and the
    /// magnitude freezes at the first print, so a session pays for one regroup rather than one per
    /// trade (this is the per-trade path).
    ///
    /// Deliberately *not* gated on a layer being enabled, unlike
    /// [`record_trade`](Self::record_trade): the grid also sizes the footprint ladder, which needs
    /// right-width rows with the heatmap off.
    pub fn observe_tape_price_grid(&mut self, step: Decimal, reference_price: Option<Decimal>) {
        let grid = (step, reference_price);
        if self.last_tape_price_grid == Some(grid) {
            return;
        }
        self.last_tape_price_grid = Some(grid);
        self.worker.send(BookCommand::TapePriceGrid {
            step,
            reference_price,
        });
    }

    /// Whether the scripted starvation hook is holding this print back. Free when off: the option
    /// is `None` outside a capture run, so a live tape pays one `is_some` per print.
    fn starved_at(&mut self, timestamp_ms: i64) -> bool {
        let Some(after_ms) = self.starve_tape_after_ms else {
            return false;
        };
        let first = *self.first_print_ms.get_or_insert(timestamp_ms);
        timestamp_ms.saturating_sub(first) > after_ms
    }

    /// Stop feeding the tape once the session is `after_ms` old, leaving the book running. The
    /// scripted way to reach a starved tape, a market state (a book that changes while nothing
    /// prints) no setting produces. It withholds prints through the same call the feed uses rather
    /// than forging the caption, so the axis reports the age it genuinely observes. Bars,
    /// indicators and the simulator are fed upstream and untouched, which is the contrast under
    /// test.
    #[cfg(any(feature = "scenario-harness", test))]
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

    pub fn health(&mut self) -> OrderflowHealth {
        self.sync_published();
        self.published.health.clone()
    }

    /// Timestamp of the newest accepted book event, from the frame's mirror. Read-only twin of
    /// [`Self::health`] for callers holding `&self`, like the status bar's tape-age readout.
    #[must_use]
    pub fn last_event_ms(&self) -> Option<i64> {
        self.published.health.last_event_ms
    }

    /// Whether the map is open but not yet (or no longer) backed by a live book, which the app's
    /// loading overlay mirrors. Reads the frame's mirror; which statuses count as a wait is
    /// [`CaptureStatus::is_syncing`]'s call. Visibility-gated: the recorder synchronizing in the
    /// background is not worth a loading overlay.
    #[must_use]
    pub fn is_syncing(&self) -> bool {
        self.config.depth_visible() && self.published.status.is_syncing()
    }

    /// The book as the connection state reads it: off when nobody asked to see it.
    #[must_use]
    pub fn connection_book(&self) -> quantick_chart::connection_state::Book {
        let drawn = self.config.depth_visible();
        quantick_chart::connection_state::Book::when_drawn(drawn, &self.published.status)
    }

    /// A setting changed: the asset on screen files it at the next frame.
    fn commit_config_changes(&mut self, before: HeatmapConfig) -> bool {
        self.look.note_config_change(&before, &self.config);
        self.apply_config(before)
    }

    /// A setting changed from outside the panel — a layer switch, a control
    /// call. A write goes through the one door every change takes; returns
    /// whether `edit` changed anything.
    pub(crate) fn edit_config(&mut self, edit: impl FnOnce(&mut HeatmapConfig)) -> bool {
        let before = self.config.clone();
        edit(&mut self.config);
        if self.config == before {
            return false;
        }
        self.commit_config_changes(before);
        true
    }

    /// Independent presentation gains on the asset appearance owner. Where a
    /// gain may be set is `quantick_control_handlers::bubble_scale`'s rule;
    /// which gains are admitted is `quantick_orderflow`'s.
    pub(crate) fn bubble_display_scale(&self, target: BubbleScaleTarget) -> f32 {
        self.config.bubbles.display_scale.get(target)
    }

    /// A refused gain changes nothing, so it never marks the asset dirty.
    pub(crate) fn set_bubble_display_scale(
        &mut self,
        target: BubbleScaleTarget,
        value: f32,
    ) -> bool {
        self.edit_config(|config| {
            config.bubbles.display_scale.try_set(target, value);
        })
    }

    /// The same without a setting: a gesture, or an asset's settings worn.
    fn apply_config(&mut self, before: HeatmapConfig) -> bool {
        self.config.sanitize();
        if self.config == before {
            return false;
        }
        // Explicit display changes begin a new epoch; advancing the clock,
        // changing book generations, following prices or resizing the lane
        // never clears it.
        if !self.config.same_tape_grouping(&before) {
            self.tape_rebuilds
                .get_mut()
                .start_over(self.tape_dots.get_mut());
            self.past_dots.get_mut().clear();
        }
        if self.config.native_tape() != before.native_tape()
            || self.config.volume_dots.enabled != before.volume_dots.enabled
        {
            self.reset_pending_tape();
        }
        let capture_grouping_changed = self.config.price_grouping != before.price_grouping;
        let restart_required = capture_grouping_changed && self.config.enabled;
        if restart_required {
            // Stage the bucket: visual changes apply now, the destructive
            // grouping reset only after the feed accepts the restart command.
            self.pending_capture_grouping_previous = Some(before.price_grouping);
            self.worker.send(BookCommand::ApplyVisualConfig(Box::new(
                self.config.clone(),
            )));
        } else if capture_grouping_changed {
            self.last_seen_base = self.config.price_grouping;
            self.worker.send(BookCommand::ApplyVisualConfig(Box::new(
                self.config.clone(),
            )));
            self.worker
                .send(BookCommand::ApplyGroupingNow(self.config.price_grouping));
        } else {
            self.worker.send(BookCommand::ApplyVisualConfig(Box::new(
                self.config.clone(),
            )));
        }
        restart_required
    }

    /// The lane's market clock: only the independent tape advances between
    /// market events. Ordinary lanes retain their existing event-anchored
    /// clock, including BTC.
    pub(crate) fn lane_now_ms(&self) -> Option<i64> {
        self.pane_tape.lane_now_ms(self.config.native_tape())
    }

    /// The market's clock for every pane config ([`PaneTape::market_now_ms`]).
    pub(crate) fn market_now_ms(&self) -> Option<i64> {
        self.pane_tape.market_now_ms()
    }

    /// Advance the lane's clock to the host's monotonic instant
    /// ([`PaneTape::follow_live`]).
    pub(crate) fn set_live_clock_at(&mut self, applied_ms: Option<i64>, monotonic_ms: u64) {
        let native_tape = self.config.native_tape();
        self.pane_tape
            .follow_live(native_tape, applied_ms, monotonic_ms);
    }

    /// Place the lane's clock at the replay's position
    /// ([`PaneTape::follow_replay`]).
    pub(crate) fn set_replay_clock_at(
        &mut self,
        position_ms: i64,
        applied_ms: Option<i64>,
        next_unapplied_ms: Option<i64>,
    ) {
        let native_tape = self.config.native_tape();
        self.pane_tape
            .follow_replay(native_tape, position_ms, applied_ms, next_unapplied_ms);
    }

    /// Every opening burst the source recorded, the published ones folded.
    pub(crate) fn recorded_opening_bursts(&self) -> Vec<i64> {
        self.pane_tape
            .opening_bursts(self.published.frame.as_deref())
    }

    /// The worker's progress, for the health summary.
    pub(crate) fn worker_progress(&self) -> crate::worker_progress::ProgressSnapshot {
        self.worker.progress()
    }

    pub fn reset_summary_counters(&mut self) {
        self.worker.send(BookCommand::ResetSummaryCounters);
    }

    /// The stored presets, which an asset's declared look names.
    pub(crate) fn bubble_presets(&self) -> &bubble_presets::BubblePresetFile {
        self.look.presets()
    }

    /// The asset this view's settings belong to, once a tab has bound one.
    pub(crate) fn asset(&self) -> Option<&AssetBinding> {
        self.look.asset()
    }

    /// Switch "Save changes for this asset" ([`BubbleLook::set_save_changes`]).
    pub(crate) fn set_save_asset_changes(&mut self, on: bool) -> Option<SaveSwitch> {
        self.look.set_save_changes(on)
    }

    /// Start the same-frame tape over: its prints, their dots and its held
    /// end go, and the worker hears the new epoch.
    fn reset_pending_tape(&mut self) {
        self.tape_dots.get_mut().clear();
        self.tape_rebuilds.get_mut().discard();
        self.past_dots.get_mut().clear();
        self.set_tape_end(quantick_orderflow::tape_view::TapeEnd::Live);
        let epoch = self.pane_tape.reset();
        self.published.frame = None;
        self.worker.send(BookCommand::TapeEpoch(epoch));
    }

    pub(crate) fn apply_named_preset_with(
        &mut self,
        name: &str,
        spheres: bool,
        appearance: &quantick_appearance::orderflow::OrderflowAppearance,
    ) -> Result<bool, String> {
        let before = self.config.clone();
        let changed =
            self.look
                .apply_named_preset_with(name, &mut self.config, spheres, appearance)?;
        if changed {
            self.commit_config_changes(before);
        }
        Ok(changed)
    }

    pub(crate) fn bubble_presets_page(
        &self,
        input: &quantick_control_schema::bubble_preset::ListInput,
    ) -> Result<quantick_control_schema::bubble_preset::ListResult, String> {
        quantick_control_schema::bubble_preset::page(input, &self.look)
    }

    /// Put `settings` on screen as the asset `binding` names. The flow
    /// pane's candle aggression is the tab's to apply.
    pub(crate) fn bind_asset(&mut self, binding: AssetBinding, settings: &AssetBubbles) {
        self.look.bind(binding);
        self.wear_asset(settings, false);
    }

    /// The layer mask observes this view's tab ([`BubbleLook::settle_old_tape_fold`]).
    pub(crate) fn settle_old_tape_fold(&mut self) -> bool {
        self.look.settle_old_tape_fold(&self.config)
    }

    /// Take the presets another view reloaded, file what this one changed,
    /// then wear what another view filed since ([`BubbleLook::sync`]).
    /// Returns the candle aggression to put on the flow pane when it adopted.
    pub(crate) fn sync_asset(&mut self, candle_aggression: bool) -> Option<bool> {
        self.look.asset()?;
        if self.look.take_old_tape_fold(&self.config) {
            self.set_lane_enabled(false);
        }
        if self.look.take_reloaded_presets() {
            self.follow_declared_look(false);
        }
        let flow_ignore_opening = self.flow_execution.ignore_opening();
        let adopted = self
            .look
            .sync(&self.config, candle_aggression, flow_ignore_opening)?;
        self.wear_asset(&adopted.settings, !adopted.lane_moved);
        Some(adopted.settings.candle_aggression)
    }

    /// Take the presets file `loaded` again ([`BubbleLook::reload`]).
    fn reload_presets_from(&mut self, loaded: LoadedPresets) {
        if let Some(worn) = self.look.reload(loaded, &mut self.config) {
            self.wear_asset(&worn.settings, worn.keep_navigation);
        }
    }

    /// Wear the asset's declared look when nobody tuned it
    /// ([`BubbleLook::follow_declared`]).
    fn follow_declared_look(&mut self, reloaded_here: bool) {
        if let Some(worn) = self.look.follow_declared(reloaded_here) {
            self.wear_asset(&worn.settings, worn.keep_navigation);
        }
    }

    /// Put `settings` on screen without counting it as an edit
    /// ([`BubbleLook::wear`]), with the flow pane's opening rule.
    fn wear_asset(&mut self, settings: &AssetBubbles, keep_navigation: bool) {
        let before = self.config.clone();
        self.look.wear(settings, keep_navigation, &mut self.config);
        self.flow_execution
            .set_ignore_opening(settings.flow_ignore_opening);
        self.apply_config(before);
    }
}

#[cfg(test)]
impl OrderflowView {
    /// The last capture bucket adopted into the UI mirror.
    #[must_use]
    pub fn base_capture_grouping(&self) -> Decimal {
        self.published.base_price_grouping
    }

    /// Deterministic shorthand for tests that do not inspect arrival latency.
    pub fn handle_depth_event(&mut self, event: DepthEvent) {
        let received_at_ms = match &event {
            DepthEvent::Snapshot { observed_at_ms, .. } => *observed_at_ms,
            DepthEvent::Update { event_time_ms, .. } => *event_time_ms,
            DepthEvent::Status { .. } => 0,
        };
        self.handle_depth_event_at(event, received_at_ms);
    }

    pub(crate) fn with_worker_for_test(symbol: &str, worker: BookWorker) -> Self {
        let mut view = Self::new(symbol);
        view.worker = worker;
        view
    }

    pub(crate) fn apply_named_preset(&mut self, name: &str, spheres: bool) -> Result<bool, String> {
        self.apply_named_preset_with(
            name,
            spheres,
            &quantick_appearance::ResolvedTheme::bundled_dark().orderflow,
        )
    }
}

#[cfg(test)]
/// The whole-look pick as the settings picker makes it
/// ([`BubbleLook::apply_preset`]), for tests.
impl OrderflowView {
    pub(crate) fn apply_preset(&mut self, name: &str, spheres: bool) -> bool {
        self.look.apply_preset(name, &mut self.config, spheres)
    }

    /// Name of the preset the panel currently wears.
    pub(crate) fn active_preset_for_test(&self) -> &str {
        self.look.look_name()
    }

    /// Read-only view of the heatmap config, for app-level assertions.
    pub(crate) fn config_for_test(&self) -> &HeatmapConfig {
        &self.config
    }

    pub(crate) fn stage_capture_grouping_for_test(&mut self, grouping: Decimal) -> bool {
        let before = self.config.clone();
        self.config.price_grouping = grouping;
        self.capture_grouping_draft = grouping.to_f64().unwrap_or(self.capture_grouping_draft);
        self.commit_config_changes(before)
    }

    pub(crate) fn base_capture_grouping_for_test(&mut self) -> Decimal {
        self.flush_for_test();
        self.published.base_price_grouping
    }

    /// Wait until the worker has applied and published everything sent so
    /// far, then adopt the result. Makes the async pipeline deterministic in
    /// tests; production code never blocks on the worker.
    pub(crate) fn flush_for_test(&mut self) {
        self.worker.flush();
        self.sync_published();
    }
}

#[cfg(test)]
mod tests;
