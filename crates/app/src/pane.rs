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

use crate::bands::{self, Band, BandLabel, Bands};
use crate::candle_view::draw_candle;
use crate::chart::{self, PriceScale};
use crate::chart_layers::{ChartLayer, LayerActions, LayerBlock, blocks};
use crate::config::FeedCapabilities;
use crate::drawings::{
    self, ChartPoint, DrawContext, Drawing, DrawingBand, DrawingStyle, Drawings,
};
use crate::indicator_render::{self, PlotX};
use crate::indicator_worker::{
    IndicatorCommand, IndicatorSource, IndicatorWorker, MAX_LANE_RUNGS, SlotId,
};
use crate::indicators::{IndicatorViews, MIN_PANE_HEIGHT_PX, PaneSizing};
use crate::orderflow_view::{OrderflowView, VisibleBarTimeline};
use crate::paper_trading::{ChartInput, PaperTrading};
use crate::plot_area::{self, PlotAreas, fmt_time_as, plot_split, split_time_strip};
use crate::pointer_compass;
use crate::price_view::PriceView;
use crate::state::{BarKind, BarSpec, ChartState, ImbalanceUnit};
use crate::style::ChartStyle;
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::{Tool, ToolRail};
use crate::viewport::Viewport;
use quantick_orderflow::{
    LANE_WINDOW_PRESETS_MS, LaneWindow, MAX_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_WINDOW_MS,
    format_window_ms, lane_lag_label, lane_window_label, reserved_span_ms, same_lane_window,
};

/// Hit radius for selecting a drawing anchor, in logical pixels.
const DRAWING_SELECT_RADIUS_PX: f32 = 10.0;
/// Hit radius for a selected drawing's editable anchor.
pub const DRAWING_ANCHOR_RADIUS_PX: f32 = 12.0;
/// Minimum pointer travel that turns one press/release into drag placement.
/// How near the pointer must be to a bar's open / high / low / close for
/// the magnet to take the anchor. Generous enough to catch the swing you
/// aimed at, tight enough to still draw a free diagonal between bars.
const MAGNET_REACH_PX: f32 = 12.0;
/// How much of a sidebar candle's lane its *body* takes, as a fraction of the
/// half-lane.
///
/// Seven tenths, so the body reads as a body and the wick still shows either
/// side of it. Derived from the lane rather than fixed, so widening the lane
/// widens the candle instead of leaving a wider gap around the same sliver.
const SIDEBAR_BODY_FRAC: f32 = 0.35;
/// The candle magnet has no reach: [`drawings::AnchorSnap::NearestOhlc`]
/// never lets go, however far the pointer floats from the candle.
const MAGNET_REACH_UNLIMITED_PX: f32 = f32::INFINITY;
const DRAWING_DRAG_THRESHOLD_PX: f32 = 4.0;

/// How far a press must travel before its release counts as "the trader
/// dragged this object out" instead of "the trader clicked".
///
/// Deliberately larger than [`DRAWING_DRAG_THRESHOLD_PX`], which answers a
/// different question: whether a gesture already under way is a drag at all.
/// This one decides whether a *release* finishes an object, and it was
/// sharing that four-pixel answer — which is inside the wander of an ordinary
/// click.
///
/// What that cost, reported from the running build: a click meant to start a
/// fixed-range profile placed **both** its anchors, so the object was born
/// less than one bar wide ("1 of 1 bars"), and completing it disarmed the
/// tool. Moving the pointer afterwards then did nothing at all, which reads
/// exactly like a frozen chart — the trader is waiting for a range to follow
/// their hand and there is no longer a draft to follow it.
///
/// A release under this distance leaves the draft alive instead, so the
/// gesture becomes the click-move-click the hand was already doing.
const DRAWING_DRAG_COMPLETES_PX: f32 = 12.0;

/// A pointer and a modifier for a run with nobody at the keyboard — see
/// [`ChartPane::parked_hand`]. Never constructed outside the harness hook.
#[derive(Debug, Clone, Copy)]
pub struct ParkedHand {
    pub position: egui::Pos2,
    pub constrain: drawings::Constrain,
}
/// How far the pointer must travel before a freehand stroke records another
/// point. Chosen so a hand-drawn circle keeps its shape while a half-second
/// scribble stores tens of anchors instead of hundreds — every anchor is
/// paint and hit-test work on every frame for the rest of the session.
const FREEHAND_MIN_STEP_PX: f32 = 4.0;
/// Hard ceiling on one stroke, so a pointer that never stops moving cannot
/// turn a single drawing into an unbounded cost.
const FREEHAND_MAX_POINTS: usize = 512;

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
/// Font size of the caption beside a dashed vertical mark, in points.
///
/// Shared by the venue seam and the tape-gap mark rather than written at each:
/// they are the same kind of caption answering the same question about the bars
/// either side of a line, and two copies would drift the first time one moved.
const SEAM_LABEL_PT: f32 = 11.0;
/// Gap between a dashed vertical mark and its caption, in pixels — and the
/// caption's drop from the top of the pane.
const SEAM_LABEL_INSET_PX: f32 = 4.0;

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
/// The flow pane, or one of the context panes by its slot in the stack —
/// `Time(0)` is the top context chart, `Time(1)` the one under it. A two-arm
/// enum lived here before the stack existed, and it is how the three-pane
/// canvas shipped with a dead second chart: every reader mapped `Time` to the
/// *first* context pane, so clicking the second focused the first, the BARS
/// group changed the first, and "add indicator" landed on the first. A side
/// that cannot name a pane cannot address it.
///
/// Slots are positions in [`crate::tab::Tab::time_panes`], and moving a pane
/// within the stack moves what a slot names — which is right for focus (it
/// follows the chart the trader is looking at) and irrelevant for everything
/// else, which addresses panes through [`PaneIndex`] and lives one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum PaneSide {
    #[default]
    Flow,
    Time(usize),
}

impl PaneSide {
    /// This side as a [`PaneIndex`] address: `0` is the flow pane, `1..` the
    /// context stack — the order [`crate::tab::Tab::pane_at`] uses.
    #[must_use]
    pub const fn index(self) -> PaneIndex {
        match self {
            Self::Flow => 0,
            Self::Time(slot) => slot + 1,
        }
    }

    /// The side a [`PaneIndex`] address names. The inverse of [`Self::index`].
    #[must_use]
    pub const fn from_index(index: PaneIndex) -> Self {
        match index {
            0 => Self::Flow,
            slot => Self::Time(slot - 1),
        }
    }

    /// What the chrome calls this pane: "Flow", "Timeframe", "Timeframe 2".
    ///
    /// The top context chart keeps the bare name every menu and status line
    /// showed while it was the only one; the number appears only where there
    /// is a second chart to tell it from.
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Flow => "Flow".to_owned(),
            Self::Time(0) => "Timeframe".to_owned(),
            Self::Time(slot) => format!("Timeframe {}", slot + 1),
        }
    }
}

/// Half-width of the divider's grab area, which reaches a little into both
/// panes so the handle is catchable without widening the rule itself.
pub const CANVAS_DIVIDER_HANDLE_PX: f32 = 5.0;

/// Where the divider sits when the split is first shown.
///
/// Roughly a third to the context pane, the rest to the flow pane. An even
/// split says the two charts matter equally, and in quantick they do not: the
/// heatmap is what the product is for, and the timeframe chart beside it is
/// context. The opening canvas should say so before the trader touches
/// anything — a default is an argument about what matters.
pub const DEFAULT_PANE_FRACTION: f32 = 0.35;

/// A time pane's area, split into the strip its selector sits in and the
/// chart below it.
pub struct TimePaneAreas {
    pub header: egui::Rect,
    pub chart: egui::Rect,
}

/// Hold a stored split inside the canvas.
///
/// A sanity clamp, not a floor. The floor is
/// [`canvas_layout::MIN_PANE_WIDTH_PX`], and it is applied where the canvas
/// width is known — inside the splitter, on every frame, for every pane.
/// Holding a *second* floor here as a share of the canvas is what made
/// collapse-by-drag unreachable: the share (a quarter) always bound before the
/// width (120 px) could, so a drag restarted at 400 px of a 1600 px canvas
/// every frame and could never travel far enough in one to dismiss the column.
/// One floor, one owner, and it is the one that knows how wide the canvas is.
#[must_use]
pub fn clamp_pane_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        DEFAULT_PANE_FRACTION
    }
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

/// Type size of the axis under the tape.
const LANE_AXIS_FONT_PX: f32 = 10.0;

/// Breathing room between the tape's two axis labels, and between the warning
/// and the strip's own right edge.
const LANE_AXIS_GAP_PX: f32 = 8.0;

/// Pixels of lane the ladder spends one rung on.
///
/// A rung is a full evaluation of every hosted indicator, so this is the knob
/// that trades cost against smoothness. Six pixels draws a curve that reads as
/// continuous at the sizes the lane is ever given, and keeps a wide lane well
/// under [`MAX_LANE_RUNGS`].
const LANE_RUNG_PX: f32 = 6.0;

/// How many rungs a lane this wide is worth sampling at.
///
/// Zero for a chart with no lane — the signal that no ladder is walked at all.
fn lane_rungs(lane_width_px: f32) -> usize {
    if !lane_width_px.is_finite() || lane_width_px <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rungs = (lane_width_px / LANE_RUNG_PX) as usize;
    rungs.clamp(1, MAX_LANE_RUNGS)
}

/// The tape switch's chip, in logical pixels.
///
/// A fixed size rather than one measured off its own text: the hit rect is
/// registered in the input pass and painted in the pass after it, and two
/// measurements of one chip are two chances for the button to be somewhere the
/// click is not.
const TAPE_SWITCH_SIZE: egui::Vec2 = egui::vec2(54.0, 18.0);
/// Inset of that chip from the canvas's top-right corner.
const TAPE_SWITCH_INSET: egui::Vec2 = egui::vec2(8.0, 4.0);
/// Gap between the switch and whatever sits to its left.
const TAPE_SWITCH_GAP_PX: f32 = 6.0;
/// Room the switch takes off the right edge, for anything else that wants the
/// same corner — the book status badge is the one thing that does.
const TAPE_SWITCH_RESERVED_PX: f32 = TAPE_SWITCH_SIZE.x + TAPE_SWITCH_INSET.x + TAPE_SWITCH_GAP_PX;
/// Corner radius of the chip, matching the status badge it sits beside.
const TAPE_SWITCH_ROUNDING_PX: f32 = 3.0;
/// Chip background opacity over the canvas, resting and hovered. The resting
/// value is the status badge's, so the two read as one family of chrome.
const TAPE_SWITCH_FILL_ALPHA: u8 = 165;
const TAPE_SWITCH_HOVER_FILL_ALPHA: u8 = 210;
/// Opacity of the hover outline, relative to the chip's own accent.
const TAPE_SWITCH_HOVER_STROKE_ALPHA: f32 = 0.7;
/// Width of every line the chip draws.
const TAPE_SWITCH_STROKE_PX: f32 = 1.0;
/// State dot: how far its centre sits from the chip's left edge, and its
/// radius. Filled means the tape is on the canvas, hollow means it is not.
const TAPE_SWITCH_DOT_X_PX: f32 = 9.0;
const TAPE_SWITCH_DOT_RADIUS_PX: f32 = 3.0;
/// Where the label starts, measured from the same edge as the dot.
const TAPE_SWITCH_LABEL_X_PX: f32 = 17.0;
/// Label size, matching the status badge's.
const TAPE_SWITCH_FONT_PX: f32 = 11.0;
/// The label itself. Short by necessity: the chip sits over market data.
const TAPE_SWITCH_LABEL: &str = "tape";

/// Where the tape switch sits on a canvas this size.
///
/// The canvas's top-right corner: the tape's own corner, so the switch that
/// puts it there and takes it away is on it. One function, read by the input
/// pass and the paint pass alike.
#[must_use]
pub(crate) fn tape_switch_rect(chart_rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            chart_rect.right() - TAPE_SWITCH_INSET.x - TAPE_SWITCH_SIZE.x,
            chart_rect.top() + TAPE_SWITCH_INSET.y,
        ),
        TAPE_SWITCH_SIZE,
    )
}

/// Half-height of the grab band over a pane's top edge, in pixels.
///
/// The rule itself stays a hairline — a thick bar between a chart and its
/// indicator reads as a wall in the data. The handle around it is what makes
/// it catchable, and the resize cursor is the only thing that announces it:
/// the same bargain the live lane's divider and the canvas split already
/// strike.
const PANE_DIVIDER_HANDLE_PX: f32 = 4.0;

/// How near an overlay's plotted line a double click has to land to be read as
/// a click on *that line* rather than on the chart behind it.
///
/// The same order as the drawings' own pick tolerance, and for the same
/// reason: a one-pixel line needs a grab band wider than itself or it can only
/// be hit by luck. Kept modest so that a double click in open chart still means
/// "back to the live edge" — the gesture only changes meaning where a curve
/// actually is.
const PLOT_PICK_TOLERANCE_PX: f32 = 5.0;

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

/// The divider along a pane's top edge, as a resize handle.
///
/// The band it opens is the pane *below* it, which is what a top edge means:
/// drag it up and that pane grows into the chart, drag it down and it gives
/// the room back. Double click hands the pane to the automatic layout again —
/// the same escape the price axis and every pane's own scale offer.
///
/// Returns the sizing the pane should now have, or `None` when the divider was
/// not touched this frame.
fn pane_divider_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    slot: &crate::indicators::PaneSlot,
    plot: egui::Rect,
) -> Option<PaneSizing> {
    let edge = slot.rect.top();
    let handle = ui.interact(
        egui::Rect::from_min_max(
            egui::pos2(plot.left(), edge - PANE_DIVIDER_HANDLE_PX),
            egui::pos2(plot.right(), edge + PANE_DIVIDER_HANDLE_PX),
        ),
        id,
        egui::Sense::click_and_drag(),
    );
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if handle.double_clicked() {
        return Some(PaneSizing::Auto);
    }
    if handle.dragged() {
        let delta = handle.drag_delta().y;
        if delta != 0.0 {
            // Dragging the top edge upwards makes the band below it taller.
            // The floor is not applied here: `PaneSizing::desired` owns it, so
            // a drag and the automatic layout cannot disagree about how short
            // a pane may be.
            return Some(PaneSizing::Manual(slot.rect.height() - delta));
        }
    }
    None
}

/// The gesture that pans a pane's own scale: press inside the pane and drag it
/// up or down, exactly as a press on the candles drags price.
///
/// Separate from [`axis_zoom_gesture`] because they are different verbs on the
/// same axis — the gutter *scales* it, the body *moves* it — and the candles
/// already split them that way. A pane whose body did nothing left the axis
/// reachable only from the gutter at the far side of the chart, which is a
/// long way to travel to move a curve out of its own way.
///
/// `auto` is the range the last frame fitted; without one there is nothing to
/// take manual control *from*, and only the reset stays available.
/// `primary_free` is false while the primary button belongs to something else
/// — a drawing tool placing an object, or a drawing being dragged. The wheel
/// and the axis still answer then: an armed tool takes the *button*, not the
/// pane (audit S2).
fn pane_pan_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    body: egui::Rect,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
    primary_free: bool,
) -> PaneGesture {
    let response = ui.interact(body, id, egui::Sense::click_and_drag());
    if response.double_clicked() && primary_free {
        view.reset();
    }
    if !primary_free {
        let mut gesture = PaneGesture::default();
        if response.hovered() {
            gesture.scroll_y = ui.input(|input| input.raw_scroll_delta.y);
        }
        return gesture;
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // The pane's own axis moves here; time is the candles' to move, and the
    // caller applies it once for however many panes are stacked — panning
    // three panes' worth of x would move the chart three times per drag.
    let mut gesture = PaneGesture::default();
    if response.dragged() {
        gesture.pan_x = response.drag_delta().x;
        if let Some(auto) = auto {
            let height = body.height();
            let delta = response.drag_delta().y;
            if delta != 0.0 && height > 1.0 {
                let (lo, hi) = view.resolve(auto);
                let per_px = (hi - lo) / f64::from(height);
                view.pan_screen(f64::from(delta), per_px, auto);
            }
        }
    }
    if response.hovered() {
        gesture.scroll_y = ui.input(|input| input.raw_scroll_delta.y);
    }
    gesture
}

/// What a drag or scroll over a pane body owes the *chart* — the pane's own
/// axis has already been moved by the time this is returned.
///
/// A pane is a band of the same time axis the candles draw, so the gestures
/// that mean "time" there mean time here too: drag sideways to pan it, scroll
/// to zoom it. Collected rather than applied on the spot because the viewport
/// belongs to the pane's owner, and because every stacked pane would otherwise
/// apply its own copy of the same drag.
#[derive(Default)]
struct PaneGesture {
    /// Horizontal drag, in pixels.
    pan_x: f32,
    /// Wheel travel while hovering, in egui's scroll units.
    scroll_y: f32,
}

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
///
/// `flips` is the price gutter's privilege: an expanding drag past the flip
/// threshold turns the chart upside down ([`PriceView::drag_zoom`]), and the
/// drag's sense mirrors with it — the same downward drag that flattened the
/// chart grows it again past the flip. Indicator gutters pass `false`: a
/// pane's values have no upside down. The wheel never flips on either.
///
/// Returns the band's response, so the price gutter can hang its context menu
/// off the very region the gesture owns.
fn axis_zoom_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    band: egui::Rect,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
    flips: bool,
) -> egui::Response {
    let response = ui.interact(band, id, egui::Sense::click_and_drag());
    // The cursor is the affordance (audit F5): nothing else on the band says
    // it scales, mirroring the lane divider's own rule that the pointer's
    // shape is what announces a gesture.
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.double_clicked() {
        view.reset();
    }
    let Some(auto) = auto else {
        return response;
    };
    // Primary only: the band now also answers a right-click with its context
    // menu, and egui's `dragged()` counts any button — an unfiltered check
    // would let a slipped right-press zoom (or, on the price gutter, flip)
    // the chart while swallowing the menu it was aiming for.
    if response.dragged_by(egui::PointerButton::Primary) {
        // Drag up → compress the span (a taller trace); down → expand it —
        // mirrored once the chart is upside down.
        let sense = if flips && view.is_inverted() {
            -1.0
        } else {
            1.0
        };
        let factor = f64::from(sense * response.drag_delta().y / AXIS_ZOOM_DRAG_PX).exp();
        if flips {
            view.drag_zoom(factor, auto);
        } else {
            view.zoom(factor, auto);
        }
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll.abs() > 0.0 {
            view.zoom(f64::from(-scroll / AXIS_ZOOM_SCROLL_PX).exp(), auto);
        }
    }
    response
}

/// Wheel travel that doubles or halves what a time axis shows.
///
/// One number for the candles, the lane and every pane body, so a scroll means
/// the same amount of zoom wherever the pointer happens to be resting. It was
/// already one number — written out four times.
const SCROLL_ZOOM_PX: f32 = 300.0;

/// How often the forming bar's footprint ladder is re-snapshotted for
/// drawing, in seconds. ~10 Hz: the eye reads the pattern, not the ticking
/// digits, and a layout that repaints per print reflows under the pointer.
const LIVE_LADDER_REFRESH_S: f64 = 0.1;

/// Pixels of drag on the lane's own time strip that double or halve its window.
///
/// Matches the candles' own feel: dragging the time axis zooms it by
/// `exp(dx / 120)`, so the two panes answer a drag at the same rate even
/// though they are zooming different things.
const LANE_ZOOM_DRAG_PX: f32 = 120.0;

/// Width of the jump-to-live chip on the time strip, in pixels.
const LIVE_CHIP_WIDTH_PX: f32 = 56.0;
/// Gap between the chip and the strip's right edge, in pixels.
const LIVE_CHIP_MARGIN_PX: f32 = 6.0;
/// Vertical inset of the chip inside the strip, in pixels.
const LIVE_CHIP_VPAD_PX: f32 = 3.0;

/// Where the jump-to-live chip sits (audit F6): right-aligned inside the
/// history segment of the time strip — the live end of the axis, which is
/// where the eye looks for the way back. One geometry for the input region
/// and the paint, so the click can never miss the pixels.
fn live_chip_rect(history_strip: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            history_strip.right() - LIVE_CHIP_MARGIN_PX - LIVE_CHIP_WIDTH_PX,
            history_strip.top() + LIVE_CHIP_VPAD_PX,
        ),
        egui::pos2(
            history_strip.right() - LIVE_CHIP_MARGIN_PX,
            history_strip.bottom() - LIVE_CHIP_VPAD_PX,
        ),
    )
}

/// Paint the jump-to-live chip: solid accent, dark ink — the chip language
/// of the price gutter, because this too is a statement about the axis.
/// Accent, not amber: it is a control, not a provenance statement.
fn draw_live_chip(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, egui::Rounding::same(3.0), theme::ACCENT);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "» live",
        egui::FontId::proportional(11.0),
        theme::CHIP_INK,
    );
}

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

/// Why an armed instance's region cannot honestly be tested right now, or
/// `None` when it can.
///
/// One rule, two readers: [`ChartPane::strategy_region`] shuts the gate on it
/// and [`ChartPane::badge_text_for`] prints it. Two copies would let the chart
/// paint a running bot over a region every bar is refused against — the
/// divergence a trader only discovers by watching a setup go by, which is
/// exactly how this was found.
///
/// The order is the order the trader can act on: another market needs the
/// region redrawn, a lost series needs the drawing re-anchored, a hidden one
/// needs a click.
fn region_pause(drawing: &drawings::Drawing, all_hidden: bool) -> Option<&'static str> {
    if drawing.foreign_market {
        return Some("region on another market — paused");
    }
    if drawing.off_series {
        return Some("region off its series — paused");
    }
    if drawing.hidden || all_hidden {
        return Some("region hidden — paused");
    }
    None
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
    /// A grab on one of the object's handles. `handle` indexes the tool's own
    /// handle list, which is the anchors for almost every tool but not for
    /// all of them — a channel's rail handles move anchors they do not sit
    /// on, so only the tool may turn this index into new geometry.
    Handle {
        drawing_index: usize,
        handle: usize,
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

/// What a pane resolved on *another* pane's shared marks this frame.
///
/// Said in market time and price, because those are the only coordinates two
/// panes of a tab agree on — a bar index means nothing across two cuts of the
/// same tape (`docs/ux/drawing-tools-2026-08.md` §D7). The pane that holds the
/// object turns them back into its own bar space, so the trader edits the one
/// object rather than a copy of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SharedEdit {
    /// Take the selection, without moving anything.
    Select(usize),
    /// Put one anchor at this instant and price.
    MoveAnchor {
        index: usize,
        anchor: usize,
        time_ms: i64,
        price: f64,
    },
    /// Shift every anchor of the object by this much time and price.
    Translate {
        index: usize,
        delta_ms: i64,
        delta_price: f64,
    },
}

/// A shared mark under the pointer, as the tab resolved it for one pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedPick {
    /// Which pane's store holds it, as a [`PaneIndex`].
    ///
    /// Explicit rather than "the other pane": with a context stack beside the
    /// flow pane there is more than one other, and a mark shared across three
    /// panes has two panes mirroring it. An edit that guessed its owner would
    /// land on whichever chart the guess named.
    pub owner: PaneIndex,
    /// Its index in the owning pane's store.
    pub index: usize,
    /// Which handle was grabbed, or `None` for the body.
    pub anchor: Option<usize>,
    /// Locked geometry refuses to move, exactly as it does on its own pane.
    pub locked: bool,
}

/// One drawing resolved under the pointer for an on-demand control capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlDrawingHit {
    pub id: drawings::DrawingId,
    pub tool_id: &'static str,
    pub label: String,
    pub user_label_present: bool,
    pub handle_index: Option<usize>,
    pub selected: bool,
    pub locked: bool,
}

/// One price a drawing declares for the price axis to tag.
///
/// Carries both the pixel and the price because the two answer different
/// questions and are read off one scale: the painter needs the height, and
/// anything reading the trader's levels as data needs the number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PriceAxisLevel {
    /// The object that declared it.
    pub id: drawings::DrawingId,
    /// Where it sits on the axis, in screen pixels.
    pub y: f32,
    /// What that height reads as on the pane's price scale.
    pub price: f64,
    /// The object's own colour — the tag is the object, said on the axis.
    pub color: egui::Color32,
}

/// What the price axis may not write a round number over this frame.
///
/// Two sources, kept apart because they are *stored* differently and not
/// because they mean different things: the chips the axis draws itself are a
/// pair that fits inline, and the levels are the list already gathered for
/// painting, borrowed rather than copied into a third container once a frame.
struct PriceAxisClaims<'a> {
    /// The pointer's tag and the last-price chip.
    marks: pointer_compass::AxisClaims,
    /// One per level a drawing declared.
    levels: &'a [PriceAxisLevel],
}

impl PriceAxisClaims<'_> {
    /// Every claimed height, from both sources, allocating nothing.
    fn heights(&self) -> impl Iterator<Item = f32> + '_ {
        self.marks
            .iter()
            .copied()
            .chain(self.levels.iter().map(|level| level.y))
    }
}

/// What the pointer's compass will draw this frame, and where.
///
/// One decision, read twice: the axes consult it before labelling themselves
/// so they can leave the coordinate alone, and the paint pass draws exactly
/// what it says.
struct PointerCompass {
    readout: pointer_compass::PointerReadout,
    /// The price half is drawn — its layer is on, the pointer is over the
    /// price band, and the crosshair is not already writing one.
    price: bool,
    /// The time half is drawn — its layer is on and a bar is under the
    /// pointer.
    time: bool,
}

/// Semantic meaning of the pointer over one chart pane.
///
/// Coordinates are kept internal here. `app::control` owns the transport DTO
/// and converts every float into a canonical decimal string, preventing UI
/// types and non-canonical JSON numbers from leaking onto the wire.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ControlPointerHit {
    pub screen_x_px: f32,
    pub screen_y_px: f32,
    pub band: String,
    pub axis_value: Option<f64>,
    pub axis_unit: String,
    pub slot: Option<usize>,
    pub bar: Option<quantick_engine::Bar>,
    pub flow_cell: Option<crate::orderflow_view::FlowCellHit>,
    pub drawing: Option<ControlDrawingHit>,
}

/// What a pane did to another pane's marks in one frame.
///
/// The gesture flags bracket the edits so a whole drag lands on the owning
/// store as one undo entry — the same coalescing a drag on the object's own
/// chart gets, because it is the same gesture on the same object.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SharedInteraction {
    /// The pane whose store this lands on.
    ///
    /// Carried rather than recomputed: a drag outlives the pick that started
    /// it — `commit_gesture` fires on release, by which time the pointer may
    /// be nowhere near the mark — so the owner is remembered with the gesture
    /// or it is lost exactly when it is needed.
    pub owner: Option<PaneIndex>,
    pub edit: Option<SharedEdit>,
    pub begin_gesture: bool,
    pub commit_gesture: bool,
}

/// What the pointer is doing this frame, as the shared-mark handler needs it.
struct SharedPointer {
    /// Where it is, wherever that is. The band below decides what counts.
    position: Option<egui::Pos2>,
    /// The band drawings live in on this pane, this frame.
    ///
    /// A press must land inside it. A drag already running is *clamped* into
    /// it instead, exactly as a drag on this pane's own marks is: the canvas
    /// narrows by the inspector's width on the very frame a press opens it
    /// (§D8), and a gesture that stopped whenever the pointer left the
    /// shrunken pane would die on the frame it was born.
    area: egui::Rect,
    /// Whether floating chrome — the inspector, the manager, a flyout — is
    /// under it right now.
    ///
    /// Read at press time only, like every other pointer path here:
    /// continuity, not priority. A drag that started on the canvas keeps
    /// running while the pointer crosses a panel, which matters most here of
    /// all — the press that selects a mark is what *opens* the inspector, and
    /// the inspector opens over the chart the mark is on.
    over_chrome: bool,
    pressed: bool,
    down: bool,
    released: bool,
    history_right: f32,
    total: usize,
    magnet: bool,
}

/// Where a pane sits in its tab: `0` is the flow pane, `1..` the context
/// stack, top to bottom.
///
/// An address, never a position on screen — the context stack is drawn left of
/// the flow pane, and a reader who took this for a left-to-right order would
/// mirror every edit.
pub type PaneIndex = usize;

/// A gesture a pane is running on another pane's mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SharedDrag {
    #[default]
    None,
    /// Dragging the whole object.
    Body { index: usize },
    /// Dragging one handle.
    Anchor { index: usize, anchor: usize },
    /// The mark is locked: the gesture is ours (the chart must not pan under
    /// it) but the geometry stays exactly where the trader left it.
    Blocked,
}

impl SharedDrag {
    const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Which handle of a projected object `pos` grabs, if any.
///
/// The one handle rule, shared by a pane's own marks and the mirrored ones,
/// so the two can never disagree about what a press landed on.
fn anchor_hit(points: &[egui::Pos2], pos: egui::Pos2) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| (index, point.distance_sq(pos)))
        .filter(|(_, distance_sq)| {
            *distance_sq <= DRAWING_ANCHOR_RADIUS_PX * DRAWING_ANCHOR_RADIUS_PX
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
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
    /// Raised when a tool whose content is words was just placed, so the
    /// host puts the caret in the object it just made.
    ///
    /// Selecting a drawing raises the context bar, which is everything a
    /// note needs *except* somewhere to type. This is that somewhere, and it
    /// is on the chart: a note typed in a panel is read with the eye crossing
    /// the screen between keystrokes, and until the first word lands the
    /// object under the pointer just says "Note" in grey.
    pub begin_text_edit: &'a mut bool,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// The symbol to name while the series is still empty.
    pub symbol: &'a str,
    /// The tab's paper-trading simulator. Both panes *draw* its lines — the
    /// same instrument at the same prices — while only one *handles* them.
    pub paper: &'a mut PaperTrading,
    /// Whether this pane is the one paper trading takes its pointer from.
    ///
    /// Whether this pane's pointer drives order entry this frame.
    ///
    /// True on the pane the pointer is *in* — every visible pane is a
    /// trading surface, and a level is as true on a context chart as on the
    /// flow chart, so holding the buy modifier over any of them aims there.
    /// While a paper line is being dragged it stays with the pane the drag
    /// started in: the grabbed price must not jump to a different scale
    /// halfway through the gesture.
    pub paper_takes_input: bool,
    /// Whether the position HUD anchors on this pane. Follows *focus*, not
    /// the pointer: there is one HUD, it must not flicker between panes as
    /// the hand crosses them, and focus is the app's existing answer to
    /// "which pane is the trader working in".
    pub paper_hud_here: bool,
    /// What the pointer grabs among the *other* pane's shared marks, and
    /// whether that mark is locked.
    ///
    /// Resolved by the tab before the panes are borrowed one at a time, since
    /// answering it needs both panes at once. `None` on an unsplit tab, and
    /// on any tab where nothing is shared.
    pub shared_pick: Option<SharedPick>,
    /// What this pane did to a shared mark, for the tab to apply to the pane
    /// that owns it.
    pub shared: SharedInteraction,
    /// Stretches of market time this tab's tape does not cover, left by a
    /// reconnect that kept the timeline (see [`crate::feed::FeedGap`]).
    ///
    /// Passed per frame rather than held per pane: the holes belong to the
    /// tab's one tape, and every pane cuts its own bars from that same tape.
    /// A copy per pane would be the same list written twice, and two lists
    /// that can disagree about where the market went quiet is exactly the
    /// class of bug the honesty rule exists to prevent.
    pub feed_gaps: &'a [crate::feed::FeedGap],
    /// What the running source can actually produce. The layer menu offers a
    /// layer this feed has no data for as disabled-with-a-reason rather than as
    /// a switch that would do nothing — the wording the toolbar already uses.
    pub capabilities: FeedCapabilities,
    /// Whether the running feed *infers* the aggressor side (MT5 tick rule,
    /// or a replay of such a session) instead of the venue reporting it. The
    /// footprint layer's content is entirely buyer-vs-seller, so it carries
    /// this label in its own legend — the status bar's note is not enough
    /// there (data honesty).
    pub side_inferred: bool,
    /// The window's footprint setup — the last one the trader used anywhere
    /// (env > `config/footprint.toml` > saved edits > defaults). A chart
    /// configured on its own overrides it; see
    /// [`ChartPane::footprint_config`].
    pub footprint: &'a crate::footprint_config::FootprintConfig,
    /// Where the layer menu leaves the two switches the pane does not own.
    /// Drained by the app once the canvas is done (see [`LayerActions`]).
    pub layers: &'a mut LayerActions,
}

/// Which side of the candles a drawing pass paints on.
///
/// One function serves both, taking this rather than being copied: the
/// projection, the band filter, the off-series fade and the style resolution
/// are the same work whichever side is being drawn, and a second copy of them
/// would drift on the first change to any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawPass {
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
    pagination_revision: u64,
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
    /// Whether the app has put the active layout on this pane — its
    /// indicators and its market's drawings. `false` from construction until
    /// the first frame that sees the pane, so a pane opened by any path (a
    /// new tab, a split, a restore) is seeded exactly once.
    pub layout_seeded: bool,
    /// Which of the workspace's layouts this pane shows — its indicator set
    /// and the drawings it keeps. `None` until the app seeds the pane, when
    /// it takes the focused pane's layout (or the book's default); a restored
    /// workspace sets it before seeding. Per pane, because two charts side by
    /// side are two readings of one market, and a CVD on one is not a CVD the
    /// other asked for.
    pub layout: Option<crate::layouts::LayoutId>,
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
    /// Whether the user wants the live strip shown. The pixels it actually
    /// gets are still capability-gated — see [`Self::live_strip_width`].
    pub live_strip_visible: bool,
    /// Whether the candle footprint layer is on. What a fresh launch opens
    /// with is `config/chart-layers.toml`, not this initialiser — see
    /// [`crate::chart_layers`]; the ladder still follows the zoom's LOD, so it
    /// draws nothing where the candle is too narrow to read.
    pub footprint_visible: bool,
    /// This chart's own footprint setup, once it has been configured here.
    ///
    /// `None` — the default — means "follow the window's last setup", which
    /// keeps the common case (one chart, one taste) behaving like a global
    /// setting. A split layout is two readings of the same market (a 90-day
    /// context chart beside a 50-tick flow chart) and one set of thresholds
    /// cannot serve both, so the moment a chart is configured on its own it
    /// keeps its own.
    pub footprint_override: Option<crate::footprint_config::FootprintConfig>,
    /// The footprint's sticky detail level (hysteresis on zoom-out).
    footprint_lod: crate::footprint_render::FootprintLod,
    /// The forming bar's ladder as last snapshotted for drawing, with the
    /// frame time it was taken and the slot it belongs to. Refreshed at
    /// ~10 Hz, not per print — the eye reads patterns, and a frozen layout
    /// cannot reflow under the pointer — but *immediately* when the slot
    /// changes: at a bar close the previous bar's ladder must never linger
    /// on the new bar, not even for one throttle interval.
    footprint_live: Option<(f64, usize, quantick_engine::BarFootprint)>,
    /// Bumped whenever [`Self::footprint_live`] is re-taken or cleared — the
    /// cache key the range-profile drawings use to notice the live edge
    /// moved, so they re-fold at the snapshot cadence, never per paint.
    footprint_live_version: u64,

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
    pub imbalance_unit: ImbalanceUnit,

    // Pan/zoom navigation over the bar series. It owns the history pane only:
    // the live lane is a band of screen to its right that answers to nothing
    // it does.
    pub viewport: Viewport,
    // Where the history pane ended last frame — the lane's divider, and the
    // handle that resizes it. The input pass runs before the draw computes it.
    pub last_lane_divider_x: Option<f32>,
    // The canvas the last draw used. Published for the same reason the divider
    // is: something outside the draw needs a point on this pane — the scripted
    // right-click of `QUANTICK_CONTEXT_MENU` — and computing the geometry a
    // second time is how two answers start to disagree.
    pub last_chart_rect: Option<egui::Rect>,
    // The whole rect this pane was last painted into, gutters and time strip
    // included — and set whether or not the pane had anything to draw, which
    // is what separates it from `last_chart_rect`. The feed's one-line
    // offline note is placed against it: an explanation belongs on the pane
    // with nothing in it, which is precisely the pane that has room for one.
    pub last_area: Option<egui::Rect>,
    // The price gutter of the last draw, published for the same reason: the
    // scripted right-click of `QUANTICK_CONTEXT_MENU=axis` needs a point that
    // is really on the axis, not a guess about where the gutter probably is.
    pub last_price_gutter: Option<egui::Rect>,
    // The candles' segment of the bottom time strip, published for the same
    // reason again: `QUANTICK_CONTEXT_MENU=time` needs a point that is really
    // on the time axis, and past the lane divider the strip belongs to the
    // tape's own window rather than to this menu.
    pub last_time_strip: Option<egui::Rect>,
    // The price-axis levels this frame's drawings declare. Per-frame by
    // nature, reused as a container for the same reason the band carve is —
    // and gathered before the axis labels itself, because the axis stands
    // aside where one of these is going to land.
    price_axis_levels: Vec<PriceAxisLevel>,
    // The automatic tape window at the last draw — the recent bars' typical
    // duration. Only the menu reads it, and only to state what "follows the
    // bars" currently amounts to; the drawing itself is handed the resolved
    // window, never this.
    last_lane_reference_ms: Option<i64>,
    // Whether the right-click that opened the menu landed on the tape rather
    // than on the candles. The two panes are configured apart, so the menu has
    // to know which one was asked.
    context_menu_on_tape: bool,
    // How many rungs the lane was wide enough for at the last draw, and so
    // how finely the next publish samples the forming bar across it. `0` when
    // there is no lane: the worker then walks no ladder and the panes draw
    // nothing on the tape, which is the whole cost of this feature on a chart
    // that has no tape to draw on.
    lane_rungs: usize,
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
    /// The bands the last [`Self::draw_chart`] painted, kept for the passes
    /// that run outside it: the tab's shared-drawing projection and the
    /// inspector's "which band is this on". Reused every frame rather than
    /// rebuilt, so the draw pass allocates no container.
    last_bands: Bands,
    /// The price band's label, shared into every carve instead of cloned.
    price_band_label: std::sync::Arc<str>,
    // The raw canvas area the last frame split into chart, panes and gutters.
    // Kept so a caller that needs a band it does not otherwise see — the pane
    // axis tests aiming a drag at a pane's own gutter — asks `plot_split` for
    // it rather than re-deriving the layout and drifting from it.
    pub last_plot_area: Option<egui::Rect>,
    // Pointer position over the plot this frame, for the crosshair.
    pub hover_pos: Option<egui::Pos2>,
    /// Whether the pointer is over the tape switch in the canvas's top-right
    /// corner. Read by the paint pass, which runs after the input pass and has
    /// no `Ui` of its own to ask.
    tape_switch_hovered: bool,

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
    paper_hud_anchor: Option<(egui::Rect, PriceScale)>,
    /// Price under the right-click that opened the layer menu — the trade
    /// section's anchor. Refreshed by every secondary click on the canvas.
    context_menu_price: Option<f64>,
    /// The placing entries of the last right-click: each registry tool that
    /// declares a `context_menu_label`, with the chart point *its own*
    /// `anchor_snap` resolved for that click — so the menu never re-derives
    /// a projection and a new tool's snap rule needs no edit here.
    context_menu_places: Vec<(drawings::DrawingTool, ChartPoint)>,
    /// The drawing under the last right-click, resolved at press time like
    /// the price and the tape flag. Held as an id, not an index: the menu
    /// stays open across frames, and an index can go stale under it.
    /// `pub(crate)` so the menu tests can stage the click's outcome.
    pub(crate) context_menu_drawing: Option<drawings::DrawingId>,
    /// Rename buffer for the layer menu's drawing section, seeded from the
    /// clicked object's current name on the press that opened the menu.
    context_menu_rename: String,
    /// Test-only trace of the drawing section's widgets, the
    /// `layer_menu_rects` idiom: label → rect, rebuilt per menu frame.
    #[cfg(test)]
    pub drawing_menu_rects: Vec<(&'static str, egui::Rect)>,
    /// Armed strategy instances riding this pane's drawings. The kernel
    /// (`quantick-strategy`) judges; this pane only anchors and paints.
    pub strategies: crate::strategy_anchors::StrategyAnchors,
    /// Closed bars awaiting strategy evaluation, each with the slot it
    /// closed at. Pushed by `ingest_live_trade` only while instances
    /// exist, drained by the tab in the same ingestion sweep — the slot
    /// and the drawings' anchors are therefore read against one cut of
    /// the series.
    strategy_pending: Vec<(quantick_engine::Bar, usize)>,
    /// The drawing whose "Add strategy…" was clicked; the app drains it
    /// and opens the arming dialog over this pane.
    pub(crate) strategy_popup_request: Option<drawings::DrawingId>,
    /// Simulator commands the drawing menu owes the paper host — cancelling
    /// a resting retest limit on disarm/removal. The pane cannot reach the
    /// tab's simulator from inside the menu, so the tab drains this on the
    /// same frame ([`crate::tab::TabState::apply_strategy_cleanup`]).
    strategy_cleanup: Vec<quantick_sim::Command>,

    /// User drawings live entirely in the app overlay layer, never in market
    /// state, so chart/backtest/bot determinism stays untouched.
    pub drawings: Drawings,
    // Drawing placement/movement state. Anchors are chart coordinates; only
    // the current hover and press position are transient pixels.
    pub drawing_hover: Option<ChartPoint>,
    /// The object whose *content* is being edited off-canvas right now —
    /// the on-chart note editor's subject. Told to the pane by the host each
    /// frame, because the editor is chrome and lives above the canvas; the
    /// object it holds the words for must not paint them twice.
    pub content_editing: Option<usize>,
    /// The band the next anchor would land in, as the input pass resolved it.
    /// The draw pass puts the accent hairline on its top edge — one band at a
    /// time, and none at all when no tool is armed.
    drawing_band_hint: Option<egui::Rect>,
    pub drawing_press_position: Option<egui::Pos2>,
    pub drawing_press_started_empty: bool,
    /// The hand a run has when nobody is at the mouse — the
    /// `QUANTICK_DRAWING_DRAFT` harness hook. `None` for every real session.
    ///
    /// The live preview of a half-placed object is the whole feedback of a
    /// multi-anchor gesture, and it is the one surface a click-free launch
    /// could not reach: it exists only between two clicks, and only while a
    /// pointer is over the chart. Both halves are read exactly where the real
    /// pointer and the real modifier are read and nowhere else, so everything
    /// downstream — the tool's shaping, the hint chip, the rubber band — runs
    /// the same code a hand runs.
    pub parked_hand: Option<ParkedHand>,
    /// The last screen position a freehand stroke actually recorded, so the
    /// capture decimates as it goes rather than storing every mouse event.
    freehand_last_position: Option<egui::Pos2>,
    /// What the press resolved under the Pointer tool, held until the click
    /// it belongs to completes. `Some(None)` is a real answer — a press on
    /// empty canvas, the one that deselects.
    ///
    /// It exists because the canvas is not the same shape before and after a
    /// selection: the pinned inspector is a side panel laid out *before* the
    /// central panel, so the frame a selection appears is the frame the chart
    /// narrows by the panel's width and every drawing slides left with it.
    /// Re-hit-testing on the release would be asking a different chart.
    pub drawing_press_pick: Option<Option<usize>>,
    /// Where a move/resize gesture pressed, while it is still under the drag
    /// threshold. `None` once the threshold is passed — from then on the
    /// object follows the pointer for the rest of the gesture.
    ///
    /// Without it, one pixel of hand tremor during a *click* re-angles a
    /// channel or shifts a level the trader placed deliberately, and records
    /// it as an undo step. Placement already refused to turn a twitch into a
    /// drag (`DRAWING_DRAG_THRESHOLD_PX`); moving now refuses too.
    pub drawing_drag_pending_from: Option<egui::Pos2>,
    pub drawing_drag: DrawingDrag,
    /// A gesture this pane is running on a mark the other pane holds, and the
    /// two pieces of pointer state it needs: where the press landed while the
    /// drag threshold is still unmet, and the market instant and price the
    /// pointer was last over — what a body drag sends its deltas against.
    shared_drag: SharedDrag,
    /// The pane whose mark [`Self::shared_drag`] is moving, for as long as it
    /// is moving it.
    shared_drag_owner: Option<PaneIndex>,
    shared_drag_pending_from: Option<egui::Pos2>,
    shared_pointer_mark: Option<(i64, f64)>,
    /// A re-anchor owed to the drawings, holding the slot count of the series
    /// they were last anchored to.
    ///
    /// A reset empties the pane, and an empty series cannot say where an
    /// instant lands — so the answer is deferred to the first frame that has
    /// bars again, rather than clamping every mark onto a series that is not
    /// there yet.
    pending_reanchor: Option<usize>,
    /// The pane opened by a click on its own collapsed strip, carried to the
    /// frame that may hold the second half of a double click.
    ///
    /// A collapsed strip changes shape the instant it is clicked, so a gesture
    /// made *of* two clicks cannot be read from one frame's geometry. This is
    /// the one piece of state that spans them.
    strip_expanded: Option<SlotId>,
    /// An indicator whose settings a gesture on this pane asked for, waiting
    /// for the app to open the dialog.
    ///
    /// Parked rather than acted on: the dialog is the app's — one dialog for
    /// the whole window — and the gestures that ask for it are read deep inside
    /// this pane's input pass, holding borrows the app's state cannot cross.
    /// The same shape `pending_spec` uses for the other direction.
    pending_settings: Option<SlotId>,
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
        // `bars → time` opens on a real timeframe, not a one-second chart
        // (audit QW2): the same 1m the split's time pane opens on.
        let mut time_interval_ms = crate::time_header::DEFAULT_INTERVAL_MS;
        let mut imbalance_target = 100;
        let mut imbalance_unit = ImbalanceUnit::Trades;
        match &spec {
            BarSpec::Tick(n) => tick_n = *n,
            BarSpec::Volume(u) => volume_units = u.to_f64().unwrap_or(volume_units),
            BarSpec::Dollar(d) => dollar_notional = d.to_f64().unwrap_or(dollar_notional),
            BarSpec::Time(ms) => time_interval_ms = *ms,
            BarSpec::Imbalance(unit, target) => {
                imbalance_unit = *unit;
                imbalance_target = *target;
            }
        }

        Self {
            id,
            kind: spec.kind(),
            state: ChartState::new(spec),
            pagination_revision: 0,
            orderflow,
            indicator_worker: IndicatorWorker::spawn(),
            indicators: IndicatorViews::new(),
            layout_seeded: false,
            layout: None,
            layout_label: String::new(),
            drawings_key: None,
            drawings_saved_revision: 0,
            legend_collapsed: false,
            live_strip_visible: false,
            footprint_visible: false,
            footprint_override: None,
            footprint_lod: crate::footprint_render::FootprintLod::default(),
            footprint_live: None,
            footprint_live_version: 0,
            // The backfill divider opens off: it is a full-height rule across
            // the candles for a boundary that matters once, when reading how
            // far the live tape goes back. Nothing is hidden about the data —
            // the mark is one click away in the layer menu, and the bars
            // either side of it are exactly what they were.
            hidden_layers: BTreeSet::from([ChartLayer::BackfillDivider]),
            #[cfg(test)]
            layer_menu_rects: Vec::new(),
            pending_spec: None,
            tick_n,
            volume_units,
            dollar_notional,
            time_interval_ms,
            imbalance_target,
            imbalance_unit,
            viewport: Viewport::new(),
            last_lane_divider_x: None,
            last_chart_rect: None,
            last_area: None,
            last_price_gutter: None,
            last_time_strip: None,
            price_axis_levels: Vec::new(),
            last_lane_reference_ms: None,
            context_menu_on_tape: false,
            lane_rungs: 0,
            price_view: PriceView::new(),
            last_auto_range: None,
            last_chart_height: 1.0,
            last_chart_top: 0.0,
            last_chart_area: None,
            last_bands: Bands::new(),
            price_band_label: std::sync::Arc::from(bands::PRICE_BAND_LABEL),
            last_plot_area: None,
            hover_pos: None,
            tape_switch_hovered: false,
            history_prefix: Vec::new(),
            paper_hud_anchor: None,
            context_menu_price: None,
            context_menu_places: Vec::new(),
            context_menu_drawing: None,
            context_menu_rename: String::new(),
            #[cfg(test)]
            drawing_menu_rects: Vec::new(),
            strategies: crate::strategy_anchors::StrategyAnchors::default(),
            strategy_pending: Vec::new(),
            strategy_popup_request: None,
            strategy_cleanup: Vec::new(),
            drawings: Drawings::default(),
            drawing_hover: None,
            content_editing: None,
            drawing_band_hint: None,
            drawing_press_position: None,
            drawing_press_started_empty: false,
            parked_hand: None,
            freehand_last_position: None,
            drawing_press_pick: None,
            drawing_drag_pending_from: None,
            drawing_drag: DrawingDrag::None,
            shared_drag: SharedDrag::None,
            shared_drag_owner: None,
            shared_drag_pending_from: None,
            shared_pointer_mark: None,
            pending_reanchor: None,
            strip_expanded: None,
            pending_settings: None,
        }
    }

    /// Whether any placed or in-flight drawing is a fixed-range volume
    /// profile — the second consumer of the footprint ladders, keeping
    /// accumulation on while the layer itself is hidden. O(drawings), once
    /// per frame, never on the ingestion path.
    fn wants_range_profile(&self) -> bool {
        self.drawings
            .items()
            .iter()
            .map(|drawing| drawing.tool)
            .chain(self.drawings.draft().map(|draft| draft.tool))
            .any(|tool| tool.id() == crate::frvp::TOOL_ID)
    }

    /// The footprint setup this chart draws with: its own once configured
    /// here, else the window's last one. See [`Self::footprint_override`].
    pub fn footprint_config<'a>(
        &'a self,
        window: &'a crate::footprint_config::FootprintConfig,
    ) -> &'a crate::footprint_config::FootprintConfig {
        self.footprint_override.as_ref().unwrap_or(window)
    }

    /// Put this pane on `spec` outright, selectors included.
    ///
    /// Startup-scoped: the caller is a workspace restoring the bar rule this
    /// pane was last read on, into a pane that has not drawn a frame yet. A
    /// live change goes through `pending_spec` instead, so the frame carrying
    /// it paints the loading overlay before the rebuild replays the tape —
    /// there is no tape to replay here, and nothing to paint over.
    ///
    /// The selectors move with the spec, because the BARS group reads *them*:
    /// setting the state alone would restore a chart whose own controls
    /// disagreed with it, and the trader's first touch of the parameter would
    /// snap the chart back to a rule they never chose.
    pub fn set_spec(&mut self, spec: BarSpec) {
        let changed = self.state.spec() != &spec;
        self.kind = spec.kind();
        match &spec {
            BarSpec::Tick(n) => self.tick_n = *n,
            BarSpec::Volume(units) => {
                self.volume_units = units.to_f64().unwrap_or(self.volume_units);
            }
            BarSpec::Dollar(notional) => {
                self.dollar_notional = notional.to_f64().unwrap_or(self.dollar_notional);
            }
            BarSpec::Time(ms) => self.time_interval_ms = *ms,
            BarSpec::Imbalance(unit, target) => {
                self.imbalance_unit = *unit;
                self.imbalance_target = *target;
            }
        }
        self.state.set_spec(spec);
        if changed {
            self.bump_pagination_revision();
        }
    }

    /// Revision protecting the closed-bar prefix exposed through paginated
    /// chart-window reads. Live appends do not advance it; rewrites do.
    #[must_use]
    pub fn pagination_revision(&self) -> u64 {
        self.pagination_revision
    }

    fn bump_pagination_revision(&mut self) {
        self.pagination_revision = self.pagination_revision.saturating_add(1);
    }

    #[cfg(test)]
    /// Give this chart its own footprint setup, the way the settings window
    /// does when a knob moves on it.
    pub fn set_footprint_override(
        &mut self,
        config: Option<crate::footprint_config::FootprintConfig>,
    ) {
        self.footprint_override = config;
    }

    /// An egui interaction id scoped to this pane.
    fn interaction_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.id))
    }

    /// Whether `layer` is painted on this pane right now.
    ///
    /// Every arm reads the one field that already owns that layer, so the menu
    /// and the toolbar/dock can never disagree about a pixel. `style` is the
    /// window's, passed in because the grid lives there and a pane holding a
    /// copy of it is exactly the disagreement this avoids.
    ///
    /// A layer this pane has no machinery for reports hidden: a time pane runs
    /// no tape (§11), so it has no heatmap to show.
    pub fn layer_visible(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        let tape = self.orderflow.as_ref();
        match layer {
            // The tape's three report the *switch*, not what survives the tape
            // being off: a trader who takes the band away and puts it back gets
            // the tape they had, and the state file records the same.
            ChartLayer::TapeChart => tape.is_some_and(OrderflowView::lane_enabled),
            ChartLayer::TapeHeatmap => tape.is_some_and(OrderflowView::lane_depth_visible),
            ChartLayer::TapeBubbles => tape.is_some_and(OrderflowView::lane_bubbles_enabled),
            ChartLayer::Heatmap => tape.is_some_and(OrderflowView::depth_visible),
            ChartLayer::Bubbles => tape.is_some_and(OrderflowView::bubbles_enabled),
            // Footprint reads the pane's own retained trades, not the tape
            // machinery, so it works on flow and time panes alike.
            ChartLayer::Footprint => self.footprint_visible,
            ChartLayer::LiveStrip => self.orderflow.is_some() && self.live_strip_visible,
            ChartLayer::LaneMarks => tape.is_some_and(OrderflowView::lane_marks_visible),
            ChartLayer::FlowLegend => tape.is_some_and(OrderflowView::legend_visible),
            ChartLayer::BookStatus => tape.is_some_and(OrderflowView::status_badge_visible),
            ChartLayer::DepthGaps => tape.is_some_and(OrderflowView::gaps_visible),
            ChartLayer::Grid => style.canvas.grid_enabled,
            // The toolbox's global eye already owns this one, undo history and
            // all; the menu is a second door to the same switch.
            ChartLayer::Drawings => !self.drawings.all_hidden(),
            ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PointerPrice
            | ChartLayer::PointerTime
            | ChartLayer::PaperTrading
            | ChartLayer::TradePaint => !self.hidden_layers.contains(&layer),
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
            ChartLayer::TapeChart => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_enabled(visible);
                }
            }
            ChartLayer::TapeHeatmap => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_depth_visible(visible);
                }
            }
            ChartLayer::TapeBubbles => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_bubbles_enabled(visible);
                }
            }
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
            ChartLayer::Footprint => self.footprint_visible = visible,
            ChartLayer::LiveStrip => self.live_strip_visible = visible,
            ChartLayer::LaneMarks => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_marks_visible(visible);
                }
            }
            ChartLayer::FlowLegend => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_legend_visible(visible);
                }
            }
            ChartLayer::BookStatus => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_status_badge_visible(visible);
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
            | ChartLayer::PointerPrice
            | ChartLayer::PointerTime
            | ChartLayer::PaperTrading
            | ChartLayer::TradePaint => {
                if visible {
                    self.hidden_layers.remove(&layer);
                } else {
                    self.hidden_layers.insert(layer);
                }
            }
        }
    }

    /// Whether a surface that is neither the depth map nor the bubbles needs
    /// the order-flow projection this frame.
    ///
    /// Two do: the live strip draws the same clusters the bubbles would, and
    /// the lane's marks need the frame's live edge. Without this, switching
    /// the bubbles off blanked the strip, and switching every flow layer off
    /// left the lane reserved but unmarked — a band indistinguishable from a
    /// dead feed, while its menu entry still read as on.
    fn projection_demand(&self) -> bool {
        self.live_strip_visible
            || self
                .orderflow
                .as_ref()
                .is_some_and(OrderflowView::lane_marks_visible)
    }

    /// Whether this pane draws `layer` at all, whatever the source can produce.
    ///
    /// §11 keeps the tape and everything read off it on the flow pane, so a
    /// time pane has no machinery for those five and never will.
    fn draws_layer(&self, layer: ChartLayer) -> bool {
        self.orderflow.is_some()
            || !(layer.on_tape()
                || matches!(
                    layer,
                    ChartLayer::Heatmap
                        | ChartLayer::Bubbles
                        | ChartLayer::LiveStrip
                        | ChartLayer::LaneMarks
                        | ChartLayer::FlowLegend
                        | ChartLayer::BookStatus
                        | ChartLayer::DepthGaps
                ))
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
    ) -> Option<LayerBlock> {
        if !self.draws_layer(layer) {
            return Some(blocks::WRONG_PANE);
        }
        // A layer of a tape that is not on the canvas: the switch is real and
        // remembered, but ticking it now would draw nothing. Offered disabled
        // with the reason, the same way the status badge refuses while the map
        // it reports on is hidden.
        if layer.on_tape()
            && layer != ChartLayer::TapeChart
            && !self
                .orderflow
                .as_ref()
                .is_some_and(OrderflowView::lane_enabled)
        {
            return Some(blocks::TAPE_OFF);
        }
        match layer {
            ChartLayer::TapeHeatmap => (!capabilities.book_capture).then_some(blocks::NO_BOOK),
            ChartLayer::TapeBubbles => {
                (!capabilities.traded_volume).then_some(blocks::NO_TRADED_VOLUME)
            }
            ChartLayer::Heatmap | ChartLayer::DepthGaps => {
                (!capabilities.book_capture).then_some(blocks::NO_BOOK)
            }
            // The strip draws the book and the aggressions landing into it, so
            // it takes either one and is empty only without both. Disabled
            // with the reason rather than offered as a switch that would
            // reserve width for a blank band.
            ChartLayer::LiveStrip => (!capabilities.book_capture && !capabilities.traded_volume)
                .then_some(blocks::NO_BOOK_AND_NO_VOLUME),
            // The badge reports on the book feed, so a source with no book has
            // nothing for it to say — and it is drawn with the map, so while
            // the map is hidden the switch would tick a box that draws
            // nothing. Offered disabled with the reason instead.
            ChartLayer::BookStatus => {
                if capabilities.book_capture {
                    (!self
                        .orderflow
                        .as_ref()
                        .is_some_and(OrderflowView::depth_visible))
                    .then_some(blocks::DEPTH_MAP_HIDDEN)
                } else {
                    Some(blocks::NO_BOOK)
                }
            }
            // The footprint is the buy/sell split per price: on a source that
            // prints no traded volume every cell would be an identical
            // synthetic unit — the same reason the bubbles refuse.
            ChartLayer::Bubbles | ChartLayer::Footprint => {
                (!capabilities.traded_volume).then_some(blocks::NO_TRADED_VOLUME)
            }
            _ => None,
        }
    }

    /// The same layers as [`Self::layer_visible`], reporting the *switch*
    /// rather than what the source lets through it.
    ///
    /// Only the two depth layers differ, and only because their "is it drawn"
    /// answer folds in book capture: on a source with no book they read
    /// undrawn however the switch stands. That is the right answer for a
    /// renderer and the wrong one for a file — persisting it would record a
    /// capability as the trader's choice, and their file outranks the shipped
    /// default on every market from then on, including the ones that do have a
    /// book. The setters already compare against the switch for this exact
    /// reason (`OrderflowView::set_depth_visible`); this is the reading half.
    pub fn layer_switched_on(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        let tape = self.orderflow.as_ref();
        match layer {
            ChartLayer::Heatmap => tape.is_some_and(OrderflowView::depth_switched_on),
            ChartLayer::TapeHeatmap => tape.is_some_and(OrderflowView::lane_depth_switched_on),
            other => self.layer_visible(other, style),
        }
    }

    /// Every layer this pane persists, and whether it is switched on.
    ///
    /// `style` comes from the window for the same reason it does in
    /// [`Self::layer_visible`].
    pub fn layer_states(&self, style: &ChartStyle) -> std::collections::BTreeMap<ChartLayer, bool> {
        ChartLayer::ALL
            .into_iter()
            .filter(|layer| layer.persisted())
            .map(|layer| (layer, self.layer_switched_on(layer, style)))
            .collect()
    }

    /// The same visibility as one bit per persisted layer, for change
    /// detection.
    ///
    /// The bit is the layer's index in `ALL`, which is now sixteen entries —
    /// the last bit `u32` has room for after this widening, and the reason the
    /// accumulator is not `u16` any more: a seventeenth layer would have
    /// shifted by 16, panicking in debug and silently colliding in release.
    /// The assertion below fails the build rather than the chart.
    pub fn layer_mask(&self, style: &ChartStyle) -> u32 {
        ChartLayer::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, layer)| layer.persisted() && self.layer_switched_on(*layer, style))
            .fold(0_u32, |mask, (bit, _)| mask | (1 << bit))
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
        if !self.layer_visible(layer, chrome.style) {
            self.set_layer_visible(layer, true, chrome.layers);
        }
    }

    /// The canvas right-click menu: one entry per chart layer, then one per
    /// indicator on this pane.
    ///
    /// The indicator entries drive `IndicatorViews::toggle_hidden` — the same
    /// state the toolbar's eye writes — so an indicator hidden here shows as
    /// hidden there, and the indicator state file remains its single home.
    /// Aim the next menu at one pane or the other, as a right-click would.
    #[cfg(test)]
    pub(crate) fn aim_context_menu_at_tape(&mut self, on_tape: bool) {
        self.context_menu_on_tape = on_tape;
    }

    /// Whether a click at this x belongs to the tape rather than the candles.
    ///
    /// Read off the divider the draw already published, never a second copy of
    /// the lane's geometry — the two could then disagree, and the menu would
    /// configure a pane the trader did not click. A canvas with no lane has no
    /// divider, and every click on it is the candles'.
    #[must_use]
    fn click_on_tape(&self, x: f32) -> bool {
        self.last_lane_divider_x.is_some_and(|divider| x >= divider)
    }

    /// The tape switch's click, in the input pass.
    ///
    /// Drawn by [`Self::draw_tape_switch`] in the pass after this one, off the
    /// same [`tape_switch_rect`]. Nothing is registered on a pane with no tape
    /// machinery (§11: a time pane has none), so no chip appears there to
    /// promise a band that canvas will never draw.
    fn handle_tape_switch(
        &mut self,
        ui: &egui::Ui,
        chart_rect: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.tape_switch_hovered = false;
        if self.orderflow.is_none() {
            return;
        }
        let on = self.layer_visible(ChartLayer::TapeChart, chrome.style);
        let response = ui.interact(
            tape_switch_rect(chart_rect),
            self.interaction_id("tape_switch"),
            egui::Sense::click(),
        );
        self.tape_switch_hovered = response.hovered();
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            // The chip is chrome on top of the canvas. A crosshair chasing the
            // pointer underneath it would say the chart is being hovered while
            // the pointer is reading a button.
            self.hover_pos = None;
        }
        let clicked = response.clicked();
        // `on_hover_ui` over `on_hover_text`: the closure runs only while the
        // pointer is actually on the chip, so the wording costs nothing on the
        // frames nobody is hovering — and this is a per-frame path.
        response.on_hover_ui(|ui| {
            ui.label(if on {
                "the tape is on — click to take it off the canvas"
            } else {
                "the tape is off — click to put it back"
            });
            ui.label(
                egui::RichText::new(ChartLayer::TapeChart.hint())
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
        });
        if clicked {
            self.set_layer_visible(ChartLayer::TapeChart, !on, chrome.layers);
        }
    }

    /// Paint the tape switch: a chip in the canvas's top-right corner, lit
    /// while the tape is on the canvas and muted while it is not.
    fn draw_tape_switch(&self, painter: &egui::Painter, chart_rect: egui::Rect) {
        let Some(tape) = self.orderflow.as_ref() else {
            return;
        };
        let on = tape.lane_enabled();
        let rect = tape_switch_rect(chart_rect);
        let accent = if on { theme::ACCENT } else { theme::TEXT_MUTED };
        let rounding = egui::Rounding::same(TAPE_SWITCH_ROUNDING_PX);
        painter.rect_filled(
            rect,
            rounding,
            egui::Color32::from_black_alpha(if self.tape_switch_hovered {
                TAPE_SWITCH_HOVER_FILL_ALPHA
            } else {
                TAPE_SWITCH_FILL_ALPHA
            }),
        );
        if self.tape_switch_hovered {
            painter.rect_stroke(
                rect,
                rounding,
                egui::Stroke::new(
                    TAPE_SWITCH_STROKE_PX,
                    accent.gamma_multiply(TAPE_SWITCH_HOVER_STROKE_ALPHA),
                ),
            );
        }
        // A filled dot for on, a ring for off: the state survives a screenshot
        // read in greyscale, which colour alone would not.
        let dot = egui::pos2(rect.left() + TAPE_SWITCH_DOT_X_PX, rect.center().y);
        if on {
            painter.circle_filled(dot, TAPE_SWITCH_DOT_RADIUS_PX, accent);
        } else {
            painter.circle_stroke(
                dot,
                TAPE_SWITCH_DOT_RADIUS_PX,
                egui::Stroke::new(TAPE_SWITCH_STROKE_PX, accent),
            );
        }
        painter.text(
            egui::pos2(rect.left() + TAPE_SWITCH_LABEL_X_PX, rect.center().y),
            egui::Align2::LEFT_CENTER,
            TAPE_SWITCH_LABEL,
            egui::FontId::proportional(TAPE_SWITCH_FONT_PX),
            accent,
        );
    }

    /// One layer's checkbox, wherever it is offered.
    ///
    /// Three menus show these — the candles' layer menu, the tape's, and each
    /// axis's own for the switch that belongs to it — and all three call this
    /// so a layer wears one label, one hover text and one disabled reason
    /// whichever door a trader came through. It reads and writes the field
    /// that owns the layer, never a copy.
    ///
    /// Returns why the layer could not be switched, for the caller that has a
    /// sub-entry to gate on the same answer.
    fn layer_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        layer: ChartLayer,
        chrome: &mut PaneChrome<'_>,
    ) -> Option<LayerBlock> {
        let blocked = self.layer_blocked(layer, chrome.capabilities);
        let mut visible = self.layer_visible(layer, chrome.style);
        let response = ui
            .add_enabled(
                blocked.is_none(),
                egui::Checkbox::new(&mut visible, layer.label()),
            )
            .on_hover_text(layer.hint());
        #[cfg(test)]
        self.layer_menu_rects.push((layer, response.rect));
        if let Some(reason) = blocked {
            response.on_disabled_hover_text(reason.explanation);
        } else if response.changed() {
            self.set_layer_visible(layer, visible, chrome.layers);
        }
        blocked
    }

    /// The candles' layer checkboxes: the list the menu has always shown.
    ///
    /// The tape's own entries are filtered out here and drawn by
    /// [`Self::draw_tape_menu_section`] instead — one list, split by the pane
    /// each layer belongs to, so neither menu can offer a switch for the canvas
    /// beside it.
    fn draw_chart_layer_entries(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        for layer in ChartLayer::ALL.into_iter().filter(|layer| !layer.on_tape()) {
            let blocked = self.layer_checkbox(ui, layer, chrome);
            // The footprint's knobs live in a window of their own (the
            // Profitchart-style properties dialog, the boss's ask); the menu
            // offers the door. Available with the layer off too — configuring
            // before switching on is a legitimate order of operations.
            if layer == ChartLayer::Footprint && blocked.is_none() {
                ui.indent("footprint_configure", |ui| {
                    if ui
                        .button("configure footprint…")
                        .on_hover_text(
                            "style, band fineness, imbalance thresholds, POC and \
                             badges — in their own window",
                        )
                        .clicked()
                    {
                        chrome.layers.open_footprint_settings = true;
                        ui.close_menu();
                    }
                });
            }
        }
    }

    /// What the tape draws, and how much market time it shows.
    ///
    /// Reached by right-clicking the tape itself, which is the only place
    /// these choices are about. Every entry writes the lane's own field, so
    /// the dock's copy of the same settings and this one can never disagree.
    fn draw_tape_menu_section(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        if self.orderflow.is_none() {
            return;
        }
        ui.label(
            egui::RichText::new("tape")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );

        // The same loop the candles' entries run, over the other half of the
        // list. Nothing here is a second copy of the tape's state: each
        // checkbox reads and writes the lane's own field through
        // `layer_visible` / `set_layer_visible`, which is also what puts these
        // three in the layer state file.
        for layer in ChartLayer::ALL.into_iter().filter(|layer| layer.on_tape()) {
            let _ = self.layer_checkbox(ui, layer, chrome);
        }

        let reference_ms = self.last_lane_reference_ms;
        let Some(orderflow) = self.orderflow.as_mut() else {
            return;
        };
        let current = orderflow.live_lane_window();
        let mut chosen = None;
        ui.menu_button(
            format!("tape window: {}", lane_window_label(current, reference_ms)),
            |ui| {
                let mut entry = |ui: &mut egui::Ui, option: LaneWindow| {
                    if ui
                        .selectable_label(
                            same_lane_window(current, option),
                            lane_window_label(option, reference_ms),
                        )
                        .clicked()
                    {
                        chosen = Some(option);
                        ui.close_menu();
                    }
                };
                entry(ui, LaneWindow::default());
                ui.separator();
                for ms in LANE_WINDOW_PRESETS_MS {
                    entry(ui, LaneWindow::Fixed { ms });
                }
                ui.separator();
                // Custom: the same number the presets set, typed. Seconds
                // rather than milliseconds because that is the unit the choice
                // is made in; the field clamps to what the tape can draw.
                let mut seconds = match current {
                    LaneWindow::Fixed { ms } => ms,
                    LaneWindow::Auto { .. } => reference_ms
                        .map_or(MIN_LIVE_LANE_WINDOW_MS, |reference| {
                            current.resolve_ms(reference)
                        }),
                } as f64
                    / 1_000.0;
                ui.horizontal(|ui| {
                    ui.label("custom");
                    if ui
                        .add(
                            egui::DragValue::new(&mut seconds)
                                .speed(1.0)
                                .range(
                                    (MIN_LIVE_LANE_WINDOW_MS as f64 / 1_000.0)
                                        ..=(MAX_LIVE_LANE_WINDOW_MS as f64 / 1_000.0),
                                )
                                .suffix(" s"),
                        )
                        .changed()
                    {
                        chosen = Some(LaneWindow::Fixed {
                            ms: (seconds * 1_000.0).round() as i64,
                        });
                    }
                });
            },
        )
        .response
        .on_hover_text(
            "how much market time the tape shows. Following the bars keeps roughly one bar's \
             worth of flow in the band whatever the instrument; a fixed window shows that much \
             time however fast the bars are closing, so prints stay readable through a burst",
        );
        if let Some(window) = chosen {
            orderflow.set_live_lane_window(window);
        }
    }

    pub fn draw_layer_menu(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        // The drawing under the click is the most specific thing the click
        // named, so its section rides above everything — including the
        // trade actions, which answer for a bare price, not an object.
        #[cfg(test)]
        self.drawing_menu_rects.clear();
        if let Some(id) = self.context_menu_drawing {
            match self.drawings.index_of(id) {
                Some(index) => {
                    self.draw_drawing_menu_section(ui, index);
                    ui.separator();
                }
                // Deleted while the menu was open (undo, another surface):
                // the section vanishes instead of acting on a ghost.
                None => self.context_menu_drawing = None,
            }
        }
        // The trade section rides on top, anchored at the price the
        // right-click landed on. Gated on *this pane owning the menu*, not
        // on the pointer: the menu body re-runs every frame, and a popup
        // opened near a pane's edge extends past it, so a pointer-derived
        // gate dropped the section the moment the hand travelled onto a row
        // outside the originating pane — the menu reflowing under the
        // cursor mid-reach. `context_menu_price` is per pane and stable for
        // the menu's whole life, which is exactly the lifetime wanted.
        if let Some(price) = self.context_menu_price {
            chrome.paper.context_trade_actions(ui, price);
            ui.separator();
        }
        // Tools that place at the bar under the right-click (the anchored
        // VWAP's TradingView gesture) declare their entry on the registry;
        // the click was already resolved per tool, snap rules included, so
        // the menu only offers what the capture could honestly anchor.
        if !self.context_menu_places.is_empty() {
            let places = std::mem::take(&mut self.context_menu_places);
            for &(tool, point) in &places {
                let label = tool
                    .context_menu_label()
                    .expect("only declaring tools were captured");
                if ui.button(label).on_hover_text(tool.hover_text()).clicked() {
                    self.place_drawing_point(tool, &DrawingBand::Price, point, chrome);
                    ui.close_menu();
                }
            }
            self.context_menu_places = places;
            ui.separator();
        }
        #[cfg(test)]
        self.layer_menu_rects.clear();
        // The tape is a pane of its own and is configured as one: a right-click
        // on it answers for it, and the candles' own layers stay one submenu
        // away rather than disappearing. A click on the candles sees exactly
        // the menu it always saw.
        if self.context_menu_on_tape {
            self.draw_tape_menu_section(ui, chrome);
            ui.separator();
            ui.menu_button("chart layers", |ui| {
                self.draw_chart_layer_entries(ui, chrome);
            })
            .response
            .on_hover_text("what the candles beside the tape draw");
        } else {
            ui.label(
                egui::RichText::new("chart layers")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            self.draw_chart_layer_entries(ui, chrome);
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

    /// The per-drawing section of the layer menu: the object the
    /// right-click landed on, by name, with its own actions. This is the
    /// context-menu host `drawings/action_bar.rs` reserved a seat for.
    fn draw_drawing_menu_section(&mut self, ui: &mut egui::Ui, index: usize) {
        let label = self.drawings.items()[index].display_label(index);
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        // Rename applies when the field loses focus (Enter included) — one
        // undo step, not one per keystroke. Whitespace clears back to the
        // derived label; the store normalises it.
        let rename = ui.add(
            egui::TextEdit::singleline(&mut self.context_menu_rename)
                .hint_text("name this object")
                .desired_width(150.0),
        );
        #[cfg(test)]
        self.drawing_menu_rects.push(("Rename", rename.rect));
        if rename.lost_focus() {
            let name = std::mem::take(&mut self.context_menu_rename);
            self.drawings.rename_at(index, &name);
            self.context_menu_rename = name;
        }
        self.draw_strategy_menu_entries(ui, index);
        let locked = self.drawings.items()[index].locked;
        let hidden = self.drawings.items()[index].hidden;
        let lock = ui
            .button(if locked { "Unlock" } else { "Lock" })
            .on_hover_text("a locked object rejects geometry edits and plain deletes");
        #[cfg(test)]
        self.drawing_menu_rects
            .push((if locked { "Unlock" } else { "Lock" }, lock.rect));
        if lock.clicked() {
            self.drawings.set_locked_at(index, !locked);
            ui.close_menu();
        }
        let eye = ui.button(if hidden { "Show" } else { "Hide" });
        #[cfg(test)]
        self.drawing_menu_rects
            .push((if hidden { "Show" } else { "Hide" }, eye.rect));
        if eye.clicked() {
            self.drawings.set_hidden_at(index, !hidden);
            ui.close_menu();
        }
        let delete = if locked {
            ui.add_enabled(false, egui::Button::new("Delete"))
                .on_disabled_hover_text("unlock first — a locked object never deletes by accident")
        } else {
            let delete = ui.button("Delete");
            if delete.clicked() {
                let doomed = self.drawings.items()[index].id;
                self.drawings.select(Some(index));
                if self.drawings.delete_selected(false) == drawings::DeleteOutcome::Deleted {
                    // The instance dies with its drawing, immediately — not
                    // on the next closed bar, which a quiet tape may never
                    // bring.
                    self.remove_strategy_for_drawing(doomed);
                }
                self.context_menu_drawing = None;
                ui.close_menu();
            }
            delete
        };
        #[cfg(test)]
        self.drawing_menu_rects.push(("Delete", delete.rect));
        #[cfg(not(test))]
        let _ = delete;
    }

    /// What the badge over `drawing` says, as a value.
    ///
    /// The instance's own half ([`crate::strategy_anchors::badge_text`])
    /// plus the two things it cannot know, because they are facts about the
    /// *drawing* rather than about the strategy: a region nobody can
    /// honestly test ([`region_pause`]), and a drawn span that no longer
    /// reaches the next bar. Both shut the order and the alarm together, so
    /// both owe the trader a word — a badge reading a bare "armed" over a
    /// bot that has been held for an hour is the chart lying about the one
    /// thing this badge exists to say.
    ///
    /// Neither is a disarm. The trader moves the rectangle all session; a
    /// band dragged back over the future starts firing again on the next
    /// bar, with no button to press, and the alarm never went quiet.
    ///
    /// A `String` rather than paint, so the sentence a trader reads is the
    /// sentence a test asserts and a reader that is not looking at the
    /// screen can obtain. The painter below is one consumer of it.
    #[must_use]
    pub(crate) fn badge_text_for(
        &self,
        instance: &crate::strategy_anchors::AnchoredInstance,
        drawing: &drawings::Drawing,
    ) -> String {
        let mut text = crate::strategy_anchors::badge_text(instance);
        // The region's own state first, and *instead of* the kernel's
        // reason rather than beside it. A paused or expired region makes
        // `strategy_region` refuse, which the kernel records as "region not
        // active on this bar" — true of the span, and a lie about a band
        // that is merely hidden. Two vocabularies for one fact leave the
        // trader deciding which clause to believe; the specific one wins,
        // and it is the only one carrying a way out.
        let armed = matches!(instance.armed.state(), quantick_strategy::ArmedState::Armed);
        if let Some(pause) = region_pause(drawing, self.drawings.all_hidden()) {
            text.push_str(" · ");
            text.push_str(pause);
            return text;
        }
        if armed && !self.strategy_region_can_fire(drawing.id) {
            text.push_str(" · region ended — stretch it right");
            return text;
        }
        // Otherwise the gate that actually decided, in the words that fit a
        // corner — and never present-tense about a bar it is not about.
        // This is the whole point of the badge and it was reaching only the
        // right-click menu: the trader watches the chart, and "why did
        // nothing happen" is answerable only where they are already looking.
        let held = instance.armed.hold_reason();
        // A gate that refused *this* bar is the whole answer, and it stands
        // alone.
        if let Some(held) = held.filter(|held| held.fresh) {
            text.push_str(" · ");
            text.push_str(held.reason);
            return text;
        }
        // Otherwise the ruler is what decided this bar, and its reading is
        // the only sentence here about the candle in front of the trader.
        // `status_line` has always led with it and the right-click menu
        // prints it; this badge did not, so a bar the ruler held showed an
        // older bar's refusal and nothing about its own — the divergence
        // `region_pause` above exists to end, found again the same way.
        if armed {
            text.push_str(" · ");
            text.push_str(&instance.armed.trigger().status());
        }
        if let Some(held) = held {
            text.push_str(" · last held: ");
            text.push_str(held.reason);
        }
        text
    }

    /// The badge over the drawing with this id — the lookup half of
    /// [`Self::badge_text_for`], which the painter reaches directly because
    /// it already holds both.
    ///
    /// Test-only, and gated so it cannot drift into the shipped binary: the
    /// sentence the trader reads is worth asserting, and the id is what a
    /// test has in hand. The production path a reader that is not looking
    /// at the screen would need is the control plane's scene, which does
    /// not carry armed instances yet — filed rather than widened here.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn strategy_badge_text(&self, id: drawings::DrawingId) -> String {
        let Some(instance) = self.strategies.for_drawing(id) else {
            return String::new();
        };
        let Some(index) = self.drawings.index_of(id) else {
            return String::new();
        };
        self.badge_text_for(instance, &self.drawings.items()[index])
    }

    /// The armed instance's badge, pinned to its drawing's top-left corner:
    /// state at a glance, in the state's colour. Per frame this is one
    /// bounding-box fold and one text draw per *armed* drawing — a handful
    /// at most, and nothing at all on a chart with no instances.
    fn paint_strategy_badge(
        &self,
        painter: &egui::Painter,
        instance: &crate::strategy_anchors::AnchoredInstance,
        drawing: &drawings::Drawing,
        points: &[egui::Pos2],
    ) {
        let Some(first) = points.first() else {
            return;
        };
        let anchor = points.iter().fold(*first, |corner, point| {
            egui::pos2(corner.x.min(point.x), corner.y.min(point.y))
        });
        use quantick_strategy::ArmedState;
        let color = match instance.armed.state() {
            ArmedState::Armed => theme::ACCENT,
            ArmedState::Fired { .. } => theme::AMBER,
            ArmedState::InPosition => theme::BUY,
            ArmedState::Done => theme::TEXT_MUTED,
            ArmedState::Disarmed { .. } => theme::TEXT_FAINT,
        };
        /// Badge label size — the small-annotation size the band chips use.
        const BADGE_FONT_PX: f32 = 11.0;
        /// Ground padding around the label, and the gap that lifts the
        /// badge off the drawing's top-left corner.
        const BADGE_PAD_X_PX: f32 = 3.0;
        const BADGE_PAD_Y_PX: f32 = 2.0;
        const BADGE_LIFT_PX: f32 = 4.0;
        const BADGE_CORNER_PX: f32 = 3.0;
        /// Ground opacity: readable over candles, still a whisper.
        const BADGE_GROUND_ALPHA: f32 = 0.85;
        let text = self.badge_text_for(instance, drawing);
        let position = anchor + egui::vec2(BADGE_PAD_X_PX - 1.0, -BADGE_LIFT_PX);
        // A whisper of ground behind the label so it stays readable over
        // candles; galley first, box after, text last.
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(BADGE_FONT_PX), color);
        let rect = egui::Rect::from_min_size(
            position - egui::vec2(BADGE_PAD_X_PX, galley.size().y + BADGE_PAD_Y_PX + 1.0),
            galley.size() + egui::vec2(2.0 * BADGE_PAD_X_PX, 2.0 * BADGE_PAD_Y_PX),
        );
        painter.rect_filled(
            rect,
            BADGE_CORNER_PX,
            theme::CANVAS.gamma_multiply(BADGE_GROUND_ALPHA),
        );
        painter.galley(
            rect.min + egui::vec2(BADGE_PAD_X_PX, BADGE_PAD_Y_PX),
            galley,
            color,
        );
    }

    /// The strategy seat of the per-drawing menu: arm a bot on this region,
    /// or manage the one riding it. Price-band rectangles only — the one
    /// shape whose two anchors honestly bound a price region today.
    fn draw_strategy_menu_entries(&mut self, ui: &mut egui::Ui, index: usize) {
        let drawing = &self.drawings.items()[index];
        if drawing.tool.id() != drawings::RECTANGLE_TOOL_ID || drawing.band != DrawingBand::Price {
            return;
        }
        let id = drawing.id;
        let Some(instance) = self.strategies.for_drawing(id) else {
            let add = ui.button("Add strategy…").on_hover_text(
                "arm a strategy on this region: it fires on the trigger bar, in paper trading",
            );
            #[cfg(test)]
            self.drawing_menu_rects.push(("Add strategy", add.rect));
            if add.clicked() {
                self.strategy_popup_request = Some(id);
                ui.close_menu();
            }
            return;
        };
        // One line of truth about the bot on this drawing, then its verbs.
        ui.label(
            egui::RichText::new(instance.armed.status_line())
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        let state = instance.armed.state().clone();
        use quantick_strategy::{ArmedState, DisarmReason};
        match state {
            // One Disarm arm for every state that can be called off — a
            // resting retest limit included (it can wait for hours). Only
            // the hover varies; the cleanup plumbing must never fork.
            ArmedState::Armed | ArmedState::InPosition | ArmedState::Fired { retest: true, .. } => {
                let hover = if matches!(state, ArmedState::Fired { .. }) {
                    "cancel the resting retest limit and stop watching"
                } else {
                    "stop watching; an open operation keeps its position and bracket — yours to manage"
                };
                let disarm = ui.button("Disarm").on_hover_text(hover);
                #[cfg(test)]
                self.drawing_menu_rects.push(("Disarm", disarm.rect));
                if disarm.clicked()
                    && let Some(instance) = self.strategies.for_drawing_mut(id)
                {
                    let cleanup = instance.armed.disarm(DisarmReason::User);
                    self.strategy_cleanup.extend(cleanup);
                    ui.close_menu();
                }
            }
            ArmedState::Done | ArmedState::Disarmed { .. } => {
                // A drawing with no footing on this market/series cannot be
                // honestly re-armed — and neither can a region whose drawn
                // span already ended: the instance would show "armed" while
                // the region test refuses it forever, the silent halt the
                // named disarms exist to prevent.
                let footed = {
                    let drawing = &self.drawings.items()[index];
                    region_pause(drawing, self.drawings.all_hidden()).is_none()
                };
                let span_alive = self.strategy_region_can_fire(id);
                let rearm = ui
                    .add_enabled(footed && span_alive, egui::Button::new("Re-arm"))
                    .on_hover_text("watch this region again with the same parameters")
                    .on_disabled_hover_text(if footed {
                        "the region ends before the next bar — stretch it right, or turn on \
                         \"extend right\" in its Region settings"
                    } else {
                        "this drawing belongs to another market or lost its series — redraw the \
                         region here first"
                    });
                #[cfg(test)]
                self.drawing_menu_rects.push(("Re-arm", rearm.rect));
                if rearm.clicked() {
                    self.rearm_strategy_for_drawing(id);
                    ui.close_menu();
                }
            }
            // A market entry lives for exactly one print; nothing to offer.
            ArmedState::Fired { retest: false, .. } => {}
        }
        let remove = ui.button("Remove strategy").on_hover_text(
            "detach the bot from this drawing; an open operation keeps its position and bracket",
        );
        #[cfg(test)]
        self.drawing_menu_rects
            .push(("Remove strategy", remove.rect));
        if remove.clicked() {
            self.remove_strategy_for_drawing(id);
            ui.close_menu();
        }
    }

    /// Re-arm the instance riding `drawing`, re-warming its ruler when the
    /// disarm named a *rebuilt series* (a replay seek, a bar-spec change, a
    /// market switch reset the trigger's window). Without the re-warm,
    /// "re-armed" silently means "warming up for another twenty bars" — the
    /// replay-seek trap where force bars right after a seek never fire.
    pub(crate) fn rearm_strategy_for_drawing(&mut self, drawing: drawings::DrawingId) {
        use quantick_strategy::ArmedState;
        let Some(instance) = self.strategies.for_drawing(drawing) else {
            return;
        };
        let series_changed = matches!(
            instance.armed.state(),
            ArmedState::Disarmed { reason } if reason.resets_series()
        );
        if let Some(instance) = self.strategies.for_drawing_mut(drawing) {
            instance.armed.rearm();
        }
        if series_changed {
            self.rewarm_strategy_trigger(drawing);
        }
    }

    /// Feed the last `warmup_bars` closed bars of the live series back into
    /// an instance's trigger — the arm-time warmup, repeated after a rearm
    /// whose disarm reset the ruler. Venue-prefix candles are excluded for
    /// the same reason as at arm time: they measure another ruler entirely.
    fn rewarm_strategy_trigger(&mut self, id: drawings::DrawingId) {
        let Some(instance) = self.strategies.for_drawing(id) else {
            return;
        };
        let bars = self.strategy_warmup_bars(instance.armed.trigger().warmup_bars());
        let Some(instance) = self.strategies.for_drawing_mut(id) else {
            return;
        };
        instance.armed.warm(&bars);
    }

    /// Whether the drawing's drawn span can still cover a future closed
    /// bar — the liveness half of [`Self::strategy_region`]'s `active`
    /// test, shared by arming, re-arming and the menu so the three cannot
    /// drift. The next bar to close lands at slot `closed_slots()`, so an
    /// unextended region needs its right anchor at or past that slot; an
    /// extended one never expires right.
    pub(crate) fn strategy_region_can_fire(&self, id: drawings::DrawingId) -> bool {
        let Some(index) = self.drawings.index_of(id) else {
            return false;
        };
        let drawing = &self.drawings.items()[index];
        let extend_right = drawing
            .payload
            .as_any()
            .downcast_ref::<drawings::RectanglePayload>()
            .is_some_and(|payload| payload.extend_right);
        if extend_right {
            return true;
        }
        let [a, b] = drawing.points.as_slice() else {
            return false;
        };
        #[allow(clippy::cast_precision_loss)]
        let next_slot = self.closed_slots() as f32;
        a.bar.max(b.bar) >= next_slot
    }

    /// Sweep instances whose drawing no longer exists — for the deletion
    /// paths that cannot call [`Self::remove_strategy_for_drawing`] with an
    /// id in hand (delete-all, undo, redo), so no path leaves a resting bot
    /// order with no badge over it. Cleanup is queued for the tab's
    /// same-frame drain like the menu's.
    pub(crate) fn sweep_strategy_orphans(&mut self) {
        if self.strategies.is_empty() {
            return;
        }
        let alive: Vec<drawings::DrawingId> = self
            .strategies
            .instances
            .iter()
            .map(|instance| instance.drawing)
            .filter(|id| self.drawings.index_of(*id).is_some())
            .collect();
        let cleanup = self.strategies.drop_orphans(|id| alive.contains(&id));
        self.strategy_cleanup.extend(cleanup);
    }

    /// The last `want` closed bars of the live series — never venue-prefix
    /// candles, whose bodies measure another ruler — for warming a strategy
    /// trigger at arm or re-arm time.
    pub fn strategy_warmup_bars(&self, want: usize) -> Vec<quantick_engine::Bar> {
        let slots = self.slots();
        let first_live = self.seam_slot();
        (slots.saturating_sub(want).max(first_live)..slots)
            .filter_map(|slot| self.closed_bar(slot).cloned())
            .collect()
    }

    /// Drain the cleanup commands the drawing menu queued; the tab applies
    /// them to the paper host on this same frame.
    #[must_use]
    pub fn take_strategy_cleanup(&mut self) -> Vec<quantick_sim::Command> {
        std::mem::take(&mut self.strategy_cleanup)
    }

    /// Remove the instance riding `drawing` and queue the sweep of its
    /// pending entry — every "the bot dies with its drawing" path funnels
    /// through here so none of them can orphan a resting retest limit.
    pub(crate) fn remove_strategy_for_drawing(&mut self, drawing: drawings::DrawingId) {
        let cleanup = self.strategies.remove_for_drawing(drawing);
        self.strategy_cleanup.extend(cleanup);
    }

    /// The bar spec implied by the current selector state.
    pub fn current_spec(&self) -> BarSpec {
        match self.kind {
            BarKind::Tick => BarSpec::Tick(self.tick_n.max(1)),
            BarKind::Volume => BarSpec::Volume(dec_from_f64(self.volume_units)),
            BarKind::Dollar => BarSpec::Dollar(dec_from_f64(self.dollar_notional)),
            BarKind::Time => BarSpec::Time(self.time_interval_ms.max(1)),
            BarKind::Imbalance => {
                BarSpec::Imbalance(self.imbalance_unit, self.imbalance_target.max(1))
            }
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

    /// The slot of a bar that covers only *part* of the interval it occupies,
    /// when there is one.
    ///
    /// The tape's first bar opens on its first print, which lands somewhere
    /// inside the interval rather than on its edge — and the venue candle
    /// that did cover the whole interval was dropped at the seam
    /// (`trim_to_seam`) precisely because the two overlap. So that slot holds
    /// a short bar wearing a full bar's clothes: a range folding it is missing
    /// whatever traded before the app connected, measured on a live BTCUSDT
    /// connect at 36% of a 1-minute bucket and 94% of an hourly one.
    ///
    /// No volume is invented to close the gap. The profile says the bar is
    /// partly covered and lets the trader judge it — the same contract the
    /// approximated-from-OHLC label keeps.
    ///
    /// `None` for a pane that does not cut by time (a tick or volume bar owns
    /// no interval to fall short of), and for a first bar that opens exactly
    /// on its boundary.
    pub fn partial_bucket_slot(&self) -> Option<usize> {
        let interval = self.state.spec().time_interval_ms()?;
        let first = self.state.bars().first().or_else(|| self.state.partial())?;
        let opens_inside =
            crate::resample::bucket_start(first.open_time, interval) != first.open_time;
        opens_inside.then(|| self.seam_slot())
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

    /// The slot whose bar *covers* market time `ms`, or `None` when this
    /// pane's tape does not reach that instant at all.
    ///
    /// [`Self::slot_at_time`] clamps at both ends — an instant older than the
    /// series answers slot 0, a newer one the newest slot — which is what a
    /// drawing anchor being re-hung on the closest bar wants. A mark standing
    /// for a *fill* wants the opposite answer at those edges: outside the
    /// window the pane holds, the tape on screen cannot prove the print
    /// happened, and a mark on the wrong bar is worse than no mark. Without
    /// it a replay seek — which wipes the bars and keeps the round trips,
    /// because they happened — stacks every earlier trade on whichever bar
    /// sits at the edge, and they pile up there as the replay runs on.
    ///
    /// The window is [`Self::covered_window`]'s. Between its two ends
    /// `slot_at_time` is already exact, so only the edges are decided here.
    ///
    /// Third of three deliberately different answers to one lookup, and the
    /// list is meant to stay at three. [`Self::slot_at_time`] clamps at both
    /// ends; `slot_of_time` refuses the old end but lets the new one *run on*
    /// past the newest bar, because a trend line pointing into the future
    /// belongs there; this one refuses both, because a fill does not happen
    /// in the future and did not happen before the tape began.
    ///
    /// One instant per call. A caller asking about many instants against one
    /// cut of the series — the trade paint, twice per closed round trip per
    /// frame — takes `covered_window` itself and tests against the two
    /// numbers, rather than re-deriving them thousands of times a second on
    /// the paint path.
    pub fn covering_slot_at_time(&self, ms: i64) -> Option<usize> {
        let (oldest, newest) = self.covered_window()?;
        if ms < oldest || ms > newest {
            return None;
        }
        self.slot_at_time(ms)
    }

    /// The stretch of market time this pane's bars cover, both ends
    /// inclusive: the oldest bar's open to the newest bar's last print.
    ///
    /// The oldest bar is the venue prefix's first candle when there is a
    /// prefix, so the lower end is that candle's open rather than a print —
    /// which is what "the chart reaches back this far" means to someone
    /// reading the screen, candles or prints. `None` for a pane with no bars.
    fn covered_window(&self) -> Option<(i64, i64)> {
        let newest = self
            .state
            .partial()
            .or_else(|| self.state.bars().last())
            .or_else(|| self.history_prefix.last())
            .map(|bar| bar.close_time)?;
        Some((self.slot_open_time(0)?, newest))
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
        let source_fills_it = capabilities.book_capture || capabilities.traded_volume;
        if self.live_strip_visible && self.orderflow.is_some() && source_fills_it {
            crate::live_strip::LIVE_STRIP_WIDTH_PX
        } else {
            0.0
        }
    }

    /// This pane's regions inside `area`, carved once so the input handler and
    /// the renderer can never disagree about a boundary.
    fn plot_areas(&self, area: egui::Rect, capabilities: FeedCapabilities) -> PlotAreas {
        let mut sizing = [PaneSizing::Auto; crate::indicators::MAX_PANES];
        plot_split(
            area,
            self.live_strip_width(capabilities),
            self.indicators.pane_sizing(&mut sizing),
        )
    }

    /// Reserve a slot and ask the worker to instantiate `source` behind it.
    pub fn add_indicator(&mut self, source: IndicatorSource) -> SlotId {
        let slot = self.indicators.allocate_slot(source.kind_id());
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
        // Any time-cutting pane may carry one (audit S1) — the flow pane
        // showing time bars included. On a pane with a tape the flow layers
        // simply have nothing to draw over the prefix: a venue candle has no
        // prints in it, and the projection maps only the engine's own bars
        // (see `draw_chart`'s timeline).
        if !prefix_differs(&self.history_prefix, &bars) {
            return false;
        }
        let before = self.history_prefix.len();
        self.history_prefix = bars;
        self.bump_pagination_revision();
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

    /// Hand the tape's own price grid — and the magnitude it prints at — to
    /// the order-flow engine, which sizes the capture bucket, and through it
    /// the footprint ladder's rows and the volume profile folded from them.
    ///
    /// Called after every ingest because the answer can only arrive after
    /// prints have. The view sends a command only when the answer changes, and
    /// a running GCD changes it a handful of times per session at most.
    fn publish_tape_price_step(&mut self) {
        // The engine first: a time pane has none, `panes_mut` fans every print
        // to it too, and a tuple scrutinee would read the grid before finding
        // that out — paying a read per print on a pane that can never use it.
        let Some(orderflow) = self.orderflow.as_mut() else {
            return;
        };
        if let Some(step) = self.state.tape_price_step() {
            orderflow.observe_tape_price_grid(step, self.state.tape_reference_price());
        }
    }

    /// Take a backfill batch into the series and hand the indicators the bars
    /// it produced.
    pub fn ingest_backfill(&mut self, trades: &[quantick_engine::Trade]) {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        self.state.ingest_backfill(trades);
        self.indicator_worker
            .send(IndicatorCommand::Backfilled(self.closed_bars()));
        let partial = self.partial_command();
        self.indicator_worker.send(partial);
        self.publish_tape_price_step();
    }

    /// Prepend older trades and shift everything anchored to a bar index by the
    /// number of bars they added, which is what this returns.
    pub fn prepend_history(&mut self, trades: &[quantick_engine::Trade]) -> usize {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        // Older bars shift every index up; keep the view steady.
        let added = self.state.prepend_history(trades);
        self.viewport.shift_right_edge(added as isize);
        self.drawings.shift_bars(added as isize);
        // Indicator columns shift with them: the rebuild below is a round-trip
        // away, and until it lands every value would otherwise be drawn
        // `added` slots off its own candle.
        self.indicators.shift_rows(added);
        self.publish_tape_price_step();
        // Older trades re-cut every bar; replay from scratch.
        self.send_indicator_rebuild();
        added
    }

    /// Where a market instant sits on this pane's series, as a fractional
    /// slot — the strict answer, behind re-anchoring and every edit arriving
    /// from another pane.
    ///
    /// Bar centres, not edges: an anchor is being asked which *bar* it belongs
    /// to, and the middle of that bar is where it reads as being on it. `None`
    /// means the series does not reach the instant at all.
    ///
    /// Deliberately not what [`Self::reproject`] uses. That one is answering a
    /// different question — *where do I paint a mark whose instant may be off
    /// my series?* — so it clamps to the nearest edge and reports the clamp,
    /// which is what the fade is drawn from. This one is asked before the
    /// store is written, where a clamp would silently move the trader's mark
    /// onto data it has nothing to do with. Same lookup, opposite answer at
    /// the edges, on purpose.
    ///
    /// Also not [`Self::covering_slot_at_time`], which refuses the future end
    /// as well: a drawing may point past the newest bar, a fill may not.
    fn slot_of_time(&self, time: i64) -> Option<f32> {
        // Past the newest bar first: on a time chart that space has an exact
        // clock, and asking `slot_at_time` there would clamp a future anchor
        // onto the right edge instead of letting it run on.
        if let Some(future) = self.future_slot_at_time(time) {
            return Some(future + 0.5);
        }
        // Before the first bar this pane holds. `slot_at_time` answers slot 0
        // there, which is a clamp and not a location — taking it would put the
        // anchor on a bar it has nothing to do with and say nothing about it.
        // `None` is what the off-series fade and the refused drag both read.
        if self.slot_open_time(0).is_some_and(|first| time < first) {
            return None;
        }
        let slots = self.slots();
        let slot = self.slot_at_time(time)?.min(slots.checked_sub(1)?);
        #[allow(clippy::cast_precision_loss)]
        Some(slot as f32 + 0.5)
    }

    /// Re-express this pane's drawings against the series it holds now,
    /// `old_slots` being the length of the series they were anchored to.
    ///
    /// The one call behind every re-cut — a timeframe switch, a bar-kind
    /// switch, a rewind, a symbol change. Marks are never dropped by a state
    /// change: the trader placed them and the trader removes them.
    pub fn reanchor_drawings(&mut self, old_slots: usize) {
        let new_slots = self.slots();
        // Taken out of `self` for the call: the store has to be handed this
        // pane's own time→slot answer, and a closure borrowing `&self` cannot
        // coexist with a `&mut` borrow of one of its fields.
        let mut drawings = std::mem::take(&mut self.drawings);
        drawings.reanchor(old_slots, new_slots, |time| self.slot_of_time(time));
        self.drawings = drawings;
    }

    /// Re-anchor as soon as there are bars to anchor to, after a reset left
    /// the pane empty. Cheap enough to ask every frame: it is a flag test.
    /// The indicator a gesture on this pane asked to configure, if any, taken
    /// so a request is acted on exactly once.
    pub fn take_settings_request(&mut self) -> Option<SlotId> {
        self.pending_settings.take()
    }

    /// Stand in for the gesture that raises a settings request, so the app's
    /// side of the wiring can be tested without driving egui through a pane
    /// layout it would have to re-derive.
    #[cfg(test)]
    pub fn request_settings(&mut self, slot: SlotId) {
        self.pending_settings = Some(slot);
    }

    /// Ask for a re-anchor once there are bars, for drawings adopted onto an
    /// empty series — the layout seeding a pane before its first print.
    /// Asking now would mark every anchor off a series that does not exist.
    pub fn defer_reanchor(&mut self) {
        self.pending_reanchor.get_or_insert(0);
    }

    pub fn settle_pending_reanchor(&mut self) {
        let Some(old_slots) = self.pending_reanchor else {
            return;
        };
        if self.slots() == 0 {
            return;
        }
        self.pending_reanchor = None;
        self.reanchor_drawings(old_slots);
    }

    /// Rewrite the market instants behind the selected object's anchors after
    /// a move that changed their bar positions (drag or keyboard nudge).
    ///
    /// Without this the mark moves on this chart and its shared twin stays
    /// where it was: market time is what the other panes read, so a move that
    /// does not update it has moved only half the object.
    pub fn retime_selected(&mut self) {
        let Some(index) = self.drawings.selected() else {
            return;
        };
        let Some(drawing) = self.drawings.items().get(index) else {
            return;
        };
        // Collected first so the immutable borrow of the store ends before
        // the write; every shipped tool has at most four anchors.
        let times: SmallVec<[Option<i64>; 4]> = drawing
            .points
            .iter()
            .map(|point| self.anchor_time(point.bar))
            .collect();
        self.drawings.set_times(index, &times);
    }

    /// Throw away this pane's bars, keeping the spec its own selectors ask
    /// for and the marks the trader drew.
    ///
    /// Called when the market underneath changes — a feed switch, a source
    /// reset — because a bar index means nothing across two streams. The
    /// drawings survive it: their anchors carry market time, so they are
    /// re-expressed against the refilled series rather than discarded
    /// ([`Self::settle_pending_reanchor`]). The re-anchor waits for bars to
    /// exist, because an empty series can answer nothing.
    pub fn reset_series(&mut self) {
        // A second reset before the first settled must not overwrite the
        // baseline with the empty series it is looking at now.
        let slots = self.slots();
        self.pending_reanchor.get_or_insert(slots);
        // The prefix is bar-indexed against a series that no longer exists,
        // and its seam was trimmed against a first bar that is gone. A replay
        // never has one today; the invariant must not depend on that.
        self.history_prefix.clear();
        self.state = ChartState::new(self.current_spec());
        self.bump_pagination_revision();
        self.viewport = Viewport::new();
        // Framing dies with the series; orientation is the trader's standing
        // choice about the view, not about these bars — it survives the way
        // the drawings do.
        let inverted = self.price_view.is_inverted();
        self.price_view = PriceView::new();
        self.price_view.set_inverted(inverted);
        self.last_auto_range = None;
        self.hover_pos = None;
        // Bars queued for the strategies belong to the series that just
        // died; the tab disarms the instances with the reset's own reason.
        self.strategy_pending.clear();
    }

    /// Fill a pane opened mid-session from the trades another pane of the same
    /// market already holds, keeping the backfill/live boundary where it was:
    /// a trade that was streamed live must not become "history" just because
    /// this view was opened late.
    pub fn seed_from(&mut self, trades: &[quantick_engine::Trade], backfill_count: usize) {
        if !trades.is_empty() {
            self.bump_pagination_revision();
        }
        let split = backfill_count.min(trades.len());
        self.state.ingest_backfill(&trades[..split]);
        for trade in &trades[split..] {
            self.state.ingest_live(trade);
        }
        // One rebuild rather than one command per trade: the worker is being
        // handed a whole history, not watching it arrive.
        self.publish_tape_price_step();
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
        self.publish_tape_price_step();
        // At most one bar closes per trade (an atomic market event is never
        // split), so "grew" identifies exactly the bar that closed.
        let bars_after = self.state.bars().len();
        if bars_after > bars_before
            && let Some(closed) = self.state.bars().last().cloned()
        {
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed.clone()));
            // Queued for the armed instances only while any exist: an idle
            // chart clones nothing on the per-trade path. The slot is the
            // *composed* one (venue history prefix + live bars) — the same
            // space the drawings' anchors live in, or the region's time
            // window would be off by the prefix length.
            if !self.strategies.is_empty() {
                let slot = self.history_prefix.len() + bars_after - 1;
                self.strategy_pending.push((closed, slot));
            }
        }
    }

    /// The closed bars awaiting strategy evaluation, slot each. Drained by
    /// the tab right after the ingestion sweep that queued them.
    #[must_use]
    pub fn take_strategy_bars(&mut self) -> Vec<(quantick_engine::Bar, usize)> {
        std::mem::take(&mut self.strategy_pending)
    }

    /// Resolve the drawing an instance is anchored to into the kernel's
    /// terms, for the bar that closed at `slot`: the price band between the
    /// rectangle's anchors, and whether that slot falls inside its span of
    /// the tape. `None` when the drawing is gone, marks another market, or
    /// lost its footing on this series — a region that cannot honestly be
    /// tested holds fire.
    #[must_use]
    pub fn strategy_region(
        &self,
        id: drawings::DrawingId,
        slot: usize,
    ) -> Option<(quantick_strategy::Region, bool)> {
        let index = self.drawings.index_of(id)?;
        let drawing = self.drawings.items().get(index)?;
        // Every reason a region cannot honestly be tested — another market,
        // a lost series, a drawing nobody can see — is one rule, shared with
        // the badge that has to say so ([`region_pause`]). An order fired
        // from a region nobody can see is an invisible bot; showing the
        // drawing resumes it.
        if region_pause(drawing, self.drawings.all_hidden()).is_some() {
            return None;
        }
        let [a, b] = drawing.points.as_slice() else {
            return None;
        };
        let region = quantick_strategy::Region::new(dec_from_f64(a.price), dec_from_f64(b.price));
        // An extended rectangle runs to the chart's right edge until
        // further notice, and its region does too — otherwise the bot
        // silently expires at the drawn end while the band visibly keeps
        // going (the replay trap this option exists to close).
        let extend_right = drawing
            .payload
            .as_any()
            .downcast_ref::<drawings::RectanglePayload>()
            .is_some_and(|payload| payload.extend_right);
        #[allow(clippy::cast_precision_loss)]
        let slot = slot as f32;
        let active = slot >= a.bar.min(b.bar) && (extend_right || slot <= a.bar.max(b.bar));
        Some((region, active))
    }

    /// Hand the indicators the forming bar as it stands now.
    ///
    /// Sent once per drain that took in live trades — see
    /// [`Self::ingest_live_trade`] for why it is not sent per trade.
    pub fn publish_partial(&mut self) {
        let command = self.partial_command();
        self.indicator_worker.send(command);
    }

    /// The forming bar, the run of trades behind it, and how many rungs the
    /// lane can show — everything the worker needs to preview the bar and to
    /// sample it across the tape.
    ///
    /// The run is a slice of trades the pane already owns, cloned once per
    /// drain rather than per print. The rung budget comes from the lane's
    /// width at the last draw: a chart with no lane asks for none, and the
    /// worker then walks no ladder at all.
    fn partial_command(&self) -> IndicatorCommand {
        let partial = self.state.partial().cloned();
        // No lane, no run. The clone is proportional to the forming bar's
        // trade count, and a chart with nowhere to draw the result would pay
        // it on every drain for nothing.
        let run = partial
            .as_ref()
            .filter(|_| self.lane_rungs > 0)
            .map_or_else(Vec::new, |bar| {
                let trades = self.state.trades();
                let count = usize::try_from(bar.trade_count).unwrap_or(usize::MAX);
                trades[trades.len().saturating_sub(count)..].to_vec()
            });
        IndicatorCommand::PartialUpdated {
            partial,
            run,
            rungs: self.lane_rungs,
        }
    }

    /// Convert a chart pixel into an overlay anchor. The x coordinate is a
    /// fractional bar slot, so drawings follow pan/zoom instead of being stuck
    /// to one screen pixel.
    /// The x half is shared by every band — the panes ride the candles' time
    /// axis — and only the y half asks which band it is being read against.
    fn drawing_point_at(
        &self,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
        magnet: bool,
        snap: drawings::AnchorSnap,
        band: &Band,
    ) -> Option<ChartPoint> {
        let scale = band.scale.as_ref()?;
        if total == 0 || band.rect.height() <= 1.0 {
            return None;
        }
        let bar = self.viewport.bar_at_x(pos.x, history_right, total);
        // A candle-magnet anchor cannot land where no candle is: the bar
        // clamps to the tape before the snap reads it.
        let bar = if snap == drawings::AnchorSnap::NearestOhlc {
            snap_bar_to_tape(bar, total)
        } else {
            bar
        };
        let value = match snap {
            // A mark's own rule beats the magnet toggle in both directions:
            // it snaps with the magnet off, and it snaps to *its* extreme
            // rather than to whichever of the four OHLC prices is nearest.
            drawings::AnchorSnap::BarLow => self.bar_extreme(band, bar, false),
            drawings::AnchorSnap::BarHigh => self.bar_extreme(band, bar, true),
            drawings::AnchorSnap::NearestOhlc => self.candle_nearest_ohlc(band, bar, pos.y, scale),
            drawings::AnchorSnap::Pointer => magnet
                .then(|| self.magnet_value(band, bar, pos.y, scale))
                .flatten(),
        }
        .unwrap_or_else(|| scale.price_at(pos.y));
        Some(ChartPoint::at_time(bar, value, self.anchor_time(bar)))
    }

    /// The high or low of the bar `bar` falls on, on the price band only.
    ///
    /// An indicator band has no candle, so a mark dropped there keeps the
    /// pointer's own value: inventing a high for a CVD pane would be the
    /// data-honesty failure this repo refuses, and refusing the click
    /// outright would read as a bug.
    fn bar_extreme(&self, band: &Band, bar: f32, high: bool) -> Option<f64> {
        if !matches!(band.key, DrawingBand::Price) {
            return None;
        }
        let slot = Viewport::slot_of(bar)?;
        // The forming bar counts. Marking the bar that is running *is* the
        // live use of this tool — marking a closed one is review — and
        // `closed_bar` stops one slot short of it, which would drop the mark
        // back onto the pointer's own price: exactly the failure the snap
        // exists to prevent, in the only moment it is used under pressure.
        //
        // The extreme is read at the instant of the click. A low that
        // deepens afterwards leaves the mark where the bar was when it was
        // marked, which is what the mark is a record of.
        let candle = self.candle_at_slot(slot)?;
        if high { candle.high } else { candle.low }.to_f64()
    }

    /// The candle behind a slot, the forming bar included — the one lookup
    /// every candle-reading snap shares.
    fn candle_at_slot(&self, slot: usize) -> Option<&quantick_engine::Bar> {
        self.closed_bar(slot)
            .or_else(|| (slot == self.closed_slots()).then(|| self.state.partial())?)
    }

    /// Work a shared mark that lives on the other pane, from this one.
    ///
    /// Every answer leaves in market time and price — the coordinates two cuts
    /// of one tape agree on — so the tab can hand them to the pane that holds
    /// the object without either pane learning the other's bar space.
    ///
    /// The pointer rules are the ones this pane already applies to its own
    /// marks: a handle before a body, a locked object that takes the gesture
    /// and refuses to move, and a drag threshold so a click never re-angles a
    /// level by two pixels of hand tremor.
    fn interact_shared(
        &mut self,
        ui: &egui::Ui,
        chrome: &mut PaneChrome<'_>,
        pointer: SharedPointer,
    ) {
        // The band under the pointer, on this pane's own last carve: a shared
        // mark is grabbed where it is painted, and where it is painted is the
        // band whose axis its value belongs to. Reading the candles' scale for
        // a CVD mark would send a price back to the pane that owns it.
        let mark = |pane: &Self, position: egui::Pos2| {
            let band = bands::band_at(&pane.last_bands, position)?;
            pane.drawing_point_at(
                position,
                pointer.history_right,
                pointer.total,
                pointer.magnet,
                drawings::AnchorSnap::Pointer,
                band,
            )
            .and_then(|point| Some((point.time_ms?, point.price)))
        };

        if pointer.pressed
            && !pointer.over_chrome
            && let Some((position, pick)) = pointer
                .position
                .filter(|position| pointer.area.contains(*position))
                .zip(chrome.shared_pick)
        {
            // Selecting is not moving (§D9): the press takes the object
            // whether or not the drag that may follow is allowed.
            chrome.shared.owner = Some(pick.owner);
            chrome.shared.edit = Some(SharedEdit::Select(pick.index));
            self.shared_drag_owner = Some(pick.owner);
            self.shared_drag_pending_from = Some(position);
            self.shared_pointer_mark = mark(self, position);
            self.shared_drag = if pick.locked {
                SharedDrag::Blocked
            } else {
                chrome.shared.begin_gesture = true;
                match pick.anchor {
                    Some(anchor) => SharedDrag::Anchor {
                        index: pick.index,
                        anchor,
                    },
                    None => SharedDrag::Body { index: pick.index },
                }
            };
            return;
        }

        if !self.shared_drag.is_active() {
            // Hover feedback, so a mirrored mark does not feel deader than the
            // object it is: the same three cursors its own pane shows.
            if !pointer.over_chrome
                && pointer
                    .position
                    .is_some_and(|position| pointer.area.contains(position))
                && let Some(pick) = chrome.shared_pick
            {
                ui.ctx().set_cursor_icon(match (pick.locked, pick.anchor) {
                    (true, _) => egui::CursorIcon::NotAllowed,
                    (false, Some(_)) => egui::CursorIcon::ResizeNwSe,
                    (false, None) => egui::CursorIcon::Move,
                });
            }
            return;
        }

        if pointer.released {
            chrome.shared.owner = self.shared_drag_owner;
            chrome.shared.commit_gesture = true;
            self.shared_drag = SharedDrag::None;
            self.shared_drag_owner = None;
            self.shared_drag_pending_from = None;
            self.shared_pointer_mark = None;
            return;
        }
        if !pointer.down {
            return;
        }
        // Under the threshold the object does not move at all, so a click on
        // the mirror stays a click.
        if let Some(origin) = self.shared_drag_pending_from {
            let travelled = pointer
                .position
                .is_some_and(|position| (position - origin).length() >= DRAWING_DRAG_THRESHOLD_PX);
            if !travelled {
                return;
            }
            self.shared_drag_pending_from = None;
        }
        // Clamped, not filtered: the gesture is already ours, and it keeps
        // working while the pointer travels off the pane — over the inspector
        // that this very press opened, most of all.
        let Some(position) = pointer.position.map(|position| {
            egui::pos2(
                position.x.clamp(pointer.area.left(), pointer.area.right()),
                position.y.clamp(pointer.area.top(), pointer.area.bottom()),
            )
        }) else {
            return;
        };
        // A pointer over the empty space past the newest bar of a tick or
        // volume chart names no instant, and none is invented: the mark holds
        // still for that frame rather than jumping to a guess.
        let Some((time_ms, price)) = mark(self, position) else {
            return;
        };
        // Every edit this gesture emits belongs to the pane the gesture took
        // hold of, whatever the pointer is over now.
        chrome.shared.owner = self.shared_drag_owner;
        match self.shared_drag {
            SharedDrag::Anchor { index, anchor } => {
                chrome.shared.edit = Some(SharedEdit::MoveAnchor {
                    index,
                    anchor,
                    time_ms,
                    price,
                });
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            SharedDrag::Body { index } => {
                if let Some((last_time, last_price)) = self.shared_pointer_mark {
                    chrome.shared.edit = Some(SharedEdit::Translate {
                        index,
                        delta_ms: time_ms - last_time,
                        delta_price: price - last_price,
                    });
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
            SharedDrag::Blocked => ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed),
            SharedDrag::None => {}
        }
        self.shared_pointer_mark = Some((time_ms, price));
    }

    /// The magnet, applied to the bar the pointer is over, on the band it
    /// is over.
    ///
    /// Only that bar is considered: snapping to a neighbour would move the
    /// anchor sideways, and the trader chose the bar by pointing at it. On an
    /// indicator band the candidates are that pane's own plotted values plus
    /// zero — without them a "CVD zero line" is drawn by eye while the pane's
    /// own zero rule sits right there. Never across bands: a price would be a
    /// meaningless place to snap a CVD level to.
    fn magnet_value(
        &self,
        band: &Band,
        bar: f32,
        pointer_y: f32,
        scale: &PriceScale,
    ) -> Option<f64> {
        let row = Viewport::slot_of(bar)?;
        match &band.key {
            // `candle_at_slot`, not `closed_bar`: the forming bar is a slot
            // like any other and pointing at the live candle is when a magnet
            // is used under pressure. Its two siblings — `bar_extreme` and
            // `candle_nearest_ohlc` — already read it that way, and the odd
            // one out silently returned "nothing to snap to" on the bar the
            // trader was actually on.
            DrawingBand::Price => {
                magnet_price_of(self.candle_at_slot(row)?, pointer_y, scale, MAGNET_REACH_PX)
            }
            // A time-only object has no value to snap.
            DrawingBand::AllBands => None,
            DrawingBand::Indicator(_) => {
                let view = self.indicators.visible_panes().find(|view| {
                    DrawingBand::Indicator(self.indicators.pane_key(view)) == band.key
                })?;
                bands::magnet_value_of(view, row, pointer_y, scale, MAGNET_REACH_PX)
            }
        }
    }

    /// The unconditional candle magnet: the nearest of the bar's OHLC with
    /// no reach limit, the forming bar included — [`AnchorSnap::NearestOhlc`]'s
    /// value rule. Price band only; a band with no candles answers `None`
    /// and the caller keeps the pointer's own value.
    fn candle_nearest_ohlc(
        &self,
        band: &Band,
        bar: f32,
        pointer_y: f32,
        scale: &PriceScale,
    ) -> Option<f64> {
        if !matches!(band.key, DrawingBand::Price) {
            return None;
        }
        let slot = Viewport::slot_of(bar)?;
        let candle = self.candle_at_slot(slot)?;
        magnet_price_of(candle, pointer_y, scale, MAGNET_REACH_UNLIMITED_PX)
    }

    /// The market time behind a fractional bar slot, for anchors that may have
    /// to be re-expressed on another pane (§D7 of the drawing-tools design).
    ///
    /// Only a slot that actually holds a bar has an instant behind it: the
    /// empty space past the newest bar is future the tape has not written, and
    /// naming a time there would be an invention. `None` is the honest answer
    /// there, and it is what keeps such an anchor out of a shared drawing.
    fn anchor_time(&self, bar: f32) -> Option<i64> {
        let slot = Viewport::slot_of(bar)?;
        let slots = self.slots();
        if slot < slots {
            return self.slot_open_time(slot);
        }
        // Past the newest bar. Traders draw here constantly — a channel or a
        // trend line pointing into the empty space to the right of the tape
        // is the normal way to say "if this continues". Refusing the whole
        // gesture a time would block sharing exactly where it is most used.
        //
        // On a *time* chart that space has an exact clock: the bars are one
        // fixed interval apart, so the slot after the last one is the last
        // one plus that interval. Nothing is inferred.
        //
        // On a tick or volume chart it does not: the next bar happens when
        // enough trades happen, and no elapsed time can be named for it. That
        // stays `None` — an invented timestamp is worse than a control that
        // says why it is off.
        if self.kind != BarKind::Time || self.time_interval_ms <= 0 {
            return None;
        }
        let last = slots.checked_sub(1)?;
        let ahead = i64::try_from(slot - last).ok()?;
        self.slot_open_time(last)?
            .checked_add(ahead.checked_mul(self.time_interval_ms)?)
    }

    /// Placement consumes clicks while a drawing tool is armed, preventing a
    /// mark from also panning the chart. A completed object returns to Pointer,
    /// matching the one-shot TradingView interaction.
    fn handle_drawing_placement(
        &mut self,
        ui: &egui::Ui,
        areas: &PlotAreas,
        bands: &[Band],
        chrome: &mut PaneChrome<'_>,
    ) -> bool {
        let magnet = chrome.toolrail.magnet();
        // Shift, read once for the whole pass: the preview, the press and the
        // release must agree about it as strictly as they agree about where
        // the pointer is. A parked hand supplies it for a run with nobody at
        // the keyboard, the same way it supplies the pointer.
        let constrain = if ui.input(|input| input.modifiers.shift) {
            drawings::Constrain::Level
        } else {
            self.parked_hand
                .map_or(drawings::Constrain::Free, |hand| hand.constrain)
        };
        let Some(tool) = chrome.toolrail.tool().drawing_tool() else {
            self.drawings.cancel_draft();
            self.drawing_hover = None;
            self.drawing_band_hint = None;
            self.drawing_press_position = None;
            self.drawing_press_started_empty = false;
            return false;
        };
        let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
        // Every band at once: the panes are drawing surfaces now, so hovering
        // one has to read as one rather than as dead space beneath the chart.
        let surface = bands
            .iter()
            .fold(bands[0].rect, |union, band| union.union(band.rect));
        let response = ui.interact(
            surface,
            self.interaction_id("drawing_placement"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = response.hover_pos();
        // Floating chrome is opaque to the pointer here too: a press on the
        // inspector must not drop an anchor on the canvas underneath it. The
        // Pointer path has always honoured this; placement reads the raw
        // pointer, so it has to ask the same question itself.
        let over_chrome = |ui: &egui::Ui, position: egui::Pos2| {
            ui.ctx()
                .layer_id_at(position)
                .is_some_and(|layer| layer != ui.layer_id())
        };
        let hovered = response
            .hover_pos()
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| bands::band_at(bands, position));
        // The accent hairline the draw pass puts on the band about to receive
        // the anchor — the split view's own "your next command lands here".
        let over_pane_chrome = response
            .hover_pos()
            .is_some_and(|position| Self::pane_chrome_hit(areas, position));
        self.drawing_band_hint = hovered
            .filter(|band| band.drawable() && !over_pane_chrome)
            .map(|band| band.rect);
        // The raw pointer, never the widget's hover.
        //
        // "Is this widget the top interactable" is a different question from
        // the one placement asks, which is "is the pointer inside the drawing
        // surface, and is floating chrome on top of it". The widget's answer
        // already had to be patched once, because a dragged widget is not
        // "hovered" (egui) and the rubber band blanked for exactly the frames
        // the trader was shaping the object; the patch read the raw pointer,
        // but only while a press was down.
        //
        // So the preview and the click were reading two different sources for
        // the same fact. That is the shape of the bug this change is about,
        // and it is also why the preview could not be tested at all: under a
        // headless context the widget reports no hover ever, so the preview
        // painted the bare anchors and no test could see the shape. The press
        // path's two questions are the honest ones and they are asked here
        // now, so preview and commit cannot disagree about where the pointer
        // is — in any host.
        let preview_pos = ui
            .input(|input| input.pointer.latest_pos())
            .filter(|position| surface.contains(*position))
            .or_else(|| self.parked_hand.map(|hand| hand.position));
        self.drawing_hover = preview_pos
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| {
                let (_, point) = self.shaped_placement(
                    tool,
                    areas,
                    bands,
                    position,
                    history_right,
                    magnet,
                    constrain,
                )?;
                Some(point)
            });
        if (response.hovered() || response.dragged()) && !over_pane_chrome {
            ui.ctx().set_cursor_icon(match hovered {
                Some(band) if band.drawable() => egui::CursorIcon::Crosshair,
                // A refusing band announces itself before the press, never by
                // swallowing the click that follows it.
                _ => egui::CursorIcon::NotAllowed,
            });
        }
        if let Some(refusal) = hovered.and_then(|band| band.refusal) {
            response.clone().on_hover_text(refusal);
        }

        let pressed_position = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if let Some(position) = pressed_position
            .filter(|position| surface.contains(*position) && !over_chrome(ui, *position))
            && let Some((band, point)) = self.shaped_placement(
                tool,
                areas,
                bands,
                position,
                history_right,
                magnet,
                constrain,
            )
        {
            let band = band.key.clone();
            self.drawing_press_started_empty = self.drawings.draft_len() == 0;
            self.drawing_press_position = Some(position);
            // The first anchor of a stroke also seeds its decimation, or the
            // very next frame records a second point on the same pixel and a
            // stationary click becomes a two-point "drawing".
            if tool.freehand() {
                self.freehand_last_position = Some(position);
            }
            self.place_drawing_point(tool, &band, point, chrome);
        }

        let released_position = ui.input(|input| {
            input
                .pointer
                .primary_released()
                .then(|| input.pointer.latest_pos())
                .flatten()
        });
        // A held drag, not N clicks: the press above laid the first anchor,
        // every frame the pointer stays down feeds the path, and the release
        // is what finishes the object.
        if tool.freehand() {
            if self.drawings.draft_len() > 0
                && ui.input(|input| input.pointer.primary_down())
                && let Some(position) = ui.input(|input| input.pointer.latest_pos())
                && let Some((band, position)) = self.placement_target(areas, bands, position)
                // The draft belongs to the band its first anchor landed in.
                // A hand that strays 15 px into the CVD pane mid-stroke
                // would otherwise write a CVD value into an object living on
                // the price axis — and the stroke, having no handles, could
                // only be deleted and redrawn. Points outside the draft's
                // own band are dropped; the stroke resumes when the hand
                // comes back.
                && self
                    .drawings
                    .draft()
                    .is_some_and(|draft| draft.band == tool.band_for(&band.key))
                && let Some(point) = self.drawing_point_at(
                    position,
                    history_right,
                    self.slots(),
                    magnet,
                    tool.anchor_snap(),
                    band,
                )
                // Decimate on the way in rather than simplifying afterwards.
                // A fast hand on a dense tape produces hundreds of points a
                // second, and every one of them costs a paint and a hit-test
                // on every later frame — for a shape whose whole value is
                // roughly where it is.
                && self
                    .freehand_last_position
                    .is_none_or(|last| last.distance(position) >= FREEHAND_MIN_STEP_PX)
                && self.drawings.draft_len() < FREEHAND_MAX_POINTS
            {
                self.freehand_last_position = Some(position);
                let band = band.key.clone();
                self.place_drawing_point(tool, &band, point, chrome);
            }
            if released_position.is_some() {
                self.freehand_last_position = None;
                if self.drawings.finish_draft() {
                    // Same one-shot rule the clicked tools follow.
                    if !chrome.toolrail.repeat() {
                        chrome.toolrail.arm(Tool::Pointer);
                    }
                    self.drawing_hover = None;
                }
                self.drawing_press_position = None;
                self.drawing_press_started_empty = false;
            }
            return true;
        }
        if tool.required_points() > 1
            && self.drawing_press_started_empty
            && let Some(start) = self.drawing_press_position
            && let Some(position) = released_position
            && surface.contains(position)
            && start.distance(position) >= DRAWING_DRAG_COMPLETES_PX
            && let Some((band, point)) = self.shaped_placement(
                tool,
                areas,
                bands,
                position,
                history_right,
                magnet,
                constrain,
            )
        {
            let band = band.key.clone();
            self.place_drawing_point(tool, &band, point, chrome);
        }
        if released_position.is_some() {
            self.drawing_press_position = None;
            self.drawing_press_started_empty = false;
        }
        true
    }

    /// Which band the next anchor belongs to, and where in it the pointer
    /// counts as being.
    ///
    /// A draft already down pins its band: an object with anchors in two
    /// value spaces would be a shape nobody can read. The pointer is then
    /// clamped into that band, so dragging a trend line up into the candles
    /// stretches it to the top of its own pane instead of writing a price
    /// into a CVD anchor. `None` where nothing may be placed.
    fn placement_target<'a>(
        &self,
        areas: &PlotAreas,
        bands: &'a [Band],
        position: egui::Pos2,
    ) -> Option<(&'a Band, egui::Pos2)> {
        if Self::pane_chrome_hit(areas, position) {
            return None;
        }
        let pinned = self
            .drawings
            .draft()
            .filter(|draft| draft.band != DrawingBand::AllBands)
            .and_then(|draft| bands.iter().find(|band| band.key == draft.band));
        let band = match pinned {
            Some(band) => band,
            None => bands::band_at(bands, position)?,
        };
        if !band.drawable() {
            return None;
        }
        let clamped = egui::pos2(
            position.x,
            position.y.clamp(band.rect.top(), band.rect.bottom()),
        );
        Some((band, clamped))
    }

    /// Where an anchor dropped at `position` really lands: the band that will
    /// own it, and the chart point it takes once the tool has had its say
    /// about an anchor it is still shaping
    /// ([`drawings::DrawingTool::pending_anchor`]).
    ///
    /// The preview, the press and the release all come through here. They
    /// used to compute their point apart from one another, and that is how a
    /// channel could be previewed as a corridor and then born as a line: the
    /// draft preview completed the geometry with the hovered anchor while the
    /// click that committed it read the raw pointer. One door, so the object
    /// a click creates is the one that was under the cursor when it was
    /// clicked.
    ///
    /// The shaped point is deliberately *not* re-clamped into the band. A
    /// tool floors a collapsed shape by pixels, so the anchor can end a hair
    /// outside the band it was aimed at — clamping it back would hand the
    /// degenerate case straight back to the trader, which is the whole thing
    /// being fixed.
    #[allow(clippy::too_many_arguments)]
    fn shaped_placement<'a>(
        &self,
        tool: drawings::DrawingTool,
        areas: &PlotAreas,
        bands: &'a [Band],
        position: egui::Pos2,
        history_right: f32,
        magnet: bool,
        constrain: drawings::Constrain,
    ) -> Option<(&'a Band, ChartPoint)> {
        let (band, position) = self.placement_target(areas, bands, position)?;
        let total = self.slots();
        // A tool shapes in the space it paints in, so the anchors already
        // down are handed over projected. A draft belonging to another tool
        // is not this tool's draft — `place_with` will start a fresh one, so
        // there is nothing shaped yet.
        //
        // A freehand draft is skipped, and the reason is runtime rather than
        // taste. This runs up to three times a frame while a tool is armed
        // (hover, press, release), and a pencil stroke holds up to
        // `FREEHAND_MAX_POINTS` anchors against a `SmallVec` that keeps four
        // inline — so projecting one would allocate and walk the whole stroke
        // every frame, during exactly the gesture where the hand is moving
        // fastest, to hand it to a port that has no anchor to shape: a
        // freehand tool declares no anchor count, and its draft is finished by
        // the release, never by a click. Every other tool's draft is three
        // anchors at most, which stays inline and allocates nothing.
        let shaped = match (self.drawings.draft(), band.scale.as_ref()) {
            (Some(draft), Some(scale)) if draft.tool == tool && !tool.freehand() => {
                let placed = self.projected_drawing_points(draft, history_right, total, scale);
                tool.pending_anchor(&placed, position, constrain)
            }
            _ => position,
        };
        let point = self.drawing_point_at(
            shaped,
            history_right,
            total,
            magnet,
            tool.anchor_snap(),
            band,
        )?;
        Some((band, point))
    }

    fn place_drawing_point(
        &mut self,
        tool: drawings::DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        chrome: &mut PaneChrome<'_>,
    ) {
        // A new object starts from whatever the trader told the app to
        // remember for this tool — assembled in one place, so the click path
        // and the scripted one open the same object. Existing objects are
        // never touched by that choice.
        let presets = chrome.presets;
        let completed = self.drawings.place_with(tool, band, point, |tool| {
            drawings::new_drawing_from_defaults(presets, tool)
        });
        if completed {
            // One-shot by default; the toolbox repeat pin keeps the tool
            // armed for the next object.
            if !chrome.toolrail.repeat() {
                chrome.toolrail.arm(Tool::Pointer);
            }
            // A tool whose content is words asks for the caret, not for a
            // panel — see `PaneChrome::begin_text_edit`.
            //
            // The object stands down here rather than waiting for the host to
            // notice next frame: the placement happens *inside* the canvas
            // pass, so a note placed by click would otherwise paint its grey
            // placeholder under the field that opens over it, for the one
            // frame between the two.
            if tool.holds_text() {
                self.content_editing = self.drawings.selected();
                *chrome.begin_text_edit = true;
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

    /// The topmost object of `band` under the pointer. Objects of the other
    /// bands are not candidates at all — see [`Self::drawing_in_band`].
    fn drawing_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        self.drawings
            .items()
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, drawing)| {
                self.drawings.is_visible(*index) && bands::drawing_in_band(drawing, band)
            })
            .find_map(|(index, drawing)| {
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                    content_editing: false,
                };
                drawing
                    .tool
                    .hit_test(band.rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
                    .then_some(index)
            })
    }

    /// Alt+click: deterministic z-order cycling through every visible object
    /// under the pointer. From the current selection, the next hit beneath
    /// it wins; past the bottom it wraps back to the top.
    fn drawing_below_selection(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        let hits: Vec<usize> = (0..self.drawings.items().len())
            .rev()
            .filter(|&index| self.drawings.is_visible(index))
            .filter(|&index| bands::drawing_in_band(&self.drawings.items()[index], band))
            .filter(|&index| {
                let drawing = &self.drawings.items()[index];
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                    content_editing: false,
                };
                drawing
                    .tool
                    .hit_test(band.rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
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

    /// Which handle of one object the pointer is on. The tool answers what
    /// its handles are, so the ring the trader sees is the ring they grab —
    /// a channel's width handle sits at the centre of a rail, not on the
    /// corner anchor that happens to define it.
    fn drawing_handle_in(
        &self,
        drawing_index: usize,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        if !self.drawings.is_visible(drawing_index) {
            return None;
        }
        let scale = band.scale.as_ref()?;
        let drawing = self
            .drawings
            .items()
            .get(drawing_index)
            .filter(|drawing| bands::drawing_in_band(drawing, band))?;
        let projected = self.projected_drawing_points(drawing, history_right, total, scale);
        let ctxt = DrawContext {
            payload: drawing.payload.as_ref(),
            anchors: &drawing.points,
            scale,
            px_per_bar: self.viewport.px_per_bar(),
            unit: band.unit(),
            primary_band: true,
            style: drawing.style,
            selected: self.drawings.selected() == Some(drawing_index),
            halo: false,
            content_editing: false,
        };
        anchor_hit(&drawing.tool.handles(band.rect, &projected, &ctxt), pos)
    }

    /// What a pointer at `pos` is on: a drawing's handle first, then its
    /// body. One function, so the press and the click that follows it can
    /// never answer differently — grabbing a handle *is* clicking the object,
    /// and the handle radius is the wider of the two.
    fn drawing_pick_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        self.drawing_handle_at(pos, band, history_right, total)
            .map(|(drawing_index, _)| drawing_index)
            .or_else(|| self.drawing_at(pos, band, history_right, total))
    }

    /// Apply one frame of a handle drag, with the pointer already resolved to
    /// the chart point the trader is on (magnet included).
    ///
    /// A tool that owns its handles answers with every anchor's new screen
    /// position and the host projects them back; the anchors it *derived* are
    /// exact by construction and are never snapped a second time — the magnet
    /// belongs to the point under the pointer, not to a rail computed from it.
    /// Everything else is the plain "handle `handle` is anchor `handle`" move.
    #[allow(clippy::too_many_arguments)]
    fn drag_drawing_handle(
        &mut self,
        drawing_index: usize,
        handle: usize,
        target: ChartPoint,
        band: &Band,
        history_right: f32,
        total: usize,
        constrain: drawings::Constrain,
    ) {
        let moved = band.scale.as_ref().and_then(|scale| {
            let drawing = self.drawings.items().get(drawing_index)?;
            let projected = self.projected_drawing_points(drawing, history_right, total, scale);
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band: true,
                style: drawing.style,
                selected: true,
                halo: false,
                content_editing: false,
            };
            let to = self.drawing_screen_point(target, history_right, total, scale);
            drawing
                .tool
                .drag_handle(band.rect, &projected, handle, to, &ctxt, constrain)
        });
        let Some(moved) = moved else {
            self.drawings.move_anchor(drawing_index, handle, target);
            return;
        };
        let anchors: Option<SmallVec<[ChartPoint; 4]>> = moved
            .iter()
            // Derived anchors are exact by construction — neither the magnet
            // nor a tool's own snap rule applies to them a second time.
            .map(|point| {
                self.drawing_point_at(
                    *point,
                    history_right,
                    total,
                    false,
                    drawings::AnchorSnap::Pointer,
                    band,
                )
            })
            .collect();
        if let Some(anchors) = anchors {
            self.drawings.set_points(drawing_index, &anchors);
        }
    }

    fn drawing_handle_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<(usize, usize)> {
        let selected = self.drawings.selected();
        if let Some(drawing_index) = selected
            && let Some(handle) =
                self.drawing_handle_in(drawing_index, pos, band, history_right, total)
        {
            return Some((drawing_index, handle));
        }
        (0..self.drawings.items().len())
            .rev()
            .filter(|drawing_index| Some(*drawing_index) != selected)
            .find_map(|drawing_index| {
                self.drawing_handle_in(drawing_index, pos, band, history_right, total)
                    .map(|handle| (drawing_index, handle))
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
        // The magnet applies to every anchor gesture, placement and re-drag
        // alike: a handle that snaps only while you first draw it would make
        // the second edit undo the precision of the first.
        let magnet = chrome.toolrail.magnet();
        // Remembered for inspector placement and manager centring: the pane
        // where drawings live, already free of both axes and the live lane.
        self.last_plot_area = Some(area);
        let areas = self.plot_areas(area, chrome.capabilities);
        self.last_chart_area = Some(areas.chart);
        // One carve, consumed by placement, hit-testing, dragging and — after
        // the panes have drawn — painting.
        let bands = self.bands(&areas);
        // A drawing tool consumes the *primary button*, not the chart. Pan,
        // wheel zoom, the pane dividers and the collapse chevrons all keep
        // working while one is armed: an armed tool used to return early from
        // here, which left the trader unable to move the chart they were
        // annotating (audit S2).
        let tool_armed = self.handle_drawing_placement(ui, &areas, &bands, chrome);
        let auto = self.last_auto_range;
        let height = self.last_chart_height;
        let total = self.slots();
        let divider = self.last_lane_divider_x;
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
        self.handle_tape_switch(ui, areas.chart, chrome);
        // The paper lines and the right-click price live on the candles, and
        // only there: an order is a price, not a value on someone's oscillator.
        let price_band = &bands[0];
        let drawing_scale = price_band.scale;
        // The price under a right-click, remembered before the menu eats
        // the pointer: the trade section places orders at it.
        if chart.secondary_clicked()
            && let Some(position) = chart.interact_pointer_pos()
            && areas.chart.contains(position)
            && let Some(scale) = drawing_scale.as_ref()
        {
            self.context_menu_price = Some(scale.price_at(position.y));
            // The same click, resolved once per placing tool through the
            // projection `drawing_point_at` owns, each with the tool's own
            // snap — the anchored VWAP's candle magnet included.
            let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
            self.context_menu_on_tape = self.click_on_tape(position.x);
            // The most specific thing under the click: a drawing, resolved
            // on the band the click actually landed in (a CVD line and a
            // price line can share the pixel). Right-click selects like the
            // primary press does, so the menu and the context bar agree on
            // which object is being acted on.
            let clicked = bands::band_at(&bands, position)
                .filter(|band| band.drawable())
                .and_then(|band| self.drawing_at(position, band, history_right, total));
            self.context_menu_drawing = clicked.map(|index| {
                self.drawings.select(Some(index));
                let drawing = &self.drawings.items()[index];
                self.context_menu_rename = drawing.name.clone().unwrap_or_default();
                drawing.id
            });
            self.context_menu_places.clear();
            for tool in drawings::DRAWING_TOOLS {
                if tool.context_menu_label().is_none() {
                    continue;
                }
                if let Some(point) = self.drawing_point_at(
                    position,
                    history_right,
                    total,
                    false,
                    tool.anchor_snap(),
                    price_band,
                ) {
                    self.context_menu_places.push((tool, point));
                }
            }
        }
        // Right-click: what is on this canvas, and what is not. Secondary
        // button only, so it shares no gesture with the pan, the zoom or the
        // drawing tools — a pan that ends anywhere never opens it.
        chart.context_menu(|ui| self.draw_layer_menu(ui, chrome));
        // While the menu is open the pointer is reading it, not the chart, so
        // no crosshair chases it across the candles behind it.
        if chart.context_menu_opened() {
            self.hover_pos = None;
        } else if let Some(id) = self.context_menu_drawing.take() {
            // The menu just closed. An in-flight rename commits here too:
            // dismissing the menu with an outside click is the natural
            // blur-to-commit gesture, and the TextEdit's own lost_focus
            // never runs once its closure stops being drawn.
            if let Some(index) = self.drawings.index_of(id) {
                let current = self.drawings.items()[index]
                    .name
                    .clone()
                    .unwrap_or_default();
                if self.context_menu_rename.trim() != current {
                    let name = std::mem::take(&mut self.context_menu_rename);
                    self.drawings.rename_at(index, &name);
                }
            }
        }
        let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
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
        // Simulated order lines take the pointer before the drawings: they
        // sit higher in the draw stack, and a grabbed stop is operational,
        // not annotational. The flag mirrors the drawings' gesture
        // consumption so the chart never pans under a held line; the chrome
        // gate applies at press time only, like everywhere else. Escape is
        // deliberately absent here — cancels live in the app's single escape
        // stack (`handle_drawing_keys`). Only the focused pane offers the
        // gesture, because the whole tab shares one simulator.
        //
        // Not while a drawing tool is armed: the button is the tool's, and it
        // can only be handed out once. Without this gate a click meant to
        // drop an anchor would *also* reach an order's ✕ and cancel it —
        // the early return this replaced used to hide that.
        //
        // The cmd-trading aim is the one paper gesture that claims the
        // *whole* plot rather than a line or a ✕, so it alone yields to an
        // annotation already under the pointer. The pane answers that
        // question here — paper never reads the drawings — for the same
        // reason it answers "a tool is armed": the button can be handed
        // out once, and the default buy modifier is Shift, the very key
        // that levels a channel corner.
        //
        // **Handles only, never a body.** A handle is a 12 px target where
        // the two gestures genuinely collide: Shift on a corner levels the
        // object, and there is no other way to ask for that. A body is a
        // region, and some bodies are enormous — a fixed-range profile's
        // hit test claims its whole histogram strip on purpose
        // (`fixed_range_profile.rs`), so yielding bodies meant a chart with
        // a profile on it had a hole where the aim simply did not appear.
        // Moving a body needs no modifier at all, and a body drag reads
        // Shift every frame, so pressing first and then holding it still
        // constrains the move.
        //
        // The canvas's own chrome counts too, and it does *not* live in a
        // floating layer, so `over_chrome` never sees it: the tape chip
        // and an indicator pane's header or divider are pixels a press
        // already means something on, and a modifier resting under the
        // hand must not turn "put the tape back" into "rest an order".
        //
        // Per-frame path, so it costs nothing on a frame with no modifier
        // down: the aim cannot exist without one, and only then is the
        // pick worth running — the same bounded, visible-objects-only
        // handle pick the drag initiation below performs, so an
        // *unselected* object's handle keeps its pixel too.
        let modifiers = ui.input(|input| input.modifiers);
        let modifier_down = modifiers.shift || modifiers.command || modifiers.alt;
        let canvas_claimed = pointer_position
            .filter(|_| modifier_down)
            .is_some_and(|position| {
                Self::pane_chrome_hit(&areas, position)
                    || tape_switch_rect(areas.chart).contains(position)
                    || (chrome.toolrail.tool() == Tool::Pointer
                        && !over_chrome
                        && bands::band_at(&bands, position)
                            .filter(|band| band.drawable())
                            .is_some_and(|band| {
                                self.drawing_handle_at(position, band, history_right, total)
                                    .is_some()
                            }))
            });
        let paper_layer_visible = self.layer_visible(ChartLayer::PaperTrading, chrome.style);
        // The wheel over the plot, offered to the paper layer first: with an
        // aim up it belongs to the ruler, and the chart's zoom is told below
        // to leave that frame's travel alone.
        let paper_scroll = pointer_position
            .filter(|position| drawing_area.contains(*position))
            .map_or(0.0, |_| {
                ui.input(|input| {
                    let delta = input.raw_scroll_delta;
                    // Windows turns a vertical wheel into *horizontal* scroll
                    // while a modifier is held, so the value the ruler needs
                    // arrives on `x` for exactly the gesture the ruler is
                    // made of. Reading only `y` meant the ruler saw nothing
                    // whenever the trader was actually holding the key.
                    if delta.y.abs() > f32::EPSILON {
                        delta.y
                    } else {
                        delta.x
                    }
                })
            });
        let paper_gesture = if chrome.paper_takes_input && !tool_armed {
            chrome.paper.handle_chart_input(&ChartInput {
                chart: drawing_area,
                scale: drawing_scale.as_ref(),
                pointer: pointer_position,
                primary_pressed: primary_pressed && !over_chrome,
                primary_down,
                primary_released,
                modifiers,
                canvas_claimed,
                scroll_y: paper_scroll,
                middle_pressed: ui
                    .input(|input| input.pointer.button_pressed(egui::PointerButton::Middle)),
                layer_visible: paper_layer_visible,
            })
        } else {
            if chrome.paper_takes_input {
                // A drawing tool owns the hand this frame; a stale cmd
                // preview must not keep painting under it.
                chrome.paper.clear_cmd_preview();
            }
            false
        };
        // The paper lines announce their grabbability (audit paper M3/M4):
        // drawings get hover cursors below, and a draggable stop must not
        // feel deader than an annotation — nor may the entry line's blocked
        // band refuse a pan with no explanation at all.
        // The layer gate lives inside `hover_cursor` itself, next to the
        // frame's other decisions, so it cannot be forgotten by a caller.
        if chrome.paper_takes_input
            && !over_chrome
            && !tool_armed
            && let Some(position) =
                pointer_position.filter(|position| drawing_area.contains(*position))
            && let Some(scale) = drawing_scale.as_ref()
            && let Some(cursor) = chrome.paper.hover_cursor(position, drawing_area, scale)
        {
            ui.ctx().set_cursor_icon(cursor);
        }
        let mut drawing_drag_consumes_gesture = false;
        if !paper_gesture && chrome.toolrail.tool() == Tool::Pointer {
            // The band under the pointer decides everything below: a price
            // trend line and a CVD trend line can be one pixel apart on
            // screen and mean unrelated things, so no pick ever crosses one.
            // Refusing bands and the panes' own chrome are not canvases.
            let pointer_band = pointer_position
                .filter(|_| !over_chrome)
                .filter(|position| !Self::pane_chrome_hit(&areas, *position))
                .and_then(|position| bands::band_at(&bands, position))
                .filter(|band| band.drawable());
            // Hover feedback: a resize cursor over a selected anchor, a move
            // cursor over any visible body, and not-allowed over locked
            // geometry (visible objects in the viewport only — bounded work).
            if let Some(band) = pointer_band
                && let Some(position) = pointer_position
            {
                if let Some(selected) = self.drawings.selected()
                    && self
                        .drawing_handle_in(selected, position, band, history_right, total)
                        .is_some()
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[selected].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::ResizeNwSe
                        });
                } else if let Some(hovered) = self.drawing_at(position, band, history_right, total)
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[hovered].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::Move
                        });
                }
            }
            // A click is the release of a press that never travelled, read
            // from the raw pointer rather than from the candles' response.
            // That is what makes a click in an indicator pane select at all:
            // the pane's own pan gesture covers the same pixels and would
            // otherwise be the only widget to hear it. `over_chrome` is
            // honoured at press time, so a press on a panel leaves no pending
            // origin here and no selection can be stolen through one.
            if primary_released
                && self.drawing_drag_pending_from.is_some()
                && let Some(position) = pointer_position
            {
                // Alt+click walks down the z-order through overlapping
                // objects; a plain click selects the topmost hit.
                //
                // A click selects what the *press* grabbed. The release must
                // not re-decide, because opening the panel moves the chart
                // under the pointer (see `drawing_press_pick`) and the object
                // the user pressed on is no longer at that pixel — the
                // release would wipe the selection the press just made, and
                // the panel would flicker open and shut with the mouse
                // standing still.
                //
                // Alt+click keeps re-deciding on purpose: it walks down the
                // z-order from the current selection, so it only ever runs
                // while a selection already exists and the layout is settled.
                let selected = if ui.input(|input| input.modifiers.alt) {
                    pointer_band.and_then(|band| {
                        self.drawing_below_selection(position, band, history_right, total)
                    })
                } else {
                    self.drawing_press_pick.take().unwrap_or_else(|| {
                        // No press was recorded (it landed on chrome, or off
                        // any band): fall back to asking now.
                        pointer_band.and_then(|band| {
                            self.drawing_pick_at(position, band, history_right, total)
                        })
                    })
                };
                self.drawings.select(selected);
                // A note under the pointer takes a double click: its words
                // *are* the object, so pointing at one and double clicking
                // asks to type in it — the same reading as double clicking a
                // curve to open its settings. It is read here rather than in
                // the free-chart branch above because a click on an object
                // starts a translate gesture, which is exactly what clears
                // that branch's `primary_free`. Without this, fixing a typo
                // meant hunting for a field in a panel that placing a note no
                // longer opens.
                if chart.double_clicked()
                    && let Some(index) = selected
                    && self
                        .drawings
                        .items()
                        .get(index)
                        .is_some_and(|drawing| drawing.tool.holds_text() && !drawing.locked)
                {
                    self.content_editing = Some(index);
                    *chrome.begin_text_edit = true;
                }
            }
            // Drag initiation reads the raw press (an `interact` per object
            // would be unbounded work), so it must honour the chrome gate
            // itself: a press on the inspector never grabs the stroke or the
            // handle underneath — the panel is opaque by contract.
            let mut drawing_drag_started = false;
            if primary_pressed
                && let Some(band) = pointer_band
                && let Some(position) = pointer_position
            {
                // One question, asked once, on the geometry the user was
                // actually looking at when they pressed.
                self.drawing_press_pick =
                    Some(self.drawing_pick_at(position, band, history_right, total));
                self.drawing_drag_pending_from = Some(position);
                if let Some((drawing_index, handle)) =
                    self.drawing_handle_at(position, band, history_right, total)
                {
                    self.drawings.select(Some(drawing_index));
                    self.drawing_drag = if self.drawings.items()[drawing_index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Handle {
                            drawing_index,
                            handle,
                        }
                    };
                } else if let Some(index) = self.drawing_at(position, band, history_right, total) {
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
            // A held button is not yet a drag. Until the pointer has left the
            // threshold the object does not move at all, so a click stays a
            // click — the alternative is that selecting a channel re-angles
            // it by two pixels of hand tremor, and the trader's level is
            // quietly no longer where they put it.
            //
            // `travel` is measured from the press, not accumulated per frame,
            // so crossing the threshold hands the gesture the *whole* movement
            // and the object does not trail the cursor by 4 px forever.
            let travel = match (self.drawing_drag_pending_from, pointer_position) {
                (Some(origin), Some(position)) => {
                    let travel = position - origin;
                    if travel.length() < DRAWING_DRAG_THRESHOLD_PX {
                        None
                    } else {
                        self.drawing_drag_pending_from = None;
                        Some(travel)
                    }
                }
                // No pending origin: the threshold was already passed earlier
                // in this gesture, so this frame's own delta drives it.
                (None, _) => Some(pointer_delta),
                (Some(_), None) => None,
            };
            if primary_down
                && !drawing_drag_started
                && let Some(travel) = travel
            {
                match self.drawing_drag {
                    DrawingDrag::Handle {
                        drawing_index,
                        handle,
                    } => {
                        // The object's own band, not the one under the
                        // pointer: dragging a CVD anchor up into the candles
                        // stretches it to the top of its pane, and never
                        // writes a price into a CVD anchor.
                        let dragged = self
                            .drawings
                            .items()
                            .get(drawing_index)
                            .and_then(|drawing| bands::band_of(&bands, drawing));
                        // Moving a mark keeps it glued to a bar's extreme:
                        // the rule that placed it is the rule that holds it.
                        let handle_snap = self
                            .drawings
                            .items()
                            .get(drawing_index)
                            .map_or(drawings::AnchorSnap::Pointer, |drawing| {
                                drawing.tool.anchor_snap()
                            });
                        if let Some(band) = dragged
                            && let Some(position) = pointer_position
                        {
                            let position = egui::pos2(
                                position.x.clamp(band.rect.left(), history_right),
                                position.y.clamp(band.rect.top(), band.rect.bottom()),
                            );
                            if let Some(point) = self.drawing_point_at(
                                position,
                                history_right,
                                total,
                                magnet,
                                handle_snap,
                                band,
                            ) {
                                self.drag_drawing_handle(
                                    drawing_index,
                                    handle,
                                    point,
                                    band,
                                    history_right,
                                    total,
                                    if ui.input(|input| input.modifiers.shift) {
                                        drawings::Constrain::Level
                                    } else {
                                        drawings::Constrain::Free
                                    },
                                );
                            }
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                    }
                    DrawingDrag::Translate => {
                        let dragged = self
                            .drawings
                            .selected()
                            .and_then(|index| self.drawings.items().get(index))
                            .and_then(|drawing| bands::band_of(&bands, drawing));
                        if let Some(band) = dragged
                            && let Some(scale) = band.scale
                        {
                            let (lo, hi) = scale.range();
                            let delta_bar = travel.x / self.viewport.px_per_bar();
                            // Per *band* height: a pane is a fraction of the
                            // chart's, and dividing by the candles' would move
                            // a CVD level by a fraction of the distance the
                            // pointer travelled. The sign follows the band's
                            // orientation — the object tracks the pointer,
                            // not the price axis.
                            let sign = if scale.is_inverted() { 1.0 } else { -1.0 };
                            let delta_value =
                                sign * f64::from(travel.y / band.rect.height()) * (hi - lo);
                            self.drawings.translate_selected(delta_bar, delta_value);
                            // Market time is what every other pane reads the
                            // object through; a move that left it behind
                            // would drag the mark here and leave its shared
                            // twin standing where it used to be.
                            self.retime_selected();
                        }
                    }
                    DrawingDrag::Blocked => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    }
                    DrawingDrag::None => {}
                }
            }
            // Marks the *other* pane owns, worked from this one (§D7).
            //
            // A shared object is one object, so the trader may grab it on
            // either chart it appears on — the alternative is a mark that can
            // be seen here and only deleted over there, which is the split
            // getting in the way of the work. This pane's own objects still
            // win the press: the mirror is the second answer, never the first.
            //
            // Everything below is said in market time and price, and the tab
            // hands it to the pane that holds the object. Nothing is written
            // to a copy.
            if !self.drawing_drag.is_active() {
                self.interact_shared(
                    ui,
                    chrome,
                    SharedPointer {
                        position: pointer_position,
                        area: drawing_area,
                        over_chrome,
                        pressed: primary_pressed,
                        down: primary_down,
                        released: primary_released,
                        history_right,
                        total,
                        magnet,
                    },
                );
            }
            drawing_drag_consumes_gesture =
                self.drawing_drag.is_active() || self.shared_drag.is_active();
            if primary_released {
                // One gesture, one undo entry — recorded only if it moved.
                self.drawings.commit_gesture();
                self.drawing_drag = DrawingDrag::None;
                // A press that ended in a drag rather than a click leaves its
                // answer unconsumed; it must not survive to decide the *next*
                // click, which may be somewhere else entirely. The click path
                // above already ran this frame and took it if it was a click.
                self.drawing_press_pick = None;
                self.drawing_drag_pending_from = None;
            }
        } else {
            self.drawing_drag = DrawingDrag::None;
            self.drawing_press_pick = None;
            self.drawing_drag_pending_from = None;
            self.shared_drag = SharedDrag::None;
            self.shared_drag_pending_from = None;
            self.shared_pointer_mark = None;
        }
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
        let grabbing_divider = chart.interact_pointer_pos().is_some_and(&on_divider);
        if total > 0 && chart.dragged() && !grabbing_divider && primary_free {
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
                .and_then(|pos| self.overlay_plot_at(pos))
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
        if time.hovered() || time.dragged() {
            // Same rule as the vertical axes (audit F5): the cursor is what
            // announces the zoom gesture.
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if time.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.viewport.zoom(2.0_f32.powf(scroll / SCROLL_ZOOM_PX));
            }
        }
        // The time axis's own menu, the price gutter's twin: what an axis
        // writes is switched from that axis. This one had no menu at all until
        // the compass gave it something to say.
        time.context_menu(|ui| {
            #[cfg(test)]
            self.layer_menu_rects.clear();
            let _ = self.layer_checkbox(ui, ChartLayer::PointerTime, chrome);
        });
        // Jump-to-live (audit F6): panned into history, the way back is one
        // click at the axis' live end. Registered after the strip gesture so
        // the click is the chip's, not a zoom-drag's — the lane divider's
        // own registration rule.
        if !self.viewport.follows_live() {
            let chip = ui.interact(
                live_chip_rect(history_strip),
                self.interaction_id("jump_to_live"),
                egui::Sense::click(),
            );
            if chip.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if chip.clicked() {
                self.viewport.snap_to_live();
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
                    orderflow.zoom_live_lane(2.0_f32.powf(scroll / SCROLL_ZOOM_PX));
                }
            }
        }

        // Right price gutter: the candles' own axis gesture. It spans their
        // height only — the bands below belong to the panes.
        let price_gutter = axis_zoom_gesture(
            ui,
            self.interaction_id("price_nav"),
            areas.price_gutter,
            &mut self.price_view,
            auto,
            true,
        );
        // The axis's own menu: about the scale and about what is written on
        // it, not about the canvas — the layer menu stays the canvas's
        // right-click. The compass's price half is offered here rather than
        // only in the layer menu because this is where a trader looks for
        // something about *this* axis, and where the mark it switches
        // actually appears.
        price_gutter.context_menu(|ui| {
            #[cfg(test)]
            self.layer_menu_rects.clear();
            let mut inverted = self.price_view.is_inverted();
            if ui
                .checkbox(&mut inverted, "Inverted chart")
                .on_hover_text(
                    "flip the chart upside down — low prices at the top. \
                     Also reached by dragging the axis down until the bars \
                     flatten and turn over",
                )
                .clicked()
            {
                self.price_view.set_inverted(inverted);
                ui.close_menu();
            }
            ui.separator();
            let _ = self.layer_checkbox(ui, ChartLayer::PointerPrice, chrome);
        });

        // The same gesture, once per pane, over the gutter band beside it.
        // Keyed by slot *and* pane id: slots are allocated per pane, so a
        // split's two charts can hold the same slot number and a slot-only id
        // would make one pane's axis answer for the other's.
        let pane_id = self.id;
        let mut pane_time_gesture = PaneGesture::default();
        // Collected here and parked on the pane below: the loop holds a mutable
        // borrow of `self.indicators`, and the dialog belongs to the app.
        let mut settings_request: Option<SlotId> = None;
        // Which pane, if any, was opened by a click on its own collapsed strip
        // on the last frame that had one — and what this frame decides to hand
        // to the next. See the disclosure block below.
        let strip_expanded = self.strip_expanded;
        let mut opened_from_strip = strip_expanded;
        for ((view, gutter), body) in self
            .indicators
            .visible_panes_mut()
            .zip(&areas.pane_gutters)
            .zip(&areas.indicator_panes)
        {
            axis_zoom_gesture(
                ui,
                egui::Id::new(("pane_price_nav", pane_id, view.slot)),
                *gutter,
                &mut view.scale,
                view.last_auto,
                false,
            );
            if !body.collapsed {
                // The body moves the scale the gutter scales. Registered after
                // the gutter so the two never fight over the same pixel: the
                // gutter is a band beside the pane, and egui gives an overlap
                // to the later claim.
                let gesture = pane_pan_gesture(
                    ui,
                    egui::Id::new(("pane_pan", pane_id, view.slot)),
                    body.rect,
                    &mut view.scale,
                    view.last_auto,
                    primary_free,
                );
                pane_time_gesture.pan_x += gesture.pan_x;
                pane_time_gesture.scroll_y += gesture.scroll_y;
            }
            // The disclosure, in both directions: a control that only opens is
            // half a control, so the square that brings a pane back is what
            // puts it away. Registered *last* for the same reason the body is
            // registered after the gutter — the later claim wins the overlap,
            // and this corner has to beat the pan that covers the whole band.
            let disclosure = ui.interact(
                indicator_render::pane_disclosure_rect(body.rect, body.collapsed),
                egui::Id::new(("pane_disclosure", pane_id, view.slot)),
                egui::Sense::click(),
            );
            if disclosure.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            // The pane's own handle into its settings when it is open: the
            // header row. Registered after the pan so it takes the double click
            // the body would otherwise spend resetting the scale; the pan keeps
            // every other pixel of the band.
            if !body.collapsed {
                let header = ui.interact(
                    indicator_render::pane_header_rect(body.rect, body.collapsed),
                    egui::Id::new(("pane_header", pane_id, view.slot)),
                    egui::Sense::click(),
                );
                if header.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if header.double_clicked() {
                    settings_request = Some(view.slot);
                }
            }
            // A collapsed strip is the fourth handle, and it needs the two
            // clicks of a double click read *across the frames between them*:
            // the first one expands the pane, so by the time the second arrives
            // the strip is gone, the pointer sits somewhere in the body of a
            // pane that is now a hundred pixels tall, and `body.collapsed` is
            // already false.
            //
            // Before this, that second click simply collapsed the pane again —
            // so double-clicking a collapsed strip expanded it and put it back,
            // and did nothing at all. It opens the settings instead, which is
            // the only reading that leaves the gesture worth making. What
            // carries it across the frames is `strip_expanded`: the slot whose
            // strip opened this pane, set by the click that opened it and spent
            // by the one that follows.
            if disclosure.double_clicked() && strip_expanded == Some(view.slot) {
                settings_request = Some(view.slot);
                opened_from_strip = None;
            } else if disclosure.clicked() {
                // A plain click, which egui reports only when it is *not* half
                // of a double one — so reaching here always means the previous
                // gesture is over and any flag it left is spent.
                opened_from_strip = None;
                if body.collapsed {
                    // Manual, not Auto: the automatic rule is what collapsed
                    // it, so handing it back would undo the click on the very
                    // next frame. An explicit height is served before the
                    // automatic ones and therefore always fits.
                    view.sizing = PaneSizing::Manual(MIN_PANE_HEIGHT_PX);
                    opened_from_strip = Some(view.slot);
                } else {
                    view.sizing = PaneSizing::Collapsed;
                }
            }
        }
        self.strip_expanded = opened_from_strip;
        if let Some(slot) = settings_request {
            self.pending_settings = Some(slot);
        }
        // Time, once, whichever pane the pointer was over: the panes share the
        // candles' x axis, so a sideways drag or a scroll there has to move the
        // same viewport the candles do — otherwise the same gesture would mean
        // one thing over the bars and nothing one pane below them.
        if total > 0 && pane_time_gesture.pan_x != 0.0 {
            self.viewport.pan_pixels(pane_time_gesture.pan_x, total);
        }
        // One wheel, one meaning at a time: while the ruler is walking a
        // bracket out from an aim, the same travel must not also zoom.
        if pane_time_gesture.scroll_y.abs() > 0.0 && !chrome.paper.consumed_scroll() {
            self.viewport
                .zoom(2.0_f32.powf(pane_time_gesture.scroll_y / SCROLL_ZOOM_PX));
        }
        // The dividers last of all: registered after every pane body so the
        // grab band takes the drag that would otherwise pan the pane behind
        // it, exactly as the canvas split's divider is registered after both
        // its panes and the lane's after the candles.
        let plot = areas.chart;
        for (view, slot) in self
            .indicators
            .visible_panes_mut()
            .zip(&areas.indicator_panes)
        {
            if let Some(sizing) = pane_divider_gesture(
                ui,
                egui::Id::new(("pane_divider", pane_id, view.slot)),
                slot,
                plot,
            ) {
                view.sizing = sizing;
            }
        }
    }

    /// The HUD anchor cached by the last draw, if the paper layer was
    /// painted on the pane that owns order entry.
    #[must_use]
    pub fn paper_hud_anchor(&self) -> Option<(egui::Rect, PriceScale)> {
        self.paper_hud_anchor
    }

    pub fn draw_chart(
        &mut self,
        painter: &egui::Painter,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.paper_hud_anchor = None;
        // Published before anything can return early, so an empty pane still
        // says where it is.
        self.last_area = Some(area);
        let canvas_background = background_color(chrome.style);
        painter.rect_filled(area, egui::Rounding::ZERO, canvas_background);

        // The ladders accumulate only while something consumes them: the
        // footprint layer, or a fixed-range-profile drawing (placed or being
        // placed) folding those same ladders over its bar span. Off, ingestion
        // pays nothing and holds nothing; the first frame after a switch-on
        // refolds the retained trades (declared cost, once). Then adopt the
        // book engine's capture bucket as the row grid (the instrument's
        // price_step where the feed declares one). Both before the frame
        // borrows the bar slices; both no-ops every frame but the one where
        // something changed.
        let footprint_blocked = self
            .layer_blocked(ChartLayer::Footprint, chrome.capabilities)
            .is_some();
        let footprint_on =
            (self.footprint_visible || self.wants_range_profile()) && !footprint_blocked;
        self.state.set_footprint_enabled(footprint_on);
        // Accumulating is not painting, and the candles answer to the second.
        // A range profile turns the ladders *on* without ever asking for the
        // layer, so the switch above cannot be what dresses a candle: doing
        // that put every bar into the footprint's sidebar lane, or faded its
        // body down to an outline, for a layer that then drew nothing.
        // Computed here beside the switch it is so easily confused with, and
        // before the frame borrows fields out of `self`.
        let footprint_paints =
            self.layer_visible(ChartLayer::Footprint, chrome.style) && !footprint_blocked;
        if footprint_on
            && let Some(base) = self
                .orderflow
                .as_mut()
                .map(OrderflowView::capture_grouping_now)
        {
            self.state.set_footprint_group(base);
        }

        // Field borrows, not `self` borrows: the tape below needs `&mut
        // self.orderflow` while these are alive.
        let prefix = self.history_prefix.as_slice();
        let closed = self.state.bars();
        let partial = self.state.partial();
        let closed_total = prefix.len() + closed.len();
        let total = closed_total + usize::from(partial.is_some());

        // Snapshot the forming bar's ladder at ~10 Hz rather than per print;
        // between snapshots the drawn numbers hold still. Taken here, with
        // the accumulation switch, because it has two consumers now — the
        // footprint layer and the range-profile drawings — and each reading
        // the live ladder on its own cadence would show two different bars.
        if footprint_on {
            let now = painter.ctx().input(|i| i.time);
            match self.state.partial_footprint() {
                Some(partial_ladder) => {
                    let stale =
                        self.footprint_live
                            .as_ref()
                            .is_none_or(|(taken, snapshot_slot, _)| {
                                *snapshot_slot != closed_total
                                    || now - *taken >= LIVE_LADDER_REFRESH_S
                            });
                    if stale {
                        self.footprint_live = Some((now, closed_total, partial_ladder.clone()));
                        self.footprint_live_version = self.footprint_live_version.wrapping_add(1);
                    }
                }
                None => {
                    if self.footprint_live.take().is_some() {
                        self.footprint_live_version = self.footprint_live_version.wrapping_add(1);
                    }
                }
            }
        }
        let areas = self.plot_areas(area, chrome.capabilities);
        // Indicator panes claimed the bottom band inside `plot_split`, so the
        // rect the candles scale to is the same one the input handler uses.
        let chart_rect = areas.chart;
        self.last_price_gutter = Some(areas.price_gutter);
        self.last_time_strip = Some(split_time_strip(areas.time_strip, self.last_lane_divider_x).0);
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
                orderflow.draw_status_badge(painter, chart_rect, TAPE_SWITCH_RESERVED_PX);
            }
            self.draw_tape_switch(painter, chart_rect);
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
        // Band and live edge in one look at the published book: the panes need
        // the instant the band's right edge stands for, and reading it again
        // further down would put a second worker-mutex wait on the render
        // thread for a number already in hand.
        let live_lane = self
            .orderflow
            .as_mut()
            .and_then(|orderflow| orderflow.live_lane(chart_rect.width()));
        let lane_width_px = live_lane.map_or(0.0, |lane| lane.width_px);
        // Everything left of the divider is the candles' pane. They pan and
        // zoom inside it exactly as they did when it was the whole chart.
        self.last_lane_divider_x =
            crate::orderflow_render::lane_divider_x(chart_rect, lane_width_px);
        self.last_chart_rect = Some(chart_rect);
        self.lane_rungs = lane_rungs(
            self.last_lane_divider_x
                .map_or(0.0, |divider| chart_rect.right() - divider),
        );
        let history_rect = egui::Rect::from_min_max(
            chart_rect.min,
            egui::pos2(
                self.last_lane_divider_x
                    .unwrap_or_else(|| chart_rect.right()),
                chart_rect.bottom(),
            ),
        );

        // The projection margin is enforced here, against the rect the candles
        // are actually drawn in, rather than in the input handler: panning
        // leaves the future end open and zooming knows nothing about the
        // window, and the window itself moves without any gesture at all (the
        // app resizing, the lane divider dragged, a pane collapsed). Painting
        // is the one place that sees all of it, so it is the one place the
        // rule holds — pushed fully left, the newest bar stops at the left
        // edge and the rest of the window is empty canvas to project into.
        self.viewport.clamp_to_window(history_rect.width(), total);
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
        let scale = self
            .price_view
            .scale(auto_range, chart_rect.top(), chart_rect.bottom());

        let cw = self.viewport.candle_width();
        let half = chrome.style.candles.body_half_width(cw);
        let right = history_rect.right();

        // How the candle behaves under the footprint is the *style's* answer,
        // not this function's: a style that draws inside the candle needs its
        // interior, and one that draws in a box beside it needs the candle out
        // of the way entirely. With the layer off, candles are untouched at
        // any zoom.
        // The style that will actually draw, not the one that was asked for: a
        // style below its own zoom floor hands over, and the candle must be
        // laid out for whichever one paints. Asking the requested style put a
        // sidebar lane under a style that draws full width.
        let requested_style = self
            .footprint_override
            .as_ref()
            .unwrap_or(chrome.footprint)
            .style;
        let footprint_style = self.footprint_lod.effective_style(requested_style);
        let treatment = footprint_style.candle_treatment();
        // The lane a sidebar candle keeps at the left of its slot, and the
        // style the layer leaves the candle in. Both from one function, whose
        // whole point is that they answer to `footprint_paints` and never to
        // the accumulation switch — see `footprint_render::candle_dressing`.
        let (candle_lane, faded_candles) = crate::footprint_render::candle_dressing(
            footprint_paints,
            treatment,
            cw,
            chrome.style.candles,
        );
        // The half-width the footprint's content actually spans. The lane is
        // cut out of *this*, so the candle placed beside it has to be measured
        // from the same edge — measuring from the candle's own body width put
        // it inside the box the lane was reserved next to, where the opaque
        // plate then painted straight over it.
        let content_half = treatment.content_half_width(cw, half);
        let candles = faded_candles.as_ref().unwrap_or(&chrome.style.candles);

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        // The projection builds a lane exactly when the layout draws one. Tied
        // to `lane_width_px` rather than restated, because the two decide the
        // same thing: with them apart, the newest prints would be clustered and
        // sized as lane prints and then squeezed into a single candle slot.
        // Only the engine's own bars carry tape, so the timeline starts at
        // the first *state* bar's global slot: when the window straddles the
        // venue seam (a time-cutting flow pane, audit S1), that is the seam
        // itself, not the window's first slot.
        let timeline = VisibleBarTimeline::new(
            self.state.timeline_revision(),
            closed_start.max(prefix.len()),
            visible_state,
            partial_visible,
        );
        // Two surfaces consume the projection without being the depth map or
        // the bubbles: the live strip draws the same clusters, and the lane's
        // marks need the frame's live edge. Stated here, every frame, from the
        // layers this pane owns — so with the bubbles hidden the pipeline stays
        // alive for the strip, and with every other flow layer off the lane is
        // still marked instead of being a reserved but empty band whose menu
        // entry claims it is on.
        let demand = self.projection_demand();
        let orderflow_frame = self.orderflow.as_mut().and_then(|orderflow| {
            orderflow.set_projection_demand(demand);
            // The tape's automatic window comes from the newest bars of the
            // series, never from the slice on screen: panning the candles is
            // not a statement about how much market time the tape shows.
            orderflow.project_visible(
                timeline,
                lane_width_px > 0.0,
                end == total,
                Some(quantick_orderflow::reserved_span_ms(self.state.bars())),
                scale.range(),
            )
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
                self.price_view.is_inverted(),
            );
        }

        // Bring the range-profile drawings' folds up to date before anything
        // paints over the map. Key-guarded inside: the common frame compares
        // one small key per profile object and folds nothing. It runs after
        // the heatmap projection on purpose — the map's left boundary is
        // where each profile's paint cuts from fill to silhouette, and the
        // O(cells) scan behind it is paid only while a profile object exists.
        let heat_first_slot = orderflow_frame
            .as_ref()
            .filter(|_| {
                self.orderflow
                    .as_ref()
                    .is_some_and(OrderflowView::depth_visible)
                    && self.wants_range_profile()
            })
            .and_then(|frame| frame.first_heat_slot());
        // Read before the drawings are borrowed mutably below.
        let partial_bucket_slot = self.partial_bucket_slot();
        let folding = crate::frvp::refresh(
            &mut self.drawings,
            &crate::frvp::RefreshInputs {
                state: &self.state,
                budget: crate::frvp::fold_budget(),
                prefix,
                partial_ladder: self.footprint_live.as_ref().map(|(_, _, ladder)| ladder),
                partial_version: self.footprint_live_version,
                blocked: footprint_blocked,
                side_inferred: chrome.side_inferred,
                heat_first_slot,
                draft_hover_bar: self.drawing_hover.map(|point| point.bar),
                partial_bucket_slot,
            },
        );
        if folding {
            // A range too long for one pass: paint what is folded and come
            // straight back for the next slice. Without this the fill would
            // stall wherever the tape happened to stop waking the window.
            painter.ctx().request_repaint();
        }
        // The anchored-VWAP objects' cached rows, same pass discipline: a key
        // comparison per object on the common frame, a replay only when the
        // tape or the config moved (see `crate::avwap`).
        crate::avwap::refresh(
            &mut self.drawings,
            &crate::avwap::RefreshInputs {
                state: &self.state,
                prefix: &self.history_prefix,
            },
        );

        // What the compass will say, decided before either axis labels itself:
        // both axes stand aside where a chip is going to land, and a decision
        // made twice is a decision two surfaces can disagree about.
        let compass = self.pointer_compass(chart_rect, right, total, &scale, chrome);
        // The candles' own segment of the time axis: past the lane divider the
        // strip is the tape's rolling window, which labels itself.
        let (history_strip, _) = split_time_strip(areas.time_strip, self.last_lane_divider_x);
        // Every claim below is a height a chip will *really* occupy. A claim
        // for a chip that is not drawn is a round number silently missing from
        // the axis — the mirror of the defect this mechanism exists for, and
        // the reason each one repeats its painter's own gate rather than
        // assuming it.
        let on_axis = |y: f32| (chart_rect.y_range().contains(y)).then_some(y);
        let mut price_claims = pointer_compass::AxisClaims::new();
        let mut time_claims = pointer_compass::AxisClaims::new();
        if let Some(compass) = compass.as_ref() {
            if compass.price {
                price_claims.extend(on_axis(compass.readout.position.y));
            }
            if compass.time {
                time_claims.extend(
                    pointer_compass::time_tag(painter, history_strip, &compass.readout)
                        .map(|(centre, _)| centre),
                );
            }
        }
        // The armed crosshair writes its own price tag on this axis, on the
        // same geometry and with no compass involved. It is a chip like any
        // other and the axis stands aside for it too.
        if chrome.toolrail.tool() == Tool::Crosshair
            && self.layer_visible(ChartLayer::Crosshair, chrome.style)
            && let Some(pointer) = self.hover_pos.filter(|pos| chart_rect.contains(*pos))
        {
            price_claims.extend(on_axis(pointer.y));
        }
        // The market's own chip. `draw_last_price` refuses to draw one off the
        // pane, and `PriceScale::y` extrapolates rather than clamping, so the
        // claim has to be bounded the same way or panning the last price out
        // of view would leave a hole at the top of the axis.
        if self.layer_visible(ChartLayer::LastPrice, chrome.style)
            && let Some(bar) = partial.or_else(|| closed.last())
            && let Some(price) = bar.close.to_f64()
        {
            price_claims.extend(on_axis(scale.y(price)));
        }
        // Gathered once, read twice: the axis stands aside for these just
        // below, and the same list is what gets painted onto the gutter
        // further down. Borrowed out of the pane so the container survives
        // the frame and the next one refills it rather than reallocating —
        // and lent to the axis as a slice, so the claims list stays the chips
        // the axis draws itself and never spills onto the heap.
        let mut levels = std::mem::take(&mut self.price_axis_levels);
        if self.layer_visible(ChartLayer::Drawings, chrome.style) {
            self.price_axis_levels(chart_rect, right, total, &scale, &mut levels);
        } else {
            levels.clear();
        }

        // Grid + price labels first, behind the candles. Labels anchor on the
        // gutter's edge, past the live strip when one is shown.
        let axis_x = areas.price_gutter.left();
        let price_claims = PriceAxisClaims {
            marks: price_claims,
            levels: &levels,
        };
        self.draw_price_axis(painter, chart_rect, axis_x, &scale, &price_claims, chrome);

        // Candles, clipped to their own pane: panning far enough into history
        // sends the newest bars off the right of it, and they scroll out of
        // sight behind the tape instead of being drawn over it.
        let clip = painter.with_clip_rect(history_rect);
        let viewport = &self.viewport;
        // Every candle this frame draws, in order: its bar index, the bar
        // itself, and whether it is still forming. One bar, one candle — the
        // law `Viewport::candle_width` states — so this is simply the visible
        // bars, borrowed.
        let visible_candles = |paint: &mut dyn FnMut(usize, &quantick_engine::Bar, bool)| {
            for (offset, bar) in visible_closed().enumerate() {
                paint(closed_start + offset, bar, false);
            }
            if let Some(partial) = partial_visible {
                paint(closed_total, partial, true);
            }
        };
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
                let (top, bottom) = scale.band(
                    bar.high.to_f64().unwrap_or(0.0),
                    bar.low.to_f64().unwrap_or(0.0),
                );
                clip.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - half, top),
                        egui::pos2(xc + half, bottom),
                    ),
                    egui::Rounding::ZERO,
                    canvas_background,
                );
            };
            visible_candles(&mut |index, bar, _forming| {
                clear_bar(viewport.x_center(index, right, total), bar);
            });
        }
        // Objects that are *context* rather than annotation go down here,
        // between the liquidity map and the candles: a volume profile is read
        // the way the heatmap is, and drawn over the price it tints every body
        // it covers.
        //
        // The **price band only**, and that is a correctness bound rather than
        // an optimisation. An indicator band's scale is written when its own
        // curve draws, further down this function, so a band carved here would
        // be a frame behind the plot it belongs to — which is exactly the
        // invariant the over-candles carve says it exists to keep. The price
        // band has no such dependency, so it is the one band that can be
        // carved this early and still be right. A tool wanting a background
        // pass on an indicator band would need its own carve after that pane
        // draws; there is none, and inventing a stale one for it would be
        // worse than not offering it.
        let mut carved = std::mem::take(&mut self.last_bands);
        self.carve_bands(&areas, &mut carved);
        if let Some(price_band) = carved.iter().next() {
            self.draw_drawings(painter, price_band, 0, right, total, DrawPass::UnderCandles);
        }
        // Asked once for the whole frame: on a chart where no indicator paints
        // — every chart until a script calls `barcolor` — the per-bar lookup
        // below never runs at all.
        let painted = self.indicators.paints_any();
        visible_candles(&mut |index, bar, forming| {
            let xc = viewport.x_center(index, right, total);
            // Plot rows map 1:1 onto bars (see `PlotX`), and one bar is one
            // candle at every zoom, so a drawn candle covers exactly its own
            // row.
            let paint = painted
                .then(|| self.indicators.slot_paint(index..index + 1, forming))
                .flatten();
            // A sidebar candle moves into the lane the footprint left it at
            // the slot's left edge; every other case draws where it always
            // did, at full body width. One call, two geometries — never a
            // second candle path, which would drift from this one.
            let slot = if candle_lane > 0.0 {
                // A third of the lane each side, so the body is a body and the
                // wick still has room to show either side of it.
                let sliver = (candle_lane * SIDEBAR_BODY_FRAC).max(1.0);
                crate::candle_view::BarSlot {
                    xc: xc - content_half + sliver + 1.0,
                    half_width: sliver,
                }
            } else {
                crate::candle_view::BarSlot {
                    xc,
                    half_width: half,
                }
            };
            draw_candle(&clip, slot, &scale, bar, forming, candles, paint);
        });
        // The footprint rides directly on the candles, before everything
        // drawn over them: it is a representation of the bars themselves,
        // not an annotation. Prefix (venue) candles carry no tape and draw
        // no ladder — the layer starts where trade-built bars start.
        if footprint_paints {
            // The forming bar's ladder is the ~10 Hz snapshot taken with the
            // accumulation switch at the top of the frame, shared with the
            // range-profile drawings.
            let frame = crate::footprint_render::LayerFrame {
                painter: &clip,
                chart_rect: history_rect,
                scale: &scale,
                footprints: self.state.bar_footprints(),
                first_state_slot: prefix.len(),
                visible: (start, end),
                partial: self
                    .footprint_live
                    .as_ref()
                    .map(|(_, _, ladder)| ladder)
                    .filter(|_| partial_visible.is_some()),
                partial_slot: closed_total,
                x_center: &|slot| viewport.x_center(slot, right, total),
                // The *content* half-width, which is not always the candle's.
                // A style that draws inside the candle is bounded by it; one
                // that draws in a box beside it is bounded only by the slot,
                // and charging it the candle gap as well spends a quarter of
                // the row on air twice over.
                half: content_half,
                candle_width: cw,
                side_inferred: chrome.side_inferred,
                depth_visible: self
                    .orderflow
                    .as_ref()
                    .is_some_and(OrderflowView::depth_visible),
                pixels_per_point: painter.ctx().pixels_per_point(),
                // Field access, not `self.footprint_config(..)`: the method
                // borrows all of `self` and the draw below needs
                // `self.footprint_lod` mutably. Same resolution rule.
                config: self.footprint_override.as_ref().unwrap_or(chrome.footprint),
            };
            crate::footprint_render::draw_layer(&frame, &mut self.footprint_lod);
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
        // The lane's window of tape time, and the closes inside it. Both are
        // the same for every pane, so they are resolved once here rather than
        // per pane — and both come from the tape's own numbers, so a pane's
        // curve lands under the prints it was computed from.
        let lane_window = self
            .last_lane_divider_x
            .zip(live_lane)
            .and_then(|(divider, lane)| {
                let orderflow = self.orderflow.as_ref()?;
                let window = orderflow.live_lane_window_ms(visible_state).max(1);
                Some((divider, lane.end_ms.saturating_sub(window), lane.end_ms))
            });
        let lane_steps: Vec<(i64, usize)> =
            lane_window.map_or_else(Vec::new, |(_, start_ms, _)| {
                let first = prefix.len();
                // Walked back from the newest close and reversed in place: the
                // window holds a handful of bars, and building it front to back
                // would mean scanning every closed bar the chart has ever seen.
                let mut steps: Vec<(i64, usize)> = closed
                    .iter()
                    .enumerate()
                    .rev()
                    .take_while(|(_, bar)| bar.close_time >= start_ms)
                    .map(|(index, bar)| (bar.close_time, first + index))
                    .collect();
                steps.reverse();
                steps
            });
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
                    egui::pos2(history_rect.left(), pane.rect.top()),
                    egui::pos2(history_rect.right(), pane.rect.bottom()),
                ),
                lane: lane_window.map(|(divider, start_ms, end_ms)| indicator_render::LaneFrame {
                    rect: egui::Rect::from_min_max(
                        egui::pos2(divider, pane.rect.top()),
                        egui::pos2(chart_rect.right(), pane.rect.bottom()),
                    ),
                    start_ms,
                    end_ms,
                    steps: &lane_steps,
                }),
                gutter: *gutter,
                background: canvas_background,
                grid,
                collapsed: pane.collapsed,
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
                self.price_view.is_inverted(),
            );
        }

        // The canvas's key, in a pass of its own so the bubble switch cannot
        // take it down with them. It starts below everything already stacked
        // at this corner — the chart header, the position HUD while a
        // position is open, and one row per indicator chip — so nothing at the
        // top-left prints over anything else.
        //
        // The HUD's row counts only where the HUD paints: on the focused
        // pane — exactly the condition this pane caches its anchor under,
        // further down this same draw. The anchor is not readable yet this
        // frame (it is written after the paper layer), so the condition is
        // restated here rather than read back.
        let hud_here = chrome.paper_hud_here && chrome.paper.position_summary().is_some();
        let legend_inset = crate::orderflow_render::LEGEND_HEADER_CLEARANCE_PX
            + crate::indicator_legend::hud_offset_px(hud_here)
            + crate::indicator_legend::stack_height_px(
                self.indicators.all(),
                self.legend_collapsed,
            );
        if let Some(orderflow) = self.orderflow.as_mut()
            && let Some(frame) = &orderflow_frame
        {
            orderflow.draw_legend(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
                legend_inset,
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
        //
        // Carved *here*, after the panes drew: each band's scale is then the
        // one its own curve was just drawn with, which is the invariant this
        // whole feature rests on.
        // Re-carved, not reused: the pass above ran before the indicator panes
        // drew, and every band's scale is written *by* that draw. Into the
        // pane's own buffer: same geometry as the input pass computed, no
        // container allocated, and what the tab's shared projection reads
        // afterwards.
        self.carve_bands(&areas, &mut carved);
        for (index, band) in carved.iter().enumerate() {
            self.draw_drawings(painter, band, index, right, total, DrawPass::OverCandles);
        }
        self.last_bands = carved;
        // Which band the next anchor lands in, said the way the split view
        // already says which pane has focus: one accent hairline on the top
        // edge. Painted after the drawings so a dense band cannot bury it.
        if let Some(hint) = self.drawing_band_hint {
            painter.line_segment(
                [
                    egui::pos2(hint.left(), hint.top() + 0.5),
                    egui::pos2(hint.right(), hint.top() + 0.5),
                ],
                egui::Stroke::new(1.0_f32, theme::ACCENT),
            );
        }

        // Closed-trade marks sit between the drawings and the live paper
        // lines: history under the orders that are still working. Only the
        // session's trades paint — the tape on screen proves their fills;
        // rows loaded from earlier sessions stay in the ledger. And only
        // where this pane's own bars reach the fill's instant, which is why
        // the mapping handed over is `covering_slot_at_time` and not the
        // clamping `slot_at_time`: a trade the tape has not got to yet has
        // no bar to stand on, and standing it on the edge one is the pile-up
        // a replay seek used to draw.
        if self.layer_visible(ChartLayer::TradePaint, chrome.style) {
            let frame = crate::trade_paint::TradePaintFrame {
                painter,
                chart_rect,
                scale: &scale,
                background: canvas_background,
                pointer: self.hover_pos,
                tz: chrome.tz,
            };
            // The window once, not once per fill: `draw` asks about every
            // closed round trip of the session, twice each, every frame.
            let covered = self.covered_window();
            crate::trade_paint::draw(
                &frame,
                chrome.paper.session_trades(),
                chrome.paper.selected_trade_index(),
                |ms| {
                    covered
                        .filter(|(oldest, newest)| ms >= *oldest && ms <= *newest)
                        .and_then(|_| self.slot_at_time(ms))
                },
                |slot| self.viewport.x_center(slot, right, total),
            );
        }

        // Simulated orders and the position sit above the drawings: they are
        // operational state, read against the last price painted next. The
        // unclipped painter carries their chips into the gutter. Both panes
        // paint them — one market, one set of price levels, and a level is as
        // true on the 5-minute context as it is on the flow chart. Prices out
        // of a pane's visible range simply do not draw.
        //
        // Switched off, they are only unpainted: the orders keep working and
        // the dock keeps listing them (see the layer's hint).
        if self.layer_visible(ChartLayer::PaperTrading, chrome.style) {
            // The last-price chip's row, computed up front so the paper
            // chips can dodge it: at the instant a market order fills the
            // entry *is* the last price, and two chips on one pixel mangle
            // the only persistent position statement.
            let reserved_chip_y = if self.layer_visible(ChartLayer::LastPrice, chrome.style) {
                partial
                    .or_else(|| closed.last())
                    .and_then(|bar| bar.close.to_f64())
                    .map(|price| scale.y(price))
                    .filter(|y| *y >= chart_rect.top() && *y <= chart_rect.bottom())
            } else {
                None
            };
            // Tags anchor inside the interactive plot (left of the live
            // lane when one is up) — the same right edge the input pass
            // hands to `handle_chart_input`, so a painted ✕ and its press
            // agree about where it is.
            let tag_right = self.last_lane_divider_x.unwrap_or(chart_rect.right());
            // Hover affordances paint only on the pane whose pointer feeds
            // the paper input; the others keep display-only tags. Every pane
            // still paints the lines themselves — an order is a fact about
            // the account, true on whichever chart you are looking at.
            let paper_pointer = if chrome.paper_takes_input {
                self.hover_pos.or_else(|| {
                    chrome
                        .paper
                        .forced_hover_pointer(chart_rect, tag_right, &scale)
                })
            } else {
                None
            };
            chrome.paper.draw_layer(
                painter,
                chart_rect,
                tag_right,
                axis_x,
                &scale,
                reserved_chip_y,
                paper_pointer,
            );
            if chrome.paper_hud_here {
                self.paper_hud_anchor = Some((chart_rect, scale));
            }
        }

        // Above the flow layers: everything else on the canvas is read against
        // it. Drawn on the unclipped painter so the chip reaches the gutter.
        // The trader's own levels on the axis, and then the market's price
        // over them. That order and not the other way round: a level is a
        // static annotation whose value the trader already knows, the last
        // price is live market data, and the moment the two coincide — price
        // arriving at the level — is exactly the moment the live number must
        // not be the one that gets covered.
        self.draw_axis_marks(
            painter,
            chart_rect,
            axis_x,
            &scale,
            &levels,
            partial.or_else(|| closed.last()),
            chrome,
        );
        // The candles' own marks, so they are placed and clipped in their
        // pane: where venue candles give way to bars built from prints, and
        // where backfilled prints give way to live ones.
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.draw_seam_divider(painter, history_rect, total, cw);
        }
        if self.layer_visible(ChartLayer::BackfillDivider, chrome.style) {
            self.draw_backfill_divider(painter, history_rect, total, cw);
        }
        // Under the same switch as the venue seam: both answer "what is the
        // provenance of the bars either side of this line?", and a trader who
        // turned that class of mark off meant this one too.
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.draw_feed_gaps(painter, history_rect, total, cw, chrome.feed_gaps);
        }
        self.draw_time_strip(
            painter,
            areas.time_strip,
            start,
            end,
            total,
            &time_claims,
            chrome,
        );
        if let Some(orderflow) = self.orderflow.as_ref() {
            self.draw_lane_time_axis(
                painter,
                split_time_strip(areas.time_strip, self.last_lane_divider_x).1,
                orderflow.live_lane_window_ms(closed),
                orderflow.tape_age(),
            );
            // The automatic reference this frame, kept for the tape's menu:
            // the entry that says "follows the bars" has to be able to say
            // what that works out to, and the menu is drawn without the bars
            // in reach. Recorded from the same bars the axis was just drawn
            // from, so the label and the axis can never disagree.
            self.last_lane_reference_ms = Some(reserved_span_ms(closed));
        }
        // The way back from history (audit F6), painted over the strip's
        // labels on the same geometry the input path registered.
        if !self.viewport.follows_live() {
            let (history_strip, _) = split_time_strip(areas.time_strip, self.last_lane_divider_x);
            draw_live_chip(painter, live_chip_rect(history_strip));
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
        if self.layer_visible(ChartLayer::Crosshair, chrome.style) {
            self.draw_crosshair(painter, chart_rect, axis_x, &scale, chrome);
        }
        // Last of the canvas marks, so the answer the trader is asking for by
        // holding the mouse where they are holding it is on top of the ones
        // the chart volunteers. Decided far above, where the axes read it too.
        if let Some(compass) = compass.as_ref() {
            self.draw_pointer_compass(painter, compass, axis_x, areas.time_strip, chrome);
        }
        // The status badge is not a layer: it reports whether the source is
        // healthy, and a chart with every layer off must still say that. It
        // shares the top-right corner with the tape switch, which is drawn last
        // and holds the corner itself.
        if let Some(orderflow) = self.orderflow.as_ref() {
            orderflow.draw_status_badge(painter, chart_rect, TAPE_SWITCH_RESERVED_PX);
        }
        self.draw_tape_switch(painter, chart_rect);

        // The levels' container, back on the pane for the next frame to
        // refill rather than reallocate.
        self.price_axis_levels = levels;

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
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's own geometry and window, passed rather than cached: reading a stale copy off the pane is how an axis ends up labelling a window it is no longer showing"
    )]
    fn draw_time_strip(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        start: usize,
        end: usize,
        total: usize,
        claims: &pointer_compass::AxisClaims,
        chrome: &PaneChrome<'_>,
    ) {
        painter.line_segment(
            [
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.right(), strip.top()),
            ],
            egui::Stroke::new(1.0_f32, grid_color(chrome.style)),
        );
        let font = egui::FontId::monospace(crate::chart::TIME_LABEL_FONT_PX);
        let y = strip.center().y;
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let (history_strip, _) = split_time_strip(strip, self.last_lane_divider_x);

        // Measured, not counted. One layout per format per frame — monospace,
        // so a format's sample answers for every label written in it — and the
        // stride comes out of pixels rather than out of a fixed label count
        // that a narrower strip could not honour.
        let width_of = |format: crate::chart::TimeLabelFormat| {
            painter
                .layout_no_wrap(format.sample().to_owned(), font.clone(), theme::TEXT_MUTED)
                .size()
                .x
        };
        let format = crate::chart::time_label_format(history_strip.width(), width_of);
        let label_width = width_of(format);
        // The pointer's chip is always written in full, whatever this strip
        // thinned its own labels down to, so the two extents are asked for
        // separately: a narrow strip pairs a 30 px label with a 54 px chip.
        let chip_width = width_of(crate::chart::TimeLabelFormat::Full);
        // Per *bar*, not per slot: the walk below steps a bar at a time and
        // labels the bar it lands on, so how far apart two labels end up is
        // how far apart two bars are.
        let stride = crate::chart::time_label_stride(self.viewport.px_per_bar(), label_width);

        let mut index = start;
        while index < end {
            if let Some(bar) = self.closed_bar(index) {
                let x = self.viewport.x_center(index, history_strip.right(), total);
                // The whole label, not just its centre: a label centred a few
                // pixels from the end drew its other half over the gutter.
                if crate::chart::label_fits(
                    x,
                    label_width,
                    history_strip.left(),
                    history_strip.right(),
                ) && !pointer_compass::claimed(
                    x,
                    label_width,
                    chip_width,
                    claims.iter().copied(),
                ) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        fmt_time_as(bar.open_time, chrome.tz, format),
                        font.clone(),
                        theme::TEXT_MUTED,
                    );
                }
            }
            index = index.saturating_add(stride);
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
        tape_age: Option<quantick_orderflow::TapeAge>,
    ) {
        let Some(strip) = lane_strip else {
            return;
        };
        // Clipped to the strip, because both labels are sized from the text
        // rather than from the room: a lane narrow enough to make the warning
        // wider than its own strip would otherwise push it left, over the
        // candles' own time labels. The tape's axis may run out of room; it
        // may not spill into the pane beside it.
        let painter = &painter.with_clip_rect(strip);
        let font = egui::FontId::monospace(LANE_AXIS_FONT_PX);
        // The warning is its own text, pinned to the right end of the strip,
        // and the window keeps the centre it has always had. One label growing
        // a suffix would re-centre itself every time a quiet stretch started
        // and ended — a caption sliding under a tape being read for flow. The
        // right end is also where it belongs: directly under the edge the
        // missing marks should have reached.
        let warning = lane_lag_label(window_ms, tape_age)
            .map(|lag| painter.layout_no_wrap(lag, font.clone(), theme::WARN));
        // Room the warning denies the window label. Doubled, because the window
        // keeps the strip's own centre: a centred label grows by half its
        // width towards each end, so it reaches the warning after only half
        // the distance, and subtracting the warning once would let a
        // mid-width lane pass this check and draw the two on top of each
        // other. Two gaps rather than one for the same reason — one holds the
        // warning off the strip's edge, and the other is the space between
        // the two labels, which is what the constant is for. Reserving a
        // single gap left them legal at zero pixels apart.
        let taken = warning.as_ref().map_or(0.0, |galley| {
            2.0 * (galley.size().x + 2.0 * LANE_AXIS_GAP_PX)
        });
        let window_label = format!("tape · {}", format_window_ms(window_ms));
        let window_galley = painter.layout_no_wrap(window_label, font, theme::TEXT_MUTED);
        // A strip too narrow keeps the urgent label and drops this one. The
        // window is a setting the trader chose and can read from the tape's
        // own menu; how old the newest mark is exists nowhere else.
        //
        // It applies with no warning up too, so a lane narrower than this
        // label draws no axis at all rather than a clipped one. Half a word
        // under a tape is not a shorter way of saying the same thing.
        if window_galley.size().x + taken <= strip.width() {
            painter.galley(
                egui::Align2::CENTER_CENTER
                    .align_size_within_rect(window_galley.size(), strip)
                    .min,
                window_galley,
                theme::TEXT_MUTED,
            );
        }
        if let Some(galley) = warning {
            // Right, under the edge the missing marks should have reached —
            // unless it does not fit, and then hard left instead.
            //
            // The clip decides *which end* gets cut, and for this label that
            // is the difference between a shortened sentence and a wrong
            // number. Right-aligned, a 40 px strip cuts the head off
            // "no print for 1 min 30 s" and leaves "30 s" sitting in warn
            // colour: a ninety-second hole read as three. Left-aligned the cut
            // lands on the tail, where a clipped word is visibly a clipped
            // word. A caption that runs out of room may say less; it may not
            // say something else.
            let fits = galley.size().x + 2.0 * LANE_AXIS_GAP_PX <= strip.width();
            let align = if fits {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            };
            painter.galley(
                align
                    .align_size_within_rect(
                        galley.size(),
                        strip.shrink2(egui::vec2(LANE_AXIS_GAP_PX, 0.0)),
                    )
                    .min,
                galley,
                theme::WARN,
            );
        }
    }

    /// Right-hand price axis: round-number gridlines and labels. `axis_x` is
    /// the gutter's left edge — the chart's right edge normally, the live
    /// strip's right edge while the strip sits between them.
    /// A gridline label landing on a height `claims` has already promised to a
    /// chip stays unwritten, rather than being drawn under one.
    fn draw_price_axis(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        claims: &PriceAxisClaims<'_>,
        chrome: &PaneChrome<'_>,
    ) {
        let grid = grid_color(chrome.style);
        let (lo, hi) = scale.range();
        let font = egui::FontId::monospace(chart::AXIS_LABEL_FONT_PX);
        // Measured once per frame, the way the time strip measures its own:
        // every label on this axis is one line of the same font, so one
        // layout answers for all of them.
        let label_height = painter
            .layout_no_wrap("0".to_owned(), font.clone(), theme::TEXT_MUTED)
            .size()
            .y;
        for tick in crate::chart::nice_ticks(lo, hi, 8) {
            let y = scale.y(tick);
            if y < chart_rect.top() || y > chart_rect.bottom() {
                continue;
            }
            // The *line* is drawn either way: a gridline under a chip is
            // still the grid, and hiding it would put a gap in the chart
            // wherever the pointer went.
            painter.line_segment(
                [
                    egui::pos2(chart_rect.left(), y),
                    egui::pos2(chart_rect.right(), y),
                ],
                egui::Stroke::new(1.0_f32, grid),
            );
            // The chips on this axis are the same font and padding as the
            // labels, so one extent answers for both — unlike the time strip,
            // where they differ.
            if pointer_compass::claimed(y, label_height, label_height, claims.heights()) {
                continue;
            }
            painter.text(
                egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y),
                egui::Align2::LEFT_CENTER,
                pointer_compass::price_text(tick),
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

        // Same geometry as the crosshair tag and the compass's, because it is
        // the same code: one owner for where a price sits on this axis.
        pointer_compass::paint_price_tag(
            painter,
            axis_x,
            y,
            pointer_compass::price_text(price),
            color,
            LAST_PRICE_CHIP_TEXT,
        );
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

    /// The colour an object is really painted in, honesty fade included.
    ///
    /// A mark whose anchors are outside the loaded series, or that was drawn
    /// on the instrument this tab used to show, is painted faded rather than
    /// passed off as a level on this market. Its axis tag wears the same fade
    /// for the same reason and through the same call: a full-strength chip on
    /// the gutter would be the axis making a claim the stroke beside it is
    /// explicitly not making.
    fn painted_color(drawing: &Drawing) -> egui::Color32 {
        if drawing.off_series || drawing.foreign_market {
            drawing.style.color.gamma_multiply(Self::CLAMPED_OPACITY)
        } else {
            drawing.style.color
        }
    }

    /// Opacity a drawing is painted at on a pane whose series does not reach
    /// its anchors — a mirrored mark clamped to an edge, or one of this
    /// pane's own that outlived the bars it was drawn on. Faded, not hidden
    /// and not silently snapped: the mark is real, its position on *this*
    /// chart is not exact.
    const CLAMPED_OPACITY: f32 = 0.45;

    /// Carve this pane into its bands, into a buffer the caller owns.
    ///
    /// The geometry is per-frame by nature — rects and scales move with pan
    /// and zoom — but the container is not: the caller hands the same buffer
    /// back every frame, so after the first one this allocates nothing.
    ///
    /// See [`crate::bands`] for the invariant it upholds.
    fn carve_bands(&self, areas: &PlotAreas, out: &mut Bands) {
        let history = self.drawing_area(areas.chart);
        bands::carve(
            out,
            &bands::PriceBand {
                rect: history,
                range: self
                    .last_auto_range
                    .map(|auto| self.price_view.resolve(auto)),
                top: areas.chart.top(),
                bottom: areas.chart.bottom(),
                inverted: self.price_view.is_inverted(),
            },
            &self.indicators,
            areas,
            &self.price_band_label,
        );
    }

    /// The bands of this frame, in a fresh buffer — the input pass, which has
    /// no buffer of its own to lend.
    fn bands(&self, areas: &PlotAreas) -> Bands {
        let mut out = Bands::new();
        self.carve_bands(areas, &mut out);
        out
    }

    /// What the chrome says about the band an object lives on.
    ///
    /// Answered from the indicator list rather than from the last frame's
    /// bands, so it is the same answer before the first paint and while the
    /// pane is hidden behind an eye — hidden is not removed.
    #[must_use]
    pub fn band_label(&self, drawing: &Drawing) -> BandLabel {
        match &drawing.band {
            DrawingBand::Price => BandLabel::Price,
            DrawingBand::AllBands => BandLabel::AllBands,
            DrawingBand::Indicator(key) => self
                .indicators
                .all()
                .iter()
                .filter(|view| !view.descriptor.overlay)
                .find(|view| &self.indicators.pane_key(view) == key)
                .map_or_else(
                    || BandLabel::Parked(std::sync::Arc::clone(&key.kind)),
                    |view| {
                        // A band drawing can stop painting for four different
                        // reasons, and only one of them is "the indicator is
                        // gone". A mark that vanishes unexplained is what
                        // teaches a trader to stop trusting the tool, so each
                        // reason says its own name.
                        match () {
                            () if view.error.is_some() => BandLabel::Unpainted(
                                view.label_shared(),
                                "this indicator is in error, so its pane and everything drawn on                                  it are not being painted",
                            ),
                            () if view.hidden => BandLabel::Unpainted(
                                view.label_shared(),
                                "this indicator is hidden - show it again and the object comes                                  back with it",
                            ),
                            () if view.sizing == PaneSizing::Collapsed => BandLabel::Unpainted(
                                view.label_shared(),
                                "this pane is collapsed - open it with the chevron and the object                                  is there",
                            ),
                            () => BandLabel::Indicator(view.label_shared()),
                        }
                    },
                ),
        }
    }

    /// The band key of every indicator pane, with one value its own series
    /// actually holds at `slot`.
    ///
    /// The seam the `QUANTICK_DRAWINGS_DEMO=bands` hook seeds through: an
    /// object placed at a sampled value lands on the curve it annotates, so
    /// a screenshot proves the projection rather than a number someone made
    /// up. A pane with nothing computed at that slot contributes nothing.
    #[must_use]
    pub fn indicator_band_samples(&self, slot: usize) -> Vec<(DrawingBand, f64)> {
        self.indicators
            .visible_panes()
            .filter_map(|view| {
                let value = view
                    .columns
                    .iter()
                    .find_map(|column| column.get(slot).copied().filter(|v| v.is_finite()))?;
                Some((
                    DrawingBand::Indicator(self.indicators.pane_key(view)),
                    value,
                ))
            })
            .collect()
    }

    /// Pixels inside a band that belong to the pane's own chrome rather than
    /// to its canvas: the collapse chevron and the divider grab band.
    ///
    /// A drawing gesture never takes them. egui hands an overlapping rect to
    /// whoever registers last, and both of those register after the canvas —
    /// but the drawing path reads the raw pointer rather than a response, so
    /// it has to honour that order itself instead of inheriting it. Without
    /// this, arming a tool silently kills the chevron and the pane resize.
    fn pane_chrome_hit(areas: &PlotAreas, pos: egui::Pos2) -> bool {
        areas.indicator_panes.iter().any(|slot| {
            indicator_render::pane_disclosure_rect(slot.rect, slot.collapsed).contains(pos)
                // The header opens the pane's settings; like the chevron and
                // the divider, arming a drawing tool must not silently kill it.
                || indicator_render::pane_header_rect(slot.rect, slot.collapsed).contains(pos)
                || (pos.y - slot.rect.top()).abs() <= PANE_DIVIDER_HANDLE_PX
        })
    }

    /// How much of the selected object's own axis one pixel is worth.
    ///
    /// The keyboard nudge reads this instead of the candles' scale: a level
    /// on a CVD band moved by a quantity of *price* is a wrong number
    /// delivered through the one gesture that exists for precision, and every
    /// press would record it as its own undo entry. `None` when nothing is
    /// selected, when the object is parked, or before the first frame.
    #[must_use]
    pub fn selected_value_per_px(&self) -> Option<f64> {
        let drawing = self.drawings.items().get(self.drawings.selected()?)?;
        let band = bands::band_of(&self.last_bands, drawing)?;
        let scale = band.scale?;
        let (lo, hi) = scale.range();
        let per_px = (hi - lo) / f64::from(band.rect.height().max(1.0));
        // Signed for the *screen* gesture: an upward step raises the value on
        // an upright band and lowers it on an inverted one, so the arrows
        // keep moving the object the way the key points.
        Some(if scale.is_inverted() { -per_px } else { per_px })
    }

    /// The band drawings live in: the candles minus the live lane.
    ///
    /// The lane is the tape's own reserved strip at the right edge — a live
    /// region, not a place for annotations, and a horizontal line running
    /// across it was drawing over the flow. Placement already refused to put
    /// an anchor there; paint, geometry and hit-test agree with it now, which
    /// also keeps a line's painted end and its grabbable end the same pixel.
    pub(crate) fn drawing_area(&self, chart: egui::Rect) -> egui::Rect {
        let right = self
            .last_lane_divider_x
            .unwrap_or(chart.right())
            .clamp(chart.left(), chart.right());
        egui::Rect::from_min_max(chart.min, egui::pos2(right, chart.bottom()))
    }

    /// This pane's own projection, rebuilt from the geometry the last
    /// [`Self::draw_chart`] cached. `None` before the pane has drawn once, or
    /// while it has no price range to project against.
    fn last_projection(&self) -> Option<(egui::Rect, f32, usize, PriceScale)> {
        let chart = self.last_chart_area?;
        let auto = self.last_auto_range?;
        let scale = self.price_view.scale(
            auto,
            self.last_chart_top,
            self.last_chart_top + self.last_chart_height,
        );
        let history_right = self.last_lane_divider_x.unwrap_or(chart.right());
        Some((self.drawing_area(chart), history_right, self.slots(), scale))
    }

    /// Resolve the pointer against the exact geometry the last frame painted.
    ///
    /// This is deliberately a pull operation. The normal frame loop only
    /// records the position it already needs for the crosshair; bar, price,
    /// L2 cell, and drawing hit-testing happen here only when a control client
    /// requests the cursor scope.
    #[must_use]
    pub(crate) fn control_pointer_hit(&self) -> Option<ControlPointerHit> {
        let position = self.hover_pos?;
        let chart = self.last_chart_area?;
        if !chart.contains(position) {
            return None;
        }
        let band = bands::band_at(&self.last_bands, position)?;
        let history_right = self.last_lane_divider_x.unwrap_or_else(|| chart.right());
        let total = self.slots();

        // The same question the axis compass paints the answer to, asked
        // through the same owner: a client reading the cursor and a trader
        // reading the axis may not be told two different bars.
        let slot = (total > 0 && position.x <= history_right)
            .then(|| self.viewport.slot_at_x(position.x, history_right, total))
            .flatten();
        let axis_value = band.scale.as_ref().map(|scale| scale.price_at(position.y));
        // What the pointer's y means on this band. A time-only band has no
        // value axis of its own, so y is read on the pane's price axis, which
        // is what `axis_value` below is computed from.
        let axis_unit = match &band.key {
            DrawingBand::Price | DrawingBand::AllBands => "price".to_owned(),
            DrawingBand::Indicator(_) => "indicator_value".to_owned(),
        };

        let drawing_pick = self
            .drawing_handle_at(position, band, history_right, total)
            .map(|(index, handle)| (index, Some(handle)))
            .or_else(|| {
                self.drawing_at(position, band, history_right, total)
                    .map(|index| (index, None))
            });
        let drawing = drawing_pick.and_then(|(index, handle_index)| {
            let drawing = self.drawings.items().get(index)?;
            Some(ControlDrawingHit {
                id: drawing.id,
                tool_id: drawing.tool.id(),
                label: format!("{} {}", drawing.tool.name(), index + 1),
                user_label_present: drawing.name.is_some(),
                handle_index,
                selected: self.drawings.selected() == Some(index),
                locked: drawing.locked,
            })
        });

        let lane_width_px = (chart.right() - history_right).max(0.0);
        let flow_cell = self.orderflow.as_ref().and_then(|orderflow| {
            orderflow.control_flow_cell_at(
                chart,
                &self.viewport,
                total,
                lane_width_px,
                self.price_view.is_inverted(),
                position,
            )
        });
        let band_name = crate::control::drawing_band_name(&band.key);
        Some(ControlPointerHit {
            screen_x_px: position.x,
            screen_y_px: position.y,
            band: band_name.to_owned(),
            axis_value,
            axis_unit,
            slot,
            bar: slot.and_then(|slot| self.candle_at_slot(slot).cloned()),
            flow_cell,
            drawing,
        })
    }

    /// Which overlay indicator's plotted line a pointer at `pos` is sitting
    /// on, if any — the fourth place a double click opens settings from, and
    /// the most direct one: the thing the trader wants to change is the line
    /// they are looking at.
    ///
    /// Measured against the projection the *last* frame drew (§D8: no gesture
    /// re-measures a world it moved), and against the resolved style rather
    /// than the declared one, so a plot the trader switched off in the dialog
    /// cannot be picked where it is no longer drawn.
    ///
    /// Cost: nothing per frame — this runs on a double click only, and is then
    /// bounded by the visible bars of each overlay's plots, the same span the
    /// renderer already walks every frame.
    fn overlay_plot_at(&self, pos: egui::Pos2) -> Option<SlotId> {
        let (chart, right, total, scale) = self.last_projection()?;
        let (start, end) = self.viewport.visible_range(chart.width(), total);
        let mut best: Option<(f32, SlotId)> = None;
        for view in self.indicators.visible_overlays() {
            for index in 0..view.descriptor.plots.len() {
                let Some(resolved) = view.plot_style(index) else {
                    continue;
                };
                if !resolved.visible {
                    continue;
                }
                let Some(column) = view.columns.get(index) else {
                    continue;
                };
                // The segments the renderer joins, tested as segments: at a
                // wide zoom a fast series climbs further between two bars than
                // any point tolerance would forgive, and a line you can only
                // grab directly over a bar is a line that ignores most clicks.
                let stop = end.min(column.len());
                let mut previous: Option<egui::Pos2> = None;
                for (row, value) in column[start..stop].iter().copied().enumerate() {
                    let row = row + start;
                    if value.is_nan() {
                        previous = None;
                        continue;
                    }
                    let point =
                        egui::pos2(self.viewport.x_center(row, right, total), scale.y(value));
                    if let Some(from) = previous {
                        let distance = drawings::distance_to_segment(pos, from, point);
                        if distance <= PLOT_PICK_TOLERANCE_PX
                            && best.is_none_or(|(closest, _)| distance < closest)
                        {
                            best = Some((distance, view.slot));
                        }
                    }
                    previous = Some(point);
                }
            }
        }
        best.map(|(_, slot)| slot)
    }

    /// What a pointer at `pos` grabs among `source`'s shared marks: a handle
    /// anywhere first, then the topmost body — the same order, and the same
    /// primitives, this pane uses on its own objects.
    ///
    /// Runs on the projection cached by the last frame, which is the geometry
    /// the trader was looking at when they pressed (§D8: no gesture re-measures
    /// a world it moved).
    ///
    /// Per-frame cost: nothing until a mark is actually shared, and bounded by
    /// the handful of objects on the chart when one is.
    pub fn shared_pick(&self, source: &Self, pos: egui::Pos2) -> Option<(usize, Option<usize>)> {
        if !source.drawings.items().iter().any(Drawing::shared) {
            return None;
        }
        let (_, history_right, total, _) = self.last_projection()?;
        // Only the band the pointer is in: a mirrored CVD level and a
        // mirrored price level can be one pixel apart on screen and mean
        // unrelated things, exactly as they can on the pane that owns them.
        let band = bands::band_at(&self.last_bands, pos)?;
        let scale = band.scale?;
        let mut body = None;
        for (index, drawing) in source.drawings.items().iter().enumerate().rev() {
            if !drawing.shared()
                || !source.drawings.is_visible(index)
                || !bands::drawing_in_band(drawing, band)
            {
                continue;
            }
            let Some((anchors, _)) = self.reproject(drawing) else {
                continue;
            };
            let points: SmallVec<[egui::Pos2; 4]> = anchors
                .iter()
                .map(|anchor| self.drawing_screen_point(*anchor, history_right, total, &scale))
                .collect();
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band: true,
                style: drawing.style,
                selected: source.drawings.selected() == Some(index),
                halo: false,
                content_editing: false,
            };
            // Handle drags cross the pane boundary as "move anchor N", so a
            // tool whose handles are not its anchors — a channel's rail
            // handles move anchors they do not sit on — offers none on the
            // mirror. The mark still selects and still moves as a whole
            // there; reshaping it happens on the chart it was drawn on. An
            // invisible grab point on the mirror would be worse than an
            // absent one: the ring the trader sees is the ring they get.
            if drawing.tool.handles_are_anchors(band.rect, &points, &ctxt)
                && let Some(anchor) = anchor_hit(&points, pos)
            {
                return Some((index, Some(anchor)));
            }
            if body.is_none()
                && drawing
                    .tool
                    .hit_test(band.rect, &points, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
            {
                body = Some((index, None));
            }
        }
        body
    }

    /// Apply an edit another pane of this tab made to one of this pane's
    /// shared marks, back in this pane's own bar space.
    ///
    /// The instants arrive as they were read off the other chart and are
    /// resolved here, which is what makes the two views one object rather than
    /// two copies of one.
    pub fn apply_shared_edit(&mut self, edit: SharedEdit) {
        match edit {
            SharedEdit::Select(index) => self.drawings.select(Some(index)),
            SharedEdit::MoveAnchor {
                index,
                anchor,
                time_ms,
                price,
            } => {
                let Some(bar) = self.slot_of_time(time_ms) else {
                    return;
                };
                self.drawings.move_anchor(
                    index,
                    anchor,
                    ChartPoint::at_time(bar, price, Some(time_ms)),
                );
            }
            SharedEdit::Translate {
                index,
                delta_ms,
                delta_price,
            } => self.translate_shared(index, delta_ms, delta_price),
        }
    }

    /// Move a whole shared object by an amount of market time and price.
    ///
    /// Time, not bars: the two panes cut the tape differently, so the same
    /// drag is a different number of bars on each — and market time is what
    /// both of them mean by it.
    ///
    /// Resolved in full before anything is written. A drag that would put part
    /// of the object where this pane's series cannot reach moves nothing at
    /// all, rather than leaving a shape with one end on a bar and the other on
    /// an instant that has none.
    fn translate_shared(&mut self, index: usize, delta_ms: i64, delta_price: f64) {
        let Some(drawing) = self.drawings.items().get(index) else {
            return;
        };
        if drawing.locked {
            return;
        }
        let mut moved: SmallVec<[ChartPoint; 4]> = SmallVec::new();
        for point in &drawing.points {
            let Some(time) = point.time_ms.and_then(|time| time.checked_add(delta_ms)) else {
                return;
            };
            let Some(bar) = self.slot_of_time(time) else {
                return;
            };
            moved.push(ChartPoint::at_time(
                bar,
                point.price + delta_price,
                Some(time),
            ));
        }
        for (anchor, point) in moved.into_iter().enumerate() {
            self.drawings.move_anchor(index, anchor, point);
        }
    }

    /// Paint the shared drawings that live on `source`, re-expressed on this
    /// pane (`docs/ux/drawing-tools-2026-08.md` §D7).
    ///
    /// The two panes cut the same trades into different bars, so a bar index
    /// means nothing across them — the market timestamp each anchor captured
    /// at placement is the only coordinate they share. Every anchor goes back
    /// through this pane's own `slot_at_time`.
    ///
    /// Read-only here, on purpose: selection, dragging and the inspector
    /// belong to the pane the object was drawn on. A mark that could be
    /// grabbed in two places would be two versions of one object, which is
    /// exactly the confusion sharing exists to remove.
    ///
    /// Per-frame cost: nothing at all until a drawing is actually shared —
    /// the loop below runs over `source.drawings` and does nothing for the
    /// `ThisChart` default every object opens with.
    pub fn paint_shared_from(&self, painter: &egui::Painter, source: &Self) {
        if !source.drawings.items().iter().any(Drawing::shared) {
            return;
        }
        let Some((_, history_right, total, _)) = self.last_projection() else {
            return;
        };
        for (index, drawing) in source.drawings.items().iter().enumerate() {
            if !drawing.shared() || !source.drawings.is_visible(index) {
                continue;
            }
            let Some((anchors, clamped)) = self.reproject(drawing) else {
                continue;
            };
            // A clamped anchor is an honest half-truth: the object really is
            // off the end of this pane's series, and it says so by fading
            // rather than by pretending to sit on the edge bar.
            let style = if clamped {
                DrawingStyle {
                    color: drawing.style.color.gamma_multiply(Self::CLAMPED_OPACITY),
                    fill_alpha: 0,
                    ..drawing.style
                }
            } else {
                drawing.style
            };
            // A value is portable only inside the same value space. The x half
            // crosses through market time, but a CVD level means nothing on a
            // price axis — so a band drawing appears on this pane only where
            // the same indicator does, and nowhere else. Refused by
            // construction, not by a warning the trader could ignore.
            //
            // A time-only object is the case where sharing is most obviously
            // right: one instant, marked through every band of both charts.
            for (band_index, band) in self.last_bands.iter().enumerate() {
                if !bands::drawing_in_band(drawing, band) {
                    continue;
                }
                let Some(scale) = band.scale else {
                    continue;
                };
                let clipped = painter.with_clip_rect(band.rect);
                // Stack-allocated: this is a per-frame path, and every shipped
                // tool has at most three anchors, so the heap is never touched.
                let points: SmallVec<[egui::Pos2; 4]> = anchors
                    .iter()
                    .map(|anchor| self.drawing_screen_point(*anchor, history_right, total, &scale))
                    .collect();
                // A mark selected here shows it here. The trader can take and
                // move it from this pane (`Self::interact_shared`), and a
                // selection that painted only on the other chart would leave
                // the gesture with no visible subject.
                let selected = source.drawings.selected() == Some(index);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &anchors,
                    scale: &scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: drawing.band != DrawingBand::AllBands || band_index == 0,
                    style,
                    selected,
                    halo: false,
                    // The object lives on `source`, so that is the pane that knows
                    // whether its words are in an editor right now. Left
                    // hardcoded, a shared note being typed kept painting its
                    // old words on the companion chart — the same double
                    // render the editor stands the original down to avoid.
                    content_editing: source.content_editing == Some(index),
                };
                // Both halves, so a shared object is the same object on both
                // charts. A tool whose body lives in the background pass —
                // the volume profile's histogram — would otherwise cross to
                // the companion pane as two edge lines and a level, with the
                // volume shape the trader shared missing entirely.
                //
                // The mirrored copy paints over the candles rather than under
                // them: this pass runs after the host pane's own candles are
                // down, and reaching under them would mean carving a third
                // time on the far pane's geometry. A shared mark is a
                // reference to something living on another chart, and reading
                // as one is the honest outcome.
                drawing
                    .tool
                    .paint_under(&clipped, band.rect, style, &points, &ctxt);
                // Locked geometry shows no handles on either chart: they would
                // advertise a drag that is refused.
                drawing.tool.paint(
                    &clipped,
                    band.rect,
                    style,
                    &points,
                    &ctxt,
                    selected && !drawing.locked,
                );
            }
        }
    }

    /// Where an instant later than this pane's newest bar falls, as a
    /// fractional slot past the end. `None` unless the pane's bars run on a
    /// fixed interval — see [`Self::anchor_time`] for why a tick chart has no
    /// answer here.
    fn future_slot_at_time(&self, time: i64) -> Option<f32> {
        if self.kind != BarKind::Time || self.time_interval_ms <= 0 {
            return None;
        }
        let last = self.slots().checked_sub(1)?;
        let last_open = self.slot_open_time(last)?;
        let ahead = time.checked_sub(last_open)?;
        if ahead < self.time_interval_ms {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(last as f32 + ahead as f32 / self.time_interval_ms as f32)
    }

    /// Re-express a foreign drawing's anchors in this pane's bar space.
    /// Returns the anchors and whether any of them had to be clamped to the
    /// end of this pane's series.
    fn reproject(&self, drawing: &Drawing) -> Option<(SmallVec<[ChartPoint; 4]>, bool)> {
        let slots = self.slots();
        if slots == 0 {
            return None;
        }
        let mut anchors = SmallVec::new();
        let mut clamped = false;
        for point in &drawing.points {
            let time = point.time_ms?;
            let slot = match self.slot_at_time(time) {
                Some(slot) => slot.min(slots - 1),
                // Before this pane's first bar: the series does not reach
                // back that far, so the anchor sits on the oldest bar and is
                // marked as clamped.
                None => {
                    clamped = true;
                    0
                }
            };
            // A time chart can also place an instant *past* its newest bar,
            // on the same fixed interval its bars already run on — which is
            // where a trend line pointing into the future belongs. Without
            // this the future end of a shared line would pile up on the right
            // edge instead of running on.
            if let Some(future) = self.future_slot_at_time(time) {
                anchors.push(ChartPoint::at_time(future + 0.5, point.price, Some(time)));
                continue;
            }
            // The other clamp: a time past the end of this pane's series
            // lands on the newest slot because `slot_at_time` cannot go
            // further than the tape has. Only a *closed* bar can prove that,
            // by having ended before the anchor's instant — a time inside the
            // forming bar is simply now, and fading it would be a lie in the
            // other direction.
            clamped |= self
                .closed_bar(slot)
                .is_some_and(|bar| bar.close_time < time);
            anchors.push(ChartPoint::at_time(
                slot as f32 + 0.5,
                point.price,
                Some(time),
            ));
        }
        Some((anchors, clamped))
    }

    /// Paint the completed drawing objects. This runs once per frame and is
    /// O(number of drawings); it never touches the per-trade ingestion path.
    /// Paint one band's drawings, clipped to that band.
    ///
    /// The clip is not cosmetic: a CVD line crossing into the candles reads
    /// as a price level, and the two rects are adjacent.
    fn draw_drawings(
        &self,
        painter: &egui::Painter,
        band: &Band,
        band_index: usize,
        history_right: f32,
        total: usize,
        pass: DrawPass,
    ) {
        let Some(scale) = band.scale.as_ref() else {
            return;
        };
        let chart_rect = band.rect;
        let clipped = painter.with_clip_rect(chart_rect);
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) || !bands::drawing_in_band(drawing, band) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            let selected = self.drawings.selected() == Some(index);
            // An object that crosses every band draws its stroke in each and
            // its readout and handles in the first: three copies of
            // "17 bars 4m 21s" stacked down the screen is not three facts.
            let primary_band = drawing.band != DrawingBand::AllBands || band_index == 0;
            // A mark this chart's data does not back: its anchors are outside
            // the loaded series, or it was drawn on the instrument this tab
            // used to show. It survived the change, because only the trader
            // deletes a drawing, and it says what it is by fading rather than
            // by passing itself off as a level on this market. Same opacity
            // the mirrored marks use for the same reason.
            let style = if drawing.off_series || drawing.foreign_market {
                DrawingStyle {
                    color: Self::painted_color(drawing),
                    fill_alpha: 0,
                    ..drawing.style
                }
            } else {
                drawing.style
            };
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band,
                style,
                selected,
                halo: false,
                content_editing: self.content_editing == Some(index),
            };
            // A locked object shows no resize handles: its geometry is not
            // editable, so the affordance would lie.
            if pass == DrawPass::UnderCandles {
                // The body only. Everything below this line — the caret, the
                // badges, the rubber band — is chrome about the object, and
                // chrome under the price is chrome nobody can read.
                drawing
                    .tool
                    .paint_under(&clipped, chart_rect, style, &points, &ctxt);
                continue;
            }
            drawing.tool.paint(
                &clipped,
                chart_rect,
                style,
                &points,
                &ctxt,
                selected && !drawing.locked && primary_band,
            );
            bands::paint_off_band_caret(&clipped, chart_rect, &points, drawing);
        }
        if pass == DrawPass::UnderCandles {
            return;
        }
        // Badges paint outside the visibility gate above: a hidden drawing
        // hides its geometry, never the fact that a bot rides it — an
        // invisible armed instance is the one state this surface must not
        // allow. O(armed instances), zero when none.
        for instance in &self.strategies.instances {
            let Some(index) = self.drawings.index_of(instance.drawing) else {
                continue;
            };
            let drawing = &self.drawings.items()[index];
            if !bands::drawing_in_band(drawing, band)
                || (drawing.band == DrawingBand::AllBands && band_index != 0)
            {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            self.paint_strategy_badge(&clipped, instance, drawing, &points);
        }

        // With hide-all engaged the finished object would be invisible, so
        // the rubber-band must not pretend otherwise (audit M8) — placement
        // itself releases hide-all when it commits, in `place_with`.
        if let Some(draft) = self
            .drawings
            .draft()
            .filter(|draft| bands::drawing_in_band(draft, band))
            .filter(|_| !self.drawings.all_hidden())
        {
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
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band: draft.band != DrawingBand::AllBands || band_index == 0,
                style: draft.style,
                selected: false,
                halo: false,
                content_editing: false,
            };
            // Both halves, in order. A tool whose body lives in the
            // background pass would otherwise preview as an empty outline
            // while it is being dragged out — the profile's own histogram is
            // already folded for a draft, so there is data to show.
            // Over the candles rather than under them: a preview is a thing
            // in flight, and burying it would hide the gesture.
            draft
                .tool
                .paint_under(&clipped, chart_rect, draft.style, &points, &ctxt);
            draft
                .tool
                .paint(&clipped, chart_rect, draft.style, &points, &ctxt, false);
            // What the next click will do, printed where the eye already is.
            // The rail's `n/N` badge says the same thing on the far side of
            // the screen, which is why a trader who drags a three-anchor tool
            // and lets go reads the waiting object as frozen.
            if let Some(cursor) = points.last().copied() {
                paint_placement_hint(&clipped, chart_rect, cursor, draft.tool, draft.points.len());
            }
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

        // Price tag on the axis at the cursor height, through the axis's one
        // tag owner — the compass and the last-price chip write theirs the
        // same way, so the marks that share this gutter cannot drift apart.
        pointer_compass::paint_price_tag(
            painter,
            axis_x,
            pos.y,
            pointer_compass::price_text(scale.price_at(pos.y)),
            theme::TAG_BG,
            egui::Color32::WHITE,
        );
    }

    /// Every price a visible drawing on the price band declares, in the order
    /// they were drawn, handed one at a time to `mark`.
    ///
    /// The read half of the axis tags: what the gutter says about the objects
    /// on the chart is data before it is pixels, so a test asserts on the
    /// levels rather than on a shape count, and anything that later needs to
    /// enumerate a trader's levels — a client, the assistant — asks this
    /// rather than the painter.
    ///
    /// Into a buffer the caller owns, the way the band carve is: the levels
    /// are per-frame by nature — they move with pan, zoom and the price scale
    /// — but the container is not, so after the first frame this allocates
    /// nothing. The frame gathers them once and both the axis and the tags
    /// read that one answer; walking twice would let the coordinate an axis
    /// stood aside for differ from the one a chip landed on.
    ///
    /// Objects on an indicator band are skipped. Their `y` means whatever that
    /// pane's axis means, and writing it on the price gutter would put a CVD
    /// reading where a price goes.
    pub(crate) fn price_axis_levels(
        &self,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
        out: &mut Vec<PriceAxisLevel>,
    ) {
        out.clear();
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) || matches!(drawing.band, DrawingBand::Indicator(_))
            {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            for y in drawing.tool.axis_levels(chart_rect, &points) {
                out.push(PriceAxisLevel {
                    id: drawing.id,
                    y,
                    price: scale.price_at(y),
                    color: Self::painted_color(drawing),
                });
            }
        }
    }

    /// The price gutter's own two marks, in the order they have to be painted.
    ///
    /// A level is a static annotation whose value the trader chose and already
    /// knows; the last price is live market data. The two land on the same
    /// pixel exactly when price arrives at the level — the moment the level
    /// was drawn for — so the annotation goes down first and the market's own
    /// number is the one that stays legible.
    ///
    /// One function rather than two adjacent statements, because the order
    /// *is* the rule: written inline it is a pair of lines any later edit can
    /// swap without noticing, and here it is a decision with a name on it and
    /// a test against it.
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's geometry plus the bar the chip reports, passed rather than cached: both painters need them and this exists to hold their order, not to shorten their signatures"
    )]
    fn draw_axis_marks(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        levels: &[PriceAxisLevel],
        newest: Option<&quantick_engine::Bar>,
        chrome: &PaneChrome<'_>,
    ) {
        Self::draw_drawing_axis_tags(painter, chart_rect, axis_x, levels);
        if self.layer_visible(ChartLayer::LastPrice, chrome.style)
            && let Some(bar) = newest
        {
            self.draw_last_price(painter, chart_rect, axis_x, scale, bar, chrome);
        }
    }

    /// The levels the drawings declare, written on the price axis in each
    /// object's own colour.
    /// The levels the drawings declare, written on the price axis in each
    /// object's own colour.
    ///
    /// Rides the `Drawings` layer rather than carrying a switch of its own:
    /// the tag *is* the object, said on the axis, so hiding the objects has to
    /// take their tags with it — a gutter still marked at a level whose line
    /// is gone would be the chart claiming something it is not drawing. The
    /// caller holds that gate, beside every other layer's.
    fn draw_drawing_axis_tags(
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        levels: &[PriceAxisLevel],
    ) {
        for level in levels {
            if level.y < chart_rect.top() || level.y > chart_rect.bottom() {
                continue;
            }
            // The object's own colour, in the last-price chip's language,
            // because it is the same kind of statement: a price this chart is
            // telling you about, at the height it sits. The ink is *computed*
            // rather than borrowed from that chip — the last price wears one
            // of two saturated colours and a drawing wears whatever the
            // trader picked, dark navy included.
            pointer_compass::paint_price_tag(
                painter,
                axis_x,
                level.y,
                pointer_compass::price_text(level.price),
                level.color,
                theme::ink_on(level.color),
            );
        }
    }

    /// The bar under `x`, as data, with the instant it opened.
    ///
    /// `history_right` is the candles' own right edge: past it lies the live
    /// lane, which is not made of bar slots at all. A pointer out there, or
    /// out in the projection margin past the newest bar, is over no bar — and
    /// this says so rather than naming the nearest one, because a compass that
    /// rounds empty canvas onto the last candle would put a time on a place
    /// where nothing happened.
    #[must_use]
    pub(crate) fn pointer_bar(
        &self,
        x: f32,
        history_right: f32,
        total: usize,
    ) -> Option<pointer_compass::PointerBar> {
        if total == 0 || x > history_right {
            return None;
        }
        let slot = self.viewport.slot_at_x(x, history_right, total)?;
        Some(pointer_compass::PointerBar {
            slot,
            open_time_unix_ms: self.slot_open_time(slot)?,
        })
    }

    /// What the compass will draw this frame.
    ///
    /// Decided once, before either axis labels itself: the axes read it to
    /// know which coordinates are already spoken for, and the paint reads the
    /// same answer several hundred lines later rather than working it out
    /// again. A decision made twice is a decision two surfaces can disagree
    /// about, and here the disagreement would be an axis hiding a label for a
    /// chip that never arrived.
    fn pointer_compass(
        &self,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
        chrome: &PaneChrome<'_>,
    ) -> Option<PointerCompass> {
        let price_on = self.layer_visible(ChartLayer::PointerPrice, chrome.style);
        let time_on = self.layer_visible(ChartLayer::PointerTime, chrome.style);
        if !price_on && !time_on {
            return None;
        }
        // Resolved whether or not the time half is switched on. The readout
        // says what is under the pointer, and a switch on a mark is not a
        // statement about the world: gating it here would tell the control
        // plane's cursor scope there is no bar under a candle plainly under
        // the pointer. The lookup is a division and a slot read.
        let bar = self
            .hover_pos
            .and_then(|pointer| self.pointer_bar(pointer.x, history_right, total));
        let readout = pointer_compass::readout(self.hover_pos, chart_rect, scale, bar)?;
        // The armed crosshair already writes a price on this axis. Two chips
        // stacked on one pixel is not two facts, so the mode that draws the
        // cross keeps the tag that belongs to it — the compass still supplies
        // the time half, which the crosshair has never drawn.
        //
        // The tool alone decides it: arming the crosshair turns its layer back
        // on through `unhide_layer_for_armed_tool`, so a second conjunct
        // asking whether the layer is visible could never be false and would
        // read as a condition that can be met.
        // The paper aim writes a price on this axis for the very pixel the
        // pointer is on, and while it is up it owns that chip for the same
        // reason the crosshair does.
        let crosshair_owns_the_price =
            chrome.toolrail.tool() == Tool::Crosshair || chrome.paper.aiming();
        Some(PointerCompass {
            price: price_on && !crosshair_owns_the_price,
            time: time_on && readout.bar.is_some(),
            readout,
        })
    }

    /// The pointer's compass: its price on the price axis, and the time of the
    /// bar it is over on the time axis.
    ///
    /// Two switches, one per axis ([`ChartLayer::PointerPrice`] and
    /// [`ChartLayer::PointerTime`]), each reached from that axis's own
    /// right-click menu. See [`crate::pointer_compass`] for why this exists
    /// and why it is not a crosshair.
    fn draw_pointer_compass(
        &self,
        painter: &egui::Painter,
        compass: &PointerCompass,
        axis_x: f32,
        time_strip: egui::Rect,
        chrome: &PaneChrome<'_>,
    ) {
        if compass.price {
            pointer_compass::paint_price_mark(painter, axis_x, &compass.readout);
        }
        if compass.time {
            let (history_strip, _) = split_time_strip(time_strip, self.last_lane_divider_x);
            pointer_compass::paint_time_mark(painter, history_strip, &compass.readout, chrome.tz);
        }
    }

    /// A vertical marker where venue candles give way to bars this app built
    /// from prints.
    ///
    /// Dashed, and in a nearly transparent white rather than the backfill
    /// divider's amber. Both mark provenance, but they are read differently:
    /// the backfill divider answers a question asked once, while this one sits
    /// on the chart the entire session, so it is drawn to be *found* rather
    /// than noticed (see [`theme::SEAM_LINE`]). The dash still says it is a
    /// different kind of boundary. Left of it the bars are the venue's own
    /// summaries — one price per interval, with the aggressor split only where
    /// the venue publishes one. Right of it every bar was cut from prints this
    /// app saw.
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
        draw_dashed_vertical(
            painter,
            x,
            pane,
            SEAM_DASH_PX,
            SEAM_GAP_PX,
            theme::SEAM_LINE,
        );
        painter.text(
            egui::pos2(x - SEAM_LABEL_INSET_PX, pane.top() + SEAM_LABEL_INSET_PX),
            egui::Align2::RIGHT_TOP,
            "venue",
            egui::FontId::proportional(SEAM_LABEL_PT),
            theme::SEAM_LABEL,
        );
    }

    /// The bar the tape resumed into after a gap: the first closed bar opening
    /// at or after the gap's far side.
    ///
    /// A binary search rather than a scan. This runs per gap per frame, and a
    /// linear walk over a chart holding thousands of bars would put that on the
    /// render thread every frame of a session that reconnected once.
    ///
    /// Only the trade-derived series is searched. A gap is left by a live
    /// reconnect, and the venue prefix in front of it is candle history the
    /// venue summarized long before this session opened its socket.
    fn gap_slot(&self, gap: crate::feed::FeedGap) -> Option<usize> {
        let bars = self.state.bars();
        let index = bars.partition_point(|bar| bar.open_time < gap.to_ms);
        (index < bars.len()).then(|| self.history_prefix.len() + index)
    }

    /// Vertical markers where the tape has a hole no print covers.
    ///
    /// A reconnect that keeps the timeline is the whole point of having a
    /// reconnect beside a reload — but the market that traded while nobody was
    /// listening cannot be recovered, and butting the two halves of the session
    /// against each other would draw one continuous tape that never existed.
    /// So the hole is drawn: dashed, amber-ish, with the silence named beside
    /// it, at the bar the stream resumed into.
    fn draw_feed_gaps(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
        gaps: &[crate::feed::FeedGap],
    ) {
        for gap in gaps {
            let Some(slot) = self.gap_slot(*gap) else {
                continue;
            };
            if slot == 0 || slot >= total {
                continue;
            }
            let x = self.viewport.x_center(slot, pane.right(), total) - candle_width / 2.0;
            if x < pane.left() || x > pane.right() {
                continue; // off-screen
            }
            draw_dashed_vertical(painter, x, pane, SEAM_DASH_PX, SEAM_GAP_PX, theme::GAP_LINE);
            // On the right of its line, where the venue seam's caption is on
            // the left: the two can land on the same bar, and a trader has to
            // be able to tell which line each word belongs to.
            painter.text(
                egui::pos2(x + SEAM_LABEL_INSET_PX, pane.top() + SEAM_LABEL_INSET_PX),
                egui::Align2::LEFT_TOP,
                format!("{} gap", crate::feed::stall::spoken_ms(gap.duration_ms())),
                egui::FontId::proportional(SEAM_LABEL_PT),
                theme::GAP_LABEL,
            );
        }
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

/// Chip metrics for the placement hint: it rides beside the cursor without
/// sitting under it, and it is the same 10 px plate the ruler readout uses.
const HINT_CURSOR_OFFSET_PX: egui::Vec2 = egui::vec2(14.0, 14.0);
const HINT_TEXT_PX: f32 = 10.0;
const HINT_PAD_X_PX: f32 = 5.0;
const HINT_PAD_Y_PX: f32 = 3.0;
const HINT_RADIUS_PX: f32 = 3.0;
const HINT_PLATE: egui::Color32 = egui::Color32::from_rgba_premultiplied(14, 18, 26, 216);

/// Tell the trader what the next click does, beside the cursor.
///
/// A tool that knows says so in words (`placement_hint`); one that does not
/// still reports its progress, because "2/3" beats an object that appears to
/// have stopped responding. Nothing is drawn once the last anchor is placed —
/// there is no next click to describe.
fn paint_placement_hint(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    cursor: egui::Pos2,
    tool: drawings::DrawingTool,
    placed: usize,
) {
    let required = tool.required_points();
    if required < 2 || placed == 0 || placed >= required {
        return;
    }
    let text = tool
        .placement_hint(placed)
        .map_or_else(|| format!("{placed}/{required}"), str::to_owned);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(HINT_TEXT_PX),
        theme::TEXT_PRIMARY,
    );
    let size = galley.size() + egui::vec2(2.0 * HINT_PAD_X_PX, 2.0 * HINT_PAD_Y_PX);
    // Flip to the other side of the cursor rather than let the chip leave the
    // chart: a hint half off-screen is worse than no hint.
    let mut min = cursor + HINT_CURSOR_OFFSET_PX;
    if min.x + size.x > chart_rect.right() {
        min.x = cursor.x - HINT_CURSOR_OFFSET_PX.x - size.x;
    }
    if min.y + size.y > chart_rect.bottom() {
        min.y = cursor.y - HINT_CURSOR_OFFSET_PX.y - size.y;
    }
    let plate = egui::Rect::from_min_size(min, size);
    painter.rect_filled(plate, egui::Rounding::same(HINT_RADIUS_PX), HINT_PLATE);
    painter.galley(
        plate.min + egui::vec2(HINT_PAD_X_PX, HINT_PAD_Y_PX),
        galley,
        theme::TEXT_PRIMARY,
    );
}

/// The open / high / low / close of `candle` nearest to the pointer on
/// screen, when one is within reach.
///
/// This is the difference between a line that *looks* drawn off the swing
/// high and one that is (`docs/ux/drawing-tools-2026-08.md` §D6). Nothing in
/// reach returns `None` and the free price is used — a magnet that always
/// snaps is a magnet you cannot draw a diagonal with.
/// Clamp a fractional bar coordinate onto the bars that exist:
/// `0 ..= total - 1`. The candle magnet's time half — a snap that reads a
/// candle must stand on one.
fn snap_bar_to_tape(bar: f32, total: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    bar.clamp(0.0, total.saturating_sub(1) as f32)
}

fn magnet_price_of(
    candle: &quantick_engine::Bar,
    pointer_y: f32,
    scale: &PriceScale,
    reach_px: f32,
) -> Option<f64> {
    [candle.open, candle.high, candle.low, candle.close]
        .into_iter()
        .filter_map(|price| {
            let price = price.to_f64()?;
            let distance = (scale.y(price) - pointer_y).abs();
            (distance <= reach_px).then_some((distance, price))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, price)| price)
}

#[cfg(test)]
mod tests;
