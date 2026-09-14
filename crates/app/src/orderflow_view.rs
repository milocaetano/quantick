//! egui facade for the asynchronous order-flow heatmap.
//!
//! All book state (history, synchronization, projection) lives in
//! [`quantick_orderflow::engine::BookEngine`] on the worker thread owned by
//! [`crate::orderflow_worker::BookWorker`]. This layer only forwards commands,
//! mirrors the published snapshot for the current frame and converts
//! normalized primitives into egui shapes. Nothing here can block the UI on a
//! dense book: drawing always uses the latest already-built frame.

use eframe::egui;
use quantick_engine::{Bar, Trade};
use quantick_orderbook::{BookSide, DepthEvent};
use quantick_orderflow::engine::{BookLadder, BookPublished, CaptureStatus, OrderflowHealth};
use quantick_orderflow::{HeatmapConfig, LaneWindow, reserved_span_ms};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::bubble_presets::{self, BubblePresetFile, PresetSource};
use crate::orderflow_render::{OrderflowRenderStyle, ProjectedLayout};
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::viewport::Viewport;

mod frame;
mod settings;

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

/// A displayed resting-liquidity cell resolved under the pointer.
///
/// This is an application-internal semantic result. The control module maps
/// it into its owned wire DTO, so no renderer or `egui` type crosses the
/// control boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowCellHit {
    pub generation: u64,
    pub side: BookSide,
    pub price_bucket: Decimal,
    pub price_span: Decimal,
    pub quantity: Decimal,
    pub start_slot: usize,
    pub end_slot_exclusive: usize,
    pub live_lane: bool,
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
    /// The `(step, reference_price)` pair last sent to the engine, so the
    /// per-trade path sends one command per change rather than one per print.
    ///
    /// Both halves are in the key: either one moving is a different answer,
    /// and keying on the step alone would swallow the first magnitude a chart
    /// ever learns whenever the grid happened to settle first.
    last_tape_price_grid: Option<(Decimal, Option<Decimal>)>,
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
            last_tape_price_grid: None,
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

    /// The health mirror already used by the last application frame.
    ///
    /// Unlike [`Self::health`], this does not synchronize with the worker. A
    /// coherent control capture reads the same published frame the user saw,
    /// without letting one requested scope advance another scope underneath
    /// the capture.
    #[must_use]
    pub(crate) fn cached_health(&self) -> &OrderflowHealth {
        &self.published.health
    }

    /// The book frame the last application frame published, for a control
    /// capture.
    ///
    /// Same contract as [`Self::cached_health`], and for the same reason: it
    /// does not synchronize with the worker, so one requested scope cannot
    /// advance another scope underneath the capture. The ladder is `None`
    /// while capture is off or the book has no snapshot yet — an honest
    /// absence, never an empty book.
    #[must_use]
    pub(crate) fn cached_book(&self) -> (&CaptureStatus, Option<&BookLadder>, Decimal) {
        (
            &self.published.status,
            self.published.ladder.as_deref(),
            self.published.base_price_grouping,
        )
    }

    /// The live lane's right edge as the last application frame published it,
    /// under the same no-sync contract as [`Self::cached_health`].
    ///
    /// `None` when no flow layer is drawn: a chart with no lane has no edge to
    /// report, and the newest print the engine happens to have seen is not one.
    #[must_use]
    pub(crate) fn cached_live_end_ms(&self) -> Option<i64> {
        self.published.live_end_ms
    }

    /// The heatmap setup this chart is drawing with, for a control capture.
    #[must_use]
    pub(crate) fn cached_config(&self) -> &HeatmapConfig {
        &self.config
    }

    /// Resolve the topmost displayed L2 heat cell under `position` from the
    /// frame the chart most recently painted.
    ///
    /// Projection and hit testing share [`ProjectedLayout`]. The lookup is
    /// O(visible cells), runs only for an explicit cursor snapshot, and never
    /// participates in painting or ingestion.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn control_flow_cell_at(
        &self,
        chart_rect: egui::Rect,
        viewport: &Viewport,
        total_bars: usize,
        lane_width_px: f32,
        inverted: bool,
        position: egui::Pos2,
    ) -> Option<FlowCellHit> {
        let frame = self.published.frame.as_deref()?;
        let layout = ProjectedLayout::new(
            chart_rect,
            viewport,
            total_bars,
            frame.first_bar_index,
            frame.slot_count,
            lane_width_px,
        )
        .with_inverted(inverted);
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

        let style = OrderflowRenderStyle::from_config(&self.config, egui::Color32::TRANSPARENT);
        let cell = frame.projection.cells.iter().rev().find(|cell| {
            layout
                .heat_cell_rect(cell.x0, cell.x1, cell.y0, cell.y1, style.min_cell_height)
                .contains(position)
        })?;

        let regions = frame.slot_count.max(1);
        let has_lane = frame.projection.live_now_x.is_some();
        let bar_regions = regions.saturating_sub(usize::from(has_lane));
        if bar_regions == 0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first_region = (cell.x0.clamp(0.0, 1.0) * regions as f64).floor() as usize;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let after_region = (cell.x1.clamp(0.0, 1.0) * regions as f64).ceil() as usize;
        let touches_lane = has_lane && after_region > bar_regions;
        // A cell that lies wholly in the live lane has no closed bar under it;
        // an empty range at the lane boundary says so, instead of borrowing
        // the last bar's slot and calling it the cell's.
        let (first_bar_region, after_bar_region) = if first_region >= bar_regions {
            (bar_regions, bar_regions)
        } else {
            (
                first_region,
                after_region.max(first_region + 1).min(bar_regions),
            )
        };

        Some(FlowCellHit {
            generation: cell.generation,
            side: cell.side,
            price_bucket: cell.price_bucket,
            price_span: frame.projection.effective_grouping.bucket_width,
            quantity: cell.quantity,
            start_slot: frame.first_bar_index + first_bar_region,
            end_slot_exclusive: frame.first_bar_index + after_bar_region,
            live_lane: touches_lane,
        })
    }

    /// Pull the newest worker snapshot into this frame's mirror. Cheap: one
    /// mutex lock and a small clone (frames are shared through `Arc`).
    fn sync_published(&mut self) {
        self.published = self.worker.published();
        let base = self.published.base_price_grouping;
        self.adopt_base(base);
    }

    /// Take an engine-chosen capture bucket into the UI mirror.
    ///
    /// Split out of [`Self::sync_published`] because the footprint's row width
    /// needs the bucket without the rest of the published state, and the
    /// adoption rule is the same either way.
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

    /// The capture bucket, taking whatever the engine has published since the
    /// last look.
    ///
    /// [`base_capture_grouping`](Self::base_capture_grouping) reads the mirror
    /// as it stands, which is right for a caller that has already synced this
    /// frame. The footprint's row width has no such caller: every
    /// `sync_published` site is gated on a layer or a dock tab being open, and
    /// the ladder is drawn when all of them are off — the very case this
    /// sizing exists for. Reading through here rather than off the mirror is
    /// what keeps the rows from depending on a diagnostics log having run.
    pub fn capture_grouping_now(&mut self) -> Decimal {
        let base = self.worker.published_base_grouping();
        // The mirror takes it too. A split's *other* pane is handed the row
        // width through `base_capture_grouping`, which reads the mirror and
        // takes `&self` — so leaving the mirror behind here would let the two
        // panes of one chart draw the same market on different rows, which is
        // the divergence this whole sizing path exists to prevent. Cheap: the
        // field is a `Decimal`, and the rest of the published snapshot is
        // untouched because nothing here has read it.
        self.published.base_price_grouping = base;
        self.adopt_base(base);
        base
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
    pub fn tape_age(&self) -> Option<quantick_orderflow::TapeAge> {
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
        // The send-once cache is keyed on what was *sent*, so a new market
        // whose tape prints on the same grid would be suppressed — and a grid
        // the engine refused while the trader had picked a bucket by hand
        // would never be re-offered once auto sizing is re-armed here.
        self.last_tape_price_grid = None;
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

    /// Tell the engine the price grid the tape prints on, and the magnitude
    /// it prints at.
    ///
    /// Both, because a tick alone does not size a row: BTCUSDT's real tick is
    /// a cent and its rows are dollars. The engine folds the one toward the
    /// other; it only ever had the price when a *book snapshot* carried one,
    /// so a chart with no L2 fell back to raw cents.
    ///
    /// Sent only when the answer changes, which a running GCD makes rare: it
    /// starts as nothing, names a grid once the tape has shown one, and only
    /// ever narrows from there. The magnitude is frozen at the chart's first
    /// print and never moves at all. On the trader's own recordings the pair
    /// settles within about thirty prints and never moves again, so a session
    /// pays for one regroup rather than one per trade — which matters, because
    /// this is called from the per-trade path.
    ///
    /// Deliberately *not* gated on a layer being enabled, unlike
    /// [`record_trade`](Self::record_trade): the grid sizes the footprint
    /// ladder as well as the liquidity map, and a trader reading a ladder with
    /// the heatmap switched off needs its rows the right width just the same.
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

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
    use quantick_orderflow::{
        BubbleSizeReference, BubbleStyle, ConsumptionMark, MAX_LIVE_LANE_SHARE,
        MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_ZOOM,
    };

    use crate::bubble_presets::BubblePreset;
    use crate::chart::PriceScale;
    use crate::live_strip;

    /// The ask, in one test: the toolbar governs the candles and nothing else.
    /// Every one of the four movements — each layer switched off *and* back on
    /// — has to leave the tape exactly where the trader left it.

    #[test]
    fn a_direct_read_of_the_capture_bucket_leaves_the_mirror_agreeing() {
        // Two readers of one number. `capture_grouping_now` takes it straight
        // from the worker mailbox, because the footprint's row width is needed
        // on frames where nothing syncs; `base_capture_grouping` reads the
        // mirror and takes `&self`, and is how a split hands the row width to
        // its other pane. If the direct read does not leave the mirror behind,
        // the two panes of one chart draw the same market on different rows.
        //
        // The worker is flushed but the view is deliberately NOT synced: a sync
        // would write the mirror by itself and the test would pass either way.
        let mut view = OrderflowView::new("WINV26");
        let before = view.base_capture_grouping();
        view.observe_tape_price_grid(Decimal::from(5), Some(Decimal::from(140_000)));
        view.worker.flush();

        let direct = view.capture_grouping_now();
        assert_ne!(direct, before, "the engine never took the tape's grid");
        assert_eq!(direct, Decimal::from(5));
        assert_eq!(
            view.base_capture_grouping(),
            direct,
            "the mirror the other pane reads disagrees with the mailbox this one read",
        );
    }
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
        use quantick_orderflow::LiveLaneStyle;
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

    #[test]
    fn semantic_pointer_resolves_the_same_heat_cell_the_renderer_projects() {
        let mut view = OrderflowView::new("BTCUSDT");
        view.set_enabled(true, 10);
        view.handle_depth_event(snapshot_event(10));
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
        let bars = [bar(900, 1_100)];
        view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));
        view.flush_for_test();
        view.project_visible(visible_timeline(&bars), true, true, None, (98.0, 102.0));

        let frame = view.published.frame.as_deref().expect("published frame");
        let cell = frame.projection.cells.last().expect("one displayed cell");
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1_000.0, 600.0));
        let viewport = Viewport::new();
        let layout = ProjectedLayout::new(
            chart,
            &viewport,
            1,
            frame.first_bar_index,
            frame.slot_count,
            0.0,
        );
        let style = OrderflowRenderStyle::from_config(&view.config, egui::Color32::TRANSPARENT);
        let position = layout
            .heat_cell_rect(cell.x0, cell.x1, cell.y0, cell.y1, style.min_cell_height)
            .center();

        let hit = view
            .control_flow_cell_at(chart, &viewport, 1, 0.0, false, position)
            .expect("the painted cell is semantically resolved");
        assert_eq!(hit.generation, cell.generation);
        assert_eq!(hit.side, cell.side);
        assert_eq!(hit.price_bucket, cell.price_bucket);
        assert_eq!(hit.quantity, cell.quantity);
        assert_eq!(hit.start_slot, 0);
        assert_eq!(hit.end_slot_exclusive, 1);
        assert!(!hit.live_lane);
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
            Some(quantick_orderflow::TapeAge::Behind(14_000)),
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

mod worker_diagnostics;
