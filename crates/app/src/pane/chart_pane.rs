//! The chart pane itself: what one pane owns between frames. The passes that read it live in the
//! siblings (`draw_chart`, `frame`, `gestures`), so this file is state and accessors, not drawing.

use eframe::egui;

use quantick_control_handlers::bubble_scale::BubbleScalePane;
use quantick_layers::ChartLayer;

use crate::bands;
use crate::chart::PriceScale;
use crate::config::FeedCapabilities;
use crate::drawings::{self, Drawings};
use crate::indicator_worker::{IndicatorWorker, LaneTransport, SlotId};
use crate::indicators::{IndicatorViews, PaneSizing};
use crate::orderflow_view::OrderflowView;
use crate::plot_area::{PlotAreas, plot_split};
use crate::price_view::PriceView;
use crate::state::{BarConfiguration, BarSpec, ChartState, SpecSelector};
use crate::toolrail::Tool;

use super::context_menu::ContextPress;
use super::menus::{PaneMenuHosts, PaneMenuIntent, PaneMenuView, TapeMenuView};
use super::*;

#[cfg(test)]
impl ChartPane {
    pub(crate) fn install_layer_probe(&mut self) {
        render_registry::probe::install(self);
    }
}

/// Which side of the candles a drawing pass paints on. One function serves both, taking this rather
/// than being copied: projection, band filter, off-series fade and style resolution are the same
/// work either way, and a second copy would drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawPass {
    /// Before the candles: the object's own body, for the few tools that are
    /// context rather than annotation. See [`quantick_drawings::MarkTool::paint_under`].
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
    pub(super) history_worker: history_worker::HistoryWorker,
    /// The tape, and everything read off it: the live lane, the heatmap, the bubbles, the live
    /// strip. `None` is what makes a time pane a time pane: §11 keeps the flow layers on the flow
    /// pane, and a pane that never draws them has no business running a book worker thread.
    pub orderflow: Option<OrderflowView>,
    /// Background thread owning the `IndicatorHost`; the UI only sends
    /// commands and applies the delta events back.
    pub indicator_worker: IndicatorWorker,
    /// The UI's copy of every indicator's plot columns (see
    /// [`crate::indicators`]).
    pub indicators: IndicatorViews,
    /// Headless pane binding; the workspace session owns authoritative membership.
    pub(crate) layout: quantick_workspace::pane_layout::PaneLayout,
    /// Whether this pane's on-chart indicator legend is folded to its count puck. Per pane, not per
    /// window: a split is two readings of the same market, and the corner pressure that makes a
    /// trader fold the flow pane's legend (bubbles, book, the position HUD) is absent on the time
    /// pane. Expanded by default.
    pub legend_collapsed: bool,
    /// Requested switches not already owned by another feature, and the
    /// headless catalog that resolves policy for every layer.
    pub layers: quantick_layers::LayerState,
    pub(super) layer_renderers: &'static render_registry::RenderRegistry,
    /// The candle footprint layer as this pane has it — see
    /// [`PaneFootprint`].
    pub footprint: PaneFootprint,

    /// The bar rule this pane is on, and the parameter every other kind is
    /// holding for the trader — see [`SpecSelector`].
    pub spec: SpecSelector,

    // Pan/zoom navigation over the bar series. It owns the history pane only:
    // the live lane is a band of screen to its right that answers to nothing
    // it does.
    pub model: quantick_chart_interaction::pane::Model,
    /// What the last draw measured, for the passes that are not the draw — see
    /// [`PaneFrame`].
    pub frame: PaneFrame,
    // The price-axis levels this frame's drawings declare: per-frame, reused as a container, and
    // gathered before the axis labels itself because the axis stands aside where one of these
    // lands.
    pub(super) price_axis_levels: Vec<PriceAxisLevel>,
    // How many rungs the lane was wide enough for at the last draw, so how finely the next publish
    // samples the forming bar across it. `0` with no lane: the worker walks no ladder and the panes
    // draw nothing on the tape.
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

    /// Venue candles in front of the trade-derived series, folded to this pane's interval.
    ///
    /// Deliberately outside `ChartState`, which rebuilds its bars from retained trades on every
    /// spec change and would eat a prefix kept inside it on the first chip click. It is composed
    /// with the engine's bars where they are read ([`Self::slots`], [`Self::closed_bar`], the
    /// rebuild payload, the draw).
    ///
    /// Non-empty on any pane cutting by a foldable time interval: the split's time pane, and the
    /// flow pane whenever its spec is `BarSpec::Time` (audit S1). A venue candle has no tape, so
    /// the flow layers draw nothing over it.
    pub history_prefix: quantick_chart::venue_history::VenueHistory,

    /// Where the position HUD anchors this frame: the chart rect and price scale, cached by the
    /// draw while the paper layer is painted on the pane that owns order entry. The HUD draws in
    /// `tab.rs`, where the paper host is mutably reachable outside the chrome's shared borrow.
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
}

impl ChartPane {
    /// The tape's two modes, `(tape_only, native_tape)`: tape only hides the
    /// candles, the native tape fits the shared price axis beside them or alone.
    pub(crate) fn tape_modes(&self) -> (bool, bool) {
        self.orderflow.as_ref().map_or((false, false), |view| {
            let config = view.cached_config();
            (config.tape_only(), config.native_tape())
        })
    }

    /// What the bubble size rule reads about this pane, the facts its size
    /// sliders and `orderflow.bubbles.size.set` both judge; `None` without
    /// an order-flow view.
    pub(crate) fn bubble_scale_facts(&self) -> Option<BubbleScalePane> {
        let config = self.orderflow.as_ref()?.cached_config();
        Some(BubbleScalePane {
            tick_chart: self.state.tick_membership().is_some(),
            candle_bubbles: config.show_aggressions,
            tape_only: config.tape_only(),
            tape_bubbles: config.lane_aggressions_drawn(),
        })
    }

    /// Every mode-entry path starts at its own fit, including source presets:
    /// a new axis source (tape or candles) or entering or leaving tape only.
    pub(crate) fn sync_price_axis_mode(&mut self) {
        let modes = self.tape_modes();
        use quantick_chart_interaction::pane::{Intent, update};
        let effects = update(
            &mut self.model,
            Intent::PriceAxisMode {
                tape_only: modes.0,
                native_tape: modes.1,
            },
        );
        if !effects.is_empty() {
            self.price_view.reset();
            self.frame.auto_range = None;
        }
    }

    pub(crate) fn series_read(&self) -> gesture::PaneSeriesRead<'_> {
        gesture::PaneSeriesRead {
            history_prefix: &self.history_prefix,
            state: &self.state,
            spec: &self.spec,
        }
    }
    pub(crate) fn drawing_projection(&self) -> gesture::DrawingProjection<'_> {
        gesture::DrawingProjection {
            series: self.series_read(),
            viewport: &self.model.viewport,
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
            projection: gesture::DrawingProjection {
                series: gesture::PaneSeriesRead {
                    history_prefix: &self.history_prefix,
                    state: &self.state,
                    spec: &self.spec,
                },
                viewport: &self.model.viewport,
                indicators: &self.indicators,
            },
            drawings: &mut self.drawings,
        }
    }

    /// This pane's store and its gesture's copy of the anchors, beside the
    /// series they anchor to — see [`gesture::PaneMarksMut`].
    pub(super) fn marks_mut(&mut self) -> gesture::PaneMarksMut<'_> {
        gesture::PaneMarksMut {
            drawings: &mut self.drawings,
            gestures: &mut self.gestures,
            series: gesture::PaneSeriesRead {
                history_prefix: &self.history_prefix,
                state: &self.state,
                spec: &self.spec,
            },
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
    ) -> (&mut PaneStrategies, gesture::PaneSeriesRead<'_>) {
        (
            &mut self.strategies,
            gesture::PaneSeriesRead {
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
        self.layout.layout_id()
    }
    pub(crate) fn layout_seeded(&self) -> bool {
        self.layout.seeded()
    }
    pub(crate) fn request_opening_layout(&mut self, id: Option<crate::layouts::LayoutId>) {
        self.layout.request_opening(id);
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

        let drawings = Drawings::default();
        let model = quantick_chart_interaction::pane::Model {
            selection: drawings.selection_handle(),
            ..Default::default()
        };
        Self {
            id,
            spec: selector,
            state: ChartState::new(spec),
            history_worker: Default::default(),
            orderflow,
            indicator_worker: IndicatorWorker::spawn(),
            indicators: IndicatorViews::new(),
            layout: quantick_workspace::pane_layout::PaneLayout::default(),
            legend_collapsed: false,
            layers: quantick_layers::LayerState::new(render_registry::standard().layers()),
            layer_renderers: render_registry::standard(),
            footprint: PaneFootprint::default(),
            // The backfill divider opens off: a full-height rule across the candles for a boundary
            // that matters once, when reading how far the live tape goes back. Nothing is hidden
            // about the data; the mark is one click away in the layer menu.
            model,
            frame: PaneFrame::default(),
            price_axis_levels: Vec::new(),
            lane: LaneTransport::default(),
            price_view: PriceView::new(),
            price_band_label: std::sync::Arc::from(bands::PRICE_BAND_LABEL),
            hover_pos: None,
            tape_switch: TapeSwitch::default(),
            history_prefix: quantick_chart::venue_history::VenueHistory::default(),
            paper_hud_anchor: None,
            context_menu: PaneContextMenu::default(),
            strategies: PaneStrategies::default(),
            drawings,
            gestures: PaneGestures::default(),
        }
    }

    /// Whether any placed or in-flight drawing is a fixed-range volume profile, the second consumer
    /// of the footprint ladders, keeping accumulation on while the layer is hidden. O(drawings),
    /// once per frame, never on the ingestion path.
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

    /// Put this pane on `spec` outright, selectors included. Startup-scoped: the caller is a
    /// workspace restoring the bar rule this pane was last read on, into a pane that has not drawn
    /// a frame yet; a live change goes through [`SpecSelector::pending`] so the frame carrying it
    /// paints the loading overlay before the rebuild replays the tape.
    ///
    /// The selectors move with the spec because the BARS group reads them: setting the state alone
    /// would restore a chart whose own controls disagreed with it, and the first touch of the
    /// parameter would snap it back to a rule the trader never chose.
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
        self.model.history.revision()
    }

    pub(super) fn bump_pagination_revision(&mut self) {
        self.model.history.changed();
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

    /// Width reserved for the live strip this frame. The aggression histogram runs on the trade
    /// stream every source provides (replay included), so without book data the strip degrades to
    /// that histogram alone. A pane with no tape has no strip (§11).
    pub fn live_strip_width(&self, capabilities: FeedCapabilities) -> f32 {
        // Both halves of the strip come from the source: resting depth and the aggressions landing
        // into it. A source producing neither fills none of it, and the band would be an empty rect
        // narrowing the candles for good. Passed in rather than cached on the pane, for the reason
        // `layer_blocked` states: the running feed is resolved once per frame by the caller, and a
        // copy here would be one more thing to keep in step when MetaTrader narrows its
        // capabilities mid-session.
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

    /// The indicator a gesture on this pane asked to configure, if any, taken so a request is acted
    /// on exactly once.
    pub fn take_settings_request(&mut self) -> Option<SlotId> {
        self.model.pending_settings.take().map(SlotId)
    }

    pub(crate) fn take_indicator_guide_request(&mut self) -> Option<(SlotId, bool)> {
        self.model
            .pending_indicator_guide
            .take()
            .map(|(slot, enabled)| (SlotId(slot), enabled))
    }

    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) fn first_indicator_pane_center(&self) -> Option<egui::Pos2> {
        self.frame
            .bands
            .get(1)
            .map(|band| drawings::pos(band.rect.center()))
    }

    #[cfg(test)]
    pub(crate) fn request_indicator_guide(&mut self, slot: SlotId, enabled: bool) {
        self.model.pending_indicator_guide = Some((slot.0, enabled));
    }

    /// Stand in for the gesture that raises a settings request, so the app's
    /// side of the wiring can be tested without driving egui through a pane
    /// layout it would have to re-derive.
    #[cfg(test)]
    pub fn request_settings(&mut self, slot: SlotId) {
        self.model.pending_settings = Some(slot.0);
    }

    /// Handle mouse navigation, TradingView-style:
    /// - drag the candles → pan time (x, moves the whole chart) and price (y);
    /// - scroll over them → zoom time;
    /// - drag the bottom time strip left/right → zoom time (spread candles);
    /// - drag the right price gutter up/down → zoom the price scale;
    /// - scroll over either axis → zoom that axis;
    /// - double-click a candle → the chart settings; open canvas → back to live
    ///   at the overview spacing, keeping the price scale;
    /// - double-click the price gutter → resume automatic price fitting.
    ///
    /// The native Tape owns its time navigation: dragging moves its history,
    /// scrolling zooms its window, and double-clicking returns it to live.
    pub fn handle_navigation(
        &mut self,
        ui: &egui::Ui,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.sync_price_axis_mode();
        self.unhide_layer_for_armed_tool(chrome);
        // The magnet applies to every anchor gesture, placement and re-drag
        // alike: a handle that snaps only while you first draw it would make
        // the second edit undo the precision of the first. An object's own
        // magnet joins it on that object's drags (`pointer_gestures`).
        let magnet = chrome.toolrail.magnet();
        // Remembered for inspector placement and manager centring: the pane
        // where drawings live, already free of both axes and the live lane.
        self.frame.plot_area = Some(area);
        let areas = self.plot_areas(area, chrome.capabilities);
        self.frame.chart_area = Some(areas.chart);
        let candles_right = self.frame.lane_divider_x.unwrap_or(areas.chart.right());
        self.model.candles_px = candles_right - areas.chart.left();
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
        // A drawing tool consumes the *primary button*, not the chart: pan, wheel zoom, pane
        // dividers and collapse chevrons keep working while one is armed (audit S2).
        let placement_id = self.interaction_id("drawing_placement");
        #[cfg(any(feature = "drawing-harness", test))]
        let hand = self
            .gestures
            .parked_hand
            .map(|hand| (hand.constrain, hand.position));
        #[cfg(not(any(feature = "drawing-harness", test)))]
        let hand: Option<(drawings::Constrain, quantick_drawings::Px)> = None;
        let options = gesture::PlacementOptions {
            tool: chrome.toolrail.tool().drawing_tool(),
            magnet: chrome.toolrail.magnet(),
            constrain: if ui.input(|input| input.modifiers.shift) {
                drawings::Constrain::Level
            } else {
                hand.map_or(drawings::Constrain::Free, |hand| hand.0)
            },
            parked_position: hand.map(|hand| hand.1),
        };
        let placement = gesture_input::update_placement(
            &mut self.gestures,
            &mut self.drawings,
            &gesture::DrawingProjection {
                series: gesture::PaneSeriesRead {
                    history_prefix: &self.history_prefix,
                    state: &self.state,
                    spec: &self.spec,
                },
                viewport: &self.model.viewport,
                indicators: &self.indicators,
            },
            gesture_input::PlacementInput {
                ui,
                areas: &areas,
                bands: &bands,
                id: placement_id,
                history_right: self.frame.lane_divider_x.unwrap_or(areas.chart.right()),
            },
            options,
            gesture::PlacementDefaults {
                presets: chrome.presets,
                repeat: chrome.toolrail.repeat(),
            },
        );
        if let Some(position) = placement.hover_position {
            self.hover_pos = position.map(drawings::pos);
        }
        if placement.completion.arm_pointer {
            chrome.toolrail.arm(Tool::Pointer);
        }
        if placement.completion.begin_text_edit {
            *chrome.begin_text_edit = true;
        }
        let tool_armed = placement.tool_armed;
        let total = self.slots();

        // Chart body: `canvas_gestures` pans and zooms it.
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
        // The tape switch, in the canvas's top-right corner. Registered after the chart body so the
        // chip sits on top of it, and the *only* way back once the tape is off: with no band there
        // is no tape to right-click, so a menu-only switch would be a one-way door.
        let tape_only = self.tape_modes().0;
        if self.orderflow.is_some() {
            let on = self.layer_visible(ChartLayer::TapeChart, chrome.style);
            let clicked = self.tape_switch.handle(
                ui,
                areas.chart,
                self.interaction_id("tape_switch"),
                on,
                tape_only,
            );
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
        let (primary_pressed, primary_down, primary_released, pointer_position, modifiers) = ui
            .input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_down(),
                    input.pointer.primary_released(),
                    input.pointer.latest_pos(),
                    input.modifiers,
                )
            });
        // Floating chrome (inspector, manager, toast, flyouts) is opaque to the pointer: while it
        // sits under the cursor the chart neither sets a cursor nor selects nor starts a drag.
        // Gated at press time only, so a drag begun on the canvas keeps running across a panel.
        let over_chrome =
            pointer_position.is_some_and(|position| gesture_input::over_chrome(ui, position));
        let pointer = SharedPointer {
            position: pointer_position.map(drawings::px),
            area: price_band.rect,
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
            tape_switch: super::tape_switch_rect(areas.chart, tape_only),
        }
        .handle(ui, chrome, &areas, &bands, &pointer, tool_armed);
        let projection = gesture::DrawingProjection {
            series: gesture::PaneSeriesRead {
                history_prefix: &self.history_prefix,
                state: &self.state,
                spec: &self.spec,
            },
            viewport: &self.model.viewport,
            indicators: &self.indicators,
        };
        // Any strategy instance bound to a drawing holds it, armed or not:
        // its extent is that strategy's window.
        let anchors = &self.strategies.anchors;
        let held = |id| anchors.for_drawing(id).is_some();
        let outcome = self.gestures.handle_pointer_tool(
            &mut self.drawings,
            &projection,
            gesture::PointerFrame {
                bands: &bands,
                cached_bands: &self.frame.bands,
                pointer: &pointer,
                on_pane_chrome: pointer_position
                    .is_some_and(|position| primary_button::pane_chrome_hit(&areas, position)),
                alt: modifiers.alt,
                shift: modifiers.shift,
                double_clicked: chart.double_clicked(),
                paper_gesture,
                pointer_tool: chrome.toolrail.tool() == Tool::Pointer,
                shared_pick: chrome.shared_pick,
                shared: chrome.shared,
                held: &held,
            },
        );
        if let Some(cursor) = outcome.cursor {
            ui.ctx().set_cursor_icon(gesture_input::cursor_icon(cursor));
        }
        if outcome.begin_text_edit {
            *chrome.begin_text_edit = true;
        }
        chrome.shared = outcome.shared;
        let drawing_drag_consumes_gesture = outcome.consumed;
        // Whether the primary button is still the chart's this frame. An armed tool, a drawing
        // being dragged and a grabbed paper line each take it, and only it: the wheel, both axis
        // gutters, the time strip, the lane and pane dividers, the collapse chevrons and the
        // middle-button pan added below keep answering. A primary drag with a tool armed is that
        // tool's second anchor, so it cannot also pan, which is why the middle button does.
        let primary_free = !tool_armed && !drawing_drag_consumes_gesture && !paper_gesture;
        chrome.layers.open_chart_settings |= self.handle_canvas_gestures(
            ui,
            super::canvas_gestures::CanvasInput {
                chart: &chart,
                area: areas.chart,
                total,
                primary_free,
                scroll_taken: chrome.paper.consumed_scroll(),
            },
        );

        axes_and_panes::handle_axis_gestures(self, ui, &areas, chrome);

        axes_and_panes::handle_indicator_pane_gestures(self, ui, &areas, chrome, primary_free);
    }

    /// The HUD anchor cached by the last draw, if the paper layer was
    /// painted on the pane that owns order entry.
    #[must_use]
    pub fn paper_hud_anchor(&self) -> Option<(egui::Rect, PriceScale)> {
        self.paper_hud_anchor
    }
}

/// The menus' door into the pane: the read-only view they draw from, and the
/// one place every [`PaneMenuIntent`] they answer with is applied.
impl ChartPane {
    /// The menu component beside the read-only view it draws from, borrowed
    /// apart so the menu can own its state while it reads the pane's.
    pub(crate) fn menu_parts<'a>(
        &'a mut self,
        capabilities: FeedCapabilities,
        style: &'a crate::style::ChartStyle,
    ) -> (
        &'a mut PaneContextMenu,
        &'a mut quantick_chart_interaction::pane::Model,
        PaneMenuView<'a>,
    ) {
        let tape = self
            .orderflow
            .as_ref()
            .filter(|_| self.model.menu.on_tape)
            .map(|orderflow| TapeMenuView {
                window: orderflow.live_lane_window(),
                reference_ms: self.frame.lane_reference_ms,
            });
        let flow_opening = self
            .orderflow
            .as_ref()
            .filter(|owner| self.state.tick_membership().is_some() && owner.flow_execution_active())
            .map(|owner| owner.ignore_flow_opening());
        let view = PaneMenuView {
            drawings: &self.drawings,
            indicators: &self.indicators,
            strategies: &self.strategies,
            series: gesture::PaneSeriesRead {
                history_prefix: &self.history_prefix,
                state: &self.state,
                spec: &self.spec,
            },
            layers: layers::PaneLayerRead {
                layers: &self.layers,
                orderflow: self.orderflow.as_ref(),
                state: &self.state,
                footprint: &self.footprint,
                drawings: &self.drawings,
            },
            capabilities,
            style,
            flow_opening,
            tape,
        };
        (&mut self.context_menu, &mut self.model, view)
    }

    /// Where the chart-layer submenu button was painted — see
    /// [`PaneContextMenu::chart_layers_center`].
    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) fn chart_layers_menu_center(&self) -> Option<egui::Pos2> {
        self.context_menu.chart_layers_center()
    }

    /// The layer menu, drawn and applied: what the right-click opens.
    pub fn draw_layer_menu(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        let intents = {
            let (menu, model, view) = self.menu_parts(chrome.capabilities, chrome.style);
            let hosts = PaneMenuHosts {
                paper: &mut *chrome.paper,
            };
            menu.draw_layer_menu(ui, &view, model, hosts)
        };
        self.apply_menu_intents(intents, chrome);
    }

    /// One layer's checkbox in an axis's own menu, applied like the layer
    /// menu's.
    pub(super) fn layer_menu_switch(
        &mut self,
        ui: &mut egui::Ui,
        layer: ChartLayer,
        chrome: &mut PaneChrome<'_>,
    ) {
        let row = self
            .layer_read()
            .row(layer, chrome.capabilities, chrome.style);
        let intent = self.context_menu.layer_checkbox(ui, row);
        self.apply_menu_intents(intent, chrome);
    }

    /// The secondary click on the canvas: what the press resolves (the price,
    /// the tape flag, the drawing, the placing entries), the layer menu it
    /// opens, and the rename an outside click commits when the menu closes.
    ///
    /// One arm of [`ChartPane::handle_navigation`], called once per frame.
    pub(super) fn handle_context_menu(
        &mut self,
        chart: &egui::Response,
        areas: &PlotAreas,
        bands: &bands::Bands,
        chrome: &mut PaneChrome<'_>,
    ) {
        // The price under a right-click, remembered before the menu eats
        // the pointer: the trade section places orders at it.
        if chart.secondary_clicked()
            && let Some(position) = chart.interact_pointer_pos()
            && let Some(press) = ContextPress::resolve(
                position,
                areas,
                bands,
                &self.drawing_projection(),
                &self.drawings,
                &self.frame,
                self.slots(),
            )
        {
            let select = self
                .context_menu
                .open_at(&mut self.model, press, &self.drawings);
            self.apply_menu_intents(select, chrome);
        }
        // Right-click: what is on this canvas, and what is not. Secondary
        // button only, so it shares no gesture with the pan, the zoom or the
        // drawing tools — a pan that ends anywhere never opens it.
        chart.context_menu(|ui| self.draw_layer_menu(ui, chrome));
        let count = self.drawings.items().len();
        let clear = self.context_menu.draw_clear_objects_confirm(
            &mut self.model,
            &chart.ctx,
            areas.chart,
            count,
            self.id,
        );
        self.apply_menu_intents(clear, chrome);
        // While the menu is open the pointer is reading it, not the chart, so
        // no crosshair chases it across the candles behind it.
        if chart.context_menu_opened() {
            self.hover_pos = None;
        } else {
            let commit = self.context_menu.close(&mut self.model, &self.drawings);
            self.apply_menu_intents(commit, chrome);
        }
    }

    /// Apply a frame's menu intents in the order they were asked.
    pub(crate) fn apply_menu_intents(
        &mut self,
        intents: impl IntoIterator<Item = PaneMenuIntent>,
        chrome: &mut PaneChrome<'_>,
    ) {
        for intent in intents {
            self.apply_menu_intent(intent, chrome);
        }
    }

    pub(crate) fn apply_menu_intent(
        &mut self,
        choice: PaneMenuIntent,
        chrome: &mut PaneChrome<'_>,
    ) {
        use quantick_chart_interaction::pane::{Effect, Intent, update};
        let id = choice.drawing_id();
        let drawing = id
            .and_then(|id| self.drawings.index_of(id))
            .map(|index| super::context_menu::drawing_fact(&self.drawings.items()[index]));
        for effect in update(&mut self.model, Intent::Menu { choice, drawing }) {
            if let Effect::Menu(intent) = effect {
                self.execute_menu_effect(intent, chrome);
            }
        }
    }

    /// The one place a menu's ask writes the pane. Each arm calls the same
    /// pane operation the click called before menus answered with intents.
    /// That is not a claim that each is a control-plane capability: several
    /// (the anchored VWAP's place; select, rename, lock, hide and delete of a
    /// drawing; the strategy seat's add, disarm, re-arm and remove — control
    /// has only `trade.strategy.select`; an indicator's hide, the footprint
    /// settings, clear objects) have no control capability today — a gap
    /// that predates this apply site. Drawing and strategy intents name the
    /// drawing by id and do nothing if it is gone.
    fn execute_menu_effect(&mut self, intent: PaneMenuIntent, chrome: &mut PaneChrome<'_>) {
        match intent {
            PaneMenuIntent::SetLayerVisible { layer, visible } => {
                self.set_layer_visible(layer, visible, chrome.layers);
            }
            PaneMenuIntent::SetPriceInverted(inverted) => self.price_view.set_inverted(inverted),
            PaneMenuIntent::SetIgnoreFlowOpening(ignore) => {
                if let Some(owner) = self.orderflow.as_mut() {
                    owner.set_ignore_flow_opening(ignore);
                }
            }
            PaneMenuIntent::OpenFootprintSettings => chrome.layers.open_footprint_settings = true,
            PaneMenuIntent::SetLaneWindow(window) => {
                if let Some(orderflow) = self.orderflow.as_mut() {
                    orderflow.set_live_lane_window(window);
                }
            }
            PaneMenuIntent::Place { tool, point } => {
                let Some(tool) = drawings::DRAWING_TOOLS
                    .into_iter()
                    .find(|entry| entry.id() == tool)
                else {
                    return;
                };
                let completion = self.gestures.place_point(
                    &mut self.drawings,
                    tool,
                    &drawings::DrawingBand::Price,
                    point,
                    gesture::PlacementDefaults {
                        presets: chrome.presets,
                        repeat: chrome.toolrail.repeat(),
                    },
                );
                if completion.arm_pointer {
                    chrome.toolrail.arm(Tool::Pointer);
                }
                if completion.begin_text_edit {
                    *chrome.begin_text_edit = true;
                }
            }
            PaneMenuIntent::ToggleIndicatorHidden(slot) => {
                self.indicators.toggle_hidden(SlotId(slot));
                chrome.layers.indicators_changed = true;
            }
            PaneMenuIntent::SelectDrawing(id) => {
                if let Some(index) = self.drawings.index_of(id) {
                    self.drawings.select(Some(index));
                }
            }
            PaneMenuIntent::RenameDrawing { id, name } => {
                if let Some(index) = self.drawings.index_of(id) {
                    self.drawings.rename_at(index, &name);
                }
            }
            PaneMenuIntent::SetDrawingLocked { id, locked } => {
                if let Some(index) = self.drawings.index_of(id) {
                    self.drawings.set_locked_at(index, locked);
                }
            }
            PaneMenuIntent::SetDrawingHidden { id, hidden } => {
                if let Some(index) = self.drawings.index_of(id) {
                    self.drawings.set_hidden_at(index, hidden);
                }
            }
            PaneMenuIntent::DeleteDrawing(id) => {
                let Some(index) = self.drawings.index_of(id) else {
                    return;
                };
                self.drawings.select(Some(index));
                if self.drawings.delete_selected(false) == drawings::DeleteOutcome::Deleted {
                    // The instance dies with its drawing, immediately — not
                    // on the next closed bar, which a quiet tape may never
                    // bring.
                    self.strategies.remove_for_drawing(id);
                }
            }
            PaneMenuIntent::StrategyAdd(id) => {
                if self.drawings.index_of(id).is_some() {
                    self.strategies.popup_request = Some(id);
                }
            }
            // The drawing went without its strategy being swept yet: the
            // seat that asked is gone, so nothing is done in its name.
            PaneMenuIntent::StrategyDisarm(id)
            | PaneMenuIntent::StrategyRearm(id)
            | PaneMenuIntent::StrategyRemove(id)
                if self.drawings.index_of(id).is_none() => {}
            PaneMenuIntent::StrategyDisarm(id) => self.strategies.disarm(id),
            PaneMenuIntent::StrategyRearm(id) => {
                let series = gesture::PaneSeriesRead {
                    history_prefix: &self.history_prefix,
                    state: &self.state,
                    spec: &self.spec,
                };
                self.strategies.rearm(id, series);
            }
            PaneMenuIntent::StrategyRemove(id) => self.strategies.remove_for_drawing(id),
            PaneMenuIntent::ObjectsAsk(action) => {
                use quantick_chart_interaction::pane::ObjectAction;
                let mut ask = crate::surfaces::drawing_chrome::DrawingChromeAsk::default();
                match action {
                    ObjectAction::Select(id) => ask.manager_select = self.drawings.index_of(id),
                    ObjectAction::ToggleHidden(id) => {
                        ask.manager_toggle_hidden = self.drawings.index_of(id)
                    }
                    ObjectAction::ToggleLocked(id) => {
                        ask.manager_toggle_locked = self.drawings.index_of(id)
                    }
                    ObjectAction::BringToFront(id) => {
                        ask.manager_bring_to_front = self.drawings.index_of(id)
                    }
                    ObjectAction::Delete(id) => ask.manager_delete = self.drawings.index_of(id),
                    ObjectAction::DeleteAll => ask.delete_all = true,
                }
                chrome.drawing_chrome.ask_from_menu(self.id, ask);
            }
        }
    }
}
