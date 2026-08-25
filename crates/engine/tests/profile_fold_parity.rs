//! A candle folded as a *range* must equal the same candle folded as rows.
//!
//! [`ProfileFold`] exists because materialising one approximated ladder per
//! venue candle costs more than a chart has to spend: read as an
//! [`ApproxSpread`](quantick_engine::ApproxSpread), a candle is three map
//! touches instead of up to two thousand rows. Speed bought with a different
//! answer is not speed, so this file pins the two readings together — same
//! grouping, same rows, same POC and value area — over the cases that make
//! them diverge if anything is wrong.
//!
//! The oracle is deliberately the *other* implementation:
//! `BarFootprint::approximated` writes the spread out row by row, and
//! [`VolumeProfile::merge`] folds those rows. (The merge itself is written on
//! top of `ProfileFold`, so it is the spread — not the accumulator — that
//! these tests hold to account. The accumulator's own contract is proved by
//! the engine's unit tests, which the merge runs through unchanged.)
//!
//! Every fixture is deterministic and written out here rather than generated
//! at random: a parity failure must be reproducible from the file alone.

use std::str::FromStr as _;

use quantick_engine::{
    Bar, BarFootprint, DEFAULT_LEVEL_CAP, FootprintBuilder, ProfileFold, Side, Trade, VolumeProfile,
};
use rust_decimal::Decimal;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

/// A venue candle: no tape behind it, only the summary a kline reports.
fn candle(i: i64, low: &str, high: &str, close: &str, buy: &str, sell: &str, trades: u64) -> Bar {
    Bar {
        open_time: 1_700_000_000_000 + i * 60_000,
        close_time: 1_700_000_000_000 + (i + 1) * 60_000 - 1,
        open: dec(low),
        high: dec(high),
        low: dec(low),
        close: dec(close),
        buy_volume: dec(buy),
        sell_volume: dec(sell),
        trade_count: trades,
    }
}

/// A real ladder, folded from trades the way the tape builds one.
fn ladder(group: Decimal, prints: &[(&str, &str, Side)]) -> BarFootprint {
    let mut builder = FootprintBuilder::new(group, DEFAULT_LEVEL_CAP);
    for (i, (price, quantity, side)) in prints.iter().enumerate() {
        builder.push(&Trade {
            agg_id: i as u64,
            timestamp_ms: 1_700_000_000_000 + i as i64,
            price: dec(price),
            quantity: dec(quantity),
            side: *side,
        });
    }
    builder.close().expect("the prints built a ladder")
}

/// Assert the two folds returned the same profile — not "close enough": the
/// same grouping, the same honesty flag, the same rows in the same order, and
/// the same reads on top of them.
#[track_caller]
fn assert_same(label: &str, merged: Option<&VolumeProfile>, folded: Option<&VolumeProfile>) {
    match (merged, folded) {
        (None, None) => {}
        (Some(merged), Some(folded)) => {
            assert_eq!(merged.group(), folded.group(), "{label}: grouping");
            assert_eq!(
                merged.is_aggregated(),
                folded.is_aggregated(),
                "{label}: aggregated flag"
            );
            assert_eq!(
                merged.levels().len(),
                folded.levels().len(),
                "{label}: row count"
            );
            assert_eq!(merged.levels(), folded.levels(), "{label}: rows");
            assert_eq!(merged.poc(), folded.poc(), "{label}: poc");
            assert_eq!(
                merged.total_volume(),
                folded.total_volume(),
                "{label}: total volume"
            );
            assert_eq!(
                merged.total_delta(),
                folded.total_delta(),
                "{label}: total delta"
            );
            for pct in ["0.5", "0.7", "0.95", "1"] {
                assert_eq!(
                    merged.value_area(dec(pct)),
                    folded.value_area(dec(pct)),
                    "{label}: value area at {pct}"
                );
            }
        }
        (merged, folded) => panic!(
            "{label}: one fold answered and the other did not ({}, {})",
            merged.is_some(),
            folded.is_some()
        ),
    }
}

/// Fold `candles` and `ladders` both ways and compare. The merge sees the
/// candles as materialised approximated ladders — the shape the fold replaces.
#[track_caller]
fn assert_parity(label: &str, group: Decimal, candles: &[Bar], ladders: &[BarFootprint]) {
    let approximated: Vec<BarFootprint> = candles
        .iter()
        .filter_map(|bar| BarFootprint::approximated(bar, group, DEFAULT_LEVEL_CAP))
        .collect();
    let merged = VolumeProfile::merge(approximated.iter().chain(ladders.iter()), DEFAULT_LEVEL_CAP);

    let mut fold = ProfileFold::new(group, DEFAULT_LEVEL_CAP);
    for bar in candles {
        fold.push_candle(bar);
    }
    for ladder in ladders {
        fold.push_ladder(ladder);
    }
    assert_same(label, merged.as_ref(), fold.profile().as_ref());
}

#[test]
fn one_candle_spread_matches_its_materialised_ladder() {
    let candles = [candle(0, "100", "104", "102", "0.6", "0.4", 10)];
    assert_parity("single candle", dec("1"), &candles, &[]);
}

#[test]
fn a_candle_narrower_than_one_bucket_still_matches() {
    // Low and high inside the same bucket: one row, every remainder on it.
    let candles = [candle(0, "100.1", "100.4", "100.2", "3", "2", 7)];
    assert_parity("sub-bucket candle", dec("1"), &candles, &[]);
}

#[test]
fn overlapping_candles_sum_the_way_the_merge_sums_them() {
    let candles = [
        candle(0, "100", "110", "105", "1.5", "2.5", 20),
        candle(1, "104", "118", "112", "3.25", "0.75", 33),
        candle(2, "96", "106", "97", "0.1", "0.9", 4),
    ];
    assert_parity("overlapping candles", dec("1"), &candles, &[]);
}

#[test]
fn candles_that_share_no_price_leave_the_gap_unprinted() {
    // Two clusters far apart: the rows between them were never traded and
    // must not appear — the fold prints coverage, not the span it crosses.
    let candles = [
        candle(0, "100", "102", "101", "1", "1", 5),
        candle(1, "980", "984", "982", "2", "3", 9),
    ];
    assert_parity("disjoint clusters", dec("1"), &candles, &[]);
}

#[test]
fn a_candle_wider_than_the_cap_coarsens_the_same_way() {
    // A 6000-wide candle at group 1 needs three doublings to fit 2000 rows.
    let candles = [candle(0, "10000", "16000", "15500", "12", "8", 400)];
    assert_parity("cap-forced doublings", dec("1"), &candles, &[]);
}

#[test]
fn candles_at_different_doublings_fold_down_to_the_coarsest() {
    let candles = [
        candle(0, "10000", "16000", "15500", "12", "8", 400),
        candle(1, "12000", "12010", "12004", "1", "1", 6),
        candle(2, "11000", "14000", "13000", "5", "4", 60),
    ];
    assert_parity("mixed doublings", dec("1"), &candles, &[]);
}

#[test]
fn tape_ladders_and_candles_fold_into_one_profile() {
    let candles = [
        candle(0, "100", "108", "104", "1.5", "2.5", 20),
        candle(1, "102", "112", "110", "0.25", "0.75", 8),
    ];
    let ladders = [
        ladder(
            dec("1"),
            &[
                ("104", "2", Side::Buy),
                ("105", "1", Side::Sell),
                ("104", "0.5", Side::Sell),
            ],
        ),
        ladder(
            dec("1"),
            &[("119", "3", Side::Buy), ("120", "4", Side::Sell)],
        ),
    ];
    assert_parity("mixed tape and candles", dec("1"), &candles, &ladders);
}

#[test]
fn a_sub_unit_grouping_matches_too() {
    // The grouping a liquid crypto pair actually charts at.
    let candles = [
        candle(0, "36000.05", "36004.95", "36002.25", "1.234", "2.345", 120),
        candle(1, "36001.15", "36009.85", "36008.55", "0.05", "0.07", 3),
    ];
    assert_parity("0.01 grouping", dec("0.01"), &candles, &[]);
}

#[test]
fn a_range_too_wide_for_the_cap_coarsens_after_the_fold() {
    // Each candle fits the cap on its own; together they span far more rows
    // than the cap allows, so the *profile* coarsens. Both folds must land on
    // the same grouping — the one that fits, not one doubling more or less.
    let candles: Vec<Bar> = (0..40)
        .map(|i| {
            let low = 1000 + i * 100;
            candle(
                i,
                &low.to_string(),
                &(low + 150).to_string(),
                &(low + 75).to_string(),
                "1.5",
                "2.5",
                17,
            )
        })
        .collect();
    assert_parity("cap-forced profile coarsening", dec("1"), &candles, &[]);
}

#[test]
fn a_candle_that_traded_nothing_is_skipped_by_both() {
    let candles = [
        candle(0, "100", "104", "102", "0", "0", 0),
        candle(1, "100", "104", "102", "1", "1", 4),
    ];
    assert_parity("zero-volume candle", dec("1"), &candles, &[]);
}

#[test]
fn an_empty_fold_has_no_profile_either_way() {
    assert_parity("nothing at all", dec("1"), &[], &[]);
    assert!(
        ProfileFold::new(dec("1"), DEFAULT_LEVEL_CAP)
            .profile()
            .is_none()
    );
}

#[test]
fn a_ladder_on_a_foreign_grouping_is_refused_by_both() {
    let foreign = ladder(dec("2"), &[("100", "1", Side::Buy)]);
    let native = ladder(dec("1"), &[("100", "1", Side::Buy)]);
    // The merge refuses a set whose base groupings disagree.
    assert!(VolumeProfile::merge([&native, &foreign], DEFAULT_LEVEL_CAP).is_none());

    let mut fold = ProfileFold::new(dec("1"), DEFAULT_LEVEL_CAP);
    assert!(fold.push_ladder(&native));
    assert!(!fold.push_ladder(&foreign), "the foreign ladder is refused");
    assert!(
        fold.profile().is_none(),
        "a refused ladder poisons the fold rather than dropping a bar silently"
    );
}

#[test]
fn the_batch_boundary_is_not_a_seam() {
    // More candles than one pending batch holds, so the fold collapses
    // mid-range. Where it collapsed must not be visible in the result.
    let candles: Vec<Bar> = (0..(ProfileFold::SPREAD_BATCH as i64 * 2 + 7))
        .map(|i| {
            let low = 500 + (i % 37);
            candle(
                i,
                &low.to_string(),
                &(low + 11).to_string(),
                &(low + 5).to_string(),
                "0.125",
                "0.375",
                9,
            )
        })
        .collect();
    assert_parity("across two batch collapses", dec("1"), &candles, &[]);
}

#[test]
fn interleaving_candles_and_ladders_changes_nothing() {
    // The merge sums; sums do not care about arrival order, and neither may
    // the fold — a chart folds a range in whatever order it walks the slots.
    let candles = [
        candle(0, "100", "108", "104", "1.5", "2.5", 20),
        candle(1, "102", "112", "110", "0.25", "0.75", 8),
    ];
    let ladders = [
        ladder(dec("1"), &[("104", "2", Side::Buy)]),
        ladder(dec("1"), &[("109", "3", Side::Sell)]),
    ];

    let mut straight = ProfileFold::new(dec("1"), DEFAULT_LEVEL_CAP);
    for bar in &candles {
        straight.push_candle(bar);
    }
    for rung in &ladders {
        straight.push_ladder(rung);
    }

    let mut interleaved = ProfileFold::new(dec("1"), DEFAULT_LEVEL_CAP);
    interleaved.push_ladder(&ladders[0]);
    interleaved.push_candle(&candles[1]);
    interleaved.push_ladder(&ladders[1]);
    interleaved.push_candle(&candles[0]);

    assert_same(
        "arrival order",
        straight.profile().as_ref(),
        interleaved.profile().as_ref(),
    );
}

/// A partial read is a *prefix* of the fold, not a preview of the end: what it
/// shows is exactly the profile of the bars folded so far. That is what makes
/// painting one honest.
#[test]
fn a_partial_read_is_the_profile_of_what_was_folded_so_far() {
    let candles = [
        candle(0, "100", "108", "104", "1.5", "2.5", 20),
        candle(1, "102", "112", "110", "0.25", "0.75", 8),
        candle(2, "96", "106", "97", "0.1", "0.9", 4),
    ];
    let mut fold = ProfileFold::new(dec("1"), DEFAULT_LEVEL_CAP);
    for (folded, bar) in candles.iter().enumerate() {
        fold.push_candle(bar);
        let prefix: Vec<BarFootprint> = candles[..=folded]
            .iter()
            .filter_map(|bar| BarFootprint::approximated(bar, dec("1"), DEFAULT_LEVEL_CAP))
            .collect();
        assert_same(
            &format!("after {} candles", folded + 1),
            VolumeProfile::merge(prefix.iter(), DEFAULT_LEVEL_CAP).as_ref(),
            fold.profile().as_ref(),
        );
    }
}

/// The bound that keeps the app on its feet: a fold of a long venue history
/// holds rows for the *profile*, never rows per bar. Twenty-five thousand
/// candles at a chart grouping used to materialise tens of millions of ladder
/// entries before the first pixel; here the whole fold stays under one capped
/// ladder plus one pending batch, whatever the range's length.
#[test]
fn a_long_range_never_holds_more_than_a_capped_ladder_and_a_batch() {
    let bound = DEFAULT_LEVEL_CAP + ProfileFold::SPREAD_BATCH;
    let mut fold = ProfileFold::new(dec("0.1"), DEFAULT_LEVEL_CAP);
    for i in 0..25_000i64 {
        let low = 36_000 + (i % 500);
        fold.push_candle(&candle(
            i,
            &low.to_string(),
            &(low + 400).to_string(),
            &(low + 200).to_string(),
            "1.5",
            "2.5",
            140,
        ));
        assert!(
            fold.rows_held() <= bound,
            "after {} candles the fold held {} rows, past the {bound}-row bound",
            i + 1,
            fold.rows_held()
        );
    }
    assert_eq!(fold.inputs(), 25_000);
    let profile = fold.profile().expect("25k candles fold to a profile");
    assert!(
        profile.levels().len() <= DEFAULT_LEVEL_CAP,
        "the profile itself answers to the level cap"
    );
    assert!(
        profile.total_volume() > Decimal::ZERO,
        "the fold conserved the candles' volume"
    );
}
