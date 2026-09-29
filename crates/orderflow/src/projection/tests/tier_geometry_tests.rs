//! Exact placement oracle before hoisting immutable per-frame lane geometry.

use super::*;
use crate::history::Aggression;
use crate::interaction::{AggressionCluster, cluster_aggressions};
use crate::projection::dots::{DotHorizon, fold_dots, native_grouping, window_start};
use crate::projection::tiers::{TierClusters, tier_primitives};
use rust_decimal::prelude::ToPrimitive as _;

mod relative_projection;

/// Preserve the original per-cluster placement operations and their order.
/// In particular, convert to floating point only after Decimal subtraction
/// and division, even for modern epoch timestamps with fractional means.
fn reference_place(
    cluster: AggressionCluster,
    live: bool,
    timeline: &BarTimeline,
    prices: PriceWindow,
    reference: Decimal,
    dots: Option<&VolumeDots>,
) -> Option<AggressionPrimitive> {
    let x = match (live, dots) {
        (true, None) => timeline.locate(cluster.timestamp_ms)?.normalized,
        (false, None) => timeline.locate_in_slot(cluster.timestamp_ms)?.normalized,
        (true, Some(dots)) if dots.tape_only => {
            let (from, now) = timeline.lane_bounds_ms()?;
            let right = timeline.live_now_position()?.normalized;
            if window_start(cluster.last_timestamp_ms, dots.tape_window_ms)
                == window_start(now, dots.tape_window_ms)
            {
                right
            } else {
                let left = timeline.locate_in_lane_clamped(from)?.normalized;
                let mean = cluster.timestamp_quantity / cluster.quantity;
                let fraction = ((mean - Decimal::from(from))
                    / Decimal::from(now.saturating_sub(from).max(1)))
                .to_f64()?;
                left + (right - left) * fraction.clamp(0.0, 1.0)
            }
        }
        (true, Some(_)) => {
            timeline
                .locate_in_lane_clamped(cluster.timestamp_ms)?
                .normalized
        }
        (false, Some(_)) => {
            let slot = timeline.slot_at(cluster.first_timestamp_ms)?;
            let (left, right) = timeline.slot_bounds(slot.index);
            (left + right) / 2.0
        }
    };
    let y = match (live, dots) {
        (_, None) => prices.y(cluster.price)?,
        (true, Some(dots)) if dots.tape_only => prices.y_unclamped(cluster.price)?,
        (true, Some(_)) => {
            prices.y_unclamped(cluster.price_bucket + cluster.price_span / Decimal::TWO)?
        }
        (false, Some(_)) => prices.y(cluster.price_bucket + cluster.price_span / Decimal::TWO)?,
    };
    let buy_share = cluster.buy_share();
    let matched_fraction = cluster.matched_fraction();
    Some(AggressionPrimitive {
        agg_id: cluster.agg_id,
        agg_ids: cluster.agg_ids,
        generation: cluster.generation,
        side: cluster.side,
        consumed_side: cluster.consumed_side,
        quantity: cluster.quantity,
        buy_share,
        live,
        price_bucket: cluster.price_bucket,
        price_span: cluster.price_span,
        price: cluster.price,
        trade_count: cluster.trade_count,
        first_timestamp_ms: cluster.first_timestamp_ms,
        last_timestamp_ms: cluster.last_timestamp_ms,
        timestamp_quantity: cluster.timestamp_quantity,
        matched_quantity: cluster.matched_quantity,
        buy_quantity: cluster.buy_quantity,
        matched_fraction,
        liquidity_event_ids: cluster.liquidity_event_ids,
        x,
        y,
        size: normalized_area_size(cluster.quantity, reference),
        folded_marks: 0,
    })
}

fn assert_native_screen_contract(actual: &[AggressionPrimitive], expected: &[AggressionPrimitive]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        let mut factual = actual.clone();
        if expected.live {
            for (old, new) in [(expected.x, actual.x), (expected.y, actual.y)] {
                assert!(
                    (old - new).abs() <= 1e-12,
                    "native screen-coordinate tolerance"
                );
                assert!(
                    (old - new).abs() * 16_384.0 < 1e-6,
                    "less than one millionth of a pixel on a 16K surface"
                );
                assert_eq!((0.0..=1.0).contains(&old), (0.0..=1.0).contains(&new));
            }
            factual.x = expected.x;
            factual.y = expected.y;
        }
        assert_eq!(
            &factual, expected,
            "all execution fields and every candle coordinate stay exact"
        );
    }
}

fn raw(epoch: i64, config: &HeatmapConfig) -> Vec<AggressionCluster> {
    let trades: Vec<_> = [
        (101, "187500.125", "0.125", Side::Buy),
        (1_101, "187505.125", "0.125", Side::Buy),
        (1_151, "187505.375", "0.375", Side::Sell),
        (1_601, "187520.125", "2.75", Side::Buy),
        (2_501, "187510.125", "0.125", Side::Buy),
        (2_549, "187510.375", "0.375", Side::Sell),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (offset, price, quantity, side))| Aggression {
        agg_id: index as u64 + 1,
        timestamp_ms: epoch + offset,
        price: dec(price),
        quantity: dec(quantity),
        side,
        generation: None,
    })
    .collect();
    cluster_aggressions(&trades, &[], native_grouping(config), 0)
}

fn check_placement(epoch: i64, mode: Option<bool>, lane: bool, now_offset: i64) {
    let config = config();
    let closed = [bar(epoch, epoch + 999), bar(epoch + 1_000, epoch + 1_999)];
    let partial = bar(epoch + 2_000, epoch + 2_549);
    let timeline = BarTimeline::from_bars(
        37,
        &closed,
        Some(&partial),
        lane.then_some(crate::LiveEdge {
            now_ms: epoch + now_offset,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage();
    let dots = mode.map(|tape_only| {
        VolumeDots::resolve(
            &DotZoom {
                tape_only,
                tape_window_ms: if tape_only { 100 } else { 250 },
                tape_level_ticks: if tape_only { 1 } else { 5 },
                candle_level_ticks: 10,
                lane_bars: Vec::new(),
            },
            &closed,
            Some(&partial),
        )
    });
    // One off-axis price verifies that live dots remain available to clip,
    // while ordinary prints and candle dots retain their existing filtering.
    let prices = PriceWindow::new(dec("187499"), dec("187515")).unwrap();
    let clusters = raw(epoch, &config);
    let fold = |live| {
        dots.as_ref().map_or_else(
            || clusters.clone(),
            |dots| {
                fold_dots(
                    clusters.clone(),
                    live,
                    dots,
                    native_grouping(&config),
                    DotHorizon {
                        recorded_from_ms: None,
                        evicted_through_ms: None,
                    },
                )
            },
        )
    };
    let tape = fold(true);
    let slot = fold(false);
    let print_reference = dec("0.625");
    let summary_reference = dec("12.75");
    let expected: Vec<_> = tape
        .iter()
        .cloned()
        .map(|cluster| (cluster, true, print_reference))
        .chain(
            slot.iter()
                .cloned()
                .map(|cluster| (cluster, false, summary_reference)),
        )
        .filter_map(|(cluster, live, reference)| {
            reference_place(cluster, live, &timeline, prices, reference, dots.as_ref())
        })
        .collect();
    let actual = tier_primitives(
        TierClusters {
            tape,
            slot,
            tape_facts: None,
        },
        &timeline,
        prices,
        print_reference,
        summary_reference,
        dots.as_ref(),
    );
    if mode == Some(true) {
        assert_native_screen_contract(&actual, &expected);
    } else {
        assert_eq!(actual, expected, "ordinary and mixed placement stay exact");
    }
    assert!(
        !actual.is_empty(),
        "candle placements remain available without a live lane"
    );
    if mode == Some(true) && lane && now_offset == 2_599 {
        let open = actual
            .iter()
            .find(|mark| mark.live && mark.first_timestamp_ms == epoch + 2_501)
            .unwrap();
        assert_eq!(open.x, 1.0, "the open native window rides NOW");
        assert_eq!(
            open.timestamp_quantity,
            Decimal::from(epoch) * dec("0.5") + dec("1268.5")
        );
        let closed = actual
            .iter()
            .find(|mark| mark.live && mark.first_timestamp_ms == epoch + 1_101)
            .unwrap();
        assert_eq!(closed.quantity, dec("0.5"));
        assert_eq!(closed.price, dec("187505.3125"));
        assert!(
            closed.x < open.x,
            "fractional closed execution time stays behind NOW"
        );
    }
}

#[test]
fn native_tape_geometry_preserves_epoch_fractional_means_and_the_forming_window() {
    for epoch in [-10_000, 0, 1_790_102_415_000] {
        for now in [2_599, 2_600, 17_600] {
            check_placement(epoch, Some(true), true, now);
        }
        check_placement(epoch, Some(true), false, 2_599);
    }
}

#[test]
fn hoisted_tape_geometry_preserves_mixed_dots_and_ordinary_slot_placement() {
    for mode in [None, Some(false)] {
        for lane in [false, true] {
            check_placement(1_790_102_415_000, mode, lane, 2_599);
        }
    }
}
