//! The chart pane itself: what one pane owns between frames.
//!
//! The passes that read it live in the siblings — `draw_chart`, `frame`,
//! `gestures` — so this file is the state and the accessors that hand it
//! over, not the drawing.

use eframe::egui;

use quantick_layers::ChartLayer;

use crate::bands;
use crate::chart::PriceScale;
use crate::config::FeedCapabilities;
use crate::drawings::{self, Drawings};
use crate::indicator_worker::{IndicatorWorker, LaneTransport, SlotId};
use crate::indicators::{IndicatorViews, PaneSizing};
use crate::orderflow_view::OrderflowView;
use crate::plot_area::{self, PlotAreas, plot_split};
use crate::price_view::PriceView;
use crate::state::{BarConfiguration, BarSpec, ChartState, SpecSelector};
use crate::toolrail::Tool;
use crate::viewport::Viewport;

use super::*;

#[cfg(test)]
impl ChartPane {
    pub(crate) fn install_layer_probe(&mut self) {
        render_registry::probe::install(self);
    }
}

/// Which side of the candles a drawing pass paints on.
///
/// One function serves both, taking this rather than being copied: the
/// projection, the band filter, the off-series fade and the style resolution
/// are the same work whichever side is being drawn, and a second copy of them
/// would drift on the first change to any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawPass {
    /// Before the candles: the object's own body, for the few tools that are
    /// context rather than annotation. See [`DrawingToolImpl::paint_under`].
    UnderCandles,
    /// After the candles: everything else, plus halo, handles and badges.
    OverCandles,
}

/// One chart pane. See the module docs for what does and does not live here.
pub struct ChartPane {
    /// Namespaces this pane's egui interaction ids. Ids are the one piece of
    /// gesture state egui keeps on our behalf, so two panes sharing an id
    /// would share a drag.
    pub id: u64,
    pub state: ChartState,
    /// Identity of the closed-bar prefix used by append-only control-plane
    /// pagination.
    ///
    /// A live bar closing appends beyond an existing page's high-water mark
    /// and deliberately leaves this unchanged. Anything that can rewrite,
    /// prepend, remove, or re-cut a closed bar advances it, so a cursor can
    /// reject a mixed view instead of silently continuing over changed data.
    pub(super) pagination_revision: u64,
    /// The tape, and everything read off it: the live lane, the heatmap, the
    /// bubbles, the live strip.
    ///
    /// `None` is what makes a time pane a time pane. §11 keeps the flow layers
    /// on the flow pane, and a pane that will never draw them has no business
    /// running a book worker thread to feed them.
    pub orderflow: Option<OrderflowView>,
    /// Background thread owning the `IndicatorHost`; the UI only sends
    /// commands and applies the delta events back.
    pub indicator_worker: IndicatorWorker,
    /// The UI's copy of every indicator's plot columns (see
    /// [`crate::indicators`]).
    pub indicators: IndicatorViews,
    /// Read-only handle to the layout session's authoritative membership.
    pub(crate) layout_view: quantick_workspace::session::LayoutView,
    /// A restored/opening request, consumed when the session seeds this pane.
    /// The outer option distinguishes a requested default from no request.
    pub(crate) opening_layout: Option<Option<crate::layouts::LayoutId>>,
    /// The layout's name, for the pane to show beside its own controls. A
    /// copy the app refreshes on a switch or a rename, so the header — drawn
    /// by the tab, which has no book — never looks it up per frame.
    ///
    /// Drawn today only by a *context* pane, in the strip that carries its
    /// timeframe chips ([`crate::time_header`]). The flow pane has no such
    /// strip, so while a context pane holds the focus its layout is named
    /// nowhere on screen — the footer strip lights the focused pane's. The
    /// copy is kept on every pane rather than on the ones that draw it, so
    /// the flow pane's own readout is a draw call and not a second
    /// bookkeeping path.
    pub layout_label: String,
    /// Which market and pane address the drawings on this pane belong to,
    /// once the layout put them here. The app compares it with the tab's
    /// market every frame and swaps the set when they part.
    pub drawings_key: Option<crate::layouts::DrawingKey>,
    /// The drawings revision last copied into the layout; a different
    /// reading means the layout is behind this pane.
    pub drawings_saved_revision: u64,
    /// Whether this pane's on-chart indicator legend is folded to its count
    /// puck. Per pane, not per window: a split is two readings of the same
    /// market, and the corner pressure that makes a trader fold the flow
    /// pane's legend — bubbles, book, the position HUD — is not on the time
    /// pane at all. Expanded by default, which is what every chart did before
    /// the fold existed.
    pub legend_collapsed: bool,
    /// Requested switches not already owned by another feature, and the
    /// headless catalog that resolves policy for every layer.
    pub layers: quantick_layers::LayerState,
    pub(super) layer_renderers: &'static render_registry::RenderRegistry,
    /// The candle footprint layer as this pane has it — see
    /// [`PaneFootprint`].
    pub footprint: PaneFootprint,

    /// Where each layer's switch landed in the last menu frame, so a test can
    /// click the real widget instead of calling the setter behind it.
    #[cfg(test)]
    pub layer_menu_rects: Vec<(ChartLayer, egui::Rect)>,

    /// The bar rule this pane is on, and the parameter every other kind is
    /// holding for the trader — see [`SpecSelector`].
    pub spec: SpecSelector,

    // Pan/zoom navigation over the bar series. It owns the history pane only:
    // the live lane is a band of screen to its right that answers to nothing
    // it does.
    pub viewport: Viewport,
    /// What the last draw measured, for the passes that are not the draw — see
    /// [`PaneFrame`].
    pub frame: PaneFrame,
    // The price-axis levels this frame's drawings declare. Per-frame by
    // nature, reused as a container for the same reason the band carve is —
    // and gathered before the axis labels itself, because the axis stands
    // aside where one of these is going to land.
    pub(super) price_axis_levels: Vec<PriceAxisLevel>,
    // How many rungs the lane was wide enough for at the last draw, and so
    // how finely the next publish samples the forming bar across it. `0` when
    // there is no lane: the worker then walks no ladder and the panes draw
    // nothing on the tape, which is the whole cost of this feature on a chart
    // that has no tape to draw on.
    pub(super) lane: LaneTransport,
    // Manual price-axis pan/zoom (auto-fit until the user drags vertically).
    pub price_view: PriceView,
    /// The price band's label, shared into every carve instead of cloned.
    pub(super) price_band_label: std::sync::Arc<str>,
    // Pointer position over the plot this frame, for the crosshair.
    pub hover_pos: Option<egui::Pos2>,
    /// The tape switch in the canvas's top-right corner — see
    /// [`TapeSwitch`].
    pub tape_switch: TapeSwitch,

    /// Venue candles standing in front of the trade-derived series, already
    /// folded to this pane's interval.
    ///
    /// Deliberately outside `ChartState`: that rebuilds its bars from retained
    /// trades on every spec change, and a prefix living inside them would be
    /// eaten by the first chip click. Kept here, it is composed with the
    /// engine's bars at the points that read them — [`Self::slots`],
    /// [`Self::closed_bar`], the rebuild payload and the draw — and survives
    /// every rebuild the engine does.
    ///
    /// Non-empty on any pane cutting by a foldable time interval — the
    /// split's time pane, and the flow pane whenever its spec is
    /// `BarSpec::Time` (audit S1). A venue candle has no tape in it, so over
    /// the prefix the flow layers simply draw nothing.
    pub history_prefix: Vec<quantick_engine::Bar>,

    /// Where the position HUD anchors this frame: the chart rect and price
    /// scale, cached by the draw while the paper layer is painted on the
    /// pane that owns order entry. The HUD itself draws in `tab.rs`, where
    /// the paper host is mutably reachable outside the chrome's shared
    /// borrow.
    pub(super) paper_hud_anchor: Option<(egui::Rect, PriceScale)>,
    /// What the right-click that opened the layer menu resolved — see
    /// [`PaneContextMenu`].
    pub context_menu: PaneContextMenu,
    /// The strategies armed on this pane's drawings — see
    /// [`PaneStrategies`].
    pub strategies: PaneStrategies,

    /// User drawings live entirely in the app overlay layer, never in market
    /// state, so chart/backtest/bot determinism stays untouched.
    pub drawings: Drawings,
    /// A drawing gesture in flight — see [`PaneGestures`].
    pub gestures: PaneGestures,
    /// A re-anchor owed to the drawings, holding the slot count of the series
    /// they were last anchored to.
    ///
    /// A reset empties the pane, and an empty series cannot say where an
    /// instant lands — so the answer is deferred to the first frame that has
    /// bars again, rather than clamping every mark onto a series that is not
    /// there yet.
    pub(super) pending_reanchor: Option<usize>,
    /// The pane opened by a click on its own collapsed strip, carried to the
    /// frame that may hold the second half of a double click.
    ///
    /// A collapsed strip changes shape the instant it is clicked, so a gesture
    /// made *of* two clicks cannot be read from one frame's geometry. This is
    /// the one piece of state that spans them.
    pub(super) strip_expanded: Option<SlotId>,
    /// An indicator whose settings a gesture on this pane asked for, waiting
    /// for the app to open the dialog.
    ///
    /// Parked rather than acted on: the dialog is the app's — one dialog for
    /// the whole window — and the gestures that ask for it are read deep inside
    /// this pane's input pass, holding borrows the app's state cannot cross.
    /// The same shape [`SpecSelector::pending`] uses for the other direction.
    pub(super) pending_settings: Option<SlotId>,
    /// A guide switch chosen in an indicator pane's context menu, parked
    /// until the app can update its layout and mirrored panes.
    pub(super) pending_indicator_guide: Option<(SlotId, bool)>,
}

impl ChartPane {
    pub(crate) fn series_read(&self) -> drawing_projection::PaneSeriesRead<'_> {
        drawing_projection::PaneSeriesRead {
            history_prefix: &self.history_prefix,
            state: &self.state,
            spec: &self.spec,
        }
    }
    pub(crate) fn drawing_projection(&self) -> drawing_projection::DrawingProjection<'_> {
        drawing_projection::DrawingProjection {
            series: self.series_read(),
            viewport: &self.viewport,
            indicators: &self.indicators,
        }
    }

    /// The last frame's geometry, lent out for a hit test — see
    /// [`PaneHitTest`].
    pub(crate) fn hit_test(&self) -> PaneHitTest<'_> {
        PaneHitTest {
            frame: &self.frame,
            price_view: &self.price_view,
            drawings: &self.drawings,
            orderflow: self.orderflow.as_ref(),
            hover_pos: self.hover_pos,
            projection: self.drawing_projection(),
        }
    }

    /// This pane's store beside the projection an edit from another pane
    /// resolves through — see [`SharedMarksMut`].
    pub(crate) fn shared_marks_mut(&mut self) -> SharedMarksMut<'_> {
        SharedMarksMut {
            projection: drawing_projection::DrawingProjection {
                series: drawing_projection::PaneSeriesRead {
                    history_prefix: &self.history_prefix,
                    state: &self.state,
                    spec: &self.spec,
                },
                viewport: &self.viewport,
                indicators: &self.indicators,
            },
            drawings: &mut self.drawings,
        }
    }

    /// The marks this pane lends its companions: its store and the object
    /// its editor holds — see [`SharedSource`].
    pub(crate) fn shared_source(&self) -> SharedSource<'_> {
        SharedSource {
            drawings: &self.drawings,
            content_editing: self.gestures.content_editing,
        }
    }

    /// The strategies beside the series their rulers warm on, for a
    /// re-arm from outside the pane.
    #[cfg(test)]
    pub(crate) fn strategies_with_series(
        &mut self,
    ) -> (&mut PaneStrategies, drawing_projection::PaneSeriesRead<'_>) {
        (
            &mut self.strategies,
            drawing_projection::PaneSeriesRead {
                history_prefix: &self.history_prefix,
                state: &self.state,
                spec: &self.spec,
            },
        )
    }

    /// One pane's side of the quick range — see [`quick_range::QuickRangeView`].
    pub(super) fn quick_range_view(
        &self,
        tab: u64,
        side: PaneSide,
    ) -> quick_range::QuickRangeView<'_> {
        quick_range::QuickRangeView {
            owner: crate::surfaces::drawing_chrome::QuickRangeOwner {
                tab,
                side,
                pane: self.id,
                revision: self.pagination_revision(),
                layout: self.layout_id().map(|id| id.0),
            },
            projection: self.drawing_projection(),
        }
    }

    /// Current membership, or the pending imported/opening choice before seeding.
    pub(crate) fn layout_id(&self) -> Option<crate::layouts::LayoutId> {
        self.opening_layout
            .unwrap_or_else(|| self.layout_view.layout())
    }
    pub(crate) fn layout_seeded(&self) -> bool {
        self.layout_view.seeded()
    }
    pub(crate) fn request_opening_layout(&mut self, id: Option<crate::layouts::LayoutId>) {
        self.opening_layout = Some(id);
    }

    /// The flow pane: quantick's own view of `symbol`, opening on bar `spec`,
    /// with the tape and every layer read off it.
    #[must_use]
    pub fn flow(id: u64, spec: impl Into<BarConfiguration>, symbol: String) -> Self {
        Self::new(id, spec.into(), Some(OrderflowView::new(symbol)))
    }

    /// The time pane: the context view beside the flow pane (§11). Time bars
    /// of `interval_ms`, no tape and no flow layers.
    #[must_use]
    pub fn time(id: u64, interval_ms: i64) -> Self {
        Self::new(id, BarSpec::Time(interval_ms.max(1)), None)
    }

    /// `id` namespaces the pane's egui interaction ids and must be unique
    /// among the panes on screen.
    fn new(id: u64, spec: impl Into<BarConfiguration>, orderflow: Option<OrderflowView>) -> Self {
        // Defaults for every kind, with the initial spec's parameter applied.
        let spec = spec.into();
        let selector = SpecSelector::new(spec);

        Self {
            id,
            spec: selector,
            state: ChartState::new(spec),
            pagination_revision: 0,
            orderflow,
            indicator_worker: IndicatorWorker::spawn(),
            indicators: IndicatorViews::new(),
            layout_view: quantick_workspace::session::LayoutView::default(),
            opening_layout: None,
            layout_label: String::new(),
            drawings_key: None,
            drawings_saved_revision: 0,
            legend_collapsed: false,
            layers: quantick_layers::LayerState::new(render_registry::standard().layers()),
            layer_renderers: render_registry::standard(),
            footprint: PaneFootprint::default(),
            // The backfill divider opens off: it is a full-height rule across
            // the candles for a boundary that matters once, when reading how
            // far the live tape goes back. Nothing is hidden about the data —
            // the mark is one click away in the layer menu, and the bars
            // either side of it are exactly what they were.
            #[cfg(test)]
            layer_menu_rects: Vec::new(),
            viewport: Viewport::new(),
            frame: PaneFrame::default(),
            price_axis_levels: Vec::new(),
            lane: LaneTransport::default(),
            price_view: PriceView::new(),
            price_band_label: std::sync::Arc::from(bands::PRICE_BAND_LABEL),
            hover_pos: None,
            tape_switch: TapeSwitch::default(),
            history_prefix: Vec::new(),
            paper_hud_anchor: None,
            context_menu: PaneContextMenu::default(),
            strategies: PaneStrategies::default(),
            drawings: Drawings::default(),
            gestures: PaneGestures::default(),
            pending_reanchor: None,
            strip_expanded: None,
            pending_settings: None,
            pending_indicator_guide: None,
        }
    }

    /// Whether any placed or in-flight drawing is a fixed-range volume
    /// profile — the second consumer of the footprint ladders, keeping
    /// accumulation on while the layer itself is hidden. O(drawings), once
    /// per frame, never on the ingestion path.
    pub(super) fn wants_range_profile(&self) -> bool {
        self.drawings
            .items()
            .iter()
            .map(|drawing| drawing.tool)
            .chain(self.drawings.draft().map(|draft| draft.tool))
            .any(|tool| tool.id() == crate::frvp::TOOL_ID)
    }

    /// The footprint setup this chart draws with: its own once configured
    /// here, else the window's last one. See [`PaneFootprint::config`].
    pub fn footprint_config<'a>(
        &'a self,
        window: &'a crate::footprint_config::FootprintConfig,
    ) -> &'a crate::footprint_config::FootprintConfig {
        self.footprint.config.as_ref().unwrap_or(window)
    }

    /// Put this pane on `spec` outright, selectors included.
    ///
    /// Startup-scoped: the caller is a workspace restoring the bar rule this
    /// pane was last read on, into a pane that has not drawn a frame yet. A
    /// live change goes through [`SpecSelector::pending`] instead, so the frame carrying
    /// it paints the loading overlay before the rebuild replays the tape —
    /// there is no tape to replay here, and nothing to paint over.
    ///
    /// The selectors move with the spec, because the BARS group reads *them*:
    /// setting the state alone would restore a chart whose own controls
    /// disagreed with it, and the trader's first touch of the parameter would
    /// snap the chart back to a rule they never chose.
    pub fn set_spec(&mut self, spec: impl Into<BarConfiguration>) {
        let spec = spec.into();
        let changed = self.state.spec() != &spec;
        self.spec.set(spec);
        self.state.set_spec(spec);
        if changed {
            self.bump_pagination_revision();
        }
    }

    /// Re-cut every retained print from the retained deal-counter readings.
    pub fn rebuild_bars(&mut self) {
        self.state.rebuild_bars();
        self.bump_pagination_revision();
    }

    /// Revision protecting the closed-bar prefix exposed through paginated
    /// chart-window reads. Live appends do not advance it; rewrites do.
    #[must_use]
    pub fn pagination_revision(&self) -> u64 {
        self.pagination_revision
    }

    pub(super) fn bump_pagination_revision(&mut self) {
        self.pagination_revision = self.pagination_revision.saturating_add(1);
    }

    #[cfg(test)]
    /// Give this chart its own footprint setup, the way the settings window
    /// does when a knob moves on it.
    pub fn set_footprint_override(
        &mut self,
        config: Option<crate::footprint_config::FootprintConfig>,
    ) {
        self.footprint.config = config;
    }

    /// An egui interaction id scoped to this pane.
    pub(super) fn interaction_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.id))
    }

    /// Width reserved for the live strip this frame. No capability gate any
    /// more: the aggression histogram runs on the trade stream, which every
    /// source provides (replay included), and without book data the strip
    /// honestly degrades to that histogram alone. A pane with no tape has no
    /// strip at all (§11).
    pub fn live_strip_width(&self, capabilities: FeedCapabilities) -> f32 {
        // Both halves of the strip come from the source: resting depth and the
        // aggressions landing into it. A source that produces neither fills
        // none of it, so the band would be an empty rect permanently narrowing
        // the candles — which is what the shipped default made reachable, the
        // layer having opened off until now. The claim that these pixels were
        // capability-gated predates the gate by some months; this is it.
        //
        // Passed in rather than cached on the pane for the reason
        // `layer_blocked` states: the running feed is resolved once per frame
        // by the caller, and a copy kept here would be one more thing to keep
        // in step when MetaTrader narrows its capabilities mid-session.
        if quantick_layers::LayerState::effective(
            ChartLayer::LiveStrip,
            self.layers.requested(ChartLayer::LiveStrip),
            self.layer_facts(Some(capabilities)),
        ) {
            crate::live_strip::LIVE_STRIP_WIDTH_PX
        } else {
            0.0
        }
    }

    /// This pane's regions inside `area`, carved once so the input handler and
    /// the renderer can never disagree about a boundary.
    pub(super) fn plot_areas(&self, area: egui::Rect, capabilities: FeedCapabilities) -> PlotAreas {
        let mut sizing = [PaneSizing::Auto; crate::indicators::MAX_PANES];
        plot_split(
            area,
            self.live_strip_width(capabilities),
            self.indicators.pane_sizing(&mut sizing),
        )
    }

    /// Re-anchor as soon as there are bars to anchor to, after a reset left
    /// the pane empty. Cheap enough to ask every frame: it is a flag test.
    /// The indicator a gesture on this pane asked to configure, if any, taken
    /// so a request is acted on exactly once.
    pub fn take_settings_request(&mut self) -> Option<SlotId> {
        self.pending_settings.take()
    }

    pub(crate) fn take_indicator_guide_request(&mut self) -> Option<(SlotId, bool)> {
        self.pending_indicator_guide.take()
    }

    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) fn first_indicator_pane_center(&self) -> Option<egui::Pos2> {
        self.frame.bands.get(1).map(|band| band.rect.center())
    }

    #[cfg(test)]
    pub(crate) fn request_indicator_guide(&mut self, slot: SlotId, enabled: bool) {
        self.pending_indicator_guide = Some((slot, enabled));
    }

    /// Stand in for the gesture that raises a settings request, so the app's
    /// side of the wiring can be tested without driving egui through a pane
    /// layout it would have to re-derive.
    #[cfg(test)]
    pub fn request_settings(&mut self, slot: SlotId) {
        self.pending_settings = Some(slot);
    }

    /// Handle mouse navigation, TradingView-style:
    /// - drag the candles → pan time (x, moves the whole chart) and price (y);
    /// - scroll over them → zoom time;
    /// - drag the bottom time strip left/right → zoom time (spread candles);
    /// - drag the right price gutter up/down → zoom the price scale;
    /// - scroll over either axis → zoom that axis;
    /// - double-click → reset to the live edge and auto-fit price.
    ///
    /// The live lane is a pane of its own and answers to none of it: a gesture
    /// that starts inside the tape moves nothing, and scrolling there zooms the
    /// tape's own window instead of the candles.
    pub fn handle_navigation(
        &mut self,
        ui: &egui::Ui,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.unhide_layer_for_armed_tool(chrome);
        // The magnet applies to every anchor gesture, placement and re-drag
        // alike: a handle that snaps only while you first draw it would make
        // the second edit undo the precision of the first.
        let magnet = chrome.toolrail.magnet();
        // Remembered for inspector placement and manager centring: the pane
        // where drawings live, already free of both axes and the live lane.
        self.frame.plot_area = Some(area);
        let areas = self.plot_areas(area, chrome.capabilities);
        self.frame.chart_area = Some(areas.chart);
        // One carve, consumed by placement, hit-testing, dragging and — after
        // the panes have drawn — painting.
        let bands = crate::bands::BandGeometry {
            auto_range: self.frame.auto_range,
            price_view: &self.price_view,
            lane_divider_x: self.frame.lane_divider_x,
            indicators: &self.indicators,
            price_label: &self.price_band_label,
        }
        .bands(&areas);
        // A drawing tool consumes the *primary button*, not the chart. Pan,
        // wheel zoom, the pane dividers and the collapse chevrons all keep
        // working while one is armed: an armed tool used to return early from
        // here, which left the trader unable to move the chart they were
        // annotating (audit S2).
        let placement_id = self.interaction_id("drawing_placement");
        #[cfg(any(feature = "drawing-harness", test))]
        let hand = self
            .gestures
            .parked_hand
            .map(|hand| (hand.constrain, hand.position));
        #[cfg(not(any(feature = "drawing-harness", test)))]
        let hand: Option<(drawings::Constrain, egui::Pos2)> = None;
        let options = placement_gestures::PlacementOptions {
            tool: chrome.toolrail.tool().drawing_tool(),
            magnet: chrome.toolrail.magnet(),
            constrain: if ui.input(|input| input.modifiers.shift) {
                drawings::Constrain::Level
            } else {
                hand.map_or(drawings::Constrain::Free, |hand| hand.0)
            },
            parked_position: hand.map(|hand| hand.1),
        };
        let placement = self.gestures.update_placement(
            &mut self.drawings,
            &drawing_projection::DrawingProjection {
                series: drawing_projection::PaneSeriesRead {
                    history_prefix: &self.history_prefix,
                    state: &self.state,
                    spec: &self.spec,
                },
                viewport: &self.viewport,
                indicators: &self.indicators,
            },
            placement_gestures::PlacementFrame {
                ui,
                areas: &areas,
                bands: &bands,
                id: placement_id,
                history_right: self.frame.lane_divider_x.unwrap_or(areas.chart.right()),
            },
            options,
            placement_gestures::PlacementDefaults {
                presets: chrome.presets,
                repeat: chrome.toolrail.repeat(),
            },
        );
        if let Some(position) = placement.hover_position {
            self.hover_pos = position;
        }
        if placement.completion.arm_pointer {
            chrome.toolrail.arm(Tool::Pointer);
        }
        if placement.completion.begin_text_edit {
            *chrome.begin_text_edit = true;
        }
        let tool_armed = placement.tool_armed;
        let auto = self.frame.auto_range;
        let height = self.frame.chart_height;
        let total = self.slots();
        let divider = self.frame.lane_divider_x;
        // Only the divider's own handle is off limits to the pan, not the whole
        // band: the resize gesture and the pan must never both fire on one
        // pixel, which is what `gesture_hits_lane` was written for — but
        // spending a third of the canvas to protect a ten-pixel handle is a
        // dead zone, not a guard.
        let on_divider = |position: egui::Pos2| {
            plot_area::gesture_hits_lane_divider(divider, position.x, LANE_HANDLE_HALF_WIDTH_PX)
        };

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            self.interaction_id("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        // While a tool is armed the placement surface owns the hover: it spans
        // every band, and the candles' own response would report `None` for a
        // pointer one pane below them.
        if !tool_armed {
            self.hover_pos = chart.hover_pos();
        }
        // The tape switch, in the canvas's top-right corner. Registered after
        // the chart body so the chip is on top of it — the lane divider's and
        // the jump-to-live chip's rule — and it is the *only* way back once the
        // tape is off: with no band there is no tape to right-click, so a
        // switch that lived only in that menu would be a one-way door.
        if self.orderflow.is_some() {
            let on = self.layer_visible(ChartLayer::TapeChart, chrome.style);
            let clicked =
                self.tape_switch
                    .handle(ui, areas.chart, self.interaction_id("tape_switch"), on);
            if self.tape_switch.hovered() {
                // The chip is chrome on top of the canvas. A crosshair chasing
                // the pointer underneath it would say the chart is being
                // hovered while the pointer is reading a button.
                self.hover_pos = None;
            }
            if clicked {
                self.set_layer_visible(ChartLayer::TapeChart, !on, chrome.layers);
            }
        } else {
            self.tape_switch.absent();
        }
        // The paper lines and the right-click price live on the candles, and
        // only there: an order is a price, not a value on someone's oscillator.
        let price_band = &bands[0];
        let history_right = self.frame.lane_divider_x.unwrap_or(areas.chart.right());
        self.quick_range_view(chrome.tab, chrome.side).handle(
            ui,
            price_band,
            history_right,
            total,
            magnet,
            chrome,
        );
        self.handle_context_menu(&chart, &areas, &bands, chrome);
        let drawing_area = price_band.rect;
        let (primary_pressed, primary_down, primary_released, pointer_position, pointer_delta) = ui
            .input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_down(),
                    input.pointer.primary_released(),
                    input.pointer.latest_pos(),
                    input.pointer.delta(),
                )
            });
        // Floating chrome (inspector, manager, toast, flyouts) is opaque to
        // the pointer: while it sits under the cursor the chart neither sets
        // a cursor nor selects nor starts a drag. The gate applies at press
        // time only — a drag that started on the canvas keeps running while
        // the pointer travels across a panel (continuity, not priority).
        let over_chrome = pointer_position
            .and_then(|position| ui.ctx().layer_id_at(position))
            .is_some_and(|layer| layer != ui.layer_id());
        let pointer = SharedPointer {
            position: pointer_position,
            area: drawing_area,
            over_chrome,
            pressed: primary_pressed,
            down: primary_down,
            released: primary_released,
            history_right,
            total,
            magnet,
        };
        let paper_layer_visible = self.layer_visible(ChartLayer::PaperTrading, chrome.style);
        let paper_gesture = primary_button::PaperArbitration {
            projection: &self.drawing_projection(),
            drawings: &self.drawings,
            layer_visible: paper_layer_visible,
        }
        .handle(ui, chrome, &areas, &bands, &pointer, tool_armed);
        let projection = drawing_projection::DrawingProjection {
            series: drawing_projection::PaneSeriesRead {
                history_prefix: &self.history_prefix,
                state: &self.state,
                spec: &self.spec,
            },
            viewport: &self.viewport,
            indicators: &self.indicators,
        };
        let outcome = self.gestures.handle_pointer_tool(
            &mut self.drawings,
            &projection,
            pointer_gestures::PointerFrame {
                ui,
                chart: &chart,
                areas: &areas,
                bands: &bands,
                cached_bands: &self.frame.bands,
                pointer: &pointer,
                pointer_delta,
                paper_gesture,
                tool: chrome.toolrail.tool(),
                shared_pick: chrome.shared_pick,
                shared: chrome.shared,
            },
        );
        if let Some(cursor) = outcome.cursor {
            ui.ctx().set_cursor_icon(cursor);
        }
        if outcome.begin_text_edit {
            *chrome.begin_text_edit = true;
        }
        chrome.shared = outcome.shared;
        let drawing_drag_consumes_gesture = outcome.consumed;
        // Whether the primary button is still the chart's this frame. An
        // armed tool, a drawing being dragged and a grabbed paper line each
        // take it — and only it. Everything that is not the primary button
        // keeps answering throughout: the wheel over the candles and over
        // every pane, both axis gutters, the time strip, the lane and pane
        // dividers, the collapse chevrons, and the middle-button pan added
        // below. A primary *drag* with a tool armed is that tool's second
        // anchor, so it cannot also pan — which is exactly why the middle
        // button does.
        let primary_free = !tool_armed && !drawing_drag_consumes_gesture && !paper_gesture;
        // Anywhere on the canvas, tape band included — the same call the wheel
        // already answers this way, and for the same reason. The lane used to
        // swallow the drag while the pointer was over it: a third of the canvas
        // where pressing and pulling did nothing at all, with nothing on screen
        // saying why. That was survivable while a lane only existed on a feed
        // with a book; now that the tape is anchored on prints it appears on
        // every feed, and the dead zone became the first thing a trader hits.
        //
        // The lane keeps the gestures that are unambiguously its own: the
        // divider resizes it, and its own time strip sets its window. A drag
        // across the band is not one of them — the tape does not pan, it is
        // pinned to the live edge, so a drag there had no second meaning to
        // protect.
        //
        // Primary only: egui's `dragged()` counts every button, and the
        // secondary drag is the quick range's (`pane/quick_range.rs`) — a pan
        // under it keeps the same bar beneath the pointer and collapses the
        // range onto its first anchor. The middle button pans below, in its
        // own block; answering it here as well doubled its speed.
        let grabbing_divider = chart.interact_pointer_pos().is_some_and(&on_divider);
        if total > 0
            && chart.dragged_by(egui::PointerButton::Primary)
            && !grabbing_divider
            && primary_free
        {
            let drag = chart.drag_delta();
            self.viewport.pan_pixels(drag.x, total);
            if let Some(auto) = auto
                && drag.y != 0.0
                && height > 1.0
            {
                let (lo, hi) = self.price_view.resolve(auto);
                let price_per_px = (hi - lo) / f64::from(height);
                self.price_view
                    .pan_screen(f64::from(drag.y), price_per_px, auto);
            }
        }
        // Not while a tool is armed: two placement clicks in a row are two
        // anchors, never a request to jump back to the live edge.
        if chart.double_clicked() && primary_free {
            // On an overlay's own line the gesture means that line: a trader
            // pointing at a curve and double clicking is asking about the
            // curve, not about the viewport. Everywhere else on the canvas it
            // still snaps back to the live edge, which is the only reading a
            // double click on empty chart can have.
            match chart
                .interact_pointer_pos()
                .and_then(|pos| self.hit_test().overlay_plot_at(pos))
            {
                Some(slot) => self.pending_settings = Some(slot),
                None => {
                    self.viewport.snap_to_live();
                    self.price_view.reset();
                }
            }
        }
        // One wheel, one meaning at a time. While an aim is up the ruler has
        // already spent this frame's travel walking a bracket out, and the
        // same roll must not also rescale the plot under it. The guard is
        // here as well as on the stacked panes' accumulated gesture, because
        // this is the canvas the aim actually lives on.
        if chart.hovered() && !chrome.paper.consumed_scroll() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                // Scroll up (positive) zooms in — the candles, wherever on the
                // canvas the pointer rests, tape band included.
                //
                // The lane used to steal this wheel while the pointer was over
                // it: one gesture, two meanings, and nothing on screen saying
                // which one you were about to get, so crossing a hairline
                // divider mid-scroll zoomed the tape instead of the chart. The
                // lane's window still zooms — from the lane's own time strip
                // below it (drag or scroll), which is the grammar every other
                // axis here already follows: an axis zooms its axis, the canvas
                // zooms the chart.
                self.viewport.zoom(2.0_f32.powf(scroll / SCROLL_ZOOM_PX));
            }
        }
        // The middle button pans, always — including mid-placement, which is
        // the whole reason it exists. A drag with a tool armed is that tool's
        // second anchor, so the primary button genuinely cannot pan then; a
        // trader who drops one end of a trend line and finds the other end
        // off screen would otherwise have to cancel the object to go look for
        // it. Same axes, same feel, a button the tools never take.
        if total > 0 {
            let (middle_down, delta) =
                ui.input(|input| (input.pointer.middle_down(), input.pointer.delta()));
            if middle_down
                && chart
                    .hover_pos()
                    .is_some_and(|position| areas.chart.contains(position) && !on_divider(position))
            {
                self.viewport.pan_pixels(delta.x, total);
                if let Some(auto) = auto
                    && delta.y != 0.0
                    && height > 1.0
                {
                    let (lo, hi) = self.price_view.resolve(auto);
                    let price_per_px = (hi - lo) / f64::from(height);
                    self.price_view
                        .pan_screen(f64::from(delta.y), price_per_px, auto);
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }

        self.handle_axis_gestures(ui, &areas, chrome);

        self.handle_indicator_pane_gestures(ui, &areas, chrome, primary_free);
    }

    /// The HUD anchor cached by the last draw, if the paper layer was
    /// painted on the pane that owns order entry.
    #[must_use]
    pub fn paper_hud_anchor(&self) -> Option<(egui::Rect, PriceScale)> {
        self.paper_hud_anchor
    }
}
