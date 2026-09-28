//! Volume dots, Bookmap style: every print lands in the dot keyed by its
//! bar, a window of market time and its native price level.
//!
//! The key is market data and nothing else. The window is
//! `floor(exchange_ts / window_ms)`, anchored at exchange epoch 0 — never at
//! the screen, the seam, the tape's start or a cut — and the level is the
//! instrument's native price grouping, never the adaptive display grouping,
//! so an axis refit cannot re-bucket a dot. Every tier and every frame
//! therefore computes the same keys, and a window whose market time has
//! passed keeps its dot: a closed dot does not move, grow or blink as the
//! tape rolls, a bar forms, the chart pans or the seam moves.
//!
//! Both sides share one dot, with the exact quantity and bought quantity, so
//! the painter draws a pie when both are in it; the prints behind it are
//! counted as a cluster (`×n`), because a dot is a fact about the market, not
//! a fold of the canvas.
//!
//! The window is the one thing the zoom picks: the smallest rung of
//! [`DOT_WINDOW_LADDER_MS`] at least one full dot wide on screen, per pane.
//! The chart hands its scale over as a [`PaneGeometry`]; nothing else — not
//! time passing, a pan, a refit or a forming bar — changes the rung.

use std::collections::BTreeMap;

use quantick_engine::Bar;
use rust_decimal::Decimal;

use crate::config::{BubbleStyle, DisplayGrouping, HeatmapConfig};
use crate::grouping::{EffectiveGrouping, bucket_for_price};
use crate::history::{Aggression, AggressorSide, CoverageSegment};
use crate::interaction::{AggressionCluster, consumed_side, generation_at, sort_clusters};
use crate::timeline::BarTimeline;

/// The windows of market time a dot may cover, in exchange milliseconds,
/// narrowest first.
pub const DOT_WINDOW_LADDER_MS: [i64; 7] = [100, 250, 500, 1_000, 2_000, 5_000, 10_000];

/// How the chart is zoomed: the pixels a bar and the tape take on screen, and
/// the series' bar opens the tape's window may reach.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneGeometry {
    /// Pixels between two neighbouring bars on the candles.
    pub px_per_bar: f32,
    /// Width of the live lane, in pixels. Zero when no lane is drawn.
    pub lane_width_px: f32,
    /// Open times, ascending, of the series' bars the tape's window may
    /// reach, whether or not the candles have them on screen: a tape dot is
    /// keyed by its true bar even while the candles are panned into history.
    pub lane_bar_opens: Vec<i64>,
}

/// The open times [`PaneGeometry::lane_bar_opens`] wants: every bar that
/// ends inside the last two `window_ms` of the series, and the one before
/// it. Twice the window because the view and the engine each resolve the
/// tape's window; a bar the engine's window reaches must be in this list, or
/// its prints would be keyed to the bar before it.
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

/// The smallest rung of [`DOT_WINDOW_LADDER_MS`] whose width on screen, at
/// `ms_per_px`, is at least `dot_px` — the widest rung when none is. The
/// answer moves only where a rung starts or stops fitting.
#[must_use]
pub fn dot_window_ms(ms_per_px: f64, dot_px: f64) -> i64 {
    let widest = DOT_WINDOW_LADDER_MS[DOT_WINDOW_LADDER_MS.len() - 1];
    if !(ms_per_px.is_finite() && ms_per_px > 0.0) {
        return widest;
    }
    DOT_WINDOW_LADDER_MS
        .into_iter()
        .find(|rung| *rung as f64 / ms_per_px >= dot_px)
        .unwrap_or(widest)
}

/// What one frame keys its dots on: a window per pane, and the bar opens the
/// tape may need beyond the bars on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeDots {
    /// The candles' window, in exchange milliseconds.
    pub candle_window_ms: i64,
    /// The tape's window, in exchange milliseconds. It also decides the
    /// pane: a dot is the tape's when its tape window starts at or after the
    /// tape does, and the candles' otherwise, whole.
    pub tape_window_ms: i64,
    /// See [`PaneGeometry::lane_bar_opens`].
    pub lane_bar_opens: Vec<i64>,
}

impl VolumeDots {
    /// The windows `geometry`'s zoom picks. A dot is `2 × max_radius`
    /// across, on both panes (see `LiveLaneStyle::pane_radii`). The tape's
    /// scale is its window over its width; the candles' is the series'
    /// typical bar, `typical_bar_ms` — the reference the tape's window is
    /// sized from, never the bars on screen — over the pixels a bar takes.
    #[must_use]
    pub fn resolve(
        geometry: &PaneGeometry,
        bubbles: &BubbleStyle,
        timeline: &BarTimeline,
        typical_bar_ms: i64,
    ) -> Self {
        let dot_px = 2.0 * f64::from(bubbles.max_radius);
        let lane_ms = timeline
            .lane_bounds_ms()
            .map_or(0, |(start, end)| end.saturating_sub(start));
        Self {
            candle_window_ms: dot_window_ms(
                typical_bar_ms as f64 / f64::from(geometry.px_per_bar),
                dot_px,
            ),
            tape_window_ms: dot_window_ms(
                lane_ms as f64 / f64::from(geometry.lane_width_px),
                dot_px,
            ),
            lane_bar_opens: geometry.lane_bar_opens.clone(),
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

/// The price level dots are keyed on: the instrument's native grouping.
pub(super) fn native_grouping(config: &HeatmapConfig) -> EffectiveGrouping {
    EffectiveGrouping::resolve(
        DisplayGrouping::Native,
        config.price_grouping,
        Decimal::ZERO,
    )
}

/// Every bar open a frame knows, ascending: the bars on screen and the ones
/// the tape reaches.
pub(super) struct BarOpens(Vec<i64>);

impl BarOpens {
    pub(super) fn new(timeline: &BarTimeline, dots: &VolumeDots) -> Self {
        let mut opens: Vec<i64> = timeline
            .bar_opens()
            .chain(dots.lane_bar_opens.iter().copied())
            .collect();
        opens.sort_unstable();
        opens.dedup();
        Self(opens)
    }

    /// The open of the bar `timestamp_ms` is in, and the next bar's open
    /// when it is known.
    fn around(&self, timestamp_ms: i64) -> Option<(i64, Option<i64>)> {
        let partition = self.0.partition_point(|open| *open <= timestamp_ms);
        let open = *self.0.get(partition.checked_sub(1)?)?;
        Some((open, self.0.get(partition).copied()))
    }
}

/// What may share a dot: the window, the bar, the recording generation and
/// the native level.
type DotKey = (i64, i64, Option<u64>, Decimal);

struct DotBuilder {
    next_open: Option<i64>,
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    quantity: Decimal,
    buy_quantity: Decimal,
    agg_ids: Vec<u64>,
}

/// Key `prints` into dots of `window_ms` on `grouping`'s levels.
///
/// A print whose bar no known open precedes is left out: it has no bar to
/// belong to. The coverage generation stays in the key, as it does for every
/// cluster, so a dot never spans a break in the recording.
pub(super) fn dot_clusters<'a>(
    prints: impl IntoIterator<Item = &'a Aggression>,
    coverage: &[CoverageSegment],
    grouping: EffectiveGrouping,
    window_ms: i64,
    bars: &BarOpens,
) -> Vec<AggressionCluster> {
    let window_ms = window_ms.max(1);
    let mut dots: BTreeMap<DotKey, DotBuilder> = BTreeMap::new();
    for print in prints {
        let Some((bar_open, next_open)) = bars.around(print.timestamp_ms) else {
            continue;
        };
        let key = (
            print.timestamp_ms.div_euclid(window_ms),
            bar_open,
            generation_at(print.timestamp_ms, coverage),
            bucket_for_price(print.price, grouping),
        );
        let bought = match print.side {
            AggressorSide::Buy => print.quantity,
            AggressorSide::Sell => Decimal::ZERO,
        };
        let dot = dots.entry(key).or_insert_with(|| DotBuilder {
            next_open,
            first_timestamp_ms: print.timestamp_ms,
            last_timestamp_ms: print.timestamp_ms,
            quantity: Decimal::ZERO,
            buy_quantity: Decimal::ZERO,
            agg_ids: Vec::new(),
        });
        dot.first_timestamp_ms = dot.first_timestamp_ms.min(print.timestamp_ms);
        dot.last_timestamp_ms = dot.last_timestamp_ms.max(print.timestamp_ms);
        dot.quantity += print.quantity;
        dot.buy_quantity += bought;
        dot.agg_ids.push(print.agg_id);
    }
    let mut clusters: Vec<AggressionCluster> = dots
        .into_iter()
        .map(|((window, bar_open, generation, level), dot)| {
            finish(
                window * window_ms,
                window_ms,
                bar_open,
                generation,
                level,
                grouping,
                dot,
            )
        })
        .collect();
    sort_clusters(&mut clusters);
    clusters
}

/// One finished dot. It sits exactly on its level, and at its window's
/// centre held inside its own bar, so a window split by a bar close draws
/// each part in the bar it traded in.
fn finish(
    window_start_ms: i64,
    window_ms: i64,
    bar_open: i64,
    generation: Option<u64>,
    level: Decimal,
    grouping: EffectiveGrouping,
    mut dot: DotBuilder,
) -> AggressionCluster {
    dot.agg_ids.sort_unstable();
    dot.agg_ids.dedup();
    let sold = dot.quantity - dot.buy_quantity;
    // The side that took more; an even split is a buy, on exact quantities,
    // so the answer never depends on the order the prints arrived in.
    let side = if sold > dot.buy_quantity {
        AggressorSide::Sell
    } else {
        AggressorSide::Buy
    };
    let low = window_start_ms.max(bar_open);
    let high = dot
        .next_open
        .map_or(i64::MAX, |next| next.saturating_sub(1))
        .min(window_start_ms + window_ms - 1)
        .max(low);
    let timestamp_ms = (window_start_ms + window_ms / 2).clamp(low, high);
    AggressionCluster {
        agg_id: dot.agg_ids[0],
        trade_count: dot.agg_ids.len(),
        agg_ids: dot.agg_ids,
        generation,
        side,
        consumed_side: consumed_side(side),
        price_bucket: level,
        price_span: grouping.bucket_width,
        quantity: dot.quantity,
        buy_quantity: dot.buy_quantity,
        price: level,
        timestamp_ms,
        first_timestamp_ms: dot.first_timestamp_ms,
        last_timestamp_ms: dot.last_timestamp_ms,
        matched_quantity: Decimal::ZERO,
        liquidity_event_ids: Vec::new(),
    }
}
