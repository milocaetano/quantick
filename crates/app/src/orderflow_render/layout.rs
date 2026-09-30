//! The projection half of the order-flow renderer: where a normalized
//! coordinate lands on the canvas, and which pane it lands in.
//!
//! Every layer painter in the sibling modules is handed a [`RenderContext`]
//! and asks this module the same questions — `x`, `y`, `band`, `pane`,
//! `layer_clip` — so the map, the bubbles, the legend and the on-demand
//! pointer resolver cannot grow two pixel mappings. Nothing here paints.

use eframe::egui;
use quantick_orderflow::projection::{PastTape, PastTapeMemory, TapeHorizontalGeometry};
use quantick_orderflow::{
    AggressionPrimitive, BubbleStyle, HeatmapCell, HeatmapProjection, LiquidityEventPrimitive,
    LiveEdge, PriceWindow,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use std::sync::Arc;

use crate::viewport::Viewport;

use super::OrderflowRenderStyle;

/// Screen x where the history pane ends and the live lane begins.
///
/// The lane is a pane pinned to the right edge of the chart, so the divider is
/// a property of the chart rect alone — no viewport, no bars. That is exactly
/// what makes panning and zooming the candles leave the tape where it is.
/// `None` when the frame has no lane, or when the lane would be wider than
/// the chart. A lane exactly as wide as the chart is a tape-only pane: the
/// divider sits on the chart's left edge and the candles get no room.
#[must_use]
pub(crate) fn lane_divider_x(chart_rect: egui::Rect, lane_width_px: f32) -> Option<f32> {
    (lane_width_px.is_finite() && lane_width_px > 0.0 && lane_width_px <= chart_rect.width())
        .then(|| chart_rect.right() - lane_width_px)
}

/// Mapping between normalized projection coordinates and the chart viewport.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectedLayout<'a> {
    pub(crate) chart_rect: egui::Rect,
    pub(crate) viewport: &'a Viewport,
    pub(crate) total_bars: usize,
    pub(crate) first_bar_index: usize,
    /// Regions the normalized x axis is divided into: one per bar, plus the
    /// live lane when the frame has one.
    pub(crate) slot_count: usize,
    /// Width in pixels of the live lane's pane, taken off the right edge of
    /// the chart. `0.0` means the frame has no lane and the candles own the
    /// whole chart.
    pub(crate) lane_width_px: f32,
    /// Whether the chart is upside down. The projection speaks in fractions
    /// of the price window and knows nothing of orientation; the flip happens
    /// here, at the same boundary where the candles' own scale flips, so the
    /// map, the bubbles and the bars they sit on turn over together.
    pub(crate) inverted: bool,
    /// The native tape's own clock, when the lane is that tape: its book is
    /// then placed by market time exactly where its dots are, rather than by
    /// the normalized x the worker laid out against an older instant.
    tape: Option<TapeClock>,
}

/// Where a market instant lands on the native tape this frame: the tape's
/// inset span and the window ending at its supplied clock. The same mapping
/// its dots are drawn on, so a print and the book of its instant share an x.
#[derive(Debug, Clone, Copy)]
struct TapeClock {
    geometry: TapeHorizontalGeometry,
    now_ms: i64,
    window_ms: i64,
}

impl<'a> ProjectedLayout<'a> {
    #[must_use]
    pub(crate) fn new(
        chart_rect: egui::Rect,
        viewport: &'a Viewport,
        total_bars: usize,
        first_bar_index: usize,
        slot_count: usize,
        lane_width_px: f32,
    ) -> Self {
        Self {
            chart_rect,
            viewport,
            total_bars,
            first_bar_index,
            slot_count,
            lane_width_px: if lane_width_px.is_finite() {
                lane_width_px.max(0.0)
            } else {
                0.0
            },
            inverted: false,
            tape: None,
        }
    }

    /// Place the lane's book on the native tape's clock: `edge` is the tape's
    /// live edge this frame, as its dots are drawn against it, and `bubbles`
    /// sizes the tape's inset exactly as the dots' pass does. `None` keeps
    /// the linear lane of every other tape.
    #[must_use]
    pub(crate) fn with_tape_clock(mut self, edge: Option<LiveEdge>, bubbles: &BubbleStyle) -> Self {
        self.tape = edge.filter(|_| self.lane_left_x().is_some()).map(|edge| {
            let lane = self.lane_rect();
            TapeClock {
                geometry: TapeHorizontalGeometry::resolve(lane.width(), lane.height(), bubbles),
                now_ms: edge.now_ms,
                window_ms: edge.window_ms,
            }
        });
        self
    }

    /// Screen x of the market instant `timestamp_ms` on the native tape;
    /// `None` without its clock.
    #[must_use]
    fn tape_x(self, timestamp_ms: i64) -> Option<f32> {
        let tape = self.tape?;
        Some(
            self.lane_rect().left()
                + tape
                    .geometry
                    .x_at_ms(timestamp_ms, tape.now_ms, tape.window_ms),
        )
    }

    /// The tape's span of a cell drawn on the native tape, left to right.
    fn tape_span(self, cell: &HeatmapCell) -> Option<(f32, f32)> {
        let (start, end) = cell.tape_ms?;
        Some((self.tape_x(start)?, self.tape_x(end)?))
    }

    /// The same layout upside down when `inverted` — see the field's note.
    #[must_use]
    pub(crate) fn with_inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    #[must_use]
    pub(super) fn x(self, normalized: f64) -> f32 {
        let normalized = finite_unit_f64(normalized) as f32;
        let regions = self.slot_count as f32;
        // `region_pos` is the position in region units over `[0, regions]`.
        // Bars go through the viewport, which owns everything left of the
        // divider; the lane — the last region — is a fixed band of screen,
        // mapped linearly and answering to nothing the candles do.
        let region_pos = normalized * regions;
        let boundary = regions - 1.0;
        match self.lane_left_x() {
            Some(divider) if regions >= 1.0 && region_pos > boundary => {
                divider + (region_pos - boundary) * self.lane_width_px
            }
            _ => self.x_at_ext(region_pos),
        }
    }

    /// Screen x of a position measured in candle widths from the first slot.
    #[must_use]
    fn x_at_ext(self, ext_pos: f32) -> f32 {
        let position = self.first_bar_index as f32 - 0.5 + ext_pos;
        self.viewport
            .x_at_bar_position(position, self.history_right(), self.total_bars)
    }

    /// Screen x where the live lane opens. `None` when this frame has no lane.
    #[must_use]
    pub(crate) fn lane_left_x(self) -> Option<f32> {
        lane_divider_x(self.chart_rect, self.lane_width_px)
    }

    /// Right edge of the candles' own pane: the divider when a lane is drawn,
    /// the chart's right edge otherwise.
    #[must_use]
    fn history_right(self) -> f32 {
        self.lane_left_x()
            .unwrap_or_else(|| self.chart_rect.right())
    }

    /// The candles' pane — everything left of the divider.
    #[must_use]
    pub(super) fn history_rect(self) -> egui::Rect {
        egui::Rect::from_min_max(
            self.chart_rect.min,
            egui::pos2(self.history_right(), self.chart_rect.bottom()),
        )
    }

    /// The tape's pane — everything right of the divider.
    #[must_use]
    pub(super) fn lane_rect(self) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.history_right(), self.chart_rect.top()),
            self.chart_rect.max,
        )
    }

    /// Whether a normalized position belongs to the tape rather than the
    /// candles. `false` for every position when the frame has no lane.
    #[must_use]
    pub(super) fn in_lane(self, normalized: f64) -> bool {
        self.lane_left_x().is_some()
            && self.slot_count >= 1
            && finite_unit_f64(normalized) > (self.slot_count as f64 - 1.0) / self.slot_count as f64
    }

    /// The pane a normalized position belongs to.
    ///
    /// Panning the candles into history sends the newest bars off the right of
    /// their own pane, which is now the divider rather than the chart edge.
    /// Clipping each primitive to its pane is what keeps the two from drawing
    /// into each other — a candle scrolls out of sight behind the tape instead
    /// of over it, and nothing on the tape reaches back across the divider.
    #[must_use]
    pub(super) fn pane(self, normalized: f64) -> egui::Rect {
        if self.lane_left_x().is_none() {
            return self.chart_rect;
        }
        if self.in_lane(normalized) {
            self.lane_rect()
        } else {
            self.history_rect()
        }
    }

    /// The pane a band spans.
    ///
    /// One that crosses the divider — a resting level that has been there since
    /// before the tape's window opened — belongs to both panes and is clipped
    /// by neither, so it reads as the single continuous run it is. Unless its
    /// history end has scrolled off the candles' pane: then only the part
    /// inside the tape's own window is still on screen, and letting the rest
    /// through would paint history time across the tape.
    #[must_use]
    pub(super) fn span_pane(self, x0: f64, x1: f64) -> egui::Rect {
        let low = self.pane(x0.min(x1));
        let high = self.pane(x0.max(x1));
        if low == high {
            return low;
        }
        if self.x(x0.min(x1)) >= self.history_right() {
            return self.lane_rect();
        }
        self.chart_rect
    }

    /// The region a layer switched on for `chart`, `lane` or both may paint.
    ///
    /// `None` when neither pane draws it, which is the whole layer switched
    /// off. Returning a region rather than filtering primitives is what keeps
    /// a run that crosses the divider honest: a resting level that has been
    /// there since before the tape's window opened is one continuous band, and
    /// hiding the map over the candles has to cut it at the divider, not drop
    /// it. It is also free — one clip rect per frame, instead of a test per
    /// cell on the densest layer the chart draws.
    #[must_use]
    pub(super) fn layer_clip(self, chart: bool, lane: bool) -> Option<egui::Rect> {
        match (chart, lane) {
            (true, true) => Some(self.chart_rect),
            // With no lane there is only one pane, and it is the chart's.
            (true, false) => Some(if self.lane_left_x().is_some() {
                self.history_rect()
            } else {
                self.chart_rect
            }),
            (false, true) => self.lane_left_x().is_some().then(|| self.lane_rect()),
            (false, false) => None,
        }
    }

    #[must_use]
    pub(super) fn y(self, normalized: f64) -> f32 {
        self.y_unclamped(finite_unit_f64(normalized))
    }

    /// [`Self::y`] off the chart too: a volume dot off the price window is
    /// hidden by its pane's clip rather than piled on the edge.
    #[must_use]
    pub(super) fn y_unclamped(self, normalized: f64) -> f32 {
        let unit = if normalized.is_finite() {
            normalized as f32
        } else {
            0.0
        };
        let unit = if self.inverted { 1.0 - unit } else { unit };
        self.chart_rect.top() + unit * self.chart_rect.height()
    }

    #[must_use]
    pub(super) fn band(self, x0: f64, x1: f64, y0: f64, y1: f64, min_height: f32) -> egui::Rect {
        let left = self.x(x0);
        let right = self.x(x1);
        let top = self.y(y0);
        let bottom = self.y(y1);
        readable_band(
            egui::Rect::from_min_max(
                egui::pos2(left.min(right), top.min(bottom)),
                egui::pos2(left.max(right), top.max(bottom)),
            ),
            min_height,
            self.span_pane(x0, x1),
        )
    }

    /// Screen rectangle occupied by one semantic heatmap cell: on the native
    /// tape, the market time it covers on the tape's own clock.
    ///
    /// Kept on the renderer's projection object so the on-demand pointer
    /// resolver and the paint path cannot grow two pixel mappings. This does
    /// no work unless a control snapshot explicitly asks what is under the
    /// cursor.
    #[must_use]
    pub(crate) fn heat_cell_rect(self, cell: &HeatmapCell, min_height: f32) -> egui::Rect {
        let Some((left, right)) = self.tape_span(cell) else {
            return self.band(cell.x0, cell.x1, cell.y0, cell.y1, min_height);
        };
        let (top, bottom) = (self.y(cell.y0), self.y(cell.y1));
        readable_band(
            egui::Rect::from_min_max(
                egui::pos2(left.min(right), top.min(bottom)),
                egui::pos2(left.max(right), top.max(bottom)),
            ),
            min_height,
            self.lane_rect(),
        )
    }

    /// The pane a cell is drawn in: the tape's, for a cell on its clock.
    #[must_use]
    pub(super) fn cell_pane(self, cell: &HeatmapCell) -> egui::Rect {
        if self.tape_span(cell).is_some() {
            self.lane_rect()
        } else {
            self.span_pane(cell.x0, cell.x1)
        }
    }

    /// Where a reduction is marked: on the native tape, at its own instant on
    /// the tape's clock, over the book it happened to.
    #[must_use]
    pub(super) fn event_band(self, event: &LiquidityEventPrimitive, min_height: f32) -> EventBand {
        let row = self.band(event.x, event.x, event.y0, event.y1, min_height);
        let tape_x = self
            .tape_x(event.timestamp_ms)
            .filter(|_| self.in_lane(event.x));
        EventBand {
            x: tape_x.unwrap_or_else(|| self.x(event.x)),
            top: row.top(),
            bottom: row.bottom(),
        }
    }
}

/// Complete input shared by the independently callable rendering layers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RenderContext<'a> {
    pub(crate) projection: &'a HeatmapProjection,
    pub(crate) layout: ProjectedLayout<'a>,
    pub(crate) style: &'a OrderflowRenderStyle,
    pub(super) tape_time: Option<(quantick_orderflow::LiveEdge, i64)>,
    pub(super) tape_prices: Option<PriceWindow>,
    pub(super) tape_memory:
        Option<&'a std::cell::RefCell<quantick_orderflow::projection::TapeDotMemory>>,
    pub(super) tape_rebuilds: Option<(
        &'a std::cell::RefCell<super::PaneTapeRebuilds>,
        &'a Arc<HeatmapProjection>,
    )>,
    pub(super) past_tape: Option<(&'a std::cell::RefCell<PastTapeMemory>, &'a PastTape)>,
    pub(super) tape_overlay: Option<&'a Arc<quantick_orderflow::projection::TapeOverlay>>,
}

impl<'a> RenderContext<'a> {
    #[must_use]
    pub(crate) fn new(
        projection: &'a HeatmapProjection,
        layout: ProjectedLayout<'a>,
        style: &'a OrderflowRenderStyle,
    ) -> Self {
        Self {
            projection,
            layout,
            style,
            tape_time: None,
            tape_prices: None,
            tape_memory: None,
            tape_rebuilds: None,
            past_tape: None,
            tape_overlay: None,
        }
    }

    /// Draw the tape held in the past from its frozen blocks instead.
    pub(crate) fn with_past_tape(
        mut self,
        past: Option<(&'a std::cell::RefCell<PastTapeMemory>, &'a PastTape)>,
    ) -> Self {
        self.past_tape = past;
        self
    }

    /// Reproject factual tape prices against the axis being painted now,
    /// before visibility and overlap decisions use their screen positions.
    pub(crate) fn with_tape_price_range(mut self, range: (f64, f64)) -> Self {
        self.tape_prices = Decimal::from_f64(range.0)
            .zip(Decimal::from_f64(range.1))
            .and_then(|(low, high)| PriceWindow::new(low, high));
        self
    }

    /// Screen x of `mark`: on the native tape, where the dots' pass draws it
    /// this frame — its execution time, or now while its dot is forming.
    #[must_use]
    pub(super) fn mark_x(&self, mark: &AggressionPrimitive) -> f32 {
        let (Some(tape), Some((edge, dot_window_ms)), true) =
            (self.layout.tape, self.tape_time, mark.live)
        else {
            return self.layout.x(mark.x);
        };
        let lane_start = 1.0 - 1.0 / self.layout.slot_count.max(1) as f64;
        let mut placed = [mark.clone()];
        quantick_orderflow::projection::position_tape_at(
            &mut placed,
            edge.now_ms,
            edge.window_ms,
            lane_start,
            dot_window_ms,
        );
        let fraction = (placed[0].x - lane_start) / (1.0 - lane_start);
        self.layout.lane_rect().left() + tape.geometry.x(fraction)
    }

    /// Reposition a cached tape against this frame's supplied market clock.
    pub(crate) fn with_tape_time(
        mut self,
        edge: quantick_orderflow::LiveEdge,
        dot_window_ms: i64,
    ) -> Self {
        self.tape_time = Some((edge, dot_window_ms));
        self
    }

    /// Keep closed tape membership in the pane that owns its source lifetime,
    /// with the accepted prints the frame carries beside its tape.
    pub(crate) fn with_tape_memory(
        mut self,
        memory: &'a std::cell::RefCell<quantick_orderflow::projection::TapeDotMemory>,
        overlay: Option<&'a Arc<quantick_orderflow::projection::TapeOverlay>>,
    ) -> Self {
        self.tape_memory = Some(memory);
        self.tape_overlay = overlay;
        self
    }

    /// Run the tape's reconciliations too large for a frame beside it,
    /// drawing the retained groups until each lands; `projection` is the
    /// frame's own, shared with them.
    pub(crate) fn with_tape_rebuilds(
        mut self,
        rebuilds: Option<(
            &'a std::cell::RefCell<super::PaneTapeRebuilds>,
            &'a Arc<HeatmapProjection>,
        )>,
    ) -> Self {
        self.tape_rebuilds = rebuilds;
        self
    }

    /// The aggressions this canvas draws as bubbles, in projection order:
    /// the display switches live in the painter, never in the projection
    /// ([`quantick_orderflow::projection::draws_bubble`]). Reads the raw style
    /// rather than the sanitized copy on purpose: `sanitized` never touches a
    /// display flag, so the filter costs no second clone per frame.
    pub(crate) fn bubbles(&self) -> impl Iterator<Item = &'a AggressionPrimitive> {
        let style = self.style;
        self.projection
            .aggressions
            .iter()
            .filter(move |mark| quantick_orderflow::projection::draws_bubble(style, mark))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EventBand {
    pub(super) x: f32,
    pub(super) top: f32,
    pub(super) bottom: f32,
}

impl EventBand {
    pub(super) fn height(self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }

    pub(super) fn center_y(self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

fn readable_band(rect: egui::Rect, minimum_height: f32, clip: egui::Rect) -> egui::Rect {
    if !rect.is_finite() {
        return egui::Rect::NOTHING;
    }
    let minimum_height = minimum_height.max(0.5);
    let readable = if rect.height() < minimum_height {
        egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(rect.width().max(0.5), minimum_height),
        )
    } else {
        rect
    };
    readable.intersect(clip)
}

fn finite_unit_f64(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
