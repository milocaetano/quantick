//! Volume dots, Bookmap style: on the candles every print lands in the dot
//! keyed by its bar and a price level; on the tape, by a window of market
//! time and a price level.
//!
//! The optional native tape uses short native-price windows as keys and
//! retains quantity-weighted execution coordinates. Its open window rides
//! NOW; its closed windows move with elapsed time. Its dots keep proportional
//! areas independently of cell size; overlapping neighbours are combined by
//! [`super::merge_tape_dots`]. The cell-centred rules below describe the
//! original mixed candle-and-tape view, which retains its existing behaviour.
//!
//! The key is market data and nothing else. A tape window is
//! `floor(exchange_ts / window_ms)`, anchored at exchange epoch 0 — never at
//! the screen, the seam, the tape's start or a cut — and cut at its bar's
//! close. A level is whole native ticks anchored at price zero, never the
//! adaptive display grouping. Every tier and every frame therefore computes
//! the same keys, and a dot whose market time has passed keeps its volume:
//! a closed dot does not move, grow or blink as the tape rolls, a bar forms
//! or closes, the chart pans or the seam moves.
//!
//! Both sides share one dot, with the exact quantity and bought quantity, so
//! the painter draws a pie when both are in it; the prints behind it are
//! counted as a cluster (`×n`), because a dot is a fact about the market, not
//! a fold of the canvas. Prints are matched to the reductions they explain one
//! by one first, as they are without dots, and only then folded, so a dot
//! carries its prints' evidence ([`fold_dots`]).
//!
//! What the zoom picks, chosen by the view with hysteresis
//! ([`DotRungMemory`]) and handed to the engine as a [`DotZoom`], so the
//! engine stays a pure function of its inputs and an autoscale wobbling at a
//! boundary does not flip a rung back and forth:
//! - the candles' level, from the candle axis at a candle dot's size
//!   ([`candle_dot_px`]). A candle dot is its whole bar at that level: it
//!   holds exactly what the bar traded there, so a busier price is always a
//!   bigger dot, as the footprint reads it;
//! - the tape's window, from the tape's own zoom, and the tape's level, from
//!   the price span of the tape's own prints — never from the candle axis, so
//!   moving the candles never regroups the tape.
//!
//! A dot is drawn at its level's centre, on the grid of its cells, so
//! neighbouring levels never crowd; it carries its quantity-weighted price
//! rounded to the native tick for the reader. The painter sizes it
//! ([`DotSizing`]): area proportional to quantity against its pane's full
//! size — the biggest dot of that pane on screen when automatic, the typed
//! `full_quantity` otherwise — and never over half its cell wide or tall, so
//! no dot hides another and squeezing an axis shrinks every dot alike.
//!
//! A print is the tape's while its tape window starts at or after the tape
//! does; the candles hold every print of their bars, so the tape's length
//! never changes the candles. A tape dot sits at its window's fixed centre, a
//! candle dot at its bar's slot centre — never at its newest print, so a
//! forming dot does not slide as prints arrive and a closed one does not move
//! on a pan.
//!
//! Retention evicts the oldest prints one by one, and recording starts at
//! some instant. A dot whose window — a tape window, or a candle dot's whole
//! bar — starts at or before the newest evicted print, or before recording
//! started, may have lost prints, so it is not drawn at all: a dot at the
//! retention edge disappears whole rather than shrinking. Every other dot —
//! the tape's windows after the horizon included — draws as usual.
//!
//! Known limitation: a print stamped with the same millisecond as a bar's
//! open belongs to the new bar, which is the timeline's rule for every mark.
//! A venue that stamps the closing print of one bar and the opening print of
//! the next with the same millisecond has the two split by that rule, not by
//! the bar builder's own count.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use quantick_engine::Bar;

use super::model::{AggressionPrimitive, normalized_area_size};
use crate::config::{
    BubbleStyle, CANDLE_DOT_RADIUS_SHARE, DisplayGrouping, HeatmapConfig, LiveLaneStyle,
    bubble_radius,
};
use crate::grouping::EffectiveGrouping;
use crate::history::LiquidityHistory;
use crate::interaction::{AggressionCluster, fold_by_key, sort_clusters};

use super::constants::{
    DOT_LEVEL_LADDER_TICKS, DOT_WINDOW_CELL_PX, DOT_WINDOW_LADDER_MS, HOLD_ABOVE, HOLD_BELOW,
    LANE_BAR_WINDOWS, MIN_DOT_RADIUS_PX,
};

/// How the chart is drawn, as the view measures it every frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneGeometry {
    /// Pixels between two neighbouring bars on the candles: a candle dot's
    /// column when it is drawn ([`DotSizing`]), never part of a key.
    pub px_per_bar: f32,
    /// Width of the live lane, in pixels. Zero when no lane is drawn.
    pub lane_width_px: f32,
    /// Market time the lane shows, in exchange milliseconds.
    pub lane_window_ms: i64,
    /// Height of the chart the price window is drawn over, in pixels.
    pub height_px: f32,
    /// `(open, close)` of the series' bars the tape may reach ([`lane_bars`]),
    /// whether or not the candles have them on screen.
    pub lane_bars: Vec<(i64, i64)>,
}

/// The bars [`PaneGeometry::lane_bars`] wants: every bar that ends inside the
/// last two `window_ms` of the series, and the one before it. Twice the window
/// because the view and the engine each resolve the tape's window; a bar the
/// engine's window reaches must be in this list, or its prints would be keyed
/// to the bar before it.
#[must_use]
pub fn lane_bars(closed: &[Bar], partial: Option<&Bar>, window_ms: i64) -> Vec<(i64, i64)> {
    let Some(newest) = partial.or(closed.last()) else {
        return Vec::new();
    };
    let start = newest
        .close_time
        .saturating_sub(window_ms.max(0).saturating_mul(LANE_BAR_WINDOWS));
    let first = closed.partition_point(|bar| bar.close_time < start);
    closed[first.saturating_sub(1)..]
        .iter()
        .chain(partial)
        .map(|bar| (bar.open_time, bar.close_time))
        .collect()
}

/// The smallest rung of `ladder` whose size on screen, at `px_per_unit`, is at
/// least `dot_px` — the widest rung when none is.
fn ideal_rung(ladder: &[i64], px_per_unit: f64, dot_px: f64) -> i64 {
    let widest = ladder[ladder.len() - 1];
    if !(px_per_unit.is_finite() && px_per_unit > 0.0) {
        return widest;
    }
    ladder
        .iter()
        .copied()
        .find(|rung| *rung as f64 * px_per_unit >= dot_px)
        .unwrap_or(widest)
}

/// The ideal rung of [`DOT_WINDOW_LADDER_MS`] at `ms_per_px`.
#[must_use]
pub fn dot_window_ms(ms_per_px: f64, dot_px: f64) -> i64 {
    ideal_rung(&DOT_WINDOW_LADDER_MS, 1.0 / ms_per_px, dot_px)
}

/// The ideal rung of [`DOT_LEVEL_LADDER_TICKS`] at `px_per_tick`.
#[must_use]
pub fn dot_level_ticks(px_per_tick: f64, dot_px: f64) -> i64 {
    ideal_rung(&DOT_LEVEL_LADDER_TICKS, px_per_tick, dot_px)
}

/// The rung of `ladder` to use now, given the one in use (`current`): it is
/// kept until the dot would be over 150 % of its cell, or under 70 % of the
/// next smaller cell, and only then replaced by the ideal rung. A cell is
/// `rung × px_per_unit` pixels.
#[must_use]
pub fn hold_rung(ladder: &[i64], current: Option<i64>, px_per_unit: f64, dot_px: f64) -> i64 {
    let ideal = ideal_rung(ladder, px_per_unit, dot_px);
    let Some(current) = current.filter(|rung| ladder.contains(rung)) else {
        return ideal;
    };
    let share = |rung: i64| dot_px / (rung as f64 * px_per_unit);
    let smaller = ladder.iter().copied().filter(|rung| *rung < current).max();
    let too_small = !share(current).is_finite() || share(current) > HOLD_ABOVE;
    let too_big = smaller.is_some_and(|rung| share(rung) < HOLD_BELOW);
    if too_small || too_big { ideal } else { current }
}

/// How wide a candle dot is at full size, in pixels: `2 × max radius ×`
/// [`CANDLE_DOT_RADIUS_SHARE`] — the size the candles' level is chosen for.
#[must_use]
pub fn candle_dot_px(bubbles: &BubbleStyle) -> f64 {
    2.0 * f64::from(bubbles.max_radius) * f64::from(CANDLE_DOT_RADIUS_SHARE)
}

/// The rungs the view chose, handed to the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct DotZoom {
    /// Use execution coordinates: the native tape, beside the candles or
    /// alone.
    pub native_tape: bool,
    /// The tape's window, in exchange milliseconds.
    pub tape_window_ms: i64,
    /// Native ticks per tape level, from the tape's own price span.
    pub tape_level_ticks: i64,
    /// Native ticks per candle level, from the candle axis.
    pub candle_level_ticks: i64,
    /// See [`PaneGeometry::lane_bars`].
    pub lane_bars: Vec<(i64, i64)>,
}

/// The rungs a view is using, so the next frame holds them through a wobble,
/// and the screen they were chosen on, so the painter fits dots to it.
#[derive(Debug, Clone, Default)]
pub struct DotRungMemory {
    tape_window_ms: Option<i64>,
    tape_level_ticks: Option<i64>,
    candle_level_ticks: Option<i64>,
    /// Tape pixels per millisecond at the last chosen zoom.
    tape_px_per_ms: Option<f64>,
    /// Candle pixels per bar at the last chosen zoom.
    px_per_bar: Option<f32>,
}

impl DotRungMemory {
    /// The rungs for this frame, each held with [`hold_rung`]: the tape's
    /// window at its width over its window; the tape's level at the chart's
    /// height over `tape_span`, the price range the tape's own prints cover
    /// (the axis' `price_range` only when there is none yet, and never under
    /// one tick); the candles' level at the chart's height over
    /// `price_range`, for a candle dot's size ([`candle_dot_px`]).
    pub fn choose(
        &mut self,
        geometry: PaneGeometry,
        config: &HeatmapConfig,
        price_range: (f64, f64),
        tape_span: Option<f64>,
    ) -> DotZoom {
        let dot_px = 2.0 * f64::from(config.bubbles.max_radius);
        let tick = native_grouping(config).bucket_width.to_f64().unwrap_or(0.0);
        let height = f64::from(geometry.height_px);
        let axis_span = price_range.1 - price_range.0;
        let tape_span = tape_span
            .filter(|span| span.is_finite())
            .unwrap_or(axis_span)
            .max(tick);
        let px_per_ms = f64::from(geometry.lane_width_px) / geometry.lane_window_ms as f64;
        // At least half a full dot wide, so a dot fitted to its column
        // ([`DotSizing`]) stays readable.
        let tape_column_px = f64::from(config.bubbles.max_radius).max(DOT_WINDOW_CELL_PX);
        // The tape the pane builds, never the switch alone: with the tape
        // off or without volume dots the candles key as the tick chart does.
        let native_tape = config.native_tape();
        let tape_window_ms = if native_tape {
            DOT_WINDOW_LADDER_MS[0]
        } else {
            hold_rung(
                &DOT_WINDOW_LADDER_MS,
                self.tape_window_ms,
                px_per_ms,
                tape_column_px,
            )
        };
        let tape_level_ticks = if native_tape {
            1
        } else {
            hold_rung(
                &DOT_LEVEL_LADDER_TICKS,
                self.tape_level_ticks,
                height * tick / tape_span,
                dot_px,
            )
        };
        let candle_level_ticks = hold_rung(
            &DOT_LEVEL_LADDER_TICKS,
            self.candle_level_ticks,
            height * tick / axis_span,
            candle_dot_px(&config.bubbles),
        );
        self.tape_window_ms = Some(tape_window_ms);
        self.tape_level_ticks = Some(tape_level_ticks);
        self.candle_level_ticks = Some(candle_level_ticks);
        self.tape_px_per_ms = Some(px_per_ms);
        self.px_per_bar = Some(geometry.px_per_bar);
        DotZoom {
            native_tape,
            tape_window_ms,
            tape_level_ticks,
            candle_level_ticks,
            lane_bars: geometry.lane_bars,
        }
    }

    /// How the painter sizes the dots of a frame built on `scale`, drawn
    /// `chart_height_px` tall on the screen of the last chosen zoom. The
    /// cells are the frame's own — its tape window, its price window — so a
    /// dot fits the cell it was keyed on. `None` before a zoom is chosen.
    #[must_use]
    pub fn sizing(&self, scale: &DotScale, chart_height_px: f32) -> Option<DotSizing> {
        let tape_px_per_ms = self.tape_px_per_ms?;
        let span = scale.price_range.1 - scale.price_range.0;
        Some(DotSizing {
            native_tape: scale.native_tape,
            tape_column_px: (scale.tape_window_ms as f64 * tape_px_per_ms) as f32,
            candle_column_px: self.px_per_bar?,
            px_per_price: f64::from(chart_height_px) / span,
            typed_full: (!scale.auto_full).then_some(scale.volume_dot_full_quantity),
        })
    }
}

/// How the painter sizes volume dots on the current screen: each dot's cell,
/// and the quantity a full-size dot holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotSizing {
    /// The frame's tape is the native tape ([`DotScale::native_tape`]): its
    /// dots are sized and merged on execution coordinates, never fitted to
    /// a cell.
    pub native_tape: bool,
    /// Screen width of one tape window, in pixels.
    pub tape_column_px: f32,
    /// Screen width of one bar on the candles, in pixels.
    pub candle_column_px: f32,
    /// Screen pixels one unit of price is tall.
    pub px_per_price: f64,
    /// The trader's typed full size; `None` sizes each pane against its own
    /// biggest dot on screen.
    pub typed_full: Option<Decimal>,
}

impl DotSizing {
    /// The `(width, height)` in pixels of the cell `mark` was keyed on: its
    /// tape window or its bar, by its level's height.
    #[must_use]
    pub fn cell(&self, mark: &AggressionPrimitive) -> (f32, f32) {
        let width = if mark.live {
            self.tape_column_px
        } else {
            self.candle_column_px
        };
        let height = mark.price_span.to_f64().unwrap_or(0.0) * self.px_per_price;
        (width, height as f32)
    }

    /// The quantity a full-size dot on `live`'s pane holds: the typed full
    /// size, or else the biggest of `marks` on that pane, so the biggest dot
    /// on screen is always full size. Never zero.
    pub fn full_quantity<'a>(
        &self,
        marks: impl IntoIterator<Item = &'a AggressionPrimitive>,
        live: bool,
    ) -> Decimal {
        let full = self.typed_full.unwrap_or_else(|| {
            marks
                .into_iter()
                .filter(|mark| mark.live == live)
                .map(|mark| mark.quantity)
                .max()
                .unwrap_or_default()
        });
        if full > Decimal::ZERO {
            full
        } else {
            Decimal::ONE
        }
    }

    /// Each pane's full size over `marks`, the dots on screen:
    /// `(tape, candles)` ([`Self::full_quantity`]).
    #[must_use]
    pub fn pane_fulls(&self, marks: &[&AggressionPrimitive]) -> (Decimal, Decimal) {
        let pane = |live| self.full_quantity(marks.iter().copied(), live);
        (pane(true), pane(false))
    }

    /// The `(normalized size, radius)` `mark` is drawn with against its
    /// pane's full size in `fulls` ([`Self::pane_fulls`]).
    #[must_use]
    pub fn draw(
        &self,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        mark: &AggressionPrimitive,
        fulls: (Decimal, Decimal),
    ) -> (f32, f32) {
        let full = if mark.live { fulls.0 } else { fulls.1 };
        let radius = self.radius(bubbles, lane, mark, full);
        (normalized_area_size(mark.quantity, full), radius)
    }

    /// The radius `mark` is drawn with against `full` ([`Self::full_quantity`]):
    /// area proportional to quantity inside [`dot_radius_range`] of its cell.
    #[must_use]
    pub fn radius(
        &self,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        mark: &AggressionPrimitive,
        full: Decimal,
    ) -> f32 {
        if mark.live && self.native_tape {
            return native_tape_radius(bubbles, mark.quantity, full);
        }
        let (minimum, maximum) = dot_radius_range(bubbles, lane, mark.live, self.cell(mark));
        bubble_radius(normalized_area_size(mark.quantity, full), minimum, maximum)
    }
}

/// The radius of a native tape dot holding `quantity` against `full`
/// ([`DotSizing::radius`] of a live mark): area proportional to quantity,
/// whatever its cell.
pub(super) fn native_tape_radius(bubbles: &BubbleStyle, quantity: Decimal, full: Decimal) -> f32 {
    bubbles.max_radius * normalized_area_size(quantity, full)
}

/// The `(smallest, largest)` radius a dot on `live`'s pane is drawn with in a
/// cell `(width, height)` pixels: the largest is the pane's own — the
/// style's on the tape, [`CANDLE_DOT_RADIUS_SHARE`] of it on the candles —
/// but never over half the cell's width or height, so neighbouring dots never
/// overlap. The smallest shrinks by the same share, keeping every dot in
/// proportion, never under [`MIN_DOT_RADIUS_PX`] nor over the largest.
#[must_use]
pub fn dot_radius_range(
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    live: bool,
    cell: (f32, f32),
) -> (f32, f32) {
    let (pane_min, pane_max) = lane.pane_radii(bubbles, live, true);
    let half = |side: f32| {
        if side.is_finite() {
            side / 2.0
        } else {
            f32::MAX
        }
    };
    let max = pane_max.min(half(cell.0)).min(half(cell.1)).max(0.0);
    let share = if pane_max > 0.0 { max / pane_max } else { 1.0 };
    let min = (pane_min * share).max(MIN_DOT_RADIUS_PX).min(max);
    (min, max)
}

/// The `(low, high)` of the prices the tape's marks traded at — their
/// quantity-weighted prices — or `None` when the tape draws none: the span
/// the tape's level is chosen from ([`DotRungMemory::choose`]) and what the
/// chart's price fit keeps on the axis.
#[must_use]
pub fn tape_price_range(marks: &[AggressionPrimitive]) -> Option<(f64, f64)> {
    let mut prices = marks.iter().filter(|mark| mark.live).map(|mark| mark.price);
    let first = prices.next()?;
    let (low, high) = prices.fold((first, first), |(low, high), price| {
        (low.min(price), high.max(price))
    });
    Some((low.to_f64()?, high.to_f64()?))
}

/// The rungs and scale a dots frame was built on, for the painter, the health
/// report and the `orderflow.bubbles` snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct DotScale {
    /// The frame was built as the native tape ([`VolumeDots::native_tape`]).
    pub native_tape: bool,
    /// The tape's window, in milliseconds.
    pub tape_window_ms: i64,
    /// Native ticks per tape level.
    pub tape_level_ticks: i64,
    /// Native ticks per candle level.
    pub candle_level_ticks: i64,
    /// The typed quantity of a full-size dot: `volume_dot_full_quantity`.
    pub volume_dot_full_quantity: Decimal,
    /// Each pane is sized against its own biggest dot on screen instead.
    pub auto_full: bool,
    /// The `(low, high)` price window the frame was placed on.
    pub price_range: (f64, f64),
}

/// What the history holds whole: nothing before recording started, nothing
/// at or before the newest evicted print. A dot is drawn only when its window
/// starts inside that, so it is drawn whole or not at all.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DotHorizon {
    pub(super) recorded_from_ms: Option<i64>,
    pub(super) evicted_through_ms: Option<i64>,
}

impl DotHorizon {
    pub(super) fn of(history: &LiquidityHistory) -> Self {
        Self {
            recorded_from_ms: history.recorded_from_ms(),
            evicted_through_ms: history.evicted_through_ms(),
        }
    }

    /// Whether a window starting at `start_ms` can have lost no print.
    fn keeps(self, start_ms: i64) -> bool {
        self.recorded_from_ms.is_none_or(|from| start_ms >= from)
            && self
                .evicted_through_ms
                .is_none_or(|horizon| start_ms > horizon)
    }
}

/// One window a print is keyed into: a tape window, or a candle dot's bar.
#[derive(Debug, Clone, Copy)]
struct Window {
    /// The tape window's start, or the candle dot's bar open: one cell of
    /// its pane in time. A tape window is one cell whatever bars close
    /// inside it, so no two tape dots share a cell.
    key: i64,
    /// Where the window's market time starts.
    start_ms: i64,
    /// Where the dot is stamped: a tape window's fixed centre, a candle
    /// dot's bar open.
    stamp_ms: i64,
}

/// What one frame keys its dots on.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeDots {
    /// The native tape retains execution coordinates and its open window
    /// follows NOW.
    pub native_tape: bool,
    /// The tape's window, in exchange milliseconds.
    pub tape_window_ms: i64,
    /// Native ticks per tape level.
    pub tape_level_ticks: i64,
    /// Native ticks per candle level.
    pub candle_level_ticks: i64,
    /// `(open, close)` of every bar the frame may key, ascending by open.
    pub bars: Vec<(i64, i64)>,
    /// Open of the bar still forming, whose close is only its latest print.
    pub forming: Option<i64>,
}

impl VolumeDots {
    /// The dots `zoom` asks for over the frame's bars and the ones the tape
    /// reaches.
    #[must_use]
    pub fn resolve(zoom: &DotZoom, closed: &[Bar], partial: Option<&Bar>) -> Self {
        let mut bars: Vec<(i64, i64)> = closed
            .iter()
            .chain(partial)
            .map(|bar| (bar.open_time, bar.close_time))
            .chain(zoom.lane_bars.iter().copied())
            .collect();
        // One entry per open, the latest close first: two snapshots of a
        // forming bar may disagree on how far it has run.
        bars.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        bars.dedup_by_key(|bar| bar.0);
        Self {
            native_tape: zoom.native_tape,
            tape_window_ms: zoom.tape_window_ms,
            tape_level_ticks: zoom.tape_level_ticks,
            candle_level_ticks: zoom.candle_level_ticks,
            bars,
            forming: partial.map(|bar| bar.open_time),
        }
    }

    /// The bar `timestamp_ms` is in, as `(open, close)`.
    fn bar_around(&self, timestamp_ms: i64) -> Option<(i64, i64)> {
        let partition = self.bars.partition_point(|bar| bar.0 <= timestamp_ms);
        self.bars.get(partition.checked_sub(1)?).copied()
    }

    /// The window `timestamp_ms` falls in on one pane: its tape window on the
    /// tape, its whole bar on the candles. Only the independent tape can keep
    /// a print whose bar is outside the supplied candle slice.
    fn window(&self, timestamp_ms: i64, live: bool) -> Option<Window> {
        Some(if live {
            if !self.native_tape {
                self.bar_around(timestamp_ms)?;
            }
            let width = self.tape_window_ms.max(1);
            let start = window_start(timestamp_ms, width);
            Window {
                key: start,
                start_ms: start,
                stamp_ms: start + width / 2,
            }
        } else {
            let (open, _) = self.bar_around(timestamp_ms)?;
            Window {
                key: open,
                start_ms: open,
                stamp_ms: open,
            }
        })
    }

    /// Native ticks per level on one pane.
    fn level_ticks(&self, live: bool) -> i64 {
        if live {
            self.tape_level_ticks
        } else {
            self.candle_level_ticks
        }
    }

    /// The rungs and scale, for the painter and the health report: the typed
    /// full size and whether each pane sizes itself, from `config`, and the
    /// price window the frame was placed on.
    #[must_use]
    pub fn scale(&self, config: &HeatmapConfig, price_range: (f64, f64)) -> DotScale {
        DotScale {
            native_tape: self.native_tape,
            tape_window_ms: self.tape_window_ms,
            tape_level_ticks: self.tape_level_ticks,
            candle_level_ticks: self.candle_level_ticks,
            volume_dot_full_quantity: super::dot_full_quantity(config),
            auto_full: config.volume_dots.auto_full,
            price_range,
        }
    }
}

/// Where the window of `window_ms` holding `timestamp_ms` starts, counted
/// from exchange epoch 0.
#[must_use]
pub(super) fn window_start(timestamp_ms: i64, window_ms: i64) -> i64 {
    let window_ms = window_ms.max(1);
    timestamp_ms.div_euclid(window_ms) * window_ms
}

/// The instrument's native price grouping: the tick a level is counted in.
pub(crate) fn native_grouping(config: &HeatmapConfig) -> EffectiveGrouping {
    EffectiveGrouping::resolve(
        DisplayGrouping::Native,
        config.price_grouping,
        Decimal::ZERO,
    )
}

/// Fold one pane's clusters — one per print, already matched to the
/// reductions they explain — into dots by window (a tape window or a candle
/// dot's bar) and level, summing
/// quantity, bought quantity and matched quantity and uniting the event ids.
/// Candle dots and ordinary lane dots need a known bar; independent tape dots
/// need only their market-time window. A window whose `horizon` may have lost
/// prints to eviction is excluded.
pub(super) fn fold_dots(
    clusters: Vec<AggressionCluster>,
    live: bool,
    dots: &VolumeDots,
    native: EffectiveGrouping,
    horizon: DotHorizon,
) -> Vec<AggressionCluster> {
    let tick = native.bucket_width;
    let width = tick * Decimal::from(dots.level_ticks(live).max(1));
    let level_of = |price: Decimal| (price / width).floor() * width;
    let keyed: Vec<AggressionCluster> = clusters
        .into_iter()
        .filter(|cluster| {
            dots.window(cluster.timestamp_ms, live)
                .is_some_and(|window| {
                    if live && dots.native_tape {
                        // Capture begins with a visible print, even inside an
                        // open window. A partially evicted window stays out.
                        horizon
                            .evicted_through_ms
                            .is_none_or(|evicted| window.start_ms > evicted)
                    } else {
                        horizon.keeps(window.start_ms)
                    }
                })
        })
        .collect();
    let key_of = |cluster: &AggressionCluster| {
        let generation = if live && dots.native_tape {
            None
        } else {
            cluster.generation
        };
        dots.window(cluster.timestamp_ms, live)
            .map(|window| (window.key, generation, level_of(cluster.price)))
    };
    let mut folded: Vec<AggressionCluster> = fold_by_key(keyed, key_of)
        .into_iter()
        .filter_map(|mut dot| {
            let window = dots.window(dot.first_timestamp_ms, live)?;
            let level = level_of(dot.price);
            let top_tick = (level + width - tick).max(level);
            if !live || !dots.native_tape {
                dot.price = ((dot.price / tick).round() * tick).clamp(level, top_tick);
            }
            dot.price_bucket = level;
            dot.price_span = width;
            dot.timestamp_ms = if live && dots.native_tape && dot.quantity > Decimal::ZERO {
                (dot.timestamp_quantity / dot.quantity).round().to_i64()?
            } else {
                window.stamp_ms
            };
            Some(dot)
        })
        .collect();
    sort_clusters(&mut folded);
    folded
}
