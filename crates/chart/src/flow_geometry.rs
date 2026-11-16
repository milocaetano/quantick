//! The order-flow pane's geometry: where a normalized projection coordinate
//! lands on screen, and which pane — the candles' history or the live tape —
//! it lands in.
//!
//! Every order-flow layer (the map, the bubbles, the legend, the pointer)
//! asks this one mapping the same questions — `x`, `y`, `band`, `pane`,
//! `layer_clip` — so they cannot grow two pixel mappings. It speaks
//! [`FlowRect`]; the window converts at its edge. Nothing here paints.

use quantick_orderflow::HeatmapCell;
pub use quantick_orderflow::projection::EventBand;
use quantick_orderflow::projection::TapeHorizontalGeometry as TapeGeometry;
use quantick_orderflow::projection::finite_unit_f64;

use crate::viewport::Viewport;

/// A horizontal pixel span, where there is one.
pub type Span = Option<(f32, f32)>;

/// An axis-aligned box, in the window's points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl FlowRect {
    /// The box nothing is inside: intersecting with it yields nothing too.
    pub const NOTHING: Self = Self {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };

    /// The box spanning two corners in any order.
    #[must_use]
    pub fn from_corners((x0, y0): (f32, f32), (x1, y1): (f32, f32)) -> Self {
        Self {
            left: x0.min(x1),
            top: y0.min(y1),
            right: x0.max(x1),
            bottom: y0.max(y1),
        }
    }

    #[must_use]
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    #[must_use]
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        [self.left, self.top, self.right, self.bottom]
            .iter()
            .all(|edge| edge.is_finite())
    }

    /// Whether the box has room on both axes.
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.width() > 0.0 && self.height() > 0.0
    }

    /// The part of this box inside `other`; it may come out inverted, which
    /// covers nothing.
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    /// The same box with its right edge at `right`.
    #[must_use]
    pub fn with_right(self, right: f32) -> Self {
        Self { right, ..self }
    }
}

/// Screen x where the history pane ends and the live lane begins.
///
/// The lane is a pane pinned to the right edge of the chart, so the divider is
/// a property of the chart rect alone — no viewport, no bars. That is exactly
/// what makes panning and zooming the candles leave the tape where it is.
/// `None` when the frame has no lane, or when the lane would be wider than
/// the chart. A lane exactly as wide as the chart is a tape-only pane: the
/// divider sits on the chart's left edge and the candles get no room.
#[must_use]
pub fn lane_divider_x(chart_rect: FlowRect, lane_width_px: f32) -> Option<f32> {
    (lane_width_px.is_finite() && lane_width_px > 0.0 && lane_width_px <= chart_rect.width())
        .then_some(chart_rect.right - lane_width_px)
}

/// Mapping between normalized projection coordinates and the chart viewport.
#[derive(Debug, Clone, Copy)]
pub struct ProjectedLayout<'a> {
    pub chart_rect: FlowRect,
    pub viewport: &'a Viewport,
    pub total_bars: usize,
    pub first_bar_index: usize,
    /// Regions the normalized x axis is divided into: one per bar, plus the
    /// live lane when the frame has one.
    pub slot_count: usize,
    /// Width in pixels of the live lane's pane, taken off the right edge of
    /// the chart. `0.0` means the frame has no lane and the candles own the
    /// whole chart.
    pub lane_width_px: f32,
    /// The live strip left of the divider: the candles' pane ends before it.
    pub trail_strip_px: f32,
    /// Whether the chart is upside down. The projection speaks in fractions
    /// of the price window and knows nothing of orientation; the flip happens
    /// here, at the same boundary where the candles' own scale flips, so the
    /// map, the bubbles and the bars they sit on turn over together.
    pub inverted: bool,
    /// Every position is on the tape, its opening too: a held tape's book.
    pub lane_only: bool,
    /// Native tape time inset the lane's depth, reductions and picking share
    /// with the dots; `None` keeps them linear (see `from_style`).
    pub tape_geometry: Option<TapeGeometry>,
}

impl<'a> ProjectedLayout<'a> {
    #[must_use]
    pub fn new(
        chart_rect: FlowRect,
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
            trail_strip_px: 0.0,
            inverted: false,
            lane_only: false,
            tape_geometry: None,
        }
    }

    /// The same layout upside down when `inverted` — see the field's note.
    #[must_use]
    pub fn with_inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    /// For primitives on the tape alone: its opening maps to the divider.
    #[must_use]
    pub fn with_lane_only(mut self) -> Self {
        self.lane_only = true;
        self
    }

    #[must_use]
    pub fn x(self, normalized: f64) -> f32 {
        let normalized = finite_unit_f64(normalized) as f32;
        let regions = self.slot_count as f32;
        // `region_pos` is the position in region units over `[0, regions]`.
        // Bars go through the viewport, which owns everything left of the
        // divider; the lane — the last region — is a fixed band of screen,
        // mapped linearly and answering to nothing the candles do.
        let region_pos = normalized * regions;
        let boundary = regions - 1.0;
        match self.lane_left_x() {
            Some(divider) if regions >= 1.0 && (self.lane_only || region_pos > boundary) => {
                divider
                    + self.tape_geometry.map_or_else(
                        || (region_pos - boundary).max(0.0) * self.lane_width_px,
                        |geometry| geometry.x(f64::from((region_pos - boundary).max(0.0))),
                    )
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
    pub fn lane_left_x(self) -> Option<f32> {
        lane_divider_x(self.chart_rect, self.lane_width_px)
    }

    /// Right edge of the candles' own pane: the divider when a lane is drawn,
    /// less the strip standing left of it; the chart's right edge otherwise.
    #[must_use]
    fn history_right(self) -> f32 {
        self.lane_left_x().map_or(self.chart_rect.right, |divider| {
            (divider - self.trail_strip_px).max(self.chart_rect.left)
        })
    }

    /// The candles' pane — everything left of the divider.
    #[must_use]
    pub fn history_rect(self) -> FlowRect {
        self.chart_rect.with_right(self.history_right())
    }

    /// The tape's pane — everything right of the divider.
    #[must_use]
    pub fn lane_rect(self) -> FlowRect {
        let left = self.lane_left_x().unwrap_or(self.chart_rect.right);
        FlowRect {
            left,
            ..self.chart_rect
        }
    }

    /// Whether a normalized position belongs to the tape rather than the
    /// candles. `false` for every position when the frame has no lane.
    #[must_use]
    pub fn in_lane(self, normalized: f64) -> bool {
        let regions = self.slot_count as f64;
        self.lane_left_x().is_some()
            && self.slot_count >= 1
            && (self.lane_only || finite_unit_f64(normalized) > (regions - 1.0) / regions)
    }

    /// The pane a normalized position belongs to.
    ///
    /// Panning the candles into history sends the newest bars off the right of
    /// their own pane, which is now the divider rather than the chart edge.
    /// Clipping each primitive to its pane is what keeps the two from drawing
    /// into each other — a candle scrolls out of sight behind the tape instead
    /// of over it, and nothing on the tape reaches back across the divider.
    #[must_use]
    pub fn pane(self, normalized: f64) -> FlowRect {
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
    pub fn span_pane(self, x0: f64, x1: f64) -> FlowRect {
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
    pub fn layer_clip(self, chart: bool, lane: bool) -> Option<FlowRect> {
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
    pub fn y(self, normalized: f64) -> f32 {
        self.y_unclamped(finite_unit_f64(normalized))
    }

    /// [`Self::y`] off the chart too: a volume dot off the price window is
    /// hidden by its pane's clip rather than piled on the edge.
    #[must_use]
    pub fn y_unclamped(self, normalized: f64) -> f32 {
        let unit = if normalized.is_finite() {
            normalized as f32
        } else {
            0.0
        };
        let unit = if self.inverted { 1.0 - unit } else { unit };
        self.chart_rect.top + unit * self.chart_rect.height()
    }

    #[must_use]
    pub fn band(self, x0: f64, x1: f64, y0: f64, y1: f64, min_height: f32) -> FlowRect {
        // A lane cell can begin exactly at the shared slot boundary. Its
        // opening belongs to the padded tape, while a historical cell ending
        // there still belongs to the candles' viewport.
        let lane_start = 1.0 - 1.0 / self.slot_count.max(1) as f64;
        let time = if self.lane_left_x().is_some() && x0 >= lane_start && x1 > lane_start {
            self.with_lane_only()
        } else {
            self
        };
        let corners = ((time.x(x0), self.y(y0)), (time.x(x1), self.y(y1)));
        readable_band(
            FlowRect::from_corners(corners.0, corners.1),
            min_height,
            self.span_pane(x0, x1),
        )
    }

    /// Screen rectangle one heat cell paints over, for the painter and the
    /// pointer alike; nothing for a current row that only previews the book.
    /// The native tape's book spans its whole lane (`book_span_px`).
    #[must_use]
    pub fn heat_cell_rect(self, cell: &HeatmapCell, min: f32, live: bool) -> FlowRect {
        let (top, bottom) = (self.y(cell.y0), self.y(cell.y1));
        match self.tape_x(|tape| tape.book_span_px(cell, self.slot_count, live)) {
            // A lead cut away at the pane edge paints and hits nothing.
            Some((x0, x1)) if x1 <= x0 => FlowRect::NOTHING,
            Some((x0, x1)) => readable_band(
                FlowRect::from_corners((x0, top), (x1, bottom)),
                min,
                self.lane_rect(),
            ),
            None if cell.current_book && cell.x1 <= cell.x0 => FlowRect::NOTHING,
            None => self.band(cell.x0, cell.x1, cell.y0, cell.y1, min),
        }
    }

    /// Screen x of a span the native tape places from its lane's left edge.
    pub fn tape_x(self, span: impl FnOnce(TapeGeometry) -> Span) -> Span {
        let (left, tape) = self.lane_left_x().zip(self.tape_geometry)?;
        span(tape).map(|(x0, x1)| (left + x0, left + x1))
    }

    /// Snapshot preview in the blank forward space, without stretching any
    /// historical interval or borrowing the tape's oldest timestamp.
    #[must_use]
    pub fn current_book_preview_rect(self, y0: f64, y1: f64) -> FlowRect {
        let (top, bottom) = (self.y(y0).min(self.y(y1)), self.y(y0).max(self.y(y1)));
        FlowRect {
            left: self.x_at_ext(self.slot_count.saturating_sub(1) as f32),
            top,
            right: self.history_right(),
            bottom,
        }
        .intersect(self.history_rect())
    }

    #[must_use]
    pub fn event_band(self, x: f64, y0: f64, y1: f64, min_height: f32) -> EventBand {
        let row = self.band(x, x, y0, y1, min_height);
        EventBand {
            x: if self.tape_geometry.is_some() && x >= 1.0 - 1.0 / self.slot_count.max(1) as f64 {
                self.with_lane_only().x(x)
            } else {
                self.x(x)
            },
            top: row.top,
            bottom: row.bottom,
        }
    }
}

/// `rect` grown about its centre to `minimum_height` (never under half a
/// point either way), then clipped to `clip`; nothing when it is not finite.
fn readable_band(rect: FlowRect, minimum_height: f32, clip: FlowRect) -> FlowRect {
    if !rect.is_finite() {
        return FlowRect::NOTHING;
    }
    let minimum_height = minimum_height.max(0.5);
    let readable = if rect.height() < minimum_height {
        let (half_width, half_height) = (rect.width().max(0.5) * 0.5, minimum_height * 0.5);
        let (cx, cy) = (
            (rect.left + rect.right) / 2.0,
            (rect.top + rect.bottom) / 2.0,
        );
        FlowRect {
            left: cx - half_width,
            top: cy - half_height,
            right: cx + half_width,
            bottom: cy + half_height,
        }
    } else {
        rect
    };
    readable.intersect(clip)
}

#[cfg(test)]
#[path = "flow_geometry_tests.rs"]
mod tests;
