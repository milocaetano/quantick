//! Renderer-independent projection of RLE history into normalized primitives.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::config::IntensityMode;
use super::history::{AggressorSide, LiquidityHistory, RestingSide};
use super::timeline::BarTimeline;

/// Exact visible price interval. `high` maps to y=0 and `low` to y=1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceWindow {
    /// Lowest visible price.
    pub low: Decimal,
    /// Highest visible price.
    pub high: Decimal,
}

impl PriceWindow {
    /// Construct a non-degenerate price window.
    #[must_use]
    pub fn new(low: Decimal, high: Decimal) -> Option<Self> {
        (high > low).then_some(Self { low, high })
    }

    /// Map a visible price to normalized screen y.
    #[must_use]
    pub fn y(&self, price: Decimal) -> Option<f64> {
        if price < self.low || price > self.high {
            return None;
        }
        ((self.high - price) / (self.high - self.low)).to_f64()
    }
}

/// One clipped liquidity rectangle ready for a backend to colour.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapCell {
    /// Synchronization generation.
    pub generation: u64,
    /// Resting side.
    pub side: RestingSide,
    /// Exact lower bucket edge before clipping.
    pub price_bucket: Decimal,
    /// Aggregated displayed quantity.
    pub quantity: Decimal,
    /// Normalized left and right positions.
    pub x0: f64,
    /// Normalized right position.
    pub x1: f64,
    /// Normalized top and bottom positions.
    pub y0: f64,
    /// Normalized bottom position.
    pub y1: f64,
    /// Gamma-adjusted colour-ramp position.
    pub intensity: f32,
    /// Final alpha after applying configured opacity.
    pub alpha: f32,
}

/// One aggressive execution ready for circles, footprint cells or tooltips.
#[derive(Debug, Clone, PartialEq)]
pub struct AggressionPrimitive {
    /// Aggregate-trade id.
    pub agg_id: u64,
    /// Book generation current at observation time.
    pub generation: Option<u64>,
    /// Taker side.
    pub side: AggressorSide,
    /// Passive side this trade attempted to consume.
    pub consumed_side: RestingSide,
    /// Exact execution quantity.
    pub quantity: Decimal,
    /// Normalized chart coordinates.
    pub x: f64,
    /// Normalized y coordinate.
    pub y: f64,
    /// `[0,1]` size factor whose square is proportional to quantity.
    pub size: f32,
}

/// A visible interval that must not be filled or connected.
#[derive(Debug, Clone, PartialEq)]
pub struct GapPrimitive {
    /// Previous synchronized generation.
    pub from_generation: Option<u64>,
    /// Replacement generation.
    pub to_generation: Option<u64>,
    /// Normalized horizontal interval.
    pub x0: f64,
    /// Normalized horizontal interval end.
    pub x1: f64,
    /// Diagnostic reason copied from history.
    pub reason: String,
}

/// Complete pure output for one chart frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapProjection {
    /// Whether the feature was enabled in sanitized configuration.
    pub enabled: bool,
    /// Visible heatmap rectangles.
    pub cells: Vec<HeatmapCell>,
    /// Visible aggressive executions.
    pub aggressions: Vec<AggressionPrimitive>,
    /// Visible continuity gaps.
    pub gaps: Vec<GapPrimitive>,
    /// Quantity that maps to full cell intensity.
    pub liquidity_reference: Decimal,
    /// Quantity that maps to full aggression size.
    pub aggression_reference: Decimal,
    /// Cells omitted by the configured primitive cap.
    pub dropped_cells: usize,
    /// Aggressions omitted by the configured primitive cap.
    pub dropped_aggressions: usize,
}

impl HeatmapProjection {
    fn empty(enabled: bool) -> Self {
        Self {
            enabled,
            cells: Vec::new(),
            aggressions: Vec::new(),
            gaps: Vec::new(),
            liquidity_reference: Decimal::ZERO,
            aggression_reference: Decimal::ZERO,
            dropped_cells: 0,
            dropped_aggressions: 0,
        }
    }
}

#[derive(Debug)]
struct DraftCell {
    generation: u64,
    side: RestingSide,
    price_bucket: Decimal,
    quantity: Decimal,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

#[derive(Debug)]
struct DraftAggression {
    agg_id: u64,
    generation: Option<u64>,
    side: AggressorSide,
    consumed_side: RestingSide,
    quantity: Decimal,
    x: f64,
    y: f64,
}

/// Project retained order flow into `[0,1] × [0,1]` chart primitives.
///
/// The function performs no allocation proportional to the full book: only
/// runs intersecting both the timeline and price window become drafts.
#[must_use]
pub fn project(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> HeatmapProjection {
    let config = history.config();
    if !config.enabled {
        return HeatmapProjection::empty(false);
    }
    let Some((time_start, time_end)) = timeline.timestamp_range() else {
        return HeatmapProjection::empty(true);
    };

    let mut drafts = Vec::new();
    for run in history.runs_intersecting(time_start, time_end) {
        let run_end = run.end_ms.or(history.latest_book_ms()).unwrap_or(time_end);
        let bucket_low = run.price_bucket;
        let bucket_high = bucket_low + config.price_grouping;
        if bucket_high <= prices.low || bucket_low >= prices.high {
            continue;
        }

        let clipped_start = run
            .start_ms
            .max(time_start)
            .max(history.retention_start_ms().unwrap_or(time_start));
        let clipped_end = run_end.min(time_end);
        let Some(x0) = timeline
            .locate_clamped(clipped_start)
            .map(|position| position.normalized)
        else {
            continue;
        };
        let Some(x1) = timeline
            .locate_clamped(clipped_end)
            .map(|position| position.normalized)
        else {
            continue;
        };
        if x1 <= x0 {
            continue;
        }

        let clipped_low = bucket_low.max(prices.low);
        let clipped_high = bucket_high.min(prices.high);
        let Some(y0) = prices.y(clipped_high) else {
            continue;
        };
        let Some(y1) = prices.y(clipped_low) else {
            continue;
        };
        if y1 <= y0 {
            continue;
        }

        drafts.push(DraftCell {
            generation: run.generation,
            side: run.side,
            price_bucket: run.price_bucket,
            quantity: run.quantity,
            x0,
            x1,
            y0,
            y1,
        });
    }

    let liquidity_reference = match config.intensity_mode {
        IntensityMode::VisibleP99 => percentile_99(drafts.iter().map(|cell| cell.quantity)),
        IntensityMode::Fixed(maximum) => maximum,
    };

    let dropped_cells = drafts.len().saturating_sub(config.max_visible_cells);
    if dropped_cells > 0 {
        // Retain the strongest walls deterministically and surface the loss.
        drafts.sort_by(|a, b| {
            b.quantity
                .cmp(&a.quantity)
                .then_with(|| a.generation.cmp(&b.generation))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.x0.total_cmp(&b.x0))
        });
        drafts.truncate(config.max_visible_cells);
    }

    let cells = drafts
        .into_iter()
        .map(|draft| {
            let intensity =
                normalized_log_intensity(draft.quantity, liquidity_reference, config.gamma);
            HeatmapCell {
                generation: draft.generation,
                side: draft.side,
                price_bucket: draft.price_bucket,
                quantity: draft.quantity,
                x0: draft.x0,
                x1: draft.x1,
                y0: draft.y0,
                y1: draft.y1,
                intensity,
                alpha: intensity * config.opacity,
            }
        })
        .collect();

    let mut aggression_drafts = Vec::new();
    if config.show_aggressions {
        for trade in history.aggressions() {
            let Some(x) = timeline
                .locate(trade.timestamp_ms)
                .map(|position| position.normalized)
            else {
                continue;
            };
            let Some(y) = prices.y(trade.price) else {
                continue;
            };
            aggression_drafts.push(DraftAggression {
                agg_id: trade.agg_id,
                generation: trade.generation,
                side: trade.side,
                consumed_side: trade.consumed_side(),
                quantity: trade.quantity,
                x,
                y,
            });
        }
    }
    let aggression_reference = percentile_99(aggression_drafts.iter().map(|trade| trade.quantity));
    let dropped_aggressions = aggression_drafts
        .len()
        .saturating_sub(config.max_aggression_primitives);
    if dropped_aggressions > 0 {
        aggression_drafts.sort_by(|a, b| {
            b.quantity
                .cmp(&a.quantity)
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        });
        aggression_drafts.truncate(config.max_aggression_primitives);
    }
    let aggressions = aggression_drafts
        .into_iter()
        .map(|draft| AggressionPrimitive {
            agg_id: draft.agg_id,
            generation: draft.generation,
            side: draft.side,
            consumed_side: draft.consumed_side,
            quantity: draft.quantity,
            x: draft.x,
            y: draft.y,
            size: normalized_area_size(draft.quantity, aggression_reference),
        })
        .collect();

    let mut gaps: Vec<GapPrimitive> = history
        .coverage_gaps()
        .filter_map(|gap| {
            let gap_end = gap.end_ms.unwrap_or(time_end);
            if gap_end <= time_start || gap.start_ms >= time_end {
                return None;
            }
            let x0 = timeline.locate_clamped(gap.start_ms.max(time_start))?;
            let x1 = timeline.locate_clamped(gap_end.min(time_end))?;
            (x1.normalized > x0.normalized).then(|| GapPrimitive {
                from_generation: gap.from_generation,
                to_generation: gap.to_generation,
                x0: x0.normalized,
                x1: x1.normalized,
                reason: gap.reason.clone(),
            })
        })
        .collect();

    // Historical trades can precede the first locally captured L2 snapshot.
    // Make that absence an explicit primitive instead of a transparent region
    // that could be mistaken for zero resting liquidity.
    match history.coverage_segments().next() {
        Some(first_coverage) if first_coverage.start_ms > time_start => {
            let unavailable_end = first_coverage.start_ms.min(time_end);
            if let (Some(x0), Some(x1)) = (
                timeline.locate_clamped(time_start),
                timeline.locate_clamped(unavailable_end),
            ) && x1.normalized > x0.normalized
            {
                gaps.push(GapPrimitive {
                    from_generation: None,
                    to_generation: Some(first_coverage.generation),
                    x0: x0.normalized,
                    x1: x1.normalized,
                    reason: "book_unavailable_before_capture".to_owned(),
                });
            }
        }
        None => gaps.push(GapPrimitive {
            from_generation: None,
            to_generation: None,
            x0: 0.0,
            x1: 1.0,
            reason: "book_unavailable_before_capture".to_owned(),
        }),
        Some(_) => {}
    }
    gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));

    HeatmapProjection {
        enabled: true,
        cells,
        aggressions,
        gaps,
        liquidity_reference,
        aggression_reference,
        dropped_cells,
        dropped_aggressions,
    }
}

fn percentile_99(values: impl Iterator<Item = Decimal>) -> Decimal {
    let mut positive: Vec<Decimal> = values
        .filter(|quantity| *quantity > Decimal::ZERO)
        .collect();
    if positive.is_empty() {
        return Decimal::ZERO;
    }
    positive.sort_unstable();
    let rank = (99 * positive.len()).div_ceil(100);
    positive[rank.saturating_sub(1)]
}

fn normalized_log_intensity(quantity: Decimal, reference: Decimal, gamma: f32) -> f32 {
    if quantity <= Decimal::ZERO || reference <= Decimal::ZERO {
        return 0.0;
    }
    let ratio = (quantity / reference).to_f64().unwrap_or(0.0).max(0.0);
    let logarithmic = ((1.0 + 9.0 * ratio).ln() / 10.0_f64.ln()).clamp(0.0, 1.0);
    logarithmic.powf(f64::from(gamma)) as f32
}

fn normalized_area_size(quantity: Decimal, reference: Decimal) -> f32 {
    if quantity <= Decimal::ZERO || reference <= Decimal::ZERO {
        return 0.0;
    }
    (quantity / reference)
        .to_f64()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        .sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderflow::config::HeatmapConfig;
    use crate::orderflow::history::LiquidityHistory;
    use quantick_engine::{Bar, Side, Trade};
    use quantick_orderbook::BookSide;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn level(price: &str, quantity: &str) -> BookLevel {
        BookLevel::new(dec(price), dec(quantity)).unwrap()
    }

    fn snapshot(update_id: u64) -> BookSnapshot {
        BookSnapshot::new(
            update_id,
            vec![level("99", "2"), level("100", "3")],
            vec![level("101", "4"), level("102", "5")],
            BookCoverage::Full,
        )
    }

    fn bar(open_ms: i64, close_ms: i64) -> Bar {
        Bar {
            open_time: open_ms,
            close_time: close_ms,
            open: dec("100"),
            high: dec("101"),
            low: dec("99"),
            close: dec("100"),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    fn config() -> HeatmapConfig {
        HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            ..HeatmapConfig::default()
        }
    }

    #[test]
    fn price_window_maps_high_to_top_and_low_to_bottom() {
        let window = PriceWindow::new(Decimal::from(100), Decimal::from(110)).unwrap();
        assert_eq!(window.y(Decimal::from(110)), Some(0.0));
        assert_eq!(window.y(Decimal::from(100)), Some(1.0));
        assert_eq!(window.y(Decimal::from(105)), Some(0.5));
        assert_eq!(window.y(Decimal::from(99)), None);
    }

    #[test]
    fn rejects_degenerate_price_windows() {
        assert!(PriceWindow::new(Decimal::ONE, Decimal::ONE).is_none());
        assert!(PriceWindow::new(Decimal::TWO, Decimal::ONE).is_none());
    }

    #[test]
    fn percentile_is_robust_to_one_large_outlier() {
        let values = (1..=100)
            .map(Decimal::from)
            .chain(std::iter::once(Decimal::from(1_000_000)));
        assert_eq!(percentile_99(values), Decimal::from(100));
    }

    #[test]
    fn log_intensity_is_monotonic_bounded_and_gamma_adjusted() {
        let reference = Decimal::from(100);
        let quiet = normalized_log_intensity(Decimal::ONE, reference, 1.0);
        let medium = normalized_log_intensity(Decimal::from(50), reference, 1.0);
        let full = normalized_log_intensity(reference, reference, 1.0);
        let above = normalized_log_intensity(Decimal::from(1_000), reference, 1.0);
        assert!(quiet > 0.0 && quiet < medium);
        assert!(medium < full);
        assert_eq!(full, 1.0);
        assert_eq!(above, 1.0);
        assert!(
            normalized_log_intensity(Decimal::from(10), reference, 0.5)
                > normalized_log_intensity(Decimal::from(10), reference, 1.0)
        );
    }

    #[test]
    fn aggression_size_uses_area_not_radius_proportionality() {
        let quarter = normalized_area_size(Decimal::from(25), Decimal::from(100));
        let full = normalized_area_size(Decimal::from(100), Decimal::from(100));
        assert!((quarter - 0.5).abs() < f32::EPSILON);
        assert_eq!(full, 1.0);
    }

    #[test]
    fn disabled_projection_is_empty_even_with_data() {
        let mut history = LiquidityHistory::new(HeatmapConfig::default());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let projection = project(&history, &timeline, prices);
        assert!(!projection.enabled);
        assert!(projection.cells.is_empty());
        assert!(projection.aggressions.is_empty());
    }

    #[test]
    fn projects_and_clips_runs_in_time_and_price() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(
                800,
                &BookDelta::new(11, 11, vec![level("100", "6")], vec![]),
            )
            .unwrap();
        // A stale event advances display coverage without splitting any run.
        history
            .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();
        let projection = project(&history, &timeline, prices);

        assert!(projection.enabled);
        assert!(!projection.cells.is_empty());
        assert!(
            projection
                .cells
                .iter()
                .all(|cell| (0.0..=1.0).contains(&cell.x0)
                    && (0.0..=1.0).contains(&cell.x1)
                    && (0.0..=1.0).contains(&cell.y0)
                    && (0.0..=1.0).contains(&cell.y1))
        );
        let old = projection
            .cells
            .iter()
            .find(|cell| cell.price_bucket == dec("100") && cell.quantity == dec("3"))
            .unwrap();
        assert!((old.x0 - 0.1).abs() < 1e-9);
        assert!((old.x1 - 0.8).abs() < 1e-9);
        // Only the lower half of bucket [100,101] is in [99.5,100.5].
        assert!((old.y0 - 0.0).abs() < 1e-9);
        assert!((old.y1 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn marks_history_before_first_snapshot_as_unavailable() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(400, 1, snapshot(10)).unwrap();
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let unavailable = projection
            .gaps
            .iter()
            .find(|gap| gap.reason == "book_unavailable_before_capture")
            .unwrap();
        assert_eq!(unavailable.x0, 0.0);
        assert!((unavailable.x1 - 0.4).abs() < 1e-9);
        assert_eq!(unavailable.to_generation, Some(1));
    }

    #[test]
    fn an_unsynchronized_history_marks_the_whole_timeline_unavailable() {
        let history = LiquidityHistory::new(config());
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert_eq!(
            projection.gaps,
            [GapPrimitive {
                from_generation: None,
                to_generation: None,
                x0: 0.0,
                x1: 1.0,
                reason: "book_unavailable_before_capture".to_owned(),
            }]
        );
    }

    #[test]
    fn resync_gap_is_a_primitive_and_runs_do_not_bridge_it() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.mark_gap(300, "sequence_gap").unwrap();
        history.install_snapshot(600, 2, snapshot(50)).unwrap();
        history
            .apply_delta(900, &BookDelta::new(50, 50, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let gap = projection
            .gaps
            .iter()
            .find(|gap| gap.reason == "sequence_gap")
            .unwrap();
        assert!((gap.x0 - 0.3).abs() < 1e-9);
        assert!((gap.x1 - 0.6).abs() < 1e-9);
        assert!(
            projection
                .cells
                .iter()
                .all(|cell| { cell.x1 <= gap.x0 || cell.x0 >= gap.x1 })
        );
    }

    #[test]
    fn aggression_uses_trade_side_without_affecting_liquidity_reference() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.record_aggression(&Trade {
            agg_id: 42,
            timestamp_ms: 500,
            price: dec("101"),
            quantity: dec("4"),
            side: Side::Buy,
        });
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let aggression = &projection.aggressions[0];
        assert_eq!(aggression.agg_id, 42);
        assert_eq!(aggression.side, Side::Buy);
        assert_eq!(aggression.consumed_side, BookSide::Ask);
        assert!((aggression.x - 0.5).abs() < 1e-9);
        assert_eq!(aggression.size, 1.0);
        assert_eq!(projection.liquidity_reference, dec("5"));
    }

    #[test]
    fn primitive_caps_report_dropped_items() {
        let limited = HeatmapConfig {
            max_visible_cells: 1,
            max_aggression_primitives: 1,
            ..config()
        };
        let mut history = LiquidityHistory::new(limited);
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        for id in 1..=3 {
            history.record_aggression(&Trade {
                agg_id: id,
                timestamp_ms: 200 + id as i64,
                price: dec("101"),
                quantity: Decimal::from(id),
                side: Side::Buy,
            });
        }
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert_eq!(projection.cells.len(), 1);
        assert_eq!(projection.dropped_cells, 3);
        assert_eq!(projection.cells[0].quantity, dec("5"));
        assert_eq!(projection.aggressions.len(), 1);
        assert_eq!(projection.dropped_aggressions, 2);
        assert_eq!(projection.aggressions[0].agg_id, 3);
    }

    #[test]
    fn projection_uses_live_end_of_partial_timeline() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(750, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let closed = [bar(0, 200)];
        let partial = bar(300, 350);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), Some(800));
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert!(
            projection
                .cells
                .iter()
                .any(|cell| cell.x1 > 0.9 && cell.x1 < 1.0)
        );
    }
}
