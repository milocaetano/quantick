//! Deterministic renderer-only grouping of captured liquidity runs.
//!
//! Capture always remains in `HeatmapConfig::price_grouping` base buckets.
//! This module performs a projection-time sweep into wider visual ranges, so
//! changing zoom or display grouping never resets honest retained history.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::config::{DisplayGrouping, MAX_DISPLAY_GROUP_MULTIPLE};
use super::history::{CoverageSegment, LiquidityRun, RestingSide};

/// Resolved visual grouping for one projection frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveGrouping {
    /// Exact capture/RLE bucket width.
    pub base_width: Decimal,
    /// Exact number of adjacent base buckets in one visual range.
    pub multiple: u32,
    /// Exact visual range width (`base_width * multiple`).
    pub bucket_width: Decimal,
}

impl EffectiveGrouping {
    /// Resolve display settings against the visible price span.
    ///
    /// The result is always an integer multiple of the positive base width.
    #[must_use]
    pub fn resolve(display: DisplayGrouping, base_width: Decimal, visible_span: Decimal) -> Self {
        let base_width = if base_width > Decimal::ZERO {
            base_width
        } else {
            Decimal::ONE
        };
        let requested = match display {
            DisplayGrouping::Native => 1,
            DisplayGrouping::Multiple(multiple) => multiple.clamp(1, MAX_DISPLAY_GROUP_MULTIPLE),
            DisplayGrouping::Adaptive { target_rows } => {
                adaptive_multiple(base_width, visible_span, target_rows)
            }
        };
        from_multiple(base_width, requested)
    }
}

fn from_multiple(base_width: Decimal, multiple: u32) -> EffectiveGrouping {
    let multiple = multiple.max(1);
    let Some(bucket_width) = base_width.checked_mul(Decimal::from(multiple)) else {
        return EffectiveGrouping {
            base_width,
            multiple: 1,
            bucket_width: base_width,
        };
    };
    EffectiveGrouping {
        base_width,
        multiple,
        bucket_width,
    }
}

fn adaptive_multiple(base_width: Decimal, visible_span: Decimal, target_rows: u32) -> u32 {
    if visible_span <= Decimal::ZERO {
        return 1;
    }
    let target_rows = target_rows.max(1);
    let requested = visible_span / base_width / Decimal::from(target_rows);
    let whole = requested.trunc();
    let ceiling = if requested > whole {
        whole + Decimal::ONE
    } else {
        whole
    };
    ceiling
        .to_u32()
        .unwrap_or(MAX_DISPLAY_GROUP_MULTIPLE)
        .clamp(1, MAX_DISPLAY_GROUP_MULTIPLE)
}

/// Exact bounds used by the grouping sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupingWindow {
    /// Inclusive visible/retained time floor.
    pub start_ms: i64,
    /// Exclusive visual timeline end.
    pub end_ms: i64,
    /// Latest timestamp for which book state is known.
    pub open_run_end_ms: i64,
    /// Visible price floor.
    pub price_low: Decimal,
    /// Visible price ceiling.
    pub price_high: Decimal,
}

/// One constant summed-quantity interval in a visual price range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualLiquidityRun {
    /// Synchronization generation. Visual runs never bridge generations.
    pub generation: u64,
    /// Resting side.
    pub side: RestingSide,
    /// Inclusive lower edge of the visual price range.
    pub price_bucket: Decimal,
    /// Sum of all captured base buckets in this visual range.
    pub quantity: Decimal,
    /// Inclusive observation timestamp.
    pub start_ms: i64,
    /// Exclusive change timestamp.
    pub end_ms: i64,
}

/// One factual change between two visual-range quantities.
///
/// Window clipping, retention clipping, snapshot starts, coverage ends and gap
/// boundaries are excluded. Consumers may therefore turn reductions into
/// neutral "displayed liquidity decreased" events without inventing causes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidityTransition {
    /// Synchronization generation.
    pub generation: u64,
    /// Resting side.
    pub side: RestingSide,
    /// Inclusive lower edge of the visual price range.
    pub price_bucket: Decimal,
    /// Exchange timestamp at which the new total became observable.
    pub timestamp_ms: i64,
    /// Summed displayed quantity immediately before the observation.
    pub before: Decimal,
    /// Summed displayed quantity immediately after the observation.
    pub after: Decimal,
}

/// Complete output of one grouping sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupedLiquidity {
    /// Constant visual-range intervals.
    pub runs: Vec<VisualLiquidityRun>,
    /// Honest, interior before/after observations.
    pub transitions: Vec<LiquidityTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    generation: u64,
    side: u8,
    price_bucket: Decimal,
}

impl GroupKey {
    fn new(generation: u64, side: RestingSide, price_bucket: Decimal) -> Self {
        Self {
            generation,
            side: side_key(side),
            price_bucket,
        }
    }

    fn public_side(self) -> RestingSide {
        if self.side == 0 {
            quantick_orderbook::BookSide::Bid
        } else {
            quantick_orderbook::BookSide::Ask
        }
    }
}

#[derive(Debug, Default)]
struct Change {
    starts: Decimal,
    ends: Decimal,
    observed: bool,
}

/// Return the visual-range lower edge containing `price`.
#[must_use]
pub fn bucket_for_price(price: Decimal, grouping: EffectiveGrouping) -> Decimal {
    (price / grouping.bucket_width).floor() * grouping.bucket_width
}

/// Sweep base RLE runs into summed visual ranges.
///
/// All supplied base runs are considered before the visual-range intersection
/// is tested. This is important for partially visible ranges: off-screen base
/// buckets still contribute to the honest total of the visible range.
#[must_use]
pub fn sweep_grouped_runs<'a>(
    runs: impl IntoIterator<Item = &'a LiquidityRun>,
    coverage: impl IntoIterator<Item = &'a CoverageSegment>,
    grouping: EffectiveGrouping,
    window: GroupingWindow,
) -> GroupedLiquidity {
    if window.end_ms <= window.start_ms
        || window.price_high <= window.price_low
        || grouping.bucket_width <= Decimal::ZERO
    {
        return GroupedLiquidity::default();
    }

    let render_end = window.end_ms.min(window.open_run_end_ms);
    if render_end < window.start_ms {
        return GroupedLiquidity::default();
    }

    let coverage: BTreeMap<u64, (i64, Option<i64>)> = coverage
        .into_iter()
        .map(|segment| (segment.generation, (segment.start_ms, segment.end_ms)))
        .collect();
    let mut groups: BTreeMap<GroupKey, BTreeMap<i64, Change>> = BTreeMap::new();

    for run in runs {
        let price_bucket = bucket_for_price(run.price_bucket, grouping);
        let bucket_high = price_bucket + grouping.bucket_width;
        if bucket_high <= window.price_low || price_bucket >= window.price_high {
            continue;
        }
        if run.start_ms > render_end || run.end_ms.is_some_and(|end_ms| end_ms <= window.start_ms) {
            continue;
        }

        let key = GroupKey::new(run.generation, run.side, price_bucket);
        let changes = groups.entry(key).or_default();
        let start_ms = run.start_ms.max(window.start_ms);
        if start_ms <= render_end {
            let start = changes.entry(start_ms).or_default();
            start.starts += run.quantity;
            start.observed |= run.start_ms >= window.start_ms;
        }

        if let Some(end_ms) = run.end_ms
            && end_ms >= window.start_ms
            && end_ms <= render_end
        {
            let end = changes.entry(end_ms).or_default();
            end.ends += run.quantity;
            end.observed = true;
        }
    }

    let mut output = GroupedLiquidity::default();
    for (key, changes) in groups {
        let side = key.public_side();
        let mut quantity = Decimal::ZERO;
        let mut interval_start = window.start_ms;

        for (timestamp_ms, change) in changes {
            let before = quantity;
            let after = before - change.ends + change.starts;
            debug_assert!(after >= Decimal::ZERO, "grouped liquidity became negative");
            if after == before {
                continue;
            }

            if before > Decimal::ZERO && timestamp_ms > interval_start {
                output.runs.push(VisualLiquidityRun {
                    generation: key.generation,
                    side,
                    price_bucket: key.price_bucket,
                    quantity: before,
                    start_ms: interval_start,
                    end_ms: timestamp_ms.min(render_end),
                });
            }

            if change.observed
                && timestamp_ms > window.start_ms
                && timestamp_ms < window.end_ms
                && transition_is_inside_coverage(&coverage, key.generation, timestamp_ms)
            {
                output.transitions.push(LiquidityTransition {
                    generation: key.generation,
                    side,
                    price_bucket: key.price_bucket,
                    timestamp_ms,
                    before,
                    after,
                });
            }
            quantity = after;
            interval_start = timestamp_ms;
        }

        if quantity > Decimal::ZERO && render_end > interval_start {
            output.runs.push(VisualLiquidityRun {
                generation: key.generation,
                side,
                price_bucket: key.price_bucket,
                quantity,
                start_ms: interval_start,
                end_ms: render_end,
            });
        }
    }

    output.runs.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| a.generation.cmp(&b.generation))
            .then_with(|| side_key(a.side).cmp(&side_key(b.side)))
            .then_with(|| a.price_bucket.cmp(&b.price_bucket))
    });
    output.transitions.sort_by(|a, b| {
        a.timestamp_ms
            .cmp(&b.timestamp_ms)
            .then_with(|| a.generation.cmp(&b.generation))
            .then_with(|| side_key(a.side).cmp(&side_key(b.side)))
            .then_with(|| a.price_bucket.cmp(&b.price_bucket))
    });
    output
}

fn transition_is_inside_coverage(
    coverage: &BTreeMap<u64, (i64, Option<i64>)>,
    generation: u64,
    timestamp_ms: i64,
) -> bool {
    coverage.get(&generation).is_some_and(|(start_ms, end_ms)| {
        timestamp_ms > *start_ms && end_ms.is_none_or(|end_ms| timestamp_ms < end_ms)
    })
}

fn side_key(side: RestingSide) -> u8 {
    match side {
        quantick_orderbook::BookSide::Bid => 0,
        quantick_orderbook::BookSide::Ask => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_orderbook::BookSide;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn run(
        side: RestingSide,
        price: &str,
        quantity: &str,
        start_ms: i64,
        end_ms: Option<i64>,
    ) -> LiquidityRun {
        LiquidityRun {
            generation: 7,
            side,
            price_bucket: dec(price),
            quantity: dec(quantity),
            start_ms,
            end_ms,
        }
    }

    fn coverage(start_ms: i64, end_ms: Option<i64>) -> CoverageSegment {
        CoverageSegment {
            generation: 7,
            start_ms,
            end_ms,
        }
    }

    fn window() -> GroupingWindow {
        GroupingWindow {
            start_ms: 0,
            end_ms: 1_000,
            open_run_end_ms: 1_000,
            price_low: dec("99"),
            price_high: dec("104"),
        }
    }

    #[test]
    fn adaptive_resolution_is_an_integer_base_multiple() {
        let grouping = EffectiveGrouping::resolve(
            DisplayGrouping::Adaptive { target_rows: 160 },
            dec("0.25"),
            dec("100"),
        );
        assert_eq!(grouping.multiple, 3);
        assert_eq!(grouping.bucket_width, dec("0.75"));

        let native =
            EffectiveGrouping::resolve(DisplayGrouping::Multiple(0), dec("0.25"), dec("100"));
        assert_eq!(native.multiple, 1);
        assert_eq!(native.bucket_width, dec("0.25"));
    }

    #[test]
    fn sums_base_runs_and_emits_one_visual_transition() {
        let runs = [
            run(BookSide::Ask, "100", "4", 100, Some(500)),
            run(BookSide::Ask, "100", "1", 500, None),
            run(BookSide::Ask, "101", "6", 100, None),
        ];
        let grouping =
            EffectiveGrouping::resolve(DisplayGrouping::Multiple(2), Decimal::ONE, dec("5"));
        let grouped = sweep_grouped_runs(&runs, [&coverage(100, None)], grouping, window());

        assert_eq!(
            grouped.runs,
            [
                VisualLiquidityRun {
                    generation: 7,
                    side: BookSide::Ask,
                    price_bucket: dec("100"),
                    quantity: dec("10"),
                    start_ms: 100,
                    end_ms: 500,
                },
                VisualLiquidityRun {
                    generation: 7,
                    side: BookSide::Ask,
                    price_bucket: dec("100"),
                    quantity: dec("7"),
                    start_ms: 500,
                    end_ms: 1_000,
                }
            ]
        );
        assert_eq!(
            grouped.transitions,
            [LiquidityTransition {
                generation: 7,
                side: BookSide::Ask,
                price_bucket: dec("100"),
                timestamp_ms: 500,
                before: dec("10"),
                after: dec("7"),
            }]
        );
    }

    #[test]
    fn partially_visible_range_keeps_offscreen_base_quantity() {
        let runs = [
            run(BookSide::Bid, "100", "2", 100, None),
            run(BookSide::Bid, "101", "3", 100, None),
        ];
        let grouping =
            EffectiveGrouping::resolve(DisplayGrouping::Multiple(2), Decimal::ONE, dec("1"));
        let grouped = sweep_grouped_runs(
            &runs,
            [&coverage(100, None)],
            grouping,
            GroupingWindow {
                price_low: dec("100.75"),
                price_high: dec("101.25"),
                ..window()
            },
        );
        assert_eq!(grouped.runs.len(), 1);
        assert_eq!(grouped.runs[0].quantity, dec("5"));
    }

    #[test]
    fn suppresses_window_and_coverage_end_removals() {
        let at_window_end = run(BookSide::Ask, "100", "4", 100, Some(1_000));
        let at_coverage_end = run(BookSide::Ask, "101", "5", 100, Some(800));
        let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, dec("5"));
        let grouped = sweep_grouped_runs(
            [&at_window_end, &at_coverage_end],
            [&coverage(100, Some(800))],
            grouping,
            window(),
        );
        assert!(grouped.transitions.is_empty());
        assert_eq!(grouped.runs.len(), 2);
    }

    #[test]
    fn visible_clipping_does_not_create_a_removal() {
        let older = run(BookSide::Bid, "100", "8", -500, None);
        let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, dec("5"));
        let grouped = sweep_grouped_runs([&older], [&coverage(-500, None)], grouping, window());
        assert!(grouped.transitions.is_empty());
        assert_eq!(grouped.runs[0].start_ms, 0);
        assert_eq!(grouped.runs[0].quantity, dec("8"));
    }
}
