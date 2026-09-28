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
//! whole rally into one mark. A cell is at most one full-size disc and its
//! dressing wide and tall — halo, rings and crown, the painter's own
//! [`BubbleStyle::dressing_margin`] — everything drawn for a mark stays
//! inside its cell, and so nothing overlaps by construction, at any zoom,
//! panned or not. Where a cell is too small for the dressing (a bar slot a
//! few pixels wide) the mark is drawn bare, a disc and nothing around it.
//!
//! A cell never spans a bar, because a mark claims its bar traded it: on the
//! candles the columns subdivide a bar's slot, and on the tape they are
//! stretches of market time counted from each bar's open, keyed by the true
//! bar from the series ([`PaneGeometry::lane_bar_opens`]) so a tape whose
//! bars are panned off the candles still bins. Rows are whole visual price
//! rows counted from price zero. A slot narrower than a disc is one cell, and
//! its disc is held to it.
//!
//! The past does not move: a cell is anchored to market time and to price,
//! never to where the window begins on screen, so a fold already drawn keeps
//! its prints while the tape rolls, the forming bar grows or the chart pans.
//! Cell sizes grow only in doublings, so the axis refitting a little does
//! not regroup either; a zoom or a refit past a doubling does. On the
//! candles a bar is one column until it is wider than two discs; past that
//! its columns follow its slot, whose span can move with the forming bar.
//!
//! The grid owns a placed mark's centre, side lean included: the painter
//! adds no lean of its own to it, so a lean can never push a disc out of its
//! cell at the chart's edge. A lone print is held inside its cell too, so it
//! may sit up to its reach — disc and dressing — from where it traded.
//!
//! The decision needs pixels the normalized projection does not carry, so the
//! chart hands them over as a [`PaneGeometry`] and the radius comes from the
//! same [`bubble_radius`](crate::bubble_radius) the painter draws with.
//! Nothing here reads a clock or iterates a hash: the same frame and the same
//! geometry bin the same way.

use std::collections::BTreeMap;

use quantick_engine::Bar;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::config::{BubbleStyle, HeatmapConfig, bubble_center_offset};
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
/// ends inside the last two `window_ms` of the series, and the one before
/// it. Twice the window because the view and the engine each resolve the
/// tape's window; a bar the engine's window reaches must be in this list, or
/// its prints would be binned as if they belonged to the bar after it.
#[must_use]
pub fn lane_bar_opens(closed: &[Bar], partial: Option<&Bar>, window_ms: i64) -> Vec<i64> {
    let Some(newest) = partial.or(closed.last()) else {
        return Vec::new();
    };
    let start = newest
        .close_time
        .saturating_sub(window_ms.max(0).saturating_mul(2));
    let first = closed.partition_point(|bar| bar.close_time < start);
    closed[first.saturating_sub(1)..]
        .iter()
        .chain(partial)
        .map(|bar| bar.open_time)
        .collect()
}

/// The smallest power of two at or above `value`, and at least one: cells
/// grow only in doublings, so a small change in scale leaves them as they
/// were.
fn doubling(value: f64) -> f64 {
    if value.is_finite() && value > 1.0 {
        2f64.powi(value.log2().ceil() as i32)
    } else {
        1.0
    }
}

fn unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The most visual rows one cell may span; a chart asking for more is
/// degenerate for the frame, and draws it unmerged rather than overflow.
const MAX_ROWS_PER_CELL: f64 = 1_048_576.0;

/// Keeps a price on a bucket's exact floor in that bucket after the float
/// round trip through the chart's y, in buckets.
const BUCKET_NUDGE: f64 = 1e-6;

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
    /// How far from its centre a mark inside the cell may reach.
    fn reach(self) -> f64 {
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

/// One pane's cell size, worked out once per frame.
#[derive(Debug, Clone, Copy)]
struct PaneCells {
    /// The radius range marks of this pane are drawn on.
    radii: (f32, f32),
    /// A full-size disc and its dressing across, in pixels: the least a cell
    /// may be, so a mark drawn whole still fits.
    diameter: f64,
    /// How tall a cell is, in price: whole rows, as many as the diameter
    /// needs rounded up to a doubling. `None` when the chart has no height,
    /// or a degenerate one that asks for an absurd number of rows.
    cell_price: Option<Decimal>,
    /// How many visual rows one cell is tall.
    rows: i64,
    /// How long a tape cell is, in milliseconds, rounded up to a doubling.
    /// Unused on the candles.
    cell_ms: i64,
    /// Width of the pane, in pixels.
    width: f64,
}

/// Everything that turns a normalized mark into its cell on screen.
struct Grid<'a> {
    opens: &'a [i64],
    regions: f64,
    slots: f64,
    px_per_bar: f64,
    lane_width: f64,
    lane_start_ms: f64,
    lane_ms_per_px: f64,
    height: f64,
    high: f64,
    span: f64,
    row_price: f64,
    candles: PaneCells,
    lane: PaneCells,
    bubbles: &'a BubbleStyle,
    margin: f64,
}

impl<'a> Grid<'a> {
    fn new(
        geometry: &'a PaneGeometry,
        timeline: &'a BarTimeline,
        prices: PriceWindow,
        row_price: Decimal,
        config: &'a HeatmapConfig,
    ) -> Self {
        let bubbles = &config.bubbles;
        let high = prices.high.to_f64().unwrap_or(0.0);
        let span = high - prices.low.to_f64().unwrap_or(0.0);
        let height = f64::from(geometry.height_px);
        let lane_width = f64::from(geometry.lane_width_px);
        let lane = timeline.lane_bounds_ms();
        let lane_ms_per_px = lane.map_or(0.0, |(start, end)| (end - start) as f64 / lane_width);
        let margin = f64::from(bubbles.dressing_margin());
        let row_px = row_price.to_f64().unwrap_or(0.0) / span * height;
        let slots = timeline.len() as f64;
        let px_per_bar = f64::from(geometry.px_per_bar);
        let pane = |radii: (f32, f32), width: f64| {
            let diameter = 2.0 * (f64::from(radii.1) + margin);
            // Rows counted in doublings, so the axis refitting a little does
            // not regroup; and in exact prices, so no float decides a row.
            let rows = doubling((diameter / row_px).ceil());
            let cell_price = (row_px.is_finite() && row_px > 0.0 && rows <= MAX_ROWS_PER_CELL)
                .then(|| row_price.checked_mul(Decimal::from(rows as i64)))
                .flatten()
                .filter(|price| *price > Decimal::ZERO);
            PaneCells {
                radii,
                diameter,
                cell_price,
                rows: rows as i64,
                cell_ms: doubling((diameter * lane_ms_per_px).ceil()) as i64,
                width,
            }
        };
        Self {
            opens: &geometry.lane_bar_opens,
            regions: timeline.region_count() as f64,
            slots,
            px_per_bar,
            lane_width,
            lane_start_ms: lane.map_or(0.0, |(start, _)| start as f64),
            lane_ms_per_px,
            height,
            high,
            span,
            row_price: row_price.to_f64().unwrap_or(0.0),
            candles: pane((bubbles.min_radius, bubbles.max_radius), slots * px_per_bar),
            lane: pane(config.live_lane.scaled_radii(bubbles), lane_width),
            bubbles,
            margin,
        }
    }

    fn pane(&self, live: bool) -> PaneCells {
        if live { self.lane } else { self.candles }
    }

    /// The mark's x in pane pixels. `None` when its pane has no width.
    fn x_px(&self, mark: &AggressionPrimitive) -> Option<f64> {
        let region = unit(mark.x) * self.regions;
        if mark.live {
            (self.lane_width > 0.0)
                .then(|| (region - (self.regions - 1.0)).clamp(0.0, 1.0) * self.lane_width)
        } else {
            (self.px_per_bar > 0.0 && self.slots >= 1.0).then_some(region * self.px_per_bar)
        }
    }

    /// Where on the tape, in pixels, an instant is drawn — past either edge
    /// too, so a cell the window cuts keeps its true size and place.
    fn lane_px(&self, timestamp_ms: i64) -> f64 {
        (timestamp_ms as f64 - self.lane_start_ms) / self.lane_ms_per_px
    }

    /// The column a mark falls in — its key and its left and right edge in
    /// pane pixels — and the bar it belongs to.
    ///
    /// On the tape a column is a stretch of market time counted from its
    /// bar's open, as long as a full disc is wide rounded up to a doubling of
    /// milliseconds: a print keeps its column while the tape rolls on and
    /// while the forming bar grows, so a fold already drawn never regroups.
    /// On the candles it is a share of the bar's slot. A slot is one column
    /// at any zoom where a bar is narrower than two discs; wider, a column
    /// follows the slot's span, which can move with the forming bar or with
    /// a pan that makes a bar the rightmost one shown.
    fn column(&self, mark: &AggressionPrimitive, pane: PaneCells) -> Option<(i64, i64, f64, f64)> {
        let x = self.x_px(mark)?;
        if mark.live {
            if !(self.lane_ms_per_px.is_finite() && self.lane_ms_per_px > 0.0) {
                return None;
            }
            let bar = self.opens.partition_point(|open| *open <= mark.placed_ms);
            let open = bar.checked_sub(1).map_or(0, |i| self.opens[i]);
            let close = self.opens.get(bar).copied().unwrap_or(i64::MAX);
            let column = (mark.placed_ms - open).div_euclid(pane.cell_ms);
            let from = open + column * pane.cell_ms;
            let to = from.saturating_add(pane.cell_ms).min(close);
            return Some((bar as i64, column, self.lane_px(from), self.lane_px(to)));
        }
        let slot = ((x / self.px_per_bar) + SLOT_EPSILON)
            .floor()
            .clamp(0.0, self.slots - 1.0);
        let left = slot * self.px_per_bar;
        let columns = (self.px_per_bar / pane.diameter).floor().max(1.0);
        let width = self.px_per_bar / columns;
        let column = ((x - left) / width).floor().clamp(0.0, columns - 1.0);
        Some((
            slot as i64,
            column as i64,
            left + column * width,
            left + (column + 1.0) * width,
        ))
    }

    /// The cell a mark falls in, or `None` when its pane is not drawn.
    fn cell(&self, mark: &AggressionPrimitive) -> Option<(CellKey, Cell)> {
        let pane = self.pane(mark.live);
        let cell_price = pane.cell_price?;
        let (bar, column, left, right) = self.column(mark, pane)?;
        // The bucket the mark is drawn in — a regional or budget fold is
        // drawn at its point of control, not at its range's floor — counted
        // from price zero, so panning never moves a print to another row.
        // The nudge keeps a price on a bucket's exact floor in that bucket.
        let price = self.high - unit(mark.y) * self.span;
        let bucket = (price / self.row_price + BUCKET_NUDGE).floor();
        if !bucket.is_finite() {
            return None;
        }
        let row = Decimal::from((bucket as i64).div_euclid(pane.rows));
        let y_of = |price: Decimal| {
            (self.high - price.to_f64().unwrap_or(self.high)) / self.span * self.height
        };
        // The key is anchored; the rectangle a mark is drawn in is what of
        // the cell is on screen, so an edge cell holds a smaller disc.
        let cell = Cell {
            left: left.max(0.0),
            right: right.min(pane.width),
            top: y_of((row + Decimal::ONE).checked_mul(cell_price)?).max(0.0),
            bottom: y_of(row.checked_mul(cell_price)?).min(self.height),
        };
        Some(((mark.live, bar, column, row.to_i64()?), cell))
    }

    /// Fold one cell into a mark drawn inside it: at the column's centre and
    /// the quantity-weighted price, its disc and dressing no bigger than the
    /// cell. A lone mark keeps its own place, held inside the cell the same
    /// way — so its traded level is at most its reach from the centre.
    fn settle(
        &self,
        cell: Cell,
        mut members: Vec<AggressionPrimitive>,
        reference: Decimal,
    ) -> AggressionPrimitive {
        let live = members[0].live;
        let lone = members.len() == 1;
        let (weight, weighted_x, weighted_y) =
            members
                .iter()
                .fold((0.0, 0.0, 0.0), |(weight, x, y), mark| {
                    let quantity = mark.quantity.to_f64().unwrap_or(0.0);
                    let own_x = self.x_px(mark).unwrap_or(0.0);
                    (
                        weight + quantity,
                        x + quantity * own_x,
                        y + quantity * unit(mark.y),
                    )
                });
        // The heaviest mark anchors the fold, the earliest on a tie.
        let heaviest = members
            .iter()
            .enumerate()
            .max_by(|(a, x), (b, y)| x.quantity.cmp(&y.quantity).then(b.cmp(a)))
            .map_or(0, |(index, _)| index);
        let anchor = members.remove(heaviest);
        let own_x = self.x_px(&anchor).unwrap_or(0.0);
        let mut mark = fold_onto(anchor, members, reference);

        let reach = cell.reach();
        mark.cell_radius_px = Some(reach as f32);
        let (minimum, maximum) = self.pane(live).radii;
        let disc = mark.drawn_disc(minimum, maximum, self.bubbles);
        let dressing = if disc.dressed { self.margin } else { 0.0 };
        let radius = (f64::from(disc.radius) + dressing).min(reach);
        // A tape cell may reach past the window's edge, so a fold there sits
        // where its prints traded rather than at the column's centre.
        let x = if lone || (live && weight <= 0.0) {
            own_x
        } else if live {
            weighted_x / weight
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
            self.bubbles.side_offset,
            false,
        ));
        // The grid owns the placement: the lean goes into the centre where
        // the cell has room for it, and the painter adds none of its own.
        let center = (y + lean).max(cell.top + radius).min(cell.bottom - radius);
        mark.y = unit(center / self.height);
        mark.x = if live {
            (self.regions - 1.0 + x / self.lane_width) / self.regions
        } else {
            x / self.px_per_bar / self.regions
        };
        let fit = reach
            .min(x - cell.left)
            .min(cell.right - x)
            .min(center - cell.top)
            .min(cell.bottom - center)
            .max(0.0);
        mark.cell_radius_px = Some(fit as f32);
        mark
    }
}
