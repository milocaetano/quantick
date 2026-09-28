//! Volume dots, Bookmap style: every print lands in the dot keyed by its
//! bar, a window of market time and a price level.
//!
//! The key is market data and nothing else. A window is
//! `floor(exchange_ts / window_ms)`, anchored at exchange epoch 0 — never at
//! the screen, the seam, the tape's start or a cut — and cut at its bar's
//! close. A level is whole native ticks anchored at price zero, never the
//! adaptive display grouping. Every tier and every frame therefore computes
//! the same keys, and a window whose market time has passed keeps its dot:
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
//! What the zoom picks:
//! - the candles' window, per bar: the smallest rung of
//!   [`DOT_WINDOW_LADDER_MS`] at least [`DOT_WINDOW_CELL_PX`] wide inside that bar's slot,
//!   from the bar's own duration taken to the next doubling, or the whole bar
//!   when the slot is no wider than that or the bar outgrows the top rung
//!   ([`dot_bar_window_ms`]). A closed bar's duration never changes, so its
//!   rung does not move when it closes or when another bar does; a forming
//!   bar's rung only ever coarsens, when its duration doubles;
//! - the tape's window and the level, chosen by the view with hysteresis
//!   ([`DotRungMemory`]) and handed to the engine as a [`DotZoom`], so the
//!   engine stays a pure function of its inputs and an autoscale wobbling at
//!   a boundary does not flip a rung back and forth.
//!
//! A dot sits at its quantity-weighted price rounded to the native tick,
//! inside its level, and is sized on one absolute scale for the whole
//! session, on both panes: area proportional to quantity, full size at the
//! dots' own `volume_dot_full_quantity` — a dot sums many prints, so the
//! prints' `size_reference_quantity` would saturate it — whatever the window,
//! the level or the bar. A bigger volume is always a bigger dot, so levels merged by a zoom
//! draw a dot as big as the sum they hold.
//!
//! A print is the tape's while its tape window starts at or after the tape
//! does, and otherwise its candle window's, so the tape never draws a print
//! older than itself at its left edge. The price of that rule is at the seam:
//! a candle window the tape is still releasing keeps growing as the tape
//! lets go of its prints, the way a forming bar does. A candle window wholly
//! older than the tape never changes again.
//!
//! A candle window is counted from its bar's open, so a bar shorter than its
//! window is one window, whatever the epoch grid does; a tape window from
//! epoch 0. A tape dot sits at its window's fixed centre; a candle dot at the
//! centre of the part of its window inside its bar — its slot's centre when
//! that part is the whole bar — where a forming bar ends at its slot's end.
//! Never at its newest print, so a forming dot does not slide as prints
//! arrive and a closed one does not move on a pan.
//!
//! Retention evicts the oldest prints one by one, and recording starts at
//! some instant. A window that starts at or before the newest evicted print,
//! or before recording started, may have lost prints, so it draws no dot at
//! all: a dot at the retention edge disappears whole rather than shrinking,
//! and the window recording started in is not drawn. Every other window —
//! the tape's, and a cut bar's windows after the horizon — draws as usual.
//!
//! Known limitation: a print stamped with the same millisecond as a bar's
//! open belongs to the new bar, which is the timeline's rule for every mark.
//! A venue that stamps the closing print of one bar and the opening print of
//! the next with the same millisecond has the two split by that rule, not by
//! the bar builder's own count.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use quantick_engine::Bar;

use crate::config::{DisplayGrouping, HeatmapConfig};
use crate::grouping::EffectiveGrouping;
use crate::history::LiquidityHistory;
use crate::interaction::{AggressionCluster, fold_by_key, sort_clusters};

/// The windows of market time a dot may cover, in exchange milliseconds,
/// narrowest first. Above the top rung a dot covers its whole bar.
pub const DOT_WINDOW_LADDER_MS: [i64; 10] = [
    100, 250, 500, 1_000, 2_000, 5_000, 10_000, 30_000, 60_000, 300_000,
];

/// The heights, in native ticks, a dot's price level may span, narrowest
/// first.
pub const DOT_LEVEL_LADDER_TICKS: [i64; 12] =
    [1, 2, 5, 10, 20, 50, 100, 200, 500, 1_000, 2_000, 5_000];

/// The screen width, in pixels, a dot's window of market time is at least
/// on either pane: a thin column, not the biggest dot, so a dot sits near the
/// moment its prints traded and only squeezing the time axis widens it.
pub const DOT_WINDOW_CELL_PX: f64 = 8.0;

/// A held rung moves down once the dot would be under this share of the next
/// smaller cell.
const HOLD_BELOW: f64 = 0.7;

/// A held rung moves up once the dot would be over this share of its cell.
const HOLD_ABOVE: f64 = 1.5;

/// How the chart is drawn, as the view measures it every frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneGeometry {
    /// Pixels between two neighbouring bars on the candles.
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
        .saturating_sub(window_ms.max(0).saturating_mul(2));
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

/// A duration taken to the next power of two, at least one millisecond.
fn doubled(duration_ms: i64) -> i64 {
    let duration = u64::try_from(duration_ms.max(1)).unwrap_or(1);
    i64::try_from(duration.next_power_of_two()).unwrap_or(i64::MAX)
}

/// The window, in milliseconds, a bar `bar_ms` long keys its candle dots on
/// at `px_per_bar`, for dots `dot_px` across: the smallest rung at least one
/// dot wide inside the bar's slot. `None` is the whole bar: a slot no wider
/// than a dot, or a bar that needs more than the top rung. The duration is
/// taken to the next doubling first, so as a forming bar runs on its window
/// only ever coarsens — never back to a finer rung, never out of the whole
/// bar once in it.
#[must_use]
pub fn dot_bar_window_ms(bar_ms: i64, px_per_bar: f64, dot_px: f64) -> Option<i64> {
    if !(px_per_bar.is_finite() && px_per_bar > dot_px) {
        return None;
    }
    let need_ms = dot_px * doubled(bar_ms) as f64 / px_per_bar;
    DOT_WINDOW_LADDER_MS
        .into_iter()
        .find(|rung| *rung as f64 >= need_ms)
}

/// The rungs the view chose, handed to the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct DotZoom {
    /// Pixels between two neighbouring bars on the candles.
    pub px_per_bar: f32,
    /// The tape's window, in exchange milliseconds.
    pub tape_window_ms: i64,
    /// Native ticks per price level, on both panes: they share the price axis.
    pub level_ticks: i64,
    /// See [`PaneGeometry::lane_bars`].
    pub lane_bars: Vec<(i64, i64)>,
}

/// The rungs a view is using, so the next frame holds them through a wobble.
#[derive(Debug, Clone, Default)]
pub struct DotRungMemory {
    tape_window_ms: Option<i64>,
    level_ticks: Option<i64>,
}

impl DotRungMemory {
    /// The rungs for this frame: the tape's window at its width over its
    /// window, the level at the chart's height over the native ticks
    /// `price_range` spans, each held with [`hold_rung`].
    pub fn choose(
        &mut self,
        geometry: PaneGeometry,
        config: &HeatmapConfig,
        price_range: (f64, f64),
    ) -> DotZoom {
        let dot_px = 2.0 * f64::from(config.bubbles.max_radius);
        let tick = native_grouping(config).bucket_width.to_f64().unwrap_or(0.0);
        let px_per_tick = f64::from(geometry.height_px) * tick / (price_range.1 - price_range.0);
        let px_per_ms = f64::from(geometry.lane_width_px) / geometry.lane_window_ms as f64;
        let tape_window_ms = hold_rung(
            &DOT_WINDOW_LADDER_MS,
            self.tape_window_ms,
            px_per_ms,
            DOT_WINDOW_CELL_PX,
        );
        let level_ticks = hold_rung(
            &DOT_LEVEL_LADDER_TICKS,
            self.level_ticks,
            px_per_tick,
            dot_px,
        );
        self.tape_window_ms = Some(tape_window_ms);
        self.level_ticks = Some(level_ticks);
        DotZoom {
            px_per_bar: geometry.px_per_bar,
            tape_window_ms,
            level_ticks,
            lane_bars: geometry.lane_bars,
        }
    }
}

/// The rungs and scales a dots frame was built on, for the health report
/// and the `orderflow.bubbles` snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct DotScale {
    /// Pixels a bar takes on the candles, which each bar's window follows.
    pub px_per_bar: f32,
    /// The newest bar's candle window, in milliseconds; `None` is the whole
    /// bar.
    pub newest_bar_window_ms: Option<i64>,
    /// The tape's window, in milliseconds.
    pub tape_window_ms: i64,
    /// Native ticks per price level.
    pub level_ticks: i64,
    /// Quantity of a full-size dot, on both panes: the dots' own
    /// `volume_dot_full_quantity`.
    pub volume_dot_full_quantity: Decimal,
    /// A full size read from the market, while the style still wants one
    /// ([`calibrated_dot_full_quantity`]).
    pub calibrated_full_quantity: Option<Decimal>,
}

/// One window a print is keyed into.
#[derive(Debug, Clone, Copy)]
struct Window {
    /// Distinguishes the windows of one bar.
    key: i64,
    bar_open: i64,
    /// Where the window's market time starts inside its bar.
    start_ms: i64,
    /// The window's fixed centre, where a tape dot sits; a candle dot is
    /// placed by [`VolumeDots::candle_place`].
    centre_ms: i64,
}

/// What the history holds whole: nothing before recording started, nothing
/// at or before the newest evicted print. A dot is drawn only when its window
/// starts inside that, so it is drawn whole or not at all.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DotHorizon {
    recorded_from_ms: Option<i64>,
    evicted_through_ms: Option<i64>,
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

/// What one frame keys its dots on.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeDots {
    /// Pixels a bar takes on the candles.
    pub px_per_bar: f32,
    /// The least screen width of a candle window, in pixels
    /// ([`DOT_WINDOW_CELL_PX`] in a real frame).
    pub dot_px: f64,
    /// The tape's window, in exchange milliseconds.
    pub tape_window_ms: i64,
    /// Native ticks per price level.
    pub level_ticks: i64,
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
            px_per_bar: zoom.px_per_bar,
            dot_px: DOT_WINDOW_CELL_PX,
            tape_window_ms: zoom.tape_window_ms,
            level_ticks: zoom.level_ticks,
            bars,
            forming: partial.map(|bar| bar.open_time),
        }
    }

    /// The window each of `bars` keys its candle dots on: what the finished
    /// half of a frame depends on.
    #[must_use]
    pub fn bar_windows(&self, bars: &[Bar]) -> Vec<Option<i64>> {
        bars.iter()
            .map(|bar| self.bar_window(bar.close_time - bar.open_time))
            .collect()
    }

    fn bar_window(&self, bar_ms: i64) -> Option<i64> {
        dot_bar_window_ms(bar_ms, f64::from(self.px_per_bar), self.dot_px)
    }

    /// The bar `timestamp_ms` is in, as `(open, close)`.
    fn bar_around(&self, timestamp_ms: i64) -> Option<(i64, i64)> {
        let partition = self.bars.partition_point(|bar| bar.0 <= timestamp_ms);
        self.bars.get(partition.checked_sub(1)?).copied()
    }

    /// The window `timestamp_ms` falls in on one pane.
    fn window(&self, timestamp_ms: i64, live: bool) -> Option<Window> {
        let (open, close) = self.bar_around(timestamp_ms)?;
        let width = if live {
            Some(self.tape_window_ms.max(1))
        } else {
            self.bar_window(close - open)
        };
        Some(match width {
            Some(width) => {
                // The tape's grid is the epoch's; a candle's is its bar's own,
                // so a bar shorter than its window is one window.
                let origin = if live { 0 } else { open };
                let start = origin + (timestamp_ms - origin).div_euclid(width) * width;
                Window {
                    key: start,
                    bar_open: open,
                    start_ms: start.max(open),
                    centre_ms: start + width / 2,
                }
            }
            None => Window {
                key: i64::MIN,
                bar_open: open,
                start_ms: open,
                centre_ms: open,
            },
        })
    }

    /// Where the candle dot whose first print is at `timestamp_ms` sits: the
    /// centre of the part of its window inside its bar, and whether that part
    /// is the whole bar. The bar ends at its close once closed, and at
    /// `slot_end_ms` while forming — never at its latest print.
    pub(super) fn candle_place(&self, timestamp_ms: i64, slot_end_ms: i64) -> Option<(i64, bool)> {
        let (open, close) = self.bar_around(timestamp_ms)?;
        let bar_end = if self.forming == Some(open) {
            slot_end_ms
        } else {
            close.saturating_add(1)
        };
        let Some(width) = self.bar_window(close - open) else {
            return Some((open, true));
        };
        let start = open + (timestamp_ms - open).div_euclid(width) * width;
        let end = start.saturating_add(width).min(bar_end).max(start + 1);
        Some((start + (end - start) / 2, start == open && end == bar_end))
    }

    /// The rungs and scales, for the health report.
    #[must_use]
    pub fn scale(&self, full: Decimal, calibrated: Option<Decimal>) -> DotScale {
        DotScale {
            px_per_bar: self.px_per_bar,
            newest_bar_window_ms: self
                .bars
                .last()
                .and_then(|(open, close)| self.bar_window(close - open)),
            tape_window_ms: self.tape_window_ms,
            level_ticks: self.level_ticks,
            volume_dot_full_quantity: full,
            calibrated_full_quantity: calibrated,
        }
    }
}

/// Drawn cells a calibration needs before it answers.
const CALIBRATION_MIN_CELLS: usize = 300;

/// The full size read from the market: the 99th percentile (nearest rank)
/// of what one cell — `window_ms` of market time, `level_ticks` native ticks
/// tall, the rungs being drawn — traded, both sides together, over the
/// retained prints, so only the top 1 % of those dots draws full size.
/// `None` until [`CALIBRATION_MIN_CELLS`] such cells exist. Keyed by market
/// data alone, so the prints' order never changes it.
#[must_use]
pub fn calibrated_dot_full_quantity(
    history: &LiquidityHistory,
    config: &HeatmapConfig,
    window_ms: i64,
    level_ticks: i64,
) -> Option<Decimal> {
    let level = native_grouping(config).bucket_width * Decimal::from(level_ticks);
    if level <= Decimal::ZERO || window_ms <= 0 {
        return None;
    }
    let mut cells: std::collections::BTreeMap<(i64, Decimal), Decimal> =
        std::collections::BTreeMap::new();
    for print in history.aggressions() {
        let key = (
            print.timestamp_ms.div_euclid(window_ms),
            (print.price / level).floor(),
        );
        *cells.entry(key).or_default() += print.quantity;
    }
    if cells.len() < CALIBRATION_MIN_CELLS {
        return None;
    }
    let mut sums: Vec<Decimal> = cells.into_values().collect();
    sums.sort_unstable();
    let rank = (sums.len() * 99).div_ceil(100);
    sums.get(rank.checked_sub(1)?).copied()
}

/// Where the window of `window_ms` holding `timestamp_ms` starts, counted
/// from exchange epoch 0.
#[must_use]
pub(super) fn window_start(timestamp_ms: i64, window_ms: i64) -> i64 {
    let window_ms = window_ms.max(1);
    timestamp_ms.div_euclid(window_ms) * window_ms
}

/// The instrument's native price grouping: the tick a level is counted in.
pub(super) fn native_grouping(config: &HeatmapConfig) -> EffectiveGrouping {
    EffectiveGrouping::resolve(
        DisplayGrouping::Native,
        config.price_grouping,
        Decimal::ZERO,
    )
}

/// Fold one pane's clusters — one per print, already matched to the
/// reductions they explain — into dots by bar, window and level, summing
/// quantity, bought quantity and matched quantity and uniting the event ids.
/// A cluster no known bar holds is left out, and so is every cluster whose
/// window `horizon` says may have lost prints — to eviction or to recording
/// starting inside it: a dot is drawn whole or not at all.
pub(super) fn fold_dots(
    clusters: Vec<AggressionCluster>,
    live: bool,
    dots: &VolumeDots,
    native: EffectiveGrouping,
    horizon: DotHorizon,
) -> Vec<AggressionCluster> {
    let tick = native.bucket_width;
    let width = tick * Decimal::from(dots.level_ticks.max(1));
    let level_of = |price: Decimal| (price / width).floor() * width;
    let keyed: Vec<AggressionCluster> = clusters
        .into_iter()
        .filter(|cluster| {
            dots.window(cluster.timestamp_ms, live)
                .is_some_and(|window| horizon.keeps(window.start_ms))
        })
        .collect();
    let key_of = |cluster: &AggressionCluster| {
        dots.window(cluster.timestamp_ms, live).map(|window| {
            (
                window.bar_open,
                window.key,
                cluster.generation,
                level_of(cluster.price),
            )
        })
    };
    let mut folded: Vec<AggressionCluster> = fold_by_key(keyed, key_of)
        .into_iter()
        .filter_map(|mut dot| {
            let window = dots.window(dot.first_timestamp_ms, live)?;
            let level = level_of(dot.price);
            let top_tick = (level + width - tick).max(level);
            dot.price = ((dot.price / tick).round() * tick).clamp(level, top_tick);
            dot.price_bucket = level;
            dot.price_span = width;
            dot.timestamp_ms = window.centre_ms;
            Some(dot)
        })
        .collect();
    sort_clusters(&mut folded);
    folded
}
