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
    PlotAreas, fmt_time_as, fmt_window, gesture_hits_lane, plot_split, split_time_strip,
};
use crate::bands::{self, Band, BandLabel, Bands};
use crate::candle_view::draw_candle;
use crate::chart::{self, PriceScale};
use crate::chart_layers::{ChartLayer, LayerActions};
use crate::config::FeedCapabilities;
use crate::drawings::{
    self, ChartPoint, DrawContext, Drawing, DrawingBand, DrawingStyle, Drawings, PresetHost,
};
use crate::indicator_render::{self, PlotX};
use crate::indicator_worker::{
    IndicatorCommand, IndicatorSource, IndicatorWorker, MAX_LANE_RUNGS, SlotId,
};
use crate::indicators::{IndicatorViews, MIN_PANE_HEIGHT_PX, PaneSizing};
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
/// How near the pointer must be to a bar's open / high / low / close for
/// the magnet to take the anchor. Generous enough to catch the swing you
/// aimed at, tight enough to still draw a free diagonal between bars.
const MAGNET_REACH_PX: f32 = 12.0;
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

impl PaneSide {
    /// The other pane of the tab — the one that mirrors this one's shared
    /// marks, and the one an edit made here has to be handed to.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Flow => Self::Time,
            Self::Time => Self::Flow,
        }
    }
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

/// Half-height of the grab band over a pane's top edge, in pixels.
///
/// The rule itself stays a hairline — a thick bar between a chart and its
/// indicator reads as a wall in the data. The handle around it is what makes
/// it catchable, and the resize cursor is the only thing that announces it:
/// the same bargain the live lane's divider and the canvas split already
/// strike.
const PANE_DIVIDER_HANDLE_PX: f32 = 4.0;

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
                view.pan(f64::from(delta) * per_px, auto);
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
fn axis_zoom_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    band: egui::Rect,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
) {
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
    /// Its index in the owning pane's store.
    pub index: usize,
    /// Which handle was grabbed, or `None` for the body.
    pub anchor: Option<usize>,
    /// Locked geometry refuses to move, exactly as it does on its own pane.
    pub locked: bool,
}

/// What a pane did to another pane's marks in one frame.
///
/// The gesture flags bracket the edits so a whole drag lands on the owning
/// store as one undo entry — the same coalescing a drag on the object's own
/// chart gets, because it is the same gesture on the same object.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SharedInteraction {
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
    /// Whether the candle footprint layer is on. Off by default: today's
    /// rendering is pixel-identical until the user asks for the ladder.
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

    /// User drawings live entirely in the app overlay layer, never in market
    /// state, so chart/backtest/bot determinism stays untouched.
    pub drawings: Drawings,
    // Drawing placement/movement state. Anchors are chart coordinates; only
    // the current hover and press position are transient pixels.
    pub drawing_hover: Option<ChartPoint>,
    /// The band the next anchor would land in, as the input pass resolved it.
    /// The draw pass puts the accent hairline on its top edge — one band at a
    /// time, and none at all when no tool is armed.
    drawing_band_hint: Option<egui::Rect>,
    pub drawing_press_position: Option<egui::Pos2>,
    pub drawing_press_started_empty: bool,
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
            footprint_visible: false,
            footprint_override: None,
            footprint_lod: crate::footprint_render::FootprintLod::default(),
            footprint_live: None,
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
            viewport: Viewport::new(),
            last_lane_divider_x: None,
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
            history_prefix: Vec::new(),
            paper_hud_anchor: None,
            context_menu_price: None,
            drawings: Drawings::default(),
            drawing_hover: None,
            drawing_band_hint: None,
            drawing_press_position: None,
            drawing_press_started_empty: false,
            drawing_press_pick: None,
            drawing_drag_pending_from: None,
            drawing_drag: DrawingDrag::None,
            shared_drag: SharedDrag::None,
            shared_drag_pending_from: None,
            shared_pointer_mark: None,
            pending_reanchor: None,
        }
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
            BarSpec::Imbalance(target) => self.imbalance_target = *target,
        }
        self.state.set_spec(spec);
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
            ChartLayer::Heatmap => tape.is_some_and(OrderflowView::depth_visible),
            ChartLayer::Bubbles => tape.is_some_and(OrderflowView::bubbles_enabled),
            // Footprint reads the pane's own retained trades, not the tape
            // machinery, so it works on flow and time panes alike.
            ChartLayer::Footprint => self.footprint_visible,
            ChartLayer::LiveStrip => self.orderflow.is_some() && self.live_strip_visible,
            ChartLayer::LaneMarks => tape.is_some_and(OrderflowView::lane_marks_visible),
            ChartLayer::DepthGaps => tape.is_some_and(OrderflowView::gaps_visible),
            ChartLayer::Grid => style.canvas.grid_enabled,
            // The toolbox's global eye already owns this one, undo history and
            // all; the menu is a second door to the same switch.
            ChartLayer::Drawings => !self.drawings.all_hidden(),
            ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
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
            // The footprint is the buy/sell split per price: on a source that
            // prints no traded volume every cell would be an identical
            // synthetic unit — the same reason the bubbles refuse.
            ChartLayer::Bubbles | ChartLayer::Footprint => (!capabilities.traded_volume)
                .then_some("this source quotes prices but prints no traded volume"),
            _ => None,
        }
    }

    /// Every layer this pane persists, and whether it is on.
    ///
    /// `style` comes from the window for the same reason it does in
    /// [`Self::layer_visible`].
    pub fn layer_states(&self, style: &ChartStyle) -> std::collections::BTreeMap<ChartLayer, bool> {
        ChartLayer::ALL
            .into_iter()
            .filter(|layer| layer.persisted())
            .map(|layer| (layer, self.layer_visible(layer, style)))
            .collect()
    }

    /// The same visibility as one bit per persisted layer, for change
    /// detection. `ALL` is fourteen entries, so the mask cannot outgrow
    /// `u16`.
    pub fn layer_mask(&self, style: &ChartStyle) -> u16 {
        ChartLayer::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, layer)| layer.persisted() && self.layer_visible(*layer, style))
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
    pub fn draw_layer_menu(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        // The trade section rides on top, on the pane that owns order
        // entry, anchored at the price the right-click landed on.
        if chrome.paper_owns_input
            && let Some(price) = self.context_menu_price
        {
            chrome.paper.context_trade_actions(ui, price);
            ui.separator();
        }
        ui.label(
            egui::RichText::new("chart layers")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        #[cfg(test)]
        self.layer_menu_rects.clear();
        for layer in ChartLayer::ALL {
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
                response.on_disabled_hover_text(reason);
            } else if response.changed() {
                self.set_layer_visible(layer, visible, chrome.layers);
            }
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
        let mut sizing = [PaneSizing::Auto; crate::indicators::MAX_PANES];
        plot_split(
            area,
            self.live_strip_width(),
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
        let partial = self.partial_command();
        self.indicator_worker.send(partial);
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
        band: &Band,
    ) -> Option<ChartPoint> {
        let scale = band.scale.as_ref()?;
        if total == 0 || band.rect.height() <= 1.0 {
            return None;
        }
        let bar = self.viewport.right_edge_bar(total) + 0.5
            - (history_right - pos.x) / self.viewport.candle_width();
        let value = magnet
            .then(|| self.magnet_value(band, bar, pos.y, scale))
            .flatten()
            .unwrap_or_else(|| scale.price_at(pos.y));
        Some(ChartPoint::at_time(bar, value, self.anchor_time(bar)))
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
            chrome.shared.edit = Some(SharedEdit::Select(pick.index));
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
            chrome.shared.commit_gesture = true;
            self.shared_drag = SharedDrag::None;
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
        if !bar.is_finite() || bar < 0.0 {
            return None;
        }
        let row = bar.floor() as usize;
        match &band.key {
            DrawingBand::Price => magnet_price_of(self.closed_bar(row)?, pointer_y, scale),
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

    /// The market time behind a fractional bar slot, for anchors that may have
    /// to be re-expressed on another pane (§D7 of the drawing-tools design).
    ///
    /// Only a slot that actually holds a bar has an instant behind it: the
    /// empty space past the newest bar is future the tape has not written, and
    /// naming a time there would be an invention. `None` is the honest answer
    /// there, and it is what keeps such an anchor out of a shared drawing.
    fn anchor_time(&self, bar: f32) -> Option<i64> {
        if !bar.is_finite() || bar < 0.0 {
            return None;
        }
        let slot = bar.floor() as usize;
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
        self.drawing_hover = response
            .hover_pos()
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| {
                let (band, position) = self.placement_target(areas, bands, position)?;
                self.drawing_point_at(position, history_right, self.slots(), magnet, band)
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
            && let Some((band, position)) = self.placement_target(areas, bands, position)
            && let Some(point) =
                self.drawing_point_at(position, history_right, self.slots(), magnet, band)
        {
            let band = band.key.clone();
            self.drawing_press_started_empty = self.drawings.draft_len() == 0;
            self.drawing_press_position = Some(position);
            self.place_drawing_point(tool, &band, point, chrome);
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
            && surface.contains(position)
            && start.distance(position) >= DRAWING_DRAG_THRESHOLD_PX
            && let Some((band, position)) = self.placement_target(areas, bands, position)
            && let Some(point) =
                self.drawing_point_at(position, history_right, self.slots(), magnet, band)
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

    fn place_drawing_point(
        &mut self,
        tool: drawings::DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        chrome: &mut PaneChrome<'_>,
    ) {
        // A new object starts from the user's explicit default preset when
        // one is set; existing objects are never touched by that choice.
        let presets = chrome.presets;
        let completed = self.drawings.place_with(tool, band, point, |tool| {
            let style = presets
                .default_style(tool.id())
                .unwrap_or_else(drawings::DrawingStyle::default);
            let mut payload = tool.default_payload();
            if let Some(name) = presets.default_preset(tool.id())
                && let Some(value) = presets.load_custom_preset(tool.id(), &name)
            {
                payload.import_preset(&value);
            }
            drawings::NewDrawing { style, payload }
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
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
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
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
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
            unit: band.unit(),
            primary_band: true,
            style: drawing.style,
            selected: self.drawings.selected() == Some(drawing_index),
            halo: false,
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
    fn drag_drawing_handle(
        &mut self,
        drawing_index: usize,
        handle: usize,
        target: ChartPoint,
        band: &Band,
        history_right: f32,
        total: usize,
    ) {
        let moved = band.scale.as_ref().and_then(|scale| {
            let drawing = self.drawings.items().get(drawing_index)?;
            let projected = self.projected_drawing_points(drawing, history_right, total, scale);
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                unit: band.unit(),
                primary_band: true,
                style: drawing.style,
                selected: true,
                halo: false,
            };
            let to = self.drawing_screen_point(target, history_right, total, scale);
            drawing
                .tool
                .drag_handle(band.rect, &projected, handle, to, &ctxt)
        });
        let Some(moved) = moved else {
            self.drawings.move_anchor(drawing_index, handle, target);
            return;
        };
        let anchors: Option<SmallVec<[ChartPoint; 4]>> = moved
            .iter()
            .map(|point| self.drawing_point_at(*point, history_right, total, false, band))
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
        let areas = self.plot_areas(area);
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
        let in_lane = |position: egui::Pos2| gesture_hits_lane(divider, position.x);

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
        }
        // Right-click: what is on this canvas, and what is not. Secondary
        // button only, so it shares no gesture with the pan, the zoom or the
        // drawing tools — a pan that ends anywhere never opens it.
        chart.context_menu(|ui| self.draw_layer_menu(ui, chrome));
        // While the menu is open the pointer is reading it, not the chart, so
        // no crosshair chases it across the candles behind it.
        if chart.context_menu_opened() {
            self.hover_pos = None;
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
        let paper_gesture = chrome.paper_owns_input
            && !tool_armed
            && chrome.paper.handle_chart_input(&ChartInput {
                chart: drawing_area,
                scale: drawing_scale.as_ref(),
                pointer: pointer_position,
                primary_pressed: primary_pressed && !over_chrome,
                primary_down,
                primary_released,
            });
        // The paper lines announce their grabbability (audit paper M3/M4):
        // drawings get hover cursors below, and a draggable stop must not
        // feel deader than an annotation — nor may the entry line's blocked
        // band refuse a pan with no explanation at all.
        if chrome.paper_owns_input
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
                        if let Some(band) = dragged
                            && let Some(position) = pointer_position
                        {
                            let position = egui::pos2(
                                position.x.clamp(band.rect.left(), history_right),
                                position.y.clamp(band.rect.top(), band.rect.bottom()),
                            );
                            if let Some(point) =
                                self.drawing_point_at(position, history_right, total, magnet, band)
                            {
                                self.drag_drawing_handle(
                                    drawing_index,
                                    handle,
                                    point,
                                    band,
                                    history_right,
                                    total,
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
                            let delta_bar = travel.x / self.viewport.candle_width();
                            // Per *band* height: a pane is a fraction of the
                            // chart's, and dividing by the candles' would move
                            // a CVD level by a fraction of the distance the
                            // pointer travelled.
                            let delta_value = -f64::from(travel.y / band.rect.height()) * (hi - lo);
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
        // Where the press landed, not where the pointer is now: a pan that
        // started on the candles keeps working when it crosses the divider.
        let dragging_candles = chart
            .interact_pointer_pos()
            .is_some_and(|press| !in_lane(press));
        if total > 0 && chart.dragged() && dragging_candles && primary_free {
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
        // Not while a tool is armed: two placement clicks in a row are two
        // anchors, never a request to jump back to the live edge.
        if chart.double_clicked() && primary_free {
            self.viewport.snap_to_live();
            self.price_view.reset();
        }
        if chart.hovered() {
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
                    .is_some_and(|position| areas.chart.contains(position) && !in_lane(position))
            {
                self.viewport.pan_pixels(delta.x, total);
                if let Some(auto) = auto
                    && delta.y != 0.0
                    && height > 1.0
                {
                    let (lo, hi) = self.price_view.resolve(auto);
                    let price_per_px = (hi - lo) / f64::from(height);
                    self.price_view.pan(f64::from(delta.y) * price_per_px, auto);
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
        let mut pane_time_gesture = PaneGesture::default();
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
            if disclosure.clicked() {
                view.sizing = if body.collapsed {
                    // Manual, not Auto: the automatic rule is what collapsed
                    // it, so handing it back would undo the click on the very
                    // next frame. An explicit height is served before the
                    // automatic ones and therefore always fits.
                    PaneSizing::Manual(MIN_PANE_HEIGHT_PX)
                } else {
                    PaneSizing::Collapsed
                };
            }
        }
        // Time, once, whichever pane the pointer was over: the panes share the
        // candles' x axis, so a sideways drag or a scroll there has to move the
        // same viewport the candles do — otherwise the same gesture would mean
        // one thing over the bars and nothing one pane below them.
        if total > 0 && pane_time_gesture.pan_x != 0.0 {
            self.viewport.pan_pixels(pane_time_gesture.pan_x, total);
        }
        if pane_time_gesture.scroll_y.abs() > 0.0 {
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
        let canvas_background = background_color(chrome.style);
        painter.rect_filled(area, egui::Rounding::ZERO, canvas_background);

        // The ladders accumulate only while the layer is on somewhere: off,
        // ingestion pays nothing and holds nothing; the first frame after a
        // switch-on refolds the retained trades (declared cost, once). Then
        // adopt the book engine's capture bucket as the row grid (the
        // instrument's price_step where the feed declares one). Both before
        // the frame borrows the bar slices; both no-ops every frame but the
        // one where something changed.
        let footprint_on = self.footprint_visible
            && self
                .layer_blocked(ChartLayer::Footprint, chrome.capabilities)
                .is_none();
        self.state.set_footprint_enabled(footprint_on);
        if footprint_on
            && let Some(base) = self
                .orderflow
                .as_ref()
                .map(OrderflowView::base_capture_grouping)
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

        // With the footprint on, the candle cedes its interior to the ladder
        // as the zoom crosses into detail: the body fill fades out and the
        // outline steps in — the reference charts' outline-box candles. With
        // the layer off the candles are untouched at any zoom.
        let mut faded_candles = None;
        if footprint_on {
            let body = crate::footprint_render::candle_body_fade(cw);
            if body < 1.0 {
                let mut faded = chrome.style.candles;
                faded.fill_opacity *= body;
                faded.outline_opacity = faded.outline_opacity.max(1.0 - body);
                faded_candles = Some(faded);
            }
        }
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
        // The live strip reads the same aggression clusters the bubbles do, so
        // it is a consumer of the projection in its own right. Stated here,
        // every frame, from the layer this pane owns: with the bubbles hidden
        // and the strip shown, the pipeline stays alive for the strip alone.
        let strip_demand = self.live_strip_visible;
        let orderflow_frame = self.orderflow.as_mut().and_then(|orderflow| {
            orderflow.set_aggression_demand(strip_demand);
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
            draw_candle(&clip, xc, half, &scale, bar, false, candles);
        }
        if let Some(partial) = partial_visible {
            let xc = self.viewport.x_center(closed_total, right, total);
            draw_candle(&clip, xc, half, &scale, partial, true, candles);
        }
        // The footprint rides directly on the candles, before everything
        // drawn over them: it is a representation of the bars themselves,
        // not an annotation. Prefix (venue) candles carry no tape and draw
        // no ladder — the layer starts where trade-built bars start.
        if self.layer_visible(ChartLayer::Footprint, chrome.style)
            && self
                .layer_blocked(ChartLayer::Footprint, chrome.capabilities)
                .is_none()
        {
            // Snapshot the forming bar's ladder at ~10 Hz rather than per
            // print; between snapshots the drawn numbers hold still.
            let now = clip.ctx().input(|i| i.time);
            match self.state.partial_footprint() {
                Some(partial) => {
                    let stale =
                        self.footprint_live
                            .as_ref()
                            .is_none_or(|(taken, snapshot_slot, _)| {
                                *snapshot_slot != closed_total
                                    || now - *taken >= LIVE_LADDER_REFRESH_S
                            });
                    if stale {
                        self.footprint_live = Some((now, closed_total, partial.clone()));
                    }
                }
                None => self.footprint_live = None,
            }
            let viewport = &self.viewport;
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
                half,
                candle_width: cw,
                side_inferred: chrome.side_inferred,
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
        // Carved into the pane's own buffer: same geometry as the input
        // pass computed, no container allocated, and what the tab's shared
        // projection reads afterwards.
        let mut carved = std::mem::take(&mut self.last_bands);
        self.carve_bands(&areas, &mut carved);
        for (index, band) in carved.iter().enumerate() {
            self.draw_drawings(painter, band, index, right, total);
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
        // rows loaded from earlier sessions stay in the ledger.
        if self.layer_visible(ChartLayer::TradePaint, chrome.style) {
            let frame = crate::trade_paint::TradePaintFrame {
                painter,
                chart_rect,
                scale: &scale,
                background: canvas_background,
                pointer: self.hover_pos,
                tz: chrome.tz,
            };
            crate::trade_paint::draw(
                &frame,
                chrome.paper.session_trades(),
                chrome.paper.selected_trade_index(),
                |ms| self.slot_at_time(ms),
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
            // the paper input; the other pane keeps display-only tags.
            let paper_pointer = if chrome.paper_owns_input {
                self.hover_pos
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
            if chrome.paper_owns_input {
                self.paper_hud_anchor = Some((chart_rect, scale));
            }
        }

        // Above the flow layers: everything else on the canvas is read against
        // it. Drawn on the unclipped painter so the chip reaches the gutter.
        if self.layer_visible(ChartLayer::LastPrice, chrome.style)
            && let Some(bar) = partial.or_else(|| closed.last())
        {
            self.draw_last_price(painter, chart_rect, axis_x, &scale, bar, chrome);
        }
        // The candles' own marks, so they are placed and clipped in their
        // pane: where venue candles give way to bars built from prints, and
        // where backfilled prints give way to live ones.
        if self.layer_visible(ChartLayer::SeamDivider, chrome.style) {
            self.draw_seam_divider(painter, history_rect, total, cw);
        }
        if self.layer_visible(ChartLayer::BackfillDivider, chrome.style) {
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
        let stride = crate::chart::time_label_stride(self.viewport.candle_width(), label_width);

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
        let (lo, hi) = band.scale?.range();
        Some((hi - lo) / f64::from(band.rect.height().max(1.0)))
    }

    /// The band drawings live in: the candles minus the live lane.
    ///
    /// The lane is the tape's own reserved strip at the right edge — a live
    /// region, not a place for annotations, and a horizontal line running
    /// across it was drawing over the flow. Placement already refused to put
    /// an anchor there; paint, geometry and hit-test agree with it now, which
    /// also keeps a line's painted end and its grabbable end the same pixel.
    fn drawing_area(&self, chart: egui::Rect) -> egui::Rect {
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
        let (auto_lo, auto_hi) = self.last_auto_range?;
        let (lo, hi) = self.price_view.resolve((auto_lo, auto_hi));
        let scale = PriceScale::from_range(
            lo,
            hi,
            self.last_chart_top,
            self.last_chart_top + self.last_chart_height,
        );
        let history_right = self.last_lane_divider_x.unwrap_or(chart.right());
        Some((self.drawing_area(chart), history_right, self.slots(), scale))
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
                unit: band.unit(),
                primary_band: true,
                style: drawing.style,
                selected: source.drawings.selected() == Some(index),
                halo: false,
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
                    unit: band.unit(),
                    primary_band: drawing.band != DrawingBand::AllBands || band_index == 0,
                    style,
                    selected,
                    halo: false,
                };
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
                    color: drawing.style.color.gamma_multiply(Self::CLAMPED_OPACITY),
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
                unit: band.unit(),
                primary_band,
                style,
                selected,
                halo: false,
            };
            // A locked object shows no resize handles: its geometry is not
            // editable, so the affordance would lie.
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
                unit: band.unit(),
                primary_band: draft.band != DrawingBand::AllBands || band_index == 0,
                style: draft.style,
                selected: false,
                halo: false,
            };
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
fn magnet_price_of(
    candle: &quantick_engine::Bar,
    pointer_y: f32,
    scale: &PriceScale,
) -> Option<f64> {
    [candle.open, candle.high, candle.low, candle.close]
        .into_iter()
        .filter_map(|price| {
            let price = price.to_f64()?;
            let distance = (scale.y(price) - pointer_y).abs();
            (distance <= MAGNET_REACH_PX).then_some((distance, price))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, price)| price)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_worker::IndicatorEvent;

    /// A chart that has never been configured follows the window's setup —
    /// which is what keeps one chart behaving like a global preference —
    /// and a chart configured on its own ignores it, which is what a split
    /// layout needs. Inverting this resolution would silently give both
    /// charts one reading.
    #[test]
    fn a_chart_follows_the_window_until_it_is_configured_itself() {
        use crate::footprint_config::{FootprintConfig, FootprintStyle};
        let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
        let window = FootprintConfig::default();
        assert_eq!(pane.footprint_config(&window), &window, "virgin follows");

        let own = FootprintConfig {
            style: FootprintStyle::Ladder,
            show_numbers: false,
            ..FootprintConfig::default()
        };
        pane.set_footprint_override(Some(own.clone()));
        assert_eq!(
            pane.footprint_config(&window),
            &own,
            "configured keeps its own"
        );
        assert_ne!(pane.footprint_config(&window), &window);

        // "follow the default again".
        pane.set_footprint_override(None);
        assert_eq!(pane.footprint_config(&window), &window);
    }

    /// The tape's lane is a live region, not a canvas. A drawing that ran
    /// across it painted over the flow — and worse, the painted end and the
    /// grabbable end were different pixels, because paint used the whole
    /// chart while placement stopped at the divider.
    #[test]
    fn the_drawing_band_stops_at_the_live_lane() {
        let chart = egui::Rect::from_min_max(egui::pos2(60.0, 80.0), egui::pos2(1_000.0, 700.0));
        let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());

        pane.last_lane_divider_x = None;
        assert_eq!(
            pane.drawing_area(chart),
            chart,
            "no lane, no carve — the whole chart is the canvas"
        );

        pane.last_lane_divider_x = Some(880.0);
        let band = pane.drawing_area(chart);
        assert_eq!(band.right(), 880.0, "the band ends where the lane begins");
        assert_eq!(band.left(), chart.left());
        assert_eq!(band.y_range(), chart.y_range());

        // A divider reported outside the chart cannot make the band bigger
        // than the chart or invert it.
        pane.last_lane_divider_x = Some(5_000.0);
        assert_eq!(pane.drawing_area(chart).right(), chart.right());
        pane.last_lane_divider_x = Some(-40.0);
        let degenerate = pane.drawing_area(chart);
        assert!(degenerate.right() >= degenerate.left());
    }

    /// A pane cutting one bar per trade, with `count` bars a second apart —
    /// so a market instant and a slot are the same statement, and a test can
    /// say which bar it means by naming the moment.
    fn pane_of_seconds(count: u64) -> ChartPane {
        use rust_decimal::Decimal;
        let mut pane = ChartPane::flow(1, BarSpec::Tick(1), "TESTUSDT".to_owned());
        let trades: Vec<_> = (0..count)
            .map(|index| quantick_engine::Trade {
                agg_id: index,
                timestamp_ms: 1_700_000_000_000 + index as i64 * 1_000,
                price: Decimal::from(100),
                quantity: Decimal::ONE,
                side: quantick_engine::Side::Buy,
            })
            .collect();
        pane.ingest_backfill(&trades);
        pane
    }

    /// A mark placed on the bar at `slot`, shared with the tab's other panes.
    fn shared_mark_at(pane: &mut ChartPane, slot: usize, price: f64) {
        let time = pane.slot_open_time(slot).expect("the slot holds a bar");
        #[allow(clippy::cast_precision_loss)]
        let point = ChartPoint::at_time(slot as f32 + 0.5, price, Some(time));
        pane.drawings.place(
            drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == "horizontal-line")
                .expect("the horizontal line is registered"),
            point,
        );
        pane.drawings
            .selected_mut()
            .expect("placement selects what it completed")
            .scope = drawings::DrawingScope::AllCharts;
    }

    #[test]
    fn an_edit_from_the_other_pane_lands_on_this_panes_own_bars() {
        let mut pane = pane_of_seconds(20);
        shared_mark_at(&mut pane, 4, 100.0);

        // The other chart reports where it put the handle in market time; this
        // pane is the one that knows which of *its* bars that is.
        let moved_to = pane.slot_open_time(11).expect("the slot holds a bar");
        pane.apply_shared_edit(SharedEdit::MoveAnchor {
            index: 0,
            anchor: 0,
            time_ms: moved_to,
            price: 101.5,
        });

        let point = pane.drawings.items()[0].points[0];
        assert_eq!(point.time_ms, Some(moved_to));
        assert_eq!(point.bar.floor() as usize, 11, "resolved into this pane");
        assert!((point.price - 101.5).abs() < f64::EPSILON);
    }

    #[test]
    fn a_body_drag_from_the_other_pane_moves_every_anchor_by_the_same_time() {
        let mut pane = pane_of_seconds(20);
        shared_mark_at(&mut pane, 4, 100.0);
        let before = pane.drawings.items()[0].points[0];

        pane.apply_shared_edit(SharedEdit::Translate {
            index: 0,
            delta_ms: 3_000,
            delta_price: 2.0,
        });

        let after = pane.drawings.items()[0].points[0];
        assert_eq!(
            after.time_ms,
            before.time_ms.map(|time| time + 3_000),
            "the drag is said in market time"
        );
        assert_eq!(
            after.bar.floor() as usize,
            7,
            "and three seconds is three bars on a one-trade-per-bar cut"
        );
        assert!((after.price - 102.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_locked_mark_refuses_an_edit_from_the_other_pane_too() {
        let mut pane = pane_of_seconds(20);
        shared_mark_at(&mut pane, 4, 100.0);
        pane.drawings
            .selected_mut()
            .expect("the mark is selected")
            .locked = true;
        let before = pane.drawings.items()[0].points.clone();

        pane.apply_shared_edit(SharedEdit::Translate {
            index: 0,
            delta_ms: 3_000,
            delta_price: 2.0,
        });
        let moved_to = pane.slot_open_time(11).expect("the slot holds a bar");
        pane.apply_shared_edit(SharedEdit::MoveAnchor {
            index: 0,
            anchor: 0,
            time_ms: moved_to,
            price: 101.5,
        });

        assert_eq!(
            pane.drawings.items()[0].points,
            before,
            "a lock protects the geometry from both charts, not just its own"
        );
    }

    #[test]
    fn a_drag_this_pane_cannot_place_moves_nothing_at_all() {
        let mut pane = pane_of_seconds(20);
        shared_mark_at(&mut pane, 4, 100.0);
        let before = pane.drawings.items()[0].points.clone();

        // Back past the first bar this pane holds: there is no slot for it,
        // and half a shape on a bar and half on an instant is not an option.
        pane.apply_shared_edit(SharedEdit::Translate {
            index: 0,
            delta_ms: -600_000,
            delta_price: 0.0,
        });

        assert_eq!(pane.drawings.items()[0].points, before);
    }

    #[test]
    fn a_recut_keeps_the_mark_on_its_own_instant() {
        let mut pane = pane_of_seconds(20);
        shared_mark_at(&mut pane, 6, 100.0);
        let placed_at = pane.drawings.items()[0].points[0]
            .time_ms
            .expect("placed on a bar");
        let old_slots = pane.slots();

        // Two trades per bar: the same tape, half the bars.
        pane.tick_n = 2;
        let spec = pane.current_spec();
        pane.state.set_spec(spec);
        pane.reanchor_drawings(old_slots);

        let point = pane.drawings.items()[0].points[0];
        assert_eq!(point.time_ms, Some(placed_at), "the instant is untouched");
        assert_eq!(
            pane.slot_at_time(placed_at),
            Some(point.bar.floor() as usize),
            "the bar is that instant, re-asked of the new cut"
        );
        assert!(!pane.drawings.items()[0].off_series);
    }

    /// A pane carrying one indicator pane of `kind`, with `columns` of
    /// committed values — the fixture every band test starts from.
    fn pane_with_indicator(kind: &str, columns: Vec<Vec<f64>>) -> ChartPane {
        let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
        add_indicator_view(&mut pane, kind, columns);
        pane
    }

    fn add_indicator_view(pane: &mut ChartPane, kind: &str, columns: Vec<Vec<f64>>) -> SlotId {
        let slot = pane.indicators.allocate_slot(kind);
        pane.indicators.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: quantick_indicators::IndicatorDescriptor {
                title: kind.to_owned(),
                short_title: None,
                overlay: false,
                plots: (0..columns.len().max(1))
                    .map(|i| quantick_indicators::PlotSpec {
                        id: quantick_indicators::PlotId::new(i),
                        title: format!("p{i}"),
                        style: quantick_indicators::PlotStyle::Line,
                        base_color: quantick_indicators::Rgba8::opaque(255, 255, 255),
                        width: 1.0,
                        offset: 0,
                        marker: None,
                    })
                    .collect(),
                fills: Vec::new(),
                inputs: Vec::new(),
            },
            columns,
            inputs: Vec::new(),
            stale: None,
        });
        slot
    }

    fn test_areas(pane: &ChartPane, rect: egui::Rect) -> PlotAreas {
        pane.plot_areas(rect)
    }

    const TEST_PLOT: egui::Rect = egui::Rect {
        min: egui::Pos2 { x: 0.0, y: 0.0 },
        max: egui::Pos2 {
            x: 1_000.0,
            y: 700.0,
        },
    };

    /// The invariant the whole feature rests on: a band's drawings are
    /// projected through the range its *curve* was drawn with, which is the
    /// auto-fit **after** the trader's own zoom. Build the scale from the
    /// auto-fit alone and every level leaves its curve the moment that pane
    /// is zoomed — the defect this test exists to make impossible.
    #[test]
    fn a_band_scale_is_the_range_its_curve_was_drawn_with() {
        let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0, -20.0]]);
        let auto = (-100.0, 100.0);
        {
            let view = pane
                .indicators
                .visible_panes_mut()
                .next()
                .expect("one pane indicator");
            view.last_auto = Some(auto);
            // The trader drags the pane's own axis.
            view.scale.pan(25.0, auto);
        }
        let areas = test_areas(&pane, TEST_PLOT);
        let bands = pane.bands(&areas);
        let band = bands
            .iter()
            .find(|band| matches!(band.key, DrawingBand::Indicator(_)))
            .expect("the indicator band");
        let resolved = pane
            .indicators
            .visible_panes()
            .next()
            .expect("one pane indicator")
            .scale
            .resolve(auto);
        assert_ne!(resolved, auto, "the fixture has to actually be zoomed");
        assert_eq!(
            band.scale.expect("a drawable band has a scale").range(),
            resolved,
            "the band projects through the range the curve was drawn with"
        );
    }

    /// A CVD level and a price level can be one pixel apart on screen and
    /// mean unrelated things, so a pick never crosses a band.
    #[test]
    fn hit_testing_never_crosses_bands() {
        let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0, 2.0]]);
        pane.indicators
            .visible_panes_mut()
            .next()
            .expect("one pane")
            .last_auto = Some((-10.0, 10.0));
        pane.last_auto_range = Some((100.0, 110.0));
        let areas = test_areas(&pane, TEST_PLOT);
        let bands = pane.bands(&areas);
        let (price, indicator) = (&bands[0], &bands[1]);

        let key = indicator.key.clone();
        pane.drawings.place_on(
            drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == "horizontal-line")
                .expect("the horizontal line is registered"),
            &key,
            ChartPoint::at(1.0, 0.0),
        );
        let scale = indicator.scale.expect("a drawable band");
        let on_the_level = egui::pos2(indicator.rect.center().x, scale.y(0.0));
        assert_eq!(
            pane.drawing_at(on_the_level, indicator, indicator.rect.right(), 3),
            Some(0),
            "found on its own band"
        );
        // The same object, asked for from the price band: a horizontal line
        // spans the whole width, so only the band rule can rule it out.
        let in_price_band = egui::pos2(price.rect.center().x, price.rect.center().y);
        assert_eq!(
            pane.drawing_at(in_price_band, price, price.rect.right(), 3),
            None,
            "a CVD level is not a price level"
        );
    }

    /// A vertical line marks an instant, so it belongs to no band and is
    /// painted through all of them — as ONE object. Modelling it as one per
    /// band would leave a half-deleted line behind every delete.
    #[test]
    fn a_time_only_object_is_one_item_that_every_band_paints() {
        let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0]]);
        pane.indicators
            .visible_panes_mut()
            .next()
            .expect("one pane")
            .last_auto = Some((-10.0, 10.0));
        pane.last_auto_range = Some((100.0, 110.0));
        let areas = test_areas(&pane, TEST_PLOT);
        let bands = pane.bands(&areas);
        let vertical = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "vertical-line")
            .expect("the vertical line is registered");
        // Placed while pointing at the *indicator* band, and still not owned
        // by it.
        pane.drawings
            .place_on(vertical, &bands[1].key, ChartPoint::at(1.0, 0.0));
        assert_eq!(
            pane.drawings.items().len(),
            1,
            "one object, not one per band"
        );
        assert_eq!(pane.drawings.items()[0].band, DrawingBand::AllBands);
        for band in &bands {
            assert!(
                bands::drawing_in_band(&pane.drawings.items()[0], band),
                "every band paints its own clipped segment"
            );
        }
    }

    /// egui gives an overlapping rect to whoever registers last, and both the
    /// chevron and the divider register after the canvas. The drawing path
    /// reads the raw pointer instead of a response, so it has to honour that
    /// order itself — or arming a tool silently kills both controls.
    #[test]
    fn the_chevron_and_the_divider_are_never_drawing_surfaces() {
        let pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0]]);
        let areas = test_areas(&pane, TEST_PLOT);
        let slot = areas.indicator_panes.first().expect("one indicator pane");
        let chevron = indicator_render::pane_disclosure_rect(slot.rect, slot.collapsed);
        assert!(ChartPane::pane_chrome_hit(&areas, chevron.center()));
        assert!(ChartPane::pane_chrome_hit(
            &areas,
            egui::pos2(slot.rect.center().x, slot.rect.top())
        ));
        assert!(
            !ChartPane::pane_chrome_hit(&areas, slot.rect.center()),
            "the middle of the pane is canvas"
        );
    }

    /// Remove an indicator and add it back — the most common thing a trader
    /// does to a pane — and its drawings have to come home. Keyed on the slot
    /// id (a monotonic counter) they would orphan every single time.
    #[test]
    fn a_band_key_survives_remove_and_re_add() {
        let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
        let first = add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        let before = pane
            .indicators
            .pane_key(pane.indicators.all().first().expect("one view"));
        pane.indicators.remove(first);
        add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        let after = pane
            .indicators
            .pane_key(pane.indicators.all().first().expect("one view"));
        assert_eq!(before, after, "the same pane, so the same key");

        // And a different indicator must never inherit it.
        add_indicator_view(&mut pane, "script.zigzag.pine", vec![vec![0.0]]);
        let other = pane
            .indicators
            .pane_key(pane.indicators.all().last().expect("two views"));
        assert_ne!(before, other);
    }

    /// Removing one pane must not renumber the ones that outlive it.
    ///
    /// With a positional ordinal, removing the first of two CVD panes turns
    /// the survivor into `{cvd, 0}` — the removed pane's key — so it starts
    /// painting the *other* pane's annotations on its own axis while its own
    /// go parked. That is one pane's marks shown on another's value scale.
    #[test]
    fn removing_a_pane_does_not_renumber_the_one_that_outlives_it() {
        let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
        let first = add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        let survivor = pane
            .indicators
            .pane_key(pane.indicators.all().last().expect("two views"));
        pane.indicators.remove(first);
        assert_eq!(
            pane.indicators
                .pane_key(pane.indicators.all().first().expect("one view left")),
            survivor,
            "the surviving pane keeps the key its drawings were placed with"
        );

        // And the ordinal the removed pane left behind is what a re-added one
        // takes, so *its* drawings come home instead of piling onto the
        // survivor's.
        add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        let readded = pane
            .indicators
            .pane_key(pane.indicators.all().last().expect("two views"));
        assert_ne!(readded, survivor);
        assert_eq!(readded.ordinal, 0);
    }

    /// A drawing whose indicator is off the chart is parked: kept, but not
    /// painted and not pickable. `band_of` answering `None` is what every
    /// caller reads to mean that.
    #[test]
    fn a_parked_drawing_belongs_to_no_band_on_screen() {
        let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0]]);
        pane.indicators
            .visible_panes_mut()
            .next()
            .expect("one pane")
            .last_auto = Some((-10.0, 10.0));
        pane.last_auto_range = Some((100.0, 110.0));
        let areas = test_areas(&pane, TEST_PLOT);
        let carved = pane.bands(&areas);
        let key = carved[1].key.clone();
        pane.drawings.place_on(
            drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == "horizontal-line")
                .expect("registered"),
            &key,
            ChartPoint::at(1.0, 0.0),
        );

        // The indicator goes away; the object does not.
        let slot = pane.indicators.all().first().expect("one view").slot;
        pane.indicators.remove(slot);
        let areas = test_areas(&pane, TEST_PLOT);
        let carved = pane.bands(&areas);
        assert_eq!(carved.len(), 1, "only the price band is left");
        assert_eq!(pane.drawings.items().len(), 1, "the object is kept");
        assert!(
            bands::band_of(&carved, &pane.drawings.items()[0]).is_none(),
            "parked: no band on screen owns it, so nothing paints or picks it"
        );
        assert!(
            !bands::drawing_in_band(&pane.drawings.items()[0], &carved[0]),
            "and it certainly does not fall back onto the price band"
        );
        assert!(matches!(
            pane.band_label(&pane.drawings.items()[0]),
            BandLabel::Parked(_)
        ));
    }

    /// Two instances of one kind are told apart by ordinal, in add order.
    #[test]
    fn two_panes_of_the_same_kind_get_different_keys() {
        let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
        add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
        let keys: Vec<_> = pane
            .indicators
            .all()
            .iter()
            .map(|view| pane.indicators.pane_key(view))
            .collect();
        assert_eq!(keys[0].ordinal, 0);
        assert_eq!(keys[1].ordinal, 1);
    }

    /// On a band the magnet snaps to the pane's own numbers — and to zero,
    /// which is the most-drawn level on a signed series.
    #[test]
    fn the_band_magnet_takes_the_panes_own_values_and_zero() {
        let view_holder = pane_with_indicator("native.cvd", vec![vec![40.0], vec![-15.0]]);
        let view = view_holder
            .indicators
            .visible_panes()
            .next()
            .expect("one pane");
        let scale = PriceScale::from_range(-100.0, 100.0, 0.0, 200.0);
        assert_eq!(
            bands::magnet_value_of(view, 0, scale.y(40.0) + 2.0, &scale, MAGNET_REACH_PX),
            Some(40.0),
            "the plotted value nearest the pointer"
        );
        assert_eq!(
            bands::magnet_value_of(view, 0, scale.y(0.0) - 1.0, &scale, MAGNET_REACH_PX),
            Some(0.0),
            "zero is always a candidate"
        );
        assert_eq!(
            bands::magnet_value_of(view, 0, scale.y(80.0), &scale, MAGNET_REACH_PX),
            None,
            "out of reach snaps nothing, so a free diagonal stays free"
        );
    }

    /// The caret that says "your object is off the top of this band" has to
    /// be a caret. Clamped to the raw edge, half of it fell outside the
    /// band's clip and it read as a smudge — which is worse than nothing,
    /// because a smudge is not a direction.
    #[test]
    fn the_off_band_caret_stays_whole_inside_its_band() {
        let band = egui::Rect::from_min_max(egui::pos2(60.0, 400.0), egui::pos2(900.0, 500.0));
        let mut drawing = drawings::Drawings::default();
        drawing.place_on(
            drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == "horizontal-line")
                .expect("registered"),
            &DrawingBand::Indicator(drawings::PaneKey {
                kind: std::sync::Arc::from("native.cvd"),
                ordinal: 0,
            }),
            ChartPoint::at(1.0, 0.0),
        );
        let object = &drawing.items()[0];

        // An anchor off the left of the window, at a value above the band.
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("caret-test"),
            ));
            bands::paint_off_band_caret(&painter, band, &[egui::pos2(-500.0, 100.0)], object);
        });
        let points: Vec<egui::Pos2> = output
            .shapes
            .iter()
            .flat_map(|clipped| match &clipped.shape {
                egui::Shape::Path(path) => path.points.clone(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(points.len(), 3, "the caret is one triangle");
        for point in points {
            assert!(
                point.x >= band.left() && point.x <= band.right(),
                "every corner is inside the band it marks: {point:?}"
            );
        }
    }

    /// A bar spanning 98 … 102, for the magnet.
    fn magnet_candle() -> quantick_engine::Bar {
        use rust_decimal::Decimal;
        quantick_engine::Bar {
            open_time: 0,
            close_time: 59_999,
            open: Decimal::from(100),
            high: Decimal::from(102),
            low: Decimal::from(98),
            close: Decimal::from(101),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    /// Ten pixels per price unit over 90 … 110, so the bar's four levels are
    /// far enough apart on screen for "nearest" to be unambiguous.
    fn magnet_scale() -> PriceScale {
        PriceScale::from_range(90.0, 110.0, 0.0, 200.0)
    }

    #[test]
    fn the_magnet_takes_the_nearest_ohlc_within_reach() {
        let candle = magnet_candle();
        let scale = magnet_scale();
        let high_y = scale.y(102.0);
        assert_eq!(
            magnet_price_of(&candle, high_y, &scale),
            Some(102.0),
            "exactly on the high"
        );
        assert_eq!(
            magnet_price_of(&candle, high_y + 4.0, &scale),
            Some(102.0),
            "inside the reach, and still nearer the high than the close"
        );
    }

    /// The point of the reach: away from every level the anchor stays free,
    /// so a diagonal drawn through open space is still a diagonal.
    #[test]
    fn the_magnet_lets_go_outside_its_reach() {
        let candle = magnet_candle();
        let scale = magnet_scale();
        assert_eq!(magnet_price_of(&candle, scale.y(105.0), &scale), None);
    }

    /// Near-ties resolve to the genuinely nearest level, never to the first
    /// one in the list.
    #[test]
    fn the_magnet_picks_the_nearest_level_not_the_first() {
        let candle = magnet_candle();
        let scale = magnet_scale();
        // Just under the low: 98 is nearer than the open at 100.
        assert_eq!(magnet_price_of(&candle, scale.y(97.6), &scale), Some(98.0));
        // Just over the close: 101 is nearer than the high at 102.
        assert_eq!(
            magnet_price_of(&candle, scale.y(101.2), &scale),
            Some(101.0)
        );
    }

    /// One geometry for the jump-to-live chip's click region and its paint
    /// (audit F6): right-aligned inside the strip, inset on every side, so
    /// the click can never miss the pixels.
    #[test]
    fn the_live_chip_sits_inside_the_strips_right_end() {
        let strip = egui::Rect::from_min_max(egui::pos2(0.0, 576.0), egui::pos2(800.0, 600.0));
        let chip = live_chip_rect(strip);
        assert!(strip.contains_rect(chip), "the chip never leaves its band");
        assert_eq!(chip.right(), strip.right() - LIVE_CHIP_MARGIN_PX);
        assert_eq!(chip.width(), LIVE_CHIP_WIDTH_PX);
        assert_eq!(chip.top(), strip.top() + LIVE_CHIP_VPAD_PX);
        assert_eq!(chip.bottom(), strip.bottom() - LIVE_CHIP_VPAD_PX);
    }

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

    /// The rung budget is the lane's width in pixels, not a constant: a lane
    /// narrow enough to be a sliver is not worth sixty evaluations, and a
    /// chart with no lane at all is worth none.
    #[test]
    fn the_rung_budget_follows_the_lane_and_is_zero_without_one() {
        assert_eq!(lane_rungs(0.0), 0, "no lane, no ladder");
        assert_eq!(lane_rungs(-5.0), 0);
        assert_eq!(lane_rungs(f32::NAN), 0);

        assert_eq!(lane_rungs(60.0), 10);
        assert_eq!(
            lane_rungs(1.0),
            1,
            "a sliver of a lane still gets its live edge"
        );
        assert_eq!(
            lane_rungs(100_000.0),
            MAX_LANE_RUNGS,
            "the ceiling is a cost statement and holds at any width"
        );
    }
}
