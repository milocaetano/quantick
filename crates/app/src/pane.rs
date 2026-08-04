//! One chart pane: everything that answers "what is on this canvas".
//!
//! A pane owns the bar series it aggregates, the viewport and price scale it is
//! read through, the drawings anchored to its bar indices and the indicator
//! slots computed over it. What it deliberately does *not* own is the market
//! feeding it — feed channels, connection state and notices belong to the tab
//! around it — or the window chrome (menus, toolbar, dock, status bar), which
//! belongs to the application around that.
//!
//! That split is what lets one tab hold two panes over the same trades: a flow
//! pane and a time-frame pane, the split view of `docs/ux/ui-design-model.md`
//! §11. Every egui interaction id a pane registers is derived from
//! [`ChartPane::id`] for the same reason — two panes registering one id would
//! share a drag.

use std::collections::BTreeSet;

use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use smallvec::SmallVec;

use crate::app::{
    PlotAreas, fmt_time, fmt_window, gesture_hits_lane, plot_split, split_time_strip,
};
use crate::candle_view::draw_candle;
use crate::chart::{self, PriceScale};
use crate::chart_layers::{ChartLayer, LayerActions};
use crate::config::FeedCapabilities;
use crate::drawings::{self, ChartPoint, DrawContext, Drawings, PresetHost};
use crate::indicator_render::{self, PlotX};
use crate::indicator_worker::{IndicatorCommand, IndicatorSource, IndicatorWorker, SlotId};
use crate::indicators::IndicatorViews;
use crate::orderflow_view::{OrderflowView, VisibleBarTimeline};
use crate::paper_trading::{ChartInput, PaperTrading};
use crate::price_view::PriceView;
use crate::state::{BarKind, BarSpec, ChartState};
use crate::style::ChartStyle;
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::{Tool, ToolRail};
use crate::viewport::Viewport;

/// Hit radius for selecting a drawing anchor, in logical pixels.
const DRAWING_SELECT_RADIUS_PX: f32 = 10.0;
/// Hit radius for a selected drawing's editable anchor.
pub const DRAWING_ANCHOR_RADIUS_PX: f32 = 12.0;
/// Minimum pointer travel that turns one press/release into drag placement.
const DRAWING_DRAG_THRESHOLD_PX: f32 = 4.0;

/// Alpha of the last-price line: legible at a glance without competing with a
/// candle or a bubble for attention.
const LAST_PRICE_LINE_ALPHA: f32 = 0.55;
/// Dash length, in pixels, of the last-price line. Dashed so it never reads as
/// a level someone drew.
const LAST_PRICE_DASH_PX: f32 = 4.0;
/// See [`LAST_PRICE_DASH_PX`].
const LAST_PRICE_GAP_PX: f32 = 4.0;
/// Ink on the last-price chip. The chip is filled with a saturated candle
/// colour, so its text is the one place on the chrome that goes dark.
const LAST_PRICE_CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(0x0E, 0x12, 0x1A);

/// Font size, in points, of the "nothing in view" line drawn where the candles
/// would be. Matches the "connecting…" line: same voice, same weight.
const EMPTY_VIEW_FONT_SIZE: f32 = 16.0;

/// Dash length, in pixels, of the venue↔prints seam marker. Long enough to
/// read as deliberate beside the solid backfill divider, short enough not to
/// be mistaken for one.
const SEAM_DASH_PX: f32 = 5.0;
/// See [`SEAM_DASH_PX`].
const SEAM_GAP_PX: f32 = 4.0;

/// A dashed vertical rule down `rect` at `x`.
///
/// The same construction the heatmap's own boundary marks use: egui has no
/// dashed line primitive for a single segment, so the dashes are drawn.
fn draw_dashed_vertical(
    painter: &egui::Painter,
    x: f32,
    rect: egui::Rect,
    dash: f32,
    gap: f32,
    color: egui::Color32,
) {
    let dash = dash.max(0.5);
    let gap = gap.max(0.0);
    let stroke = egui::Stroke::new(1.0_f32, color);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(x, y),
                egui::pos2(x, (y + dash).min(rect.bottom())),
            ],
            stroke,
        );
        y += dash + gap;
    }
}

/// Which of the canvas's panes something belongs to.
///
/// Named for where they sit in the split, because that is how the user picks
/// one: the time pane is on the left, the flow pane on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneSide {
    #[default]
    Flow,
    Time,
}

/// Width of the draggable divider between the two panes, in pixels.
pub const CANVAS_DIVIDER_PX: f32 = 4.0;
/// Half-width of the divider's grab area, which reaches a little into both
/// panes so the handle is catchable without widening the rule itself.
pub const CANVAS_DIVIDER_HANDLE_PX: f32 = 5.0;
/// Neither pane may be squeezed below this share of the canvas (§11).
pub const MIN_PANE_FRACTION: f32 = 0.25;
/// Where the divider sits when the split is first shown (§11).
pub const DEFAULT_PANE_FRACTION: f32 = 0.5;

/// The canvas carved for the Time + Flow layout.
///
/// A named shape rather than three rects, because a caller should not have to
/// remember that the middle one is the divider.
pub struct CanvasAreas {
    /// The time pane, header included.
    pub time: egui::Rect,
    /// The draggable rule between them; belongs to neither pane.
    pub divider: egui::Rect,
    /// The flow pane.
    pub flow: egui::Rect,
}

/// A time pane's area, split into the strip its selector sits in and the
/// chart below it.
pub struct TimePaneAreas {
    pub header: egui::Rect,
    pub chart: egui::Rect,
}

/// Split the canvas for the Time + Flow layout: **time pane left, flow pane
/// right**, with the divider's own strip between them (§11).
///
/// `time_fraction` is the time pane's share of the width, clamped so neither
/// pane can be squeezed below [`MIN_PANE_FRACTION`] — a pane too narrow to
/// read is not a layout, it is a lost pane.
#[must_use]
pub fn split_canvas(area: egui::Rect, time_fraction: f32) -> CanvasAreas {
    let fraction = clamp_pane_fraction(time_fraction);
    let divider_x = area.left() + area.width() * fraction;
    let half = CANVAS_DIVIDER_PX / 2.0;
    CanvasAreas {
        time: egui::Rect::from_min_max(area.min, egui::pos2(divider_x - half, area.bottom())),
        divider: egui::Rect::from_min_max(
            egui::pos2(divider_x - half, area.top()),
            egui::pos2(divider_x + half, area.bottom()),
        ),
        flow: egui::Rect::from_min_max(egui::pos2(divider_x + half, area.top()), area.max),
    }
}

/// Hold a canvas split inside the 25% minimum each pane is promised (§11).
#[must_use]
pub fn clamp_pane_fraction(fraction: f32) -> f32 {
    fraction.clamp(MIN_PANE_FRACTION, 1.0 - MIN_PANE_FRACTION)
}

/// Carve the time pane's header strip off the top of its area (§11); the rest
/// is the chart. The header is a strip rather than an overlay so the selector
/// is never painted across market data.
#[must_use]
pub fn split_time_pane(area: egui::Rect) -> TimePaneAreas {
    let split_y = (area.top() + crate::time_header::HEIGHT_PX).min(area.bottom());
    TimePaneAreas {
        header: egui::Rect::from_min_max(area.min, egui::pos2(area.right(), split_y)),
        chart: egui::Rect::from_min_max(egui::pos2(area.left(), split_y), area.max),
    }
}

/// Half-width, in pixels, of the grab area over the live lane's divider.
///
/// The line itself stays a hairline — it marks where the present begins and a
/// thick rule there would read as a wall in the data. The handle around it is
/// what makes it draggable, and the resize cursor is the only thing that says
/// so.
const LANE_HANDLE_HALF_WIDTH_PX: f32 = 5.0;

/// Pixels of drag on a vertical axis that change its span by a factor of `e`.
///
/// One number for the price gutter and for every indicator pane's gutter: the
/// axes stretch at the same rate, so the gesture feels the same wherever the
/// numbers being dragged happen to live.
const AXIS_ZOOM_DRAG_PX: f32 = 150.0;
/// The same, for a scroll over an axis rather than a drag. One wheel notch
/// reports far more units than a pointer travels in a frame, so each unit has
/// to count for less — a larger divisor, not a smaller one.
const AXIS_ZOOM_SCROLL_PX: f32 = 200.0;

/// The gesture that scales a vertical axis, wherever its numbers live: drag up
/// to compress the span, down to expand, scroll to zoom, double-click to hand
/// the axis back to auto-fit.
///
/// One implementation for the price gutter and for every indicator pane's, so
/// a third band that wants an axis — a volume profile, the tape — registers it
/// rather than copying it, and no two axes can drift apart in feel. Lives here
/// with the panes that use it: every axis in the window belongs to one.
///
/// `auto` is the range the last frame fitted; `None` means nothing has been
/// computed to scale yet, and only the reset stays available.
fn axis_zoom_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    band: egui::Rect,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
) {
    let response = ui.interact(band, id, egui::Sense::click_and_drag());
    if response.double_clicked() {
        view.reset();
    }
    let Some(auto) = auto else {
        return;
    };
    if response.dragged() {
        // Drag up → compress the span (a taller trace); down → expand it.
        view.zoom(
            f64::from(response.drag_delta().y / AXIS_ZOOM_DRAG_PX).exp(),
            auto,
        );
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll.abs() > 0.0 {
            view.zoom(f64::from(-scroll / AXIS_ZOOM_SCROLL_PX).exp(), auto);
        }
    }
}

/// Pixels of drag on the lane's own time strip that double or halve its window.
///
/// Matches the candles' own feel: dragging the time axis zooms it by
/// `exp(dx / 120)`, so the two panes answer a drag at the same rate even
/// though they are zooming different things.
const LANE_ZOOM_DRAG_PX: f32 = 120.0;

/// Whether a freshly folded prefix differs from the one already installed.
///
/// Length and the two end open-times, not a full comparison: the fold is
/// deterministic over the same base, so two runs agreeing on how many bars
/// they produced and which windows the first and last cover agree on
/// everything between. The full compare was ~129k `Decimal`s on every frame
/// of a settled interval drag.
fn prefix_differs(current: &[quantick_engine::Bar], next: &[quantick_engine::Bar]) -> bool {
    if current.len() != next.len() {
        return true;
    }
    let ends = |bars: &[quantick_engine::Bar]| {
        (
            bars.first().map(|bar| bar.open_time),
            bars.last().map(|bar| bar.open_time),
        )
    };
    ends(current) != ends(next)
}

/// Convert an explicit unmultiplied RGBA style colour to egui.
fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The canvas background colour `style` asks for.
pub fn background_color(style: &ChartStyle) -> egui::Color32 {
    color32(style.canvas.background_rgba())
}

/// The chart-grid colour `style` asks for. `TRANSPARENT` disables grid painting
/// without branching throughout the axis code.
pub fn grid_color(style: &ChartStyle) -> egui::Color32 {
    style
        .canvas
        .grid_rgba()
        .map_or(egui::Color32::TRANSPARENT, color32)
}

/// Convert a UI `f64` parameter to a positive `Decimal` for a builder threshold.
fn dec_from_f64(x: f64) -> Decimal {
    Decimal::from_f64(x.max(1e-8)).unwrap_or(Decimal::ONE)
}

/// What the pointer is currently doing to a drawing, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingDrag {
    #[default]
    None,
    Translate,
    Anchor {
        drawing_index: usize,
        point_index: usize,
    },
    /// The press landed on a locked drawing: the gesture belongs to the
    /// object (the chart must not pan) but the geometry stays put.
    Blocked,
}

impl DrawingDrag {
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// What a pane borrows from the window around it for one frame.
///
/// All of it is single-instance chrome: there is one toolbox, one preset
/// store, one appearance and one timezone however many panes are on screen —
/// and, per tab, one simulator, because one market holds one position.
/// The input pass takes this by `&mut` because placing a drawing re-arms the
/// tool; the draw pass takes it by `&`, which is what stops a paint from
/// arming anything.
pub struct PaneChrome<'a> {
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a drawings::presets::PresetStore,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// The symbol to name while the series is still empty.
    pub symbol: &'a str,
    /// The tab's paper-trading simulator. Both panes *draw* its lines — the
    /// same instrument at the same prices — while only one *handles* them.
    pub paper: &'a mut PaperTrading,
    /// Whether this pane is the one paper trading takes its pointer from.
    ///
    /// Set for the flow pane, and only for it: order entry belongs to the
    /// chart trading happens on. The time pane is the *context* view (§11) —
    /// a 90-day 1-minute chart is not a surface to place a stop on, and its
    /// price span is nothing like the one an order is sized against.
    ///
    /// It is also what keeps the gesture coherent: the simulator holds one
    /// grabbed line and one armed placement for the whole tab, so exactly one
    /// pane may drive them. Running both would let the second pane inherit a
    /// drag the first started and re-clamp it into the wrong rectangle.
    pub paper_owns_input: bool,
    /// What the running source can actually produce. The layer menu offers a
    /// layer this feed has no data for as disabled-with-a-reason rather than as
    /// a switch that would do nothing — the wording the toolbar already uses.
    pub capabilities: FeedCapabilities,
    /// Where the layer menu leaves the two switches the pane does not own.
    /// Drained by the app once the canvas is done (see [`LayerActions`]).
    pub layers: &'a mut LayerActions,
}

/// One chart pane. See the module docs for what does and does not live here.
pub struct ChartPane {
    /// Namespaces this pane's egui interaction ids. Ids are the one piece of
    /// gesture state egui keeps on our behalf, so two panes sharing an id
    /// would share a drag.
    pub id: u64,
    pub state: ChartState,
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
    /// Whether the user wants the live strip shown. The pixels it actually
    /// gets are still capability-gated — see [`Self::live_strip_width`].
    pub live_strip_visible: bool,

    /// Layers switched off that nothing else on this pane owns.
    ///
    /// The rest of the right-click menu resolves to the field that already owns
    /// its layer (see [`Self::layer_visible`]); only the chart's own marks —
    /// which had no switch before the menu existed — are held here, so the menu
    /// can never hold a second opinion about a pixel.
    pub hidden_layers: BTreeSet<ChartLayer>,
    /// Where each layer's switch landed in the last menu frame, so a test can
    /// click the real widget instead of calling the setter behind it.
    #[cfg(test)]
    pub layer_menu_rects: Vec<(ChartLayer, egui::Rect)>,

    // Bar-type selector state (one parameter retained per kind).
    pub kind: BarKind,
    // The spec the selectors ask for, applied one frame after they settle so
    // the frame carrying the change paints the loading overlay before the
    // synchronous rebuild holds this thread. See QuantickApp::apply_spec_change.
    pub pending_spec: Option<BarSpec>,
    pub tick_n: u64,
    pub volume_units: f64,
    pub dollar_notional: f64,
    pub time_interval_ms: i64,
    pub imbalance_target: u64,

    // Pan/zoom navigation over the bar series. It owns the history pane only:
    // the live lane is a band of screen to its right that answers to nothing
    // it does.
    pub viewport: Viewport,
    // Where the history pane ended last frame — the lane's divider, and the
    // handle that resizes it. The input pass runs before the draw computes it.
    pub last_lane_divider_x: Option<f32>,
    // Manual price-axis pan/zoom (auto-fit until the user drags vertically).
    pub price_view: PriceView,
    // Last frame's auto-fit price range and chart height, for pixel↔price maths
    // in the input handler (which runs before the draw computes them).
    pub last_auto_range: Option<(f64, f64)>,
    pub last_chart_height: f32,
    pub last_chart_top: f32,
    // The chart pane from the last frame (excludes axes and the live lane),
    // for inspector placement and manager centring.
    pub last_chart_area: Option<egui::Rect>,
    // The raw canvas area the last frame split into chart, panes and gutters.
    // Kept so a caller that needs a band it does not otherwise see — the pane
    // axis tests aiming a drag at a pane's own gutter — asks `plot_split` for
    // it rather than re-deriving the layout and drifting from it.
    pub last_plot_area: Option<egui::Rect>,
    // Pointer position over the plot this frame, for the crosshair.
    pub hover_pos: Option<egui::Pos2>,

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
    /// Only ever non-empty on a time pane. The flow pane is the tape's, and a
    /// venue candle has no tape in it.
    pub history_prefix: Vec<quantick_engine::Bar>,

    /// User drawings live entirely in the app overlay layer, never in market
    /// state, so chart/backtest/bot determinism stays untouched.
    pub drawings: Drawings,
    // Drawing placement/movement state. Anchors are chart coordinates; only
    // the current hover and press position are transient pixels.
    pub drawing_hover: Option<ChartPoint>,
    pub drawing_press_position: Option<egui::Pos2>,
    pub drawing_press_started_empty: bool,
    pub drawing_drag: DrawingDrag,
}

impl ChartPane {
    /// The flow pane: quantick's own view of `symbol`, opening on bar `spec`,
    /// with the tape and every layer read off it.
    #[must_use]
    pub fn flow(id: u64, spec: BarSpec, symbol: String) -> Self {
        Self::new(id, spec, Some(OrderflowView::new(symbol)))
    }

    /// The time pane: the context view beside the flow pane (§11). Time bars
    /// of `interval_ms`, no tape and no flow layers.
    #[must_use]
    pub fn time(id: u64, interval_ms: i64) -> Self {
        Self::new(id, BarSpec::Time(interval_ms.max(1)), None)
    }

    /// `id` namespaces the pane's egui interaction ids and must be unique
    /// among the panes on screen.
    fn new(id: u64, spec: BarSpec, orderflow: Option<OrderflowView>) -> Self {
        // Defaults for every kind, with the initial spec's parameter applied.
        let mut tick_n = 50;
        let mut volume_units = 5.0;
        let mut dollar_notional = 500_000.0;
        let mut time_interval_ms = 1_000;
        let mut imbalance_target = 100;
        match &spec {
            BarSpec::Tick(n) => tick_n = *n,
            BarSpec::Volume(u) => volume_units = u.to_f64().unwrap_or(volume_units),
            BarSpec::Dollar(d) => dollar_notional = d.to_f64().unwrap_or(dollar_notional),
            BarSpec::Time(ms) => time_interval_ms = *ms,
            BarSpec::Imbalance(target) => imbalance_target = *target,
        }

        Self {
            id,
            kind: spec.kind(),
            state: ChartState::new(spec),
            orderflow,
            indicator_worker: IndicatorWorker::spawn(),
            indicators: IndicatorViews::new(),
            live_strip_visible: false,
            hidden_layers: BTreeSet::new(),
            #[cfg(test)]
            layer_menu_rects: Vec::new(),
            pending_spec: None,
            tick_n,
            volume_units,
            dollar_notional,
            time_interval_ms,
            imbalance_target,
            viewport: Viewport::new(),
            last_lane_divider_x: None,
            price_view: PriceView::new(),
            last_auto_range: None,
            last_chart_height: 1.0,
            last_chart_top: 0.0,
            last_chart_area: None,
            last_plot_area: None,
            hover_pos: None,
            history_prefix: Vec::new(),
            drawings: Drawings::default(),
            drawing_hover: None,
            drawing_press_position: None,
            drawing_press_started_empty: false,
            drawing_drag: DrawingDrag::None,
        }
    }

    /// An egui interaction id scoped to this pane.
    fn interaction_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.id))
    }

    /// Whether `layer` is painted on this pane right now.
    ///
    /// Every arm reads the one field that already owns that layer, so the menu
    /// and the toolbar/dock can never disagree about a pixel. `grid` is the
    /// window's shared style flag, passed in because the window owns it and a
    /// pane holding a copy is exactly the disagreement this avoids.
    ///
    /// A layer this pane has no machinery for reports hidden: a time pane runs
    /// no tape (§11), so it has no heatmap to show.
    pub fn layer_visible(&self, layer: ChartLayer, grid: bool) -> bool {
        let tape = self.orderflow.as_ref();
        match layer {
            ChartLayer::Heatmap => tape.is_some_and(OrderflowView::depth_visible),
            ChartLayer::Bubbles => tape.is_some_and(OrderflowView::bubbles_enabled),
            ChartLayer::LiveStrip => self.orderflow.is_some() && self.live_strip_visible,
            ChartLayer::LaneMarks => tape.is_some_and(OrderflowView::lane_marks_visible),
            ChartLayer::DepthGaps => tape.is_some_and(OrderflowView::gaps_visible),
            ChartLayer::Grid => grid,
            // The toolbox's global eye already owns this one, undo history and
            // all; the menu is a second door to the same switch.
            ChartLayer::Drawings => !self.drawings.all_hidden(),
            ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PaperTrading => !self.hidden_layers.contains(&layer),
        }
    }

    /// Show or hide `layer`, writing through to whoever owns it.
    ///
    /// Display only: nothing here stops depth capture, bar building, indicator
    /// computation or a working order, so unhiding repaints the retained past
    /// instead of opening a hole in it. The grid is the window's, so that one
    /// is left in `actions` for the app to apply.
    pub fn set_layer_visible(
        &mut self,
        layer: ChartLayer,
        visible: bool,
        actions: &mut LayerActions,
    ) {
        match layer {
            ChartLayer::Heatmap => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_depth_visible(visible);
                }
            }
            ChartLayer::Bubbles => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_bubbles_enabled(visible);
                }
            }
            ChartLayer::LiveStrip => self.live_strip_visible = visible,
            ChartLayer::LaneMarks => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_marks_visible(visible);
                }
            }
            ChartLayer::DepthGaps => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_gaps_visible(visible);
                }
            }
            ChartLayer::Grid => actions.grid = Some(visible),
            ChartLayer::Drawings => self.drawings.set_all_hidden(!visible),
            ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PaperTrading => {
                if visible {
                    self.hidden_layers.remove(&layer);
                } else {
                    self.hidden_layers.insert(layer);
                }
            }
        }
    }

    /// Whether this pane draws `layer` at all, whatever the source can produce.
    ///
    /// §11 keeps the tape and everything read off it on the flow pane, so a
    /// time pane has no machinery for those five and never will.
    fn draws_layer(&self, layer: ChartLayer) -> bool {
        self.orderflow.is_some()
            || !matches!(
                layer,
                ChartLayer::Heatmap
                    | ChartLayer::Bubbles
                    | ChartLayer::LiveStrip
                    | ChartLayer::LaneMarks
                    | ChartLayer::DepthGaps
            )
    }

    /// Why `layer` cannot be shown here, if it cannot.
    ///
    /// A layer the source cannot produce — or that this pane does not draw at
    /// all — is *unavailable*, not hidden: the menu shows the entry disabled
    /// with the reason, the same wording the toolbar uses, rather than offering
    /// a switch that would do nothing.
    ///
    /// `capabilities` is passed in rather than read here so one menu frame
    /// resolves the running feed once instead of once per entry.
    pub fn layer_blocked(
        &self,
        layer: ChartLayer,
        capabilities: FeedCapabilities,
    ) -> Option<&'static str> {
        if !self.draws_layer(layer) {
            return Some("the order-flow layers are drawn on the flow pane");
        }
        match layer {
            ChartLayer::Heatmap | ChartLayer::DepthGaps => (!capabilities.book_capture)
                .then_some("order-book capture is not available for this source"),
            ChartLayer::Bubbles => (!capabilities.traded_volume)
                .then_some("this source quotes prices but prints no traded volume"),
            _ => None,
        }
    }

    /// Every layer this pane persists, and whether it is on.
    ///
    /// `grid` comes from the shared style for the same reason it does in
    /// [`Self::layer_visible`].
    pub fn layer_states(&self, grid: bool) -> std::collections::BTreeMap<ChartLayer, bool> {
        ChartLayer::ALL
            .into_iter()
            .filter(|layer| layer.persisted())
            .map(|layer| (layer, self.layer_visible(layer, grid)))
            .collect()
    }

    /// The same visibility as one bit per persisted layer, for change
    /// detection. `ALL` is a dozen entries, so the mask cannot outgrow `u16`.
    pub fn layer_mask(&self, grid: bool) -> u16 {
        ChartLayer::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, layer)| layer.persisted() && self.layer_visible(*layer, grid))
            .fold(0_u16, |mask, (bit, _)| mask | (1 << bit))
    }

    /// Apply saved visibility to this pane, ignoring layers it cannot draw.
    ///
    /// The grid is not applied here — one window, one grid, and the app sets
    /// that once rather than once per pane.
    pub fn apply_layer_states(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        let mut discarded = LayerActions::default();
        for (layer, visible) in states {
            if *layer == ChartLayer::Grid || !self.draws_layer(*layer) {
                continue;
            }
            self.set_layer_visible(*layer, *visible, &mut discarded);
        }
    }

    /// Arming a tool brings back the layer it draws on.
    ///
    /// A crosshair that draws no cross, or a line tool that places invisible
    /// objects, reads as a broken tool rather than as a hidden layer — and
    /// reaching for the tool is the user saying they want to see it.
    fn unhide_layer_for_armed_tool(&mut self, chrome: &mut PaneChrome<'_>) {
        let layer = match chrome.toolrail.tool() {
            Tool::Crosshair => ChartLayer::Crosshair,
            Tool::Drawing(_) => ChartLayer::Drawings,
            Tool::Pointer => return,
        };
        if !self.layer_visible(layer, chrome.style.canvas.grid_enabled) {
            self.set_layer_visible(layer, true, chrome.layers);
        }
    }

    /// The canvas right-click menu: one entry per chart layer, then one per
    /// indicator on this pane.
    ///
    /// The indicator entries drive `IndicatorViews::toggle_hidden` — the same
    /// state the toolbar's eye writes — so an indicator hidden here shows as
    /// hidden there, and the indicator state file remains its single home.
    pub fn draw_layer_menu(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        ui.label(
            egui::RichText::new("chart layers")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        #[cfg(test)]
        self.layer_menu_rects.clear();
        let grid = chrome.style.canvas.grid_enabled;
        for layer in ChartLayer::ALL {
            let blocked = self.layer_blocked(layer, chrome.capabilities);
            let mut visible = self.layer_visible(layer, grid);
            let response = ui
                .add_enabled(
                    blocked.is_none(),
                    egui::Checkbox::new(&mut visible, layer.label()),
                )
                .on_hover_text(layer.hint());
            #[cfg(test)]
            self.layer_menu_rects.push((layer, response.rect));
            if let Some(reason) = blocked {
                response.on_disabled_hover_text(reason);
            } else if response.changed() {
                self.set_layer_visible(layer, visible, chrome.layers);
            }
        }

        // Borrowed straight from the view list — no per-frame copy of the
        // labels — and the one mutation waits until the loop lets go.
        let mut toggled = None;
        if !self.indicators.all().is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new("indicators")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            for view in self.indicators.all() {
                let mut visible = !view.hidden;
                if ui
                    .checkbox(&mut visible, view.label())
                    .on_hover_text("hide/show without removing (no recompute)")
                    .changed()
                {
                    toggled = Some(view.slot);
                }
            }
        }
        if let Some(slot) = toggled {
            self.indicators.toggle_hidden(slot);
            chrome.layers.indicators_changed = true;
        }
    }

    /// The bar spec implied by the current selector state.
    pub fn current_spec(&self) -> BarSpec {
        match self.kind {
            BarKind::Tick => BarSpec::Tick(self.tick_n.max(1)),
            BarKind::Volume => BarSpec::Volume(dec_from_f64(self.volume_units)),
            BarKind::Dollar => BarSpec::Dollar(dec_from_f64(self.dollar_notional)),
            BarKind::Time => BarSpec::Time(self.time_interval_ms.max(1)),
            BarKind::Imbalance => BarSpec::Imbalance(self.imbalance_target.max(1)),
        }
    }

    /// How many bar slots the chart draws: the venue prefix, the closed bars
    /// the engine cut from trades, and the forming one after them.
    pub fn slots(&self) -> usize {
        self.closed_slots() + usize::from(self.state.partial().is_some())
    }

    /// Slots holding a *closed* bar — everything before the forming one.
    pub fn closed_slots(&self) -> usize {
        self.history_prefix.len() + self.state.bars().len()
    }

    /// The slot the trade-derived series starts at: the seam between venue
    /// candles and bars this app built from prints.
    pub fn seam_slot(&self) -> usize {
        self.history_prefix.len()
    }

    /// The closed bar in `slot`, from whichever series owns it.
    pub fn closed_bar(&self, slot: usize) -> Option<&quantick_engine::Bar> {
        self.history_prefix
            .get(slot)
            .or_else(|| self.state.bars().get(slot - self.history_prefix.len()))
    }

    /// When the bar in `slot` opened, across both series and the forming bar.
    ///
    /// Past the prefix this is the engine's own answer, shifted into the
    /// composed slot space — there is one rule for what a slot means and the
    /// prefix only moves where it starts.
    pub fn slot_open_time(&self, slot: usize) -> Option<i64> {
        match self.history_prefix.get(slot) {
            Some(bar) => Some(bar.open_time),
            None => self.state.slot_open_time(slot - self.seam_slot()),
        }
    }

    /// The slot showing market time `ms`, across both series.
    ///
    /// The seam rule keeps `open_time` non-decreasing across the join, so the
    /// question splits cleanly: anything from the first engine bar onward is
    /// the engine's own answer shifted by the prefix, anything before it is a
    /// search of the prefix.
    pub fn slot_at_time(&self, ms: i64) -> Option<usize> {
        let seam = self.seam_slot();
        if seam == 0 {
            return self.state.slot_at_time(ms);
        }
        if self
            .state
            .bars()
            .first()
            .or_else(|| self.state.partial())
            .is_some_and(|bar| bar.open_time <= ms)
        {
            return self.state.slot_at_time(ms).map(|slot| slot + seam);
        }
        let after = self
            .history_prefix
            .partition_point(|bar| bar.open_time <= ms);
        Some(after.saturating_sub(1))
    }

    /// The market time under the right edge of the candles' pane, or `None`
    /// while the view follows live (the right edge is the newest bar by
    /// definition, so there is nothing to remember) or when there are no bars.
    pub fn right_edge_time(&self) -> Option<i64> {
        if self.viewport.follows_live() {
            return None;
        }
        let slots = self.slots();
        let edge = self.viewport.right_edge_bar(slots);
        // Panning into the empty space past the newest bar puts the edge off
        // the series; the newest bar is the market time it is closest to.
        let slot = (edge.floor().max(0.0) as usize).min(slots.saturating_sub(1));
        self.slot_open_time(slot)
    }

    /// Width reserved for the live strip this frame. No capability gate any
    /// more: the aggression histogram runs on the trade stream, which every
    /// source provides (replay included), and without book data the strip
    /// honestly degrades to that histogram alone. A pane with no tape has no
    /// strip at all (§11).
    pub fn live_strip_width(&self) -> f32 {
        if self.live_strip_visible && self.orderflow.is_some() {
            crate::live_strip::LIVE_STRIP_WIDTH_PX
        } else {
            0.0
        }
    }

    /// This pane's regions inside `area`, carved once so the input handler and
    /// the renderer can never disagree about a boundary.
    fn plot_areas(&self, area: egui::Rect) -> PlotAreas {
        plot_split(
            area,
            self.live_strip_width(),
            self.indicators.visible_panes().count(),
        )
    }

    /// Reserve a slot and ask the worker to instantiate `source` behind it.
    pub fn add_indicator(&mut self, source: IndicatorSource) -> SlotId {
        let slot = self.indicators.allocate_slot();
        self.indicator_worker
            .send(IndicatorCommand::Add { slot, source });
        slot
    }

    /// Ask the worker to replay the chart's bars from scratch — the one
    /// command behind spec switches, prepended history and source resets, so
    /// indicators inherit correct behavior for every rebuild path.
    pub fn send_indicator_rebuild(&mut self) {
        self.indicator_worker.send(IndicatorCommand::Rebuild(
            self.closed_bars(),
            self.state.partial().cloned(),
        ));
    }

    /// Every closed bar the pane shows, prefix first — what an indicator is
    /// computed over, so an average spans the venue history rather than
    /// restarting at the first print this session saw.
    fn closed_bars(&self) -> Vec<quantick_engine::Bar> {
        let mut bars = Vec::with_capacity(self.closed_slots());
        bars.extend_from_slice(&self.history_prefix);
        bars.extend_from_slice(self.state.bars());
        bars
    }

    /// Put `bars` in front of the trade-derived series, or take the prefix
    /// away when they are empty.
    ///
    /// Everything anchored to a bar index moves with the change, exactly as a
    /// trade-history prepend moves it: the viewport keeps its right edge, the
    /// drawings keep their bars, the indicator columns keep their candles
    /// until the rebuild lands. Returns whether anything changed.
    pub fn install_history_prefix(&mut self, bars: Vec<quantick_engine::Bar>) -> bool {
        // Only a time pane ever carries one, and a time pane has no tape. Held
        // as an assertion rather than a comment because the draw path reads
        // the two as mutually exclusive.
        debug_assert!(
            bars.is_empty() || self.orderflow.is_none(),
            "a venue prefix belongs to a pane with no tape"
        );
        if !prefix_differs(&self.history_prefix, &bars) {
            return false;
        }
        let before = self.history_prefix.len();
        self.history_prefix = bars;
        // The prefix moves under a chart the user is already reading, so
        // everything anchored to a bar index moves with it — in either
        // direction. It grows when history lands; it shrinks when a coarser
        // fold makes fewer bars of the same span, or when older trades push
        // the seam back and the overlapping buckets leave.
        let delta = self.history_prefix.len() as isize - before as isize;
        self.viewport.shift_right_edge(delta);
        self.drawings.shift_bars(delta);
        // Indicator columns have no signed shift: on growth they are nudged so
        // the frames before the rebuild lands draw each value against its own
        // candle, and on a shrink the rebuild below re-cuts them wholesale a
        // round trip later. Drawings get no such second chance, which is why
        // they take the signed delta above.
        if let Ok(added) = usize::try_from(delta) {
            self.indicators.shift_rows(added);
        }
        self.send_indicator_rebuild();
        true
    }

    /// Apply the indicator worker's deltas, before the draw reads columns.
    pub fn apply_indicator_events(&mut self) {
        for event in self.indicator_worker.drain_events() {
            self.indicators.apply(event);
        }
    }

    /// Take a backfill batch into the series and hand the indicators the bars
    /// it produced.
    pub fn ingest_backfill(&mut self, trades: &[quantick_engine::Trade]) {
        self.state.ingest_backfill(trades);
        self.indicator_worker
            .send(IndicatorCommand::Backfilled(self.closed_bars()));
        self.indicator_worker.send(IndicatorCommand::PartialUpdated(
            self.state.partial().cloned(),
        ));
    }

    /// Prepend older trades and shift everything anchored to a bar index by the
    /// number of bars they added, which is what this returns.
    pub fn prepend_history(&mut self, trades: &[quantick_engine::Trade]) -> usize {
        // Older bars shift every index up; keep the view steady.
        let added = self.state.prepend_history(trades);
        self.viewport.shift_right_edge(added as isize);
        self.drawings.shift_bars(added as isize);
        // Indicator columns shift with them: the rebuild below is a round-trip
        // away, and until it lands every value would otherwise be drawn
        // `added` slots off its own candle.
        self.indicators.shift_rows(added);
        // Older trades re-cut every bar; replay from scratch.
        self.send_indicator_rebuild();
        added
    }

    /// Throw away this pane's bars and everything anchored to them, keeping
    /// the spec its own selectors ask for.
    ///
    /// Called when the market underneath changes — a feed switch, a source
    /// reset — because a bar index means nothing across two streams. The
    /// drawings are cleared by the window, which owns the notice saying so.
    pub fn reset_series(&mut self) {
        // The prefix is bar-indexed against a series that no longer exists,
        // and its seam was trimmed against a first bar that is gone. A replay
        // never has one today; the invariant must not depend on that.
        self.history_prefix.clear();
        self.state = ChartState::new(self.current_spec());
        self.viewport = Viewport::new();
        self.price_view = PriceView::new();
        self.last_auto_range = None;
        self.hover_pos = None;
    }

    /// Fill a pane opened mid-session from the trades another pane of the same
    /// market already holds, keeping the backfill/live boundary where it was:
    /// a trade that was streamed live must not become "history" just because
    /// this view was opened late.
    pub fn seed_from(&mut self, trades: &[quantick_engine::Trade], backfill_count: usize) {
        let split = backfill_count.min(trades.len());
        self.state.ingest_backfill(&trades[..split]);
        for trade in &trades[split..] {
            self.state.ingest_live(trade);
        }
        // One rebuild rather than one command per trade: the worker is being
        // handed a whole history, not watching it arrive.
        self.send_indicator_rebuild();
    }

    /// Take one live trade into the series, the tape and the indicators.
    ///
    /// The forming bar is *not* published here: it changes with every print
    /// and only its latest value is ever used, so the caller sends one
    /// [`Self::publish_partial`] at the end of the drain instead. A 500-print
    /// batch was 500 bar clones down the channel for the worker to collapse
    /// back into one. Closed bars stay per trade — each is a distinct event
    /// the indicators have to see.
    pub fn ingest_live_trade(&mut self, trade: &quantick_engine::Trade) {
        if let Some(orderflow) = self.orderflow.as_mut() {
            orderflow.record_trade(trade);
        }
        let bars_before = self.state.bars().len();
        self.state.ingest_live(trade);
        // At most one bar closes per trade (an atomic market event is never
        // split), so "grew" identifies exactly the bar that closed.
        if self.state.bars().len() > bars_before
            && let Some(closed) = self.state.bars().last()
        {
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed.clone()));
        }
    }

    /// Hand the indicators the forming bar as it stands now.
    ///
    /// Sent once per drain that took in live trades — see
    /// [`Self::ingest_live_trade`] for why it is not sent per trade.
    pub fn publish_partial(&mut self) {
        self.indicator_worker.send(IndicatorCommand::PartialUpdated(
            self.state.partial().cloned(),
        ));
    }

    /// Convert a chart pixel into an overlay anchor. The x coordinate is a
    /// fractional bar slot, so drawings follow pan/zoom instead of being stuck
    /// to one screen pixel.
    fn drawing_point_at(
        &self,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
    ) -> Option<ChartPoint> {
        let (auto_lo, auto_hi) = self.last_auto_range?;
        if total == 0 || self.last_chart_height <= 1.0 {
            return None;
        }
        let (lo, hi) = self.price_view.resolve((auto_lo, auto_hi));
        let scale = PriceScale::from_range(
            lo,
            hi,
            self.last_chart_top,
            self.last_chart_top + self.last_chart_height,
        );
        let bar = self.viewport.right_edge_bar(total) + 0.5
            - (history_right - pos.x) / self.viewport.candle_width();
        Some(ChartPoint {
            bar,
            price: scale.price_at(pos.y),
        })
    }

    /// Placement consumes clicks while a drawing tool is armed, preventing a
    /// mark from also panning the chart. A completed object returns to Pointer,
    /// matching the one-shot TradingView interaction.
    fn handle_drawing_placement(
        &mut self,
        ui: &egui::Ui,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) -> bool {
        let Some(tool) = chrome.toolrail.tool().drawing_tool() else {
            self.drawings.cancel_draft();
            self.drawing_hover = None;
            self.drawing_press_position = None;
            self.drawing_press_started_empty = false;
            return false;
        };
        let areas = self.plot_areas(area);
        let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
        let history = egui::Rect::from_min_max(
            areas.chart.min,
            egui::pos2(history_right, areas.chart.bottom()),
        );
        let response = ui.interact(
            history,
            self.interaction_id("drawing_placement"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = response.hover_pos();
        self.drawing_hover = response
            .hover_pos()
            .and_then(|position| self.drawing_point_at(position, history_right, self.slots()));
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        let pressed_position = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if let Some(position) = pressed_position.filter(|position| history.contains(*position))
            && let Some(point) = self.drawing_point_at(position, history_right, self.slots())
        {
            self.drawing_press_started_empty = self.drawings.draft_len() == 0;
            self.drawing_press_position = Some(position);
            self.place_drawing_point(tool, point, chrome);
        }

        let released_position = ui.input(|input| {
            input
                .pointer
                .primary_released()
                .then(|| input.pointer.latest_pos())
                .flatten()
        });
        if tool.required_points() > 1
            && self.drawing_press_started_empty
            && let Some(start) = self.drawing_press_position
            && let Some(position) = released_position
            && history.contains(position)
            && start.distance(position) >= DRAWING_DRAG_THRESHOLD_PX
            && let Some(point) = self.drawing_point_at(position, history_right, self.slots())
        {
            self.place_drawing_point(tool, point, chrome);
        }
        if released_position.is_some() {
            self.drawing_press_position = None;
            self.drawing_press_started_empty = false;
        }
        true
    }

    fn place_drawing_point(
        &mut self,
        tool: drawings::DrawingTool,
        point: ChartPoint,
        chrome: &mut PaneChrome<'_>,
    ) {
        // A new object starts from the user's explicit default preset when
        // one is set; existing objects are never touched by that choice.
        let presets = chrome.presets;
        let completed = self.drawings.place_with(tool, point, |tool| {
            let mut payload = tool.default_payload();
            if let Some(name) = presets.default_preset(tool.id())
                && let Some(value) = presets.load_custom_preset(tool.id(), &name)
            {
                payload.import_preset(&value);
            }
            payload
        });
        if completed {
            // One-shot by default; the toolbox repeat pin keeps the tool
            // armed for the next object.
            if !chrome.toolrail.repeat() {
                chrome.toolrail.arm(Tool::Pointer);
            }
            self.drawing_hover = None;
        }
    }

    pub fn projected_drawing_points(
        &self,
        drawing: &drawings::Drawing,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> SmallVec<[egui::Pos2; 4]> {
        drawing
            .points
            .iter()
            .map(|point| self.drawing_screen_point(*point, history_right, total, scale))
            .collect()
    }

    fn drawing_at(
        &self,
        pos: egui::Pos2,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<usize> {
        self.drawings
            .items()
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, _)| self.drawings.is_visible(*index))
            .find_map(|(index, drawing)| {
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                };
                drawing
                    .tool
                    .hit_test(chart_rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
                    .then_some(index)
            })
    }

    /// Alt+click: deterministic z-order cycling through every visible object
    /// under the pointer. From the current selection, the next hit beneath
    /// it wins; past the bottom it wraps back to the top.
    fn drawing_below_selection(
        &self,
        pos: egui::Pos2,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<usize> {
        let hits: Vec<usize> = (0..self.drawings.items().len())
            .rev()
            .filter(|&index| self.drawings.is_visible(index))
            .filter(|&index| {
                let drawing = &self.drawings.items()[index];
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                };
                drawing
                    .tool
                    .hit_test(chart_rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
            })
            .collect();
        match self
            .drawings
            .selected()
            .and_then(|current| hits.iter().position(|&index| index == current))
        {
            Some(at) => Some(hits[(at + 1) % hits.len()]),
            None => hits.first().copied(),
        }
    }

    fn drawing_anchor_in(
        &self,
        drawing_index: usize,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<usize> {
        if !self.drawings.is_visible(drawing_index) {
            return None;
        }
        let drawing = self.drawings.items().get(drawing_index)?;
        self.projected_drawing_points(drawing, history_right, total, scale)
            .iter()
            .enumerate()
            .map(|(point_index, point)| (point_index, point.distance_sq(pos)))
            .filter(|(_, distance_sq)| {
                *distance_sq <= DRAWING_ANCHOR_RADIUS_PX * DRAWING_ANCHOR_RADIUS_PX
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(point_index, _)| point_index)
    }

    fn drawing_anchor_at(
        &self,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<(usize, usize)> {
        let selected = self.drawings.selected();
        if let Some(drawing_index) = selected
            && let Some(point_index) =
                self.drawing_anchor_in(drawing_index, pos, history_right, total, scale)
        {
            return Some((drawing_index, point_index));
        }
        (0..self.drawings.items().len())
            .rev()
            .filter(|drawing_index| Some(*drawing_index) != selected)
            .find_map(|drawing_index| {
                self.drawing_anchor_in(drawing_index, pos, history_right, total, scale)
                    .map(|point_index| (drawing_index, point_index))
            })
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
        // Remembered for inspector placement and manager centring: the pane
        // where drawings live, already free of both axes and the live lane.
        self.last_plot_area = Some(area);
        self.last_chart_area = Some(self.plot_areas(area).chart);
        if self.handle_drawing_placement(ui, area, chrome) {
            return;
        }
        let areas = self.plot_areas(area);
        let auto = self.last_auto_range;
        let height = self.last_chart_height;
        let total = self.slots();
        let divider = self.last_lane_divider_x;
        let in_lane = |position: egui::Pos2| gesture_hits_lane(divider, position.x);

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            self.interaction_id("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = chart.hover_pos();
        // Right-click: what is on this canvas, and what is not. Secondary
        // button only, so it shares no gesture with the pan, the zoom or the
        // drawing tools — a pan that ends anywhere never opens it.
        chart.context_menu(|ui| self.draw_layer_menu(ui, chrome));
        // While the menu is open the pointer is reading it, not the chart, so
        // no crosshair chases it across the candles behind it.
        if chart.context_menu_opened() {
            self.hover_pos = None;
        }
        let drawing_scale = auto.map(|(auto_lo, auto_hi)| {
            let (lo, hi) = self.price_view.resolve((auto_lo, auto_hi));
            PriceScale::from_range(lo, hi, areas.chart.top(), areas.chart.bottom())
        });
        let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
        let drawing_area = egui::Rect::from_min_max(
            areas.chart.min,
            egui::pos2(history_right, areas.chart.bottom()),
        );
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
        // Simulated order lines take the pointer before the drawings: they
        // sit higher in the draw stack, and a grabbed stop is operational,
        // not annotational. The flag mirrors the drawings' gesture
        // consumption so the chart never pans under a held line; the chrome
        // gate applies at press time only, like everywhere else. Escape is
        // deliberately absent here — cancels live in the app's single escape
        // stack (`handle_drawing_keys`). Only the focused pane offers the
        // gesture, because the whole tab shares one simulator.
        let paper_gesture = chrome.paper_owns_input
            && chrome.paper.handle_chart_input(&ChartInput {
                chart: drawing_area,
                scale: drawing_scale.as_ref(),
                pointer: pointer_position,
                primary_pressed: primary_pressed && !over_chrome,
                primary_down,
                primary_released,
            });
        let mut drawing_drag_consumes_gesture = false;
        if !paper_gesture && chrome.toolrail.tool() == Tool::Pointer {
            // Hover feedback: a resize cursor over a selected anchor, a move
            // cursor over any visible body, and not-allowed over locked
            // geometry (visible objects in the viewport only — bounded work).
            if !over_chrome
                && let Some(position) =
                    pointer_position.filter(|position| drawing_area.contains(*position))
                && let Some(scale) = drawing_scale
            {
                if let Some(selected) = self.drawings.selected()
                    && self
                        .drawing_anchor_in(selected, position, history_right, total, &scale)
                        .is_some()
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[selected].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::ResizeNwSe
                        });
                } else if let Some(hovered) =
                    self.drawing_at(position, areas.chart, history_right, total, &scale)
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[hovered].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::Move
                        });
                }
            }
            if chart.clicked()
                && let Some(position) = chart.interact_pointer_pos()
                && let Some(scale) = drawing_scale
            {
                // Alt+click walks down the z-order through overlapping
                // objects; a plain click selects the topmost hit.
                let selected = if ui.input(|input| input.modifiers.alt) {
                    self.drawing_below_selection(
                        position,
                        areas.chart,
                        history_right,
                        total,
                        &scale,
                    )
                } else {
                    self.drawing_at(position, areas.chart, history_right, total, &scale)
                };
                self.drawings.select(selected);
            }
            // Drag initiation reads the raw press (an `interact` per object
            // would be unbounded work), so it must honour the chrome gate
            // itself: a press on the inspector never grabs the stroke or the
            // handle underneath — the panel is opaque by contract.
            let mut drawing_drag_started = false;
            if primary_pressed
                && !over_chrome
                && let Some(position) =
                    pointer_position.filter(|position| drawing_area.contains(*position))
                && let Some(scale) = drawing_scale
            {
                if let Some((drawing_index, point_index)) =
                    self.drawing_anchor_at(position, history_right, total, &scale)
                {
                    self.drawings.select(Some(drawing_index));
                    self.drawing_drag = if self.drawings.items()[drawing_index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Anchor {
                            drawing_index,
                            point_index,
                        }
                    };
                } else if let Some(index) =
                    self.drawing_at(position, areas.chart, history_right, total, &scale)
                {
                    self.drawings.select(Some(index));
                    self.drawing_drag = if self.drawings.items()[index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Translate
                    };
                }
                // A press that hits no geometry is not ours to interpret: it
                // belongs to whatever egui routed it to (inspector, manager,
                // chart pan). Deselection happens through the egui-routed
                // click above, which already respects floating windows.
                drawing_drag_started = self.drawing_drag.is_active();
            }
            if primary_down && !drawing_drag_started {
                match self.drawing_drag {
                    DrawingDrag::Anchor {
                        drawing_index,
                        point_index,
                    } => {
                        if let Some(position) = pointer_position {
                            let position = egui::pos2(
                                position.x.clamp(areas.chart.left(), history_right),
                                position.y.clamp(areas.chart.top(), areas.chart.bottom()),
                            );
                            if let Some(point) =
                                self.drawing_point_at(position, history_right, total)
                            {
                                self.drawings.move_anchor(drawing_index, point_index, point);
                            }
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                    }
                    DrawingDrag::Translate => {
                        if let Some(scale) = drawing_scale {
                            let (lo, hi) = scale.range();
                            let delta_bar = pointer_delta.x / self.viewport.candle_width();
                            let delta_price =
                                -f64::from(pointer_delta.y / areas.chart.height()) * (hi - lo);
                            self.drawings.translate_selected(delta_bar, delta_price);
                        }
                    }
                    DrawingDrag::Blocked => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    }
                    DrawingDrag::None => {}
                }
            }
            drawing_drag_consumes_gesture = self.drawing_drag.is_active();
            if primary_released {
                // One gesture, one undo entry — recorded only if it moved.
                self.drawings.commit_gesture();
                self.drawing_drag = DrawingDrag::None;
            }
        } else {
            self.drawing_drag = DrawingDrag::None;
        }
        // Where the press landed, not where the pointer is now: a pan that
        // started on the candles keeps working when it crosses the divider.
        let dragging_candles = chart
            .interact_pointer_pos()
            .is_some_and(|press| !in_lane(press));
        if total > 0
            && chart.dragged()
            && dragging_candles
            && !drawing_drag_consumes_gesture
            && !paper_gesture
        {
            let drag = chart.drag_delta();
            self.viewport.pan_pixels(drag.x, total);
            if let Some(auto) = auto
                && drag.y != 0.0
                && height > 1.0
            {
                let (lo, hi) = self.price_view.resolve(auto);
                let price_per_px = (hi - lo) / f64::from(height);
                self.price_view.pan(f64::from(drag.y) * price_per_px, auto);
            }
        }
        if chart.double_clicked() {
            self.viewport.snap_to_live();
            self.price_view.reset();
        }
        if chart.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                // Scroll up (positive) zooms in. Over the tape that means less
                // market time in the band; over the candles, wider candles.
                if let Some(orderflow) = self.orderflow.as_mut()
                    && chart.hover_pos().is_some_and(in_lane)
                {
                    orderflow.zoom_live_lane(2.0_f32.powf(scroll / 300.0));
                } else {
                    self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
                }
            }
        }

        // The lane's divider, as a resize handle. Registered after the chart
        // body so it takes the drag that would otherwise pan the candles
        // behind it, and it is the only place the pointer changes shape: the
        // line stays a hairline, the cursor is what says it can be moved.
        let divider = self.last_lane_divider_x.map(|x| {
            ui.interact(
                egui::Rect::from_min_max(
                    egui::pos2(x - LANE_HANDLE_HALF_WIDTH_PX, areas.chart.top()),
                    egui::pos2(x + LANE_HANDLE_HALF_WIDTH_PX, areas.chart.bottom()),
                ),
                self.interaction_id("lane_divider"),
                egui::Sense::drag(),
            )
        });
        if let Some(divider) = &divider {
            if divider.hovered() || divider.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if divider.dragged()
                && let Some(orderflow) = self.orderflow.as_mut()
            {
                // Drag left → a wider tape, at the expense of the candles.
                orderflow.resize_live_lane(divider.drag_delta().x, areas.chart.width());
            }
        }

        // Bottom time strip: drag or scroll to zoom. The segment under the
        // lane zooms the lane's window, the rest zooms the candle spacing —
        // each pane's own time axis, under the pane it belongs to.
        let (history_strip, lane_strip) =
            split_time_strip(areas.time_strip, self.last_lane_divider_x);
        let time = ui.interact(
            history_strip,
            self.interaction_id("time_nav"),
            egui::Sense::click_and_drag(),
        );
        if time.dragged() {
            // Drag right → wider candles (zoom in); left → narrower (zoom out).
            self.viewport
                .zoom((time.drag_delta().x / LANE_ZOOM_DRAG_PX).exp());
        }
        if time.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
            }
        }
        // A lane strip exists only where a lane does, so this is flow-pane
        // only by construction — the tape is what draws the segment.
        if let Some(lane_strip) = lane_strip
            && let Some(orderflow) = self.orderflow.as_mut()
        {
            let lane_time = ui.interact(
                lane_strip,
                egui::Id::new(("lane_time_nav", self.id)),
                egui::Sense::click_and_drag(),
            );
            if lane_time.dragged() {
                // Drag right → less market time in the band (zoom in), so
                // prints run across it faster and further apart.
                orderflow.zoom_live_lane((lane_time.drag_delta().x / LANE_ZOOM_DRAG_PX).exp());
            }
            if lane_time.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    orderflow.zoom_live_lane(2.0_f32.powf(scroll / 300.0));
                }
            }
        }

        // Right price gutter: the candles' own axis gesture. It spans their
        // height only — the bands below belong to the panes.
        axis_zoom_gesture(
            ui,
            self.interaction_id("price_nav"),
            areas.price_gutter,
            &mut self.price_view,
            auto,
        );

        // The same gesture, once per pane, over the gutter band beside it.
        // Keyed by slot *and* pane id: slots are allocated per pane, so a
        // split's two charts can hold the same slot number and a slot-only id
        // would make one pane's axis answer for the other's.
        let pane_id = self.id;
        for (view, gutter) in self.indicators.visible_panes_mut().zip(&areas.pane_gutters) {
            axis_zoom_gesture(
                ui,
                egui::Id::new(("pane_price_nav", pane_id, view.slot)),
                *gutter,
                &mut view.scale,
                view.last_auto,
            );
        }
    }

    pub fn draw_chart(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &PaneChrome<'_>,
    ) {
        let canvas_background = background_color(chrome.style);
        painter.rect_filled(area, egui::Rounding::ZERO, canvas_background);
        // The window's grid flag, read once: every layer question this frame
        // answers against the same value (see [`Self::layer_visible`]).
        let grid_on = chrome.style.canvas.grid_enabled;

        // Field borrows, not `self` borrows: the tape below needs `&mut
        // self.orderflow` while these are alive.
        let prefix = self.history_prefix.as_slice();
        let closed = self.state.bars();
        let partial = self.state.partial();
        let closed_total = prefix.len() + closed.len();
        let total = closed_total + usize::from(partial.is_some());
        let areas = self.plot_areas(area);
        // Indicator panes claimed the bottom band inside `plot_split`, so the
        // rect the candles scale to is the same one the input handler uses.
        let chart_rect = areas.chart;
        let pane_rects = areas.indicator_panes.clone();
        if total == 0 {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                format!("connecting to {} …", chrome.symbol),
                egui::FontId::proportional(16.0),
                theme::TEXT_MUTED,
            );
            if let Some(orderflow) = self.orderflow.as_ref() {
                orderflow.draw_status_badge(painter, chart_rect);
            }
            return;
        }

        // The live lane: a pane of its own, pinned to the right edge of the
        // chart, showing a fixed window of market time that always ends at
        // now. Fixed width, fixed pixels-per-ms: a print enters at the right
        // edge and slides left until it leaves into the slot of its own bar.
        //
        // It belongs to the tape rather than to the forming bar, which is what
        // keeps a bar close from emptying it — the reset that made the book
        // look like it was restarting every few seconds. And it is a pane
        // rather than a reservation inside the viewport, which is what keeps
        // every chart movement out of it: panning, zooming and dragging move
        // the candles beside the tape and never the tape itself, so the most
        // recent prints are on screen whatever the rest of the chart is doing.
        let lane_width_px = self
            .orderflow
            .as_mut()
            .and_then(|orderflow| orderflow.live_lane_width_px(chart_rect.width()))
            .unwrap_or(0.0);
        // Everything left of the divider is the candles' pane. They pan and
        // zoom inside it exactly as they did when it was the whole chart.
        self.last_lane_divider_x =
            crate::orderflow_render::lane_divider_x(chart_rect, lane_width_px);
        let history_rect = egui::Rect::from_min_max(
            chart_rect.min,
            egui::pos2(
                self.last_lane_divider_x
                    .unwrap_or_else(|| chart_rect.right()),
                chart_rect.bottom(),
            ),
        );

        let (start, end) = self.viewport.visible_range(history_rect.width(), total);

        // The visible closed bars, plus the partial if it falls in view. With
        // a venue prefix the window can straddle both series, so it is two
        // slices — chained where they are read rather than copied into one.
        // Copying was 24-48 KB every frame for the life of the pane, including
        // the common case of following the live edge, where the seam is three
        // months off screen and the prefix half of the window is empty.
        let closed_start = start.min(closed_total);
        let closed_end = end.min(closed_total);
        let visible_prefix = &prefix[closed_start.min(prefix.len())..closed_end.min(prefix.len())];
        let visible_state = &closed[closed_start.saturating_sub(prefix.len())
            ..closed_end.saturating_sub(prefix.len()).min(closed.len())];
        let visible_closed = || visible_prefix.iter().chain(visible_state);
        let partial_visible = partial.filter(|_| closed_total >= start && closed_total < end);

        // Auto-fit the visible bars, then apply any manual price pan/zoom. A
        // window with no bars in it still gets a scale (the last one, then the
        // newest bar), because a chart that draws nothing at all is
        // indistinguishable from a hung app — which is exactly how the blank
        // frame after a rebuild read.
        let nothing_in_view =
            visible_prefix.is_empty() && visible_state.is_empty() && partial_visible.is_none();
        let Some(auto_scale) = chart::price_window(
            visible_closed(),
            partial_visible,
            self.last_auto_range,
            partial.or_else(|| closed.last()),
            chart_rect.top(),
            chart_rect.bottom(),
        ) else {
            return;
        };
        let auto_range = auto_scale.range();
        let (lo, hi) = self.price_view.resolve(auto_range);
        let scale = PriceScale::from_range(lo, hi, chart_rect.top(), chart_rect.bottom());

        let cw = self.viewport.candle_width();
        let half = (cw * chrome.style.candles.clamped_width_frac() / 2.0).max(0.5);
        let right = history_rect.right();

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        // The projection builds a lane exactly when the layout draws one. Tied
        // to `lane_width_px` rather than restated, because the two decide the
        // same thing: with them apart, the newest prints would be clustered and
        // sized as lane prints and then squeezed into a single candle slot.
        // Flow-pane only: a pane with a tape never carries a venue prefix, so
        // the state half *is* the window here.
        let timeline = VisibleBarTimeline::new(
            self.state.timeline_revision(),
            closed_start,
            visible_state,
            partial_visible,
        );
        let orderflow_frame = self.orderflow.as_mut().and_then(|orderflow| {
            orderflow.project_visible(timeline, lane_width_px > 0.0, end == total, scale.range())
        });
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(frame) = &orderflow_frame
        {
            orderflow.draw_background(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
            );
        }

        // Grid + price labels first, behind the candles. Labels anchor on the
        // gutter's edge, past the live strip when one is shown.
        let axis_x = areas.price_gutter.left();
        self.draw_price_axis(painter, chart_rect, axis_x, &scale, chrome);

        // Candles, clipped to their own pane: panning far enough into history
        // sends the newest bars off the right of it, and they scroll out of
        // sight behind the tape instead of being drawn over it.
        let clip = painter.with_clip_rect(history_rect);
        // Clear the heat behind each candle's high–low span so a translucent
        // candle stays a clean divider — no liquidity band shows through it.
        // Where the price swept, the wall reads as consumed; bands survive only
        // in the gaps between candles and above/below each bar.
        if orderflow_frame.is_some()
            && self
                .orderflow
                .as_ref()
                .is_some_and(OrderflowView::depth_visible)
        {
            let clear_bar = |xc: f32, bar: &quantick_engine::Bar| {
                let top = scale.y(bar.high.to_f64().unwrap_or(0.0));
                let bottom = scale.y(bar.low.to_f64().unwrap_or(0.0));
                clip.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - half, top),
                        egui::pos2(xc + half, bottom),
                    ),
                    egui::Rounding::ZERO,
                    canvas_background,
                );
            };
            for (offset, bar) in visible_closed().enumerate() {
                clear_bar(
                    self.viewport.x_center(closed_start + offset, right, total),
                    bar,
                );
            }
            if let Some(partial) = partial_visible {
                clear_bar(self.viewport.x_center(closed_total, right, total), partial);
            }
        }
        for (offset, bar) in visible_closed().enumerate() {
            let index = closed_start + offset;
            let xc = self.viewport.x_center(index, right, total);
            draw_candle(&clip, xc, half, &scale, bar, false, &chrome.style.candles);
        }
        if let Some(partial) = partial_visible {
            let xc = self.viewport.x_center(closed_total, right, total);
            draw_candle(
                &clip,
                xc,
                half,
                &scale,
                partial,
                true,
                &chrome.style.candles,
            );
        }
        // Overlay indicator plots ride the candles' own clip, scale and
        // x-mapping — after candles, before aggression bubbles (the same
        // paint-order slot draw objects take).
        let plot_x = PlotX {
            viewport: &self.viewport,
            right,
            total,
        };
        // Slot -> (high_y, low_y) in pixels, for above/below-bar markers.
        let bar_extents = |slot: usize| -> Option<(f32, f32)> {
            let bar = if slot < prefix.len() {
                prefix.get(slot)
            } else if slot < closed_total {
                closed.get(slot - prefix.len())
            } else if slot == closed_total {
                partial
            } else {
                None
            }?;
            Some((
                scale.y(chart::to_f64(bar.high)),
                scale.y(chart::to_f64(bar.low)),
            ))
        };
        indicator_render::draw_overlays(
            &clip,
            self.indicators.visible_overlays(),
            &plot_x,
            &scale,
            start,
            end,
            partial_visible.map(|_| closed_total),
            &bar_extents,
        );
        // Draw objects (lines/boxes/labels) share the overlays' paint slot:
        // after candles, before aggression bubbles.
        for view in self.indicators.visible_overlays() {
            indicator_render::draw_objects(
                &clip,
                view.render_objects(),
                &plot_x,
                |v| scale.y(v),
                start,
                end,
            );
        }
        // Pane indicators stack in the band carved off above, sharing the
        // candles' x-mapping so bars and their flow read as one chart. Each
        // pane records the range it auto-fitted to, so the gesture over its
        // axis zooms the very range this frame drew.
        let grid = grid_color(chrome.style);
        for ((view, pane), gutter) in self
            .indicators
            .visible_panes_mut()
            .zip(&pane_rects)
            .zip(&areas.pane_gutters)
        {
            let auto = indicator_render::pane_auto_range(view, start, end);
            view.last_auto = auto;
            let frame = indicator_render::PaneFrame {
                rect: egui::Rect::from_min_max(
                    egui::pos2(history_rect.left(), pane.top()),
                    egui::pos2(history_rect.right(), pane.bottom()),
                ),
                gutter: *gutter,
                background: canvas_background,
                grid,
            };
            indicator_render::draw_pane(
                painter,
                &frame,
                view,
                &plot_x,
                auto.map(|auto| view.scale.resolve(auto)),
                start,
                end,
                // The slot of the forming bar counts the venue prefix too: a
                // pane's partial marker has to land on the same slot the
                // candles' does, and this pane's series starts at the prefix.
                partial_visible.map(|_| closed_total),
            );
        }
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(frame) = &orderflow_frame
        {
            orderflow.draw_aggressions(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
            );
        }

        // The live strip: the book right now plus the forming bar's
        // aggression histogram, beside the axis the price labels live on.
        // Its own rect, so chart layers never bleed into it. The histogram
        // follows `partial` (not its visible filter): the strip reports the
        // bar forming now even while the user pans through history.
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(strip) = areas.live_strip
        {
            orderflow.draw_live_strip(
                painter,
                strip,
                &scale,
                canvas_background,
                partial.map(|bar| bar.open_time),
            );
        }

        // Drawings sit above market layers and remain anchored to chart space,
        // not the screen, while the viewport moves beneath them.
        self.draw_drawings(painter, chart_rect, right, total, &scale);

        // Simulated orders and the position sit above the drawings: they are
        // operational state, read against the last price painted next. The
        // unclipped painter carries their chips into the gutter. Both panes
        // paint them — one market, one set of price levels, and a level is as
        // true on the 5-minute context as it is on the flow chart. Prices out
        // of a pane's visible range simply do not draw.
        //
        // Switched off, they are only unpainted: the orders keep working and
        // the dock keeps listing them (see the layer's hint).
        if self.layer_visible(ChartLayer::PaperTrading, grid_on) {
            chrome.paper.draw_layer(painter, chart_rect, axis_x, &scale);
        }

        // Above the flow layers: everything else on the canvas is read against
        // it. Drawn on the unclipped painter so the chip reaches the gutter.
        if self.layer_visible(ChartLayer::LastPrice, grid_on)
            && let Some(bar) = partial.or_else(|| closed.last())
        {
            self.draw_last_price(painter, chart_rect, axis_x, &scale, bar, chrome);
        }
        // The candles' own marks, so they are placed and clipped in their
        // pane: where venue candles give way to bars built from prints, and
        // where backfilled prints give way to live ones.
        if self.layer_visible(ChartLayer::SeamDivider, grid_on) {
            self.draw_seam_divider(painter, history_rect, total, cw);
        }
        if self.layer_visible(ChartLayer::BackfillDivider, grid_on) {
            self.draw_backfill_divider(painter, history_rect, total, cw);
        }
        self.draw_time_strip(painter, areas.time_strip, start, end, total, chrome);
        if let Some(orderflow) = self.orderflow.as_ref() {
            self.draw_lane_time_axis(
                painter,
                split_time_strip(areas.time_strip, self.last_lane_divider_x).1,
                orderflow.live_lane_window_ms(closed),
            );
        }
        // Panned off the data (or a rebuild re-cut the series under the
        // window): the chart is whole — axis, tape, badges — but there is
        // nothing in the candles' pane, so say so and say the way back.
        if nothing_in_view {
            painter.text(
                history_rect.center(),
                egui::Align2::CENTER_CENTER,
                "no bars in view — double-click to return to the live edge",
                egui::FontId::proportional(EMPTY_VIEW_FONT_SIZE),
                theme::TEXT_MUTED,
            );
        }
        if self.layer_visible(ChartLayer::Crosshair, grid_on) {
            self.draw_crosshair(painter, chart_rect, axis_x, &scale, chrome);
        }
        // The status badge is not a layer: it reports whether the source is
        // healthy, and a chart with every layer off must still say that.
        if let Some(orderflow) = self.orderflow.as_ref() {
            orderflow.draw_status_badge(painter, chart_rect);
        }

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.last_auto_range = Some(auto_range);
        self.last_chart_height = chart_rect.height();
        self.last_chart_top = chart_rect.top();
    }

    /// Bottom time strip: a top border and a few `HH:MM:SS` labels for the
    /// visible bars. Draggable left/right to zoom the candle spacing.
    ///
    /// The labels stay under the candles' own pane; the segment past the lane's
    /// divider is the tape's time axis and reads its window instead
    /// ([`Self::draw_lane_time_axis`]).
    fn draw_time_strip(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        start: usize,
        end: usize,
        total: usize,
        chrome: &PaneChrome<'_>,
    ) {
        painter.line_segment(
            [
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.right(), strip.top()),
            ],
            egui::Stroke::new(1.0_f32, grid_color(chrome.style)),
        );
        let font = egui::FontId::monospace(10.0);
        let y = strip.center().y;
        // Up to ~6 evenly-spaced labels across the visible closed bars.
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let (history_strip, _) = split_time_strip(strip, self.last_lane_divider_x);
        let step = (visible / 6).max(1);
        let mut index = start;
        while index < end {
            if let Some(bar) = self.closed_bar(index) {
                let x = self.viewport.x_center(index, history_strip.right(), total);
                if history_strip.x_range().contains(x) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        fmt_time(bar.open_time, chrome.tz),
                        font.clone(),
                        theme::TEXT_MUTED,
                    );
                }
            }
            index += step;
        }
    }

    /// The live lane's own time axis: how much market time the tape is
    /// showing, under the tape.
    ///
    /// The lane has no bar boundaries to label — it is one continuous window —
    /// so its axis reads the window itself. It is also the only readout of what
    /// the lane's zoom is currently worth, which is what makes dragging here
    /// something other than guesswork.
    fn draw_lane_time_axis(
        &self,
        painter: &egui::Painter,
        lane_strip: Option<egui::Rect>,
        window_ms: i64,
    ) {
        let Some(strip) = lane_strip else {
            return;
        };
        painter.text(
            strip.center(),
            egui::Align2::CENTER_CENTER,
            format!("tape · {}", fmt_window(window_ms)),
            egui::FontId::monospace(10.0),
            theme::TEXT_MUTED,
        );
    }

    /// Right-hand price axis: round-number gridlines and labels. `axis_x` is
    /// the gutter's left edge — the chart's right edge normally, the live
    /// strip's right edge while the strip sits between them.
    fn draw_price_axis(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        chrome: &PaneChrome<'_>,
    ) {
        let grid = grid_color(chrome.style);
        let (lo, hi) = scale.range();
        let font = egui::FontId::monospace(chart::AXIS_LABEL_FONT_PX);
        for tick in crate::chart::nice_ticks(lo, hi, 8) {
            let y = scale.y(tick);
            if y < chart_rect.top() || y > chart_rect.bottom() {
                continue;
            }
            painter.line_segment(
                [
                    egui::pos2(chart_rect.left(), y),
                    egui::pos2(chart_rect.right(), y),
                ],
                egui::Stroke::new(1.0_f32, grid),
            );
            painter.text(
                egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y),
                egui::Align2::LEFT_CENTER,
                format!("{tick:.2}"),
                font.clone(),
                theme::TEXT_MUTED,
            );
        }
        // The axis dividing line.
        painter.line_segment(
            [
                egui::pos2(axis_x, chart_rect.top()),
                egui::pos2(axis_x, chart_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, grid),
        );
    }

    /// The current price: a dashed line across the chart and a solid chip on
    /// the price axis, coloured by the direction of the bar carrying it.
    ///
    /// This is the always-on answer to "am I above or below?" — the question
    /// every other mark on the canvas is read against, and the one a wall of
    /// resting liquidity cannot answer on its own.
    fn draw_last_price(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        bar: &quantick_engine::Bar,
        chrome: &PaneChrome<'_>,
    ) {
        let Some(price) = bar.close.to_f64() else {
            return;
        };
        let y = scale.y(price);
        if y < chart_rect.top() || y > chart_rect.bottom() {
            return;
        }
        // Same predicate and same two colours the candle wears, so the chip
        // and the bar it reports can never disagree about direction.
        let rgb = if crate::candle_view::is_bullish(bar) {
            chrome.style.candles.bull_outline
        } else {
            chrome.style.candles.bear_outline
        };
        let color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

        // Runs through the live strip when one is shown (`axis_x` then sits
        // past it): the depth silhouette is read against this exact line.
        painter.extend(egui::Shape::dashed_line(
            &[egui::pos2(chart_rect.left(), y), egui::pos2(axis_x, y)],
            egui::Stroke::new(1.0_f32, color.gamma_multiply(LAST_PRICE_LINE_ALPHA)),
            LAST_PRICE_DASH_PX,
            LAST_PRICE_GAP_PX,
        ));

        // Same geometry as the crosshair tag, so the two never disagree about
        // where a price sits on the axis.
        let galley = painter.layout_no_wrap(
            format!("{price:.2}"),
            egui::FontId::monospace(chart::AXIS_LABEL_FONT_PX),
            LAST_PRICE_CHIP_TEXT,
        );
        let text_pos = egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, egui::Rounding::same(2.0), color);
        painter.galley(text_pos, galley, LAST_PRICE_CHIP_TEXT);
    }

    fn drawing_screen_point(
        &self,
        point: ChartPoint,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> egui::Pos2 {
        egui::pos2(
            self.viewport
                .x_at_bar_position(point.bar, history_right, total),
            scale.y(point.price),
        )
    }

    /// Paint the completed drawing objects. This runs once per frame and is
    /// O(number of drawings); it never touches the per-trade ingestion path.
    fn draw_drawings(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) {
        let clipped = painter.with_clip_rect(chart_rect);
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            let selected = self.drawings.selected() == Some(index);
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                style: drawing.style,
                selected,
                halo: false,
            };
            // A locked object shows no resize handles: its geometry is not
            // editable, so the affordance would lie.
            drawing.tool.paint(
                &clipped,
                chart_rect,
                drawing.style,
                &points,
                &ctxt,
                selected && !drawing.locked,
            );
        }

        if let Some(draft) = self.drawings.draft() {
            let mut points = self.projected_drawing_points(draft, history_right, total, scale);
            // The preview completes the geometry with the hovered anchor, in
            // both screen and chart space, so payload-driven tools can show
            // their real shape while placing.
            let mut anchors: SmallVec<[ChartPoint; 4]> = SmallVec::from_slice(&draft.points);
            if points.len() < draft.tool.required_points()
                && let Some(hover) = self.drawing_hover
            {
                points.push(self.drawing_screen_point(hover, history_right, total, scale));
                anchors.push(hover);
            }
            let ctxt = DrawContext {
                payload: draft.payload.as_ref(),
                anchors: &anchors,
                scale,
                style: draft.style,
                selected: false,
                halo: false,
            };
            draft
                .tool
                .paint(&clipped, chart_rect, draft.style, &points, &ctxt, false);
        }
    }

    /// Crosshair following the pointer, with the price shown on the axis.
    /// Drawn only while the Crosshair tool is armed on the rail (§7 — the
    /// hover crosshair is a mode, not an always-on layer).
    fn draw_crosshair(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        chrome: &PaneChrome<'_>,
    ) {
        if chrome.toolrail.tool() != Tool::Crosshair {
            return;
        }
        let Some(pos) = self.hover_pos else {
            return;
        };
        if !chart_rect.contains(pos) {
            return;
        }
        let stroke = egui::Stroke::new(1.0_f32, theme::TEXT_FAINT);
        painter.line_segment(
            [
                egui::pos2(pos.x, chart_rect.top()),
                egui::pos2(pos.x, chart_rect.bottom()),
            ],
            stroke,
        );
        // Reaches the axis through the live strip when one is shown, so the
        // cursor height can be read against the depth silhouette too.
        painter.line_segment(
            [
                egui::pos2(chart_rect.left(), pos.y),
                egui::pos2(axis_x, pos.y),
            ],
            stroke,
        );

        // Price tag on the axis at the cursor height.
        let price = scale.price_at(pos.y);
        let galley = painter.layout_no_wrap(
            format!("{price:.2}"),
            egui::FontId::monospace(chart::AXIS_LABEL_FONT_PX),
            egui::Color32::WHITE,
        );
        let text_pos = egui::pos2(
            axis_x + chart::AXIS_LABEL_GAP_PX,
            pos.y - galley.size().y / 2.0,
        );
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, egui::Rounding::same(2.0), theme::TAG_BG);
        painter.galley(text_pos, galley, egui::Color32::WHITE);
    }

    /// A vertical marker where venue candles give way to bars this app built
    /// from prints.
    ///
    /// Dashed, and in the same amber the backfill divider uses: both mark
    /// provenance, and the dash is what says this one is a *different kind* of
    /// boundary. Left of it the bars are the venue's own summaries — one price
    /// per interval, with the aggressor split only where the venue publishes
    /// one. Right of it every bar was cut from prints this app saw.
    fn draw_seam_divider(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
    ) {
        let seam = self.seam_slot();
        if seam == 0 || seam >= total {
            return;
        }
        let x = self.viewport.x_center(seam, pane.right(), total) - candle_width / 2.0;
        if x < pane.left() || x > pane.right() {
            return; // off-screen
        }
        draw_dashed_vertical(painter, x, pane, SEAM_DASH_PX, SEAM_GAP_PX, theme::AMBER);
        painter.text(
            egui::pos2(x - 4.0, pane.top() + 4.0),
            egui::Align2::RIGHT_TOP,
            "venue",
            egui::FontId::proportional(11.0),
            theme::TEXT_MUTED,
        );
    }

    /// A vertical marker separating backfilled history (left) from live (right),
    /// drawn only when the boundary falls inside the candles' pane.
    ///
    /// `pane` is the candles' own rect — the chart minus the live lane — since
    /// that is the space the viewport maps bar indices into.
    fn draw_backfill_divider(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
    ) {
        let Some(boundary) = self.state.backfill_boundary() else {
            return;
        };
        if boundary == 0 {
            return; // nothing backfilled
        }
        // The engine counts its own bars; the venue prefix sits in front of
        // them, so the slot is offset by however many bars that is.
        let boundary = boundary + self.seam_slot();
        // The divider sits at the left edge of the first live bar.
        let x = self.viewport.x_center(boundary, pane.right(), total) - candle_width / 2.0;
        if x < pane.left() || x > pane.right() {
            return; // off-screen
        }
        painter.line_segment(
            [egui::pos2(x, pane.top()), egui::pos2(x, pane.bottom())],
            egui::Stroke::new(1.0_f32, theme::AMBER),
        );
        let font = egui::FontId::proportional(11.0);
        painter.text(
            egui::pos2(x - 4.0, pane.bottom() - 4.0),
            egui::Align2::RIGHT_BOTTOM,
            "backfill",
            font.clone(),
            theme::TEXT_MUTED,
        );
        painter.text(
            egui::pos2(x + 4.0, pane.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "live",
            font,
            theme::AMBER,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Time pane left, flow pane right, on a divider that costs both of them
    /// nothing but its own width (§11).
    #[test]
    fn the_canvas_splits_time_left_flow_right_at_the_divider() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let areas = split_canvas(area, 0.5);
        assert!(
            areas.time.right() <= areas.flow.left(),
            "time is the left pane"
        );
        assert_eq!(areas.time.right(), areas.divider.left());
        assert_eq!(areas.divider.right(), areas.flow.left());
        assert_eq!(areas.divider.width(), CANVAS_DIVIDER_PX);
        assert_eq!(areas.time.left(), area.left());
        assert_eq!(areas.flow.right(), area.right());
        assert_eq!(
            areas.time.width() + areas.divider.width() + areas.flow.width(),
            area.width(),
            "the split spends the canvas exactly once"
        );
        // Both panes keep the full height: the split is vertical only.
        assert_eq!(areas.time.top(), area.top());
        assert_eq!(areas.flow.bottom(), area.bottom());
    }

    /// A pane too narrow to read is not a layout, it is a lost pane. §11
    /// promises each of them a quarter of the canvas, whatever the drag says.
    #[test]
    fn neither_pane_can_be_dragged_below_a_quarter_of_the_canvas() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        for (asked, expected) in [
            (-3.0, 0.25),
            (0.0, 0.25),
            (0.1, 0.25),
            (0.9, 0.75),
            (7.0, 0.75),
        ] {
            let areas = split_canvas(area, asked);
            // The divider sits *on* the split, so compare where the split is
            // rather than a pane width that has half a divider taken out of it.
            let split = (areas.divider.center().x - area.left()) / area.width();
            assert!(
                (split - expected).abs() < 1e-3,
                "asking for {asked} must clamp to {expected}, got {split}"
            );
            let floor = area.width() * MIN_PANE_FRACTION - CANVAS_DIVIDER_PX;
            assert!(
                areas.time.width() >= floor,
                "the time pane keeps its quarter"
            );
            assert!(areas.flow.width() >= floor, "and so does the flow pane");
        }
    }

    /// The header is a strip carved off the pane, not an overlay: the selector
    /// must never be painted across market data.
    #[test]
    fn the_time_pane_header_costs_the_chart_its_own_height() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 600.0));
        let areas = split_time_pane(area);
        assert_eq!(areas.header.height(), crate::time_header::HEIGHT_PX);
        assert_eq!(
            areas.header.bottom(),
            areas.chart.top(),
            "no gap, no overlap"
        );
        assert_eq!(areas.chart.bottom(), area.bottom());
        assert_eq!(areas.header.width(), area.width());
    }
}
