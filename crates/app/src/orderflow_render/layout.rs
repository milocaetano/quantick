//! The projection half of the order-flow renderer: where a normalized
//! coordinate lands on the canvas, and which pane it lands in.
//!
//! Every layer painter in the sibling modules is handed a [`RenderContext`]
//! and asks this module the same questions — `x`, `y`, `band`, `pane`,
//! `layer_clip` — so the map, the bubbles, the legend and the on-demand
//! pointer resolver cannot grow two pixel mappings. Nothing here paints.

use eframe::egui;
use quantick_engine::Side;
use quantick_orderflow::{AggressionPrimitive, HeatmapProjection};

use crate::viewport::Viewport;

use super::{OrderflowRenderStyle, finite_unit_f64, readable_band};

/// Screen x where the history pane ends and the live lane begins.
///
/// The lane is a pane pinned to the right edge of the chart, so the divider is
/// a property of the chart rect alone — no viewport, no bars. That is exactly
/// what makes panning and zooming the candles leave the tape where it is.
/// `None` when the frame has no lane, or when the lane would take the whole
/// chart and leave the candles nowhere to go.
#[must_use]
pub(crate) fn lane_divider_x(chart_rect: egui::Rect, lane_width_px: f32) -> Option<f32> {
    (lane_width_px.is_finite() && lane_width_px > 0.0 && lane_width_px < chart_rect.width())
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
        }
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
        let unit = finite_unit_f64(normalized) as f32;
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

    /// Screen rectangle occupied by one semantic heatmap cell.
    ///
    /// Kept on the renderer's projection object so the on-demand pointer
    /// resolver and the paint path cannot grow two pixel mappings. This does
    /// no work unless a control snapshot explicitly asks what is under the
    /// cursor.
    #[must_use]
    pub(crate) fn heat_cell_rect(
        self,
        x0: f64,
        x1: f64,
        y0: f64,
        y1: f64,
        min_height: f32,
    ) -> egui::Rect {
        self.band(x0, x1, y0, y1, min_height)
    }

    #[must_use]
    pub(super) fn event_band(self, x: f64, y0: f64, y1: f64, min_height: f32) -> EventBand {
        let top = self.y(y0);
        let bottom = self.y(y1);
        let row = readable_band(
            egui::Rect::from_min_max(
                egui::pos2(self.x(x), top.min(bottom)),
                egui::pos2(self.x(x), top.max(bottom)),
            ),
            min_height,
            self.pane(x),
        );
        EventBand {
            x: self.x(x),
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
        }
    }

    /// The aggressions this canvas draws as bubbles, in projection order.
    ///
    /// The display switches live here rather than in the projection: the
    /// clusters are a fact several surfaces read (bubbles, the consumption
    /// carve behind them, the live strip's histogram), so hiding the bubble
    /// layer has to hide bubbles — not empty the frame everyone else reads.
    /// The size reference, the dust merge and the liquidity association all
    /// saw both sides upstream, so hiding one side never rescales or
    /// re-associates the other.
    ///
    /// Reads the raw style rather than the sanitized copy on purpose:
    /// `sanitized` clamps numbers and never touches a display flag, so the two
    /// answer identically and the filter costs no second clone per frame.
    pub(crate) fn bubbles(&self) -> impl Iterator<Item = &'a AggressionPrimitive> {
        let style = self.style;
        let projection = self.projection;
        let both_sides = style.show_buy && style.show_sell;
        projection.aggressions.iter().filter(move |mark| {
            // Which pane a print belongs to is the projection's own answer —
            // the same one that clustered it on the tape's window rather than
            // history's — so the switch is read from the mark, never inferred
            // a second time from its position.
            if !(if mark.live {
                style.lane_aggression_layer
            } else {
                style.aggression_layer
            }) {
                return false;
            }
            // A mark carrying both sides — a merged cluster, or a bar summary
            // — is sized by the two together, so with one side hidden its area
            // would state a quantity the canvas is not showing. It is withheld
            // rather than drawn at a lie of a size. The projection used to
            // decide this by refusing to summarize at all; it now builds the
            // same clusters whatever is on screen, which is what keeps the
            // live strip's histogram steady while a bubble switch moves.
            if !both_sides && mark.buy_share > 0.0 && mark.buy_share < 1.0 {
                return false;
            }
            match mark.side {
                Side::Buy => style.show_buy,
                Side::Sell => style.show_sell,
            }
        })
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
