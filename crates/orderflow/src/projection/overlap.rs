//! The overlap grid: bubbles that share a cell of the canvas become one mark,
//! so no two drawn discs overlap.
//!
//! A dense tape — an MT5 feed folding several deals into each tick is the
//! case that asked for this — piles bubbles on top of each other: a green
//! disc half under a red one reads as neither, and ten specks inside a tenth
//! of a second read as noise. The budget fold (`fold`) does not help, because
//! a frame can be well inside its budget and still unreadable; the question
//! here is geometric, not a count. So, when the trader opts in, each pane is
//! cut into cells and every mark in a cell folds into one: the exact summed
//! quantity, the union of the evidence, a pie when both sides are in it, and
//! the `⊕n` label that says the canvas did this, not the market.
//!
//! A grid rather than a search for touching discs, because the search was
//! tried and failed both ways: folding only direct neighbours left a fold
//! drawn at its summed size over the next one, and following touch chained a
//! whole rally into one mark. A cell is at most one full-size disc wide and
//! tall, a mark is drawn inside its cell, and so nothing overlaps by
//! construction — at any zoom, panned or not.
//!
//! A cell never spans a bar, because a mark claims its bar traded it: on the
//! candles the columns subdivide a bar's slot, and on the tape they
//! subdivide each bar's stretch of the window, keyed by the true bar from the
//! series ([`PaneGeometry::lane_bar_opens`]) so a tape whose bars are panned
//! off the candles still bins. Rows are whole visual price rows. A slot
//! narrower than a disc is one cell, and its disc is held to it.
//!
//! The decision needs pixels the normalized projection does not carry, so the
//! chart hands them over as a [`PaneGeometry`] and the radius comes from the
//! same [`bubble_radius`] the painter draws with. Nothing here reads a clock
//! or iterates a hash: the same frame and the same geometry bin the same way.

use std::collections::BTreeMap;

use quantick_engine::Bar;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::config::{HeatmapConfig, bubble_center_offset, bubble_radius};
use crate::timeline::BarTimeline;

use super::fold::fold_onto;
use super::model::{AggressionPrimitive, HeatmapProjection, PriceWindow, frame_order};

/// The pixel geometry of the pane a frame is drawn on.
///
/// Only what the grid needs: how wide a bar and the tape are on screen, how
/// tall the chart is, and where the tape's bars begin. The candles and the
/// tape are measured separately because they are mapped separately — a bar
/// slot is `px_per_bar` wide, the tape's one region is `lane_width_px` wide —
/// and the grid never puts a mark of one in a cell of the other.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneGeometry {
    /// Pixels between two neighbouring bars on the candles.
    pub px_per_bar: f32,
    /// Width of the live lane, in pixels. Zero when no lane is drawn.
    pub lane_width_px: f32,
    /// Height of the chart, in pixels.
    pub height_px: f32,
    /// Open times, ascending, of the series' bars the tape's window may
    /// reach — whether or not the candles have them on screen. Empty bins the
    /// tape as one bar.
    pub lane_bar_opens: Vec<i64>,
}

/// The open times [`PaneGeometry::lane_bar_opens`] wants: every bar that
/// ends inside the last `window_ms` of the series, and the one before it,
/// so the bar the window opens in is known too.
#[must_use]
pub fn lane_bar_opens(closed: &[Bar], partial: Option<&Bar>, window_ms: i64) -> Vec<i64> {
    let bars: Vec<&Bar> = closed.iter().chain(partial).collect();
    let Some(newest) = bars.last() else {
        return Vec::new();
    };
    let start = newest.close_time.saturating_sub(window_ms.max(0));
    let first = bars.partition_point(|bar| bar.close_time < start);
    bars[first.saturating_sub(1)..]
        .iter()
        .map(|bar| bar.open_time)
        .collect()
}

fn unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Keeps a mark at the very start of a slot from reading as the end of the
/// previous one after the normalized round trip.
const SLOT_EPSILON: f64 = 1e-9;

/// What may fold with what: the pane, the bar, and the column and row of the
/// cell inside it.
type CellKey = (bool, i64, i64, i64);

/// One cell's rectangle, in pane pixels, upright.
#[derive(Debug, Clone, Copy)]
struct Cell {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

impl Cell {
    /// The largest radius a disc inside the cell may be drawn at.
    fn radius_cap(self) -> f64 {
        ((self.right - self.left).min(self.bottom - self.top) / 2.0).max(0.0)
    }
}

impl HeatmapProjection {
    /// Bin the bubbles into the grid — see the module note — and fold each
    /// cell into one mark, into [`overlap_marks`](Self::overlap_marks), the
    /// painter's own list.
    ///
    /// Does nothing unless [`HeatmapConfig::bubble_overlap_merge`] is on, so
    /// off is today's frame exactly. On, [`aggressions`](Self::aggressions)
    /// and the budget's [`folded_aggressions`](Self::folded_aggressions) are
    /// left as they were: every reader but the painter keeps the unfolded
    /// per-price quantities. Both sides fold together; while one side is
    /// hidden the painter draws the unfolded marks instead, since it withholds
    /// a two-sided mark then.
    pub fn merge_overlapping_bubbles(
        &mut self,
        geometry: &PaneGeometry,
        timeline: &BarTimeline,
        prices: PriceWindow,
        config: &HeatmapConfig,
    ) {
        self.overlap_marks = None;
        // The painter draws the unfolded marks while a side is hidden, and
        // nothing while no bubble layer is drawn: a fold then is pure cost.
        let both_sides = config.show_buy_aggressions && config.show_sell_aggressions;
        let any_layer = config.show_aggressions || config.lane_aggressions_drawn();
        if !config.bubble_overlap_merge || !both_sides || !any_layer {
            return;
        }
        let grid = Grid::new(
            geometry,
            timeline,
            prices,
            self.effective_grouping.bucket_width,
            config,
        );
        // One canonical order, so which mark anchors a cell never depends on
        // the order the marks arrived in; binning keeps it.
        let mut marks = self.aggressions.clone();
        marks.sort_by(frame_order);
        let mut cells: BTreeMap<CellKey, (Cell, Vec<AggressionPrimitive>)> = BTreeMap::new();
        let mut merged = Vec::with_capacity(marks.len());
        for mark in marks {
            match grid.cell(&mark) {
                Some((key, cell)) => cells.entry(key).or_insert((cell, Vec::new())).1.push(mark),
                None => merged.push(mark),
            }
        }
        for ((live, ..), (cell, members)) in cells {
            // The scale each pane's marks were drawn on — a fold is sized
            // against it, never rescaling the marks it left alone.
            let reference = if live {
                self.aggression_reference
            } else {
                self.summary_reference
            };
            merged.push(grid.settle(cell, members, reference));
        }
        merged.sort_by(frame_order);
        self.overlap_marks = Some(merged);
    }

    /// The marks the bubble painter draws: the grid's list while both sides
    /// are shown, the unfolded marks otherwise. With a side hidden the
    /// painter withholds every two-sided mark — a pie with a hidden half would
    /// state a quantity the canvas is not showing — so drawing the fold then
    /// would delete the visible side's bubbles along with the pies.
    #[must_use]
    pub fn drawn_bubbles(&self, both_sides: bool) -> &[AggressionPrimitive] {
        match &self.overlap_marks {
            Some(folded) if both_sides => folded,
            _ => &self.aggressions,
        }
    }
}

/// Everything that turns a normalized mark into its cell on screen.
struct Grid<'a> {
    timeline: &'a BarTimeline,
    opens: &'a [i64],
    regions: f64,
    slots: f64,
    px_per_bar: f64,
    lane_width: f64,
    height: f64,
    high: f64,
    span: f64,
    row_price: f64,
    candle_radii: (f32, f32),
    lane_radii: (f32, f32),
    side_offset: f32,
}

impl<'a> Grid<'a> {
    fn new(
        geometry: &'a PaneGeometry,
        timeline: &'a BarTimeline,
        prices: PriceWindow,
        row_price: Decimal,
        config: &HeatmapConfig,
    ) -> Self {
        let bubbles = &config.bubbles;
        let high = prices.high.to_f64().unwrap_or(0.0);
        let low = prices.low.to_f64().unwrap_or(0.0);
        Self {
            timeline,
            opens: &geometry.lane_bar_opens,
            regions: timeline.region_count() as f64,
            slots: timeline.len() as f64,
            px_per_bar: f64::from(geometry.px_per_bar),
            lane_width: f64::from(geometry.lane_width_px),
            height: f64::from(geometry.height_px),
            high,
            span: high - low,
            row_price: row_price.to_f64().unwrap_or(0.0),
            candle_radii: (bubbles.min_radius, bubbles.max_radius),
            lane_radii: config.live_lane.scaled_radii(bubbles),
            side_offset: bubbles.side_offset,
        }
    }

    fn radii(&self, live: bool) -> (f32, f32) {
        if live {
            self.lane_radii
        } else {
            self.candle_radii
        }
    }

    /// The mark's x in pane pixels, and the bar it belongs to with that
    /// bar's stretch of the pane. `None` when the pane has no width.
    fn bar(&self, mark: &AggressionPrimitive) -> Option<(f64, i64, f64, f64)> {
        let region = unit(mark.x) * self.regions;
        if mark.live {
            if self.lane_width <= 0.0 {
                return None;
            }
            let x = (region - (self.regions - 1.0)).clamp(0.0, 1.0) * self.lane_width;
            if self.opens.is_empty() {
                return Some((x, 0, 0.0, self.lane_width));
            }
            let bar = self.opens.partition_point(|open| *open <= mark.placed_ms);
            let left = bar
                .checked_sub(1)
                .map_or(0.0, |i| self.lane_px(self.opens[i]));
            let right = self
                .opens
                .get(bar)
                .map_or(self.lane_width, |open| self.lane_px(*open));
            return Some((x, bar as i64, left, right.max(left)));
        }
        if self.px_per_bar <= 0.0 || self.slots < 1.0 {
            return None;
        }
        let slot = (region + SLOT_EPSILON).floor().clamp(0.0, self.slots - 1.0);
        let left = slot * self.px_per_bar;
        Some((
            region * self.px_per_bar,
            slot as i64,
            left,
            left + self.px_per_bar,
        ))
    }

    /// Where on the tape, in pixels, an instant is drawn.
    fn lane_px(&self, timestamp_ms: i64) -> f64 {
        self.timeline
            .locate_in_lane_clamped(timestamp_ms)
            .map_or(0.0, |position| position.fraction * self.lane_width)
    }

    /// The cell a mark falls in, or `None` when its pane is not drawn.
    fn cell(&self, mark: &AggressionPrimitive) -> Option<(CellKey, Cell)> {
        let (x, bar, left, right) = self.bar(mark)?;
        let diameter = 2.0 * f64::from(self.radii(mark.live).1);
        let width = right - left;
        let columns = (width / diameter).floor().max(1.0);
        let column_width = width / columns;
        let column = if column_width > 0.0 {
            ((x - left) / column_width)
                .floor()
                .clamp(0.0, columns - 1.0)
        } else {
            0.0
        };
        // Whole visual rows, as many as a full disc needs, centred on a row.
        let row_px = self.row_price / self.span * self.height;
        let cell_price = if row_px.is_finite() && row_px > 0.0 {
            (diameter / row_px).ceil().max(1.0) * self.row_price
        } else {
            diameter / self.height * self.span
        };
        if !(cell_price.is_finite() && cell_price > 0.0) {
            return None;
        }
        let price = self.high - unit(mark.y) * self.span;
        let row = (price / cell_price).round();
        let y_of = |price: f64| (self.high - price) / self.span * self.height;
        let cell = Cell {
            left: left + column * column_width,
            right: left + (column + 1.0) * column_width,
            top: y_of((row + 0.5) * cell_price).max(0.0),
            bottom: y_of((row - 0.5) * cell_price).min(self.height),
        };
        Some(((mark.live, bar, column as i64, row as i64), cell))
    }

    /// Fold one cell into a mark drawn inside it: at the column's centre and
    /// the quantity-weighted price, no bigger than the cell. A lone mark keeps
    /// its own place, held inside the cell the same way.
    fn settle(
        &self,
        cell: Cell,
        mut members: Vec<AggressionPrimitive>,
        reference: Decimal,
    ) -> AggressionPrimitive {
        let live = members[0].live;
        let lone = members.len() == 1;
        let (weight, weighted_y) = members.iter().fold((0.0, 0.0), |(weight, sum), mark| {
            let quantity = mark.quantity.to_f64().unwrap_or(0.0);
            (weight + quantity, sum + quantity * unit(mark.y))
        });
        // The heaviest mark anchors the fold, the earliest on a tie.
        let heaviest = members
            .iter()
            .enumerate()
            .max_by(|(a, x), (b, y)| x.quantity.cmp(&y.quantity).then(b.cmp(a)))
            .map_or(0, |(index, _)| index);
        let anchor = members.remove(heaviest);
        let own_x = self.bar(&anchor).map_or(0.0, |(x, ..)| x);
        let mut mark = fold_onto(anchor, members, reference);

        let cap = cell.radius_cap();
        let (minimum, maximum) = self.radii(live);
        let radius = f64::from(bubble_radius(mark.size, minimum, maximum)).min(cap);
        let x = if lone {
            own_x
        } else {
            (cell.left + cell.right) / 2.0
        };
        let x = x.max(cell.left + radius).min(cell.right - radius);
        let y = if weight > 0.0 {
            weighted_y / weight
        } else {
            unit(mark.y)
        } * self.height;
        let lean = f64::from(bubble_center_offset(
            mark.buy_share,
            self.side_offset,
            false,
        ));
        let center = (y + lean).max(cell.top + radius).min(cell.bottom - radius);
        mark.y = unit((center - lean) / self.height);
        mark.x = if live {
            (self.regions - 1.0 + x / self.lane_width) / self.regions
        } else {
            x / self.px_per_bar / self.regions
        };
        mark.radius_cap_px = Some(cap as f32);
        mark
    }
}
