use std::str::FromStr as _;
fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
use super::*;
/// The other half of the same rule: a range that does *not* reach the live
/// edge is untouched by a close, and a range whose left edge moved is a
/// different range and does start again.
#[test]
fn only_a_grown_right_edge_carries_a_fold_over() {
    let a = FrvpCacheKey {
        start_slot: 0,
        end_slot: 3,
        group: dec("1"),
        series_revision: 1,
        include_partial: true,
        partial_snapshot: 7,
        value_area_pct: 70,
        blocked: false,
        side_inferred: false,
        approximate: false,
        partly_covered: false,
    };

    let grown = FrvpCacheKey {
        end_slot: 4,
        partial_snapshot: 9,
        ..a
    };
    assert!(a.grown_right(&grown), "a later right edge is an append");
    assert!(!a.same_fold(&grown));

    assert!(
        !a.grown_right(&FrvpCacheKey { end_slot: 2, ..a }),
        "a right edge dragged *back* drops bars, so the fold is not reusable",
    );
    assert!(
        !a.grown_right(&FrvpCacheKey {
            start_slot: 1,
            end_slot: 4,
            ..a
        }),
        "a moved left edge is a different range",
    );
    assert!(
        !a.grown_right(&FrvpCacheKey {
            end_slot: 4,
            group: dec("5"),
            ..a
        }),
        "a regrouped ladder has to be re-folded",
    );
    assert!(
        !a.grown_right(&FrvpCacheKey {
            end_slot: 4,
            series_revision: 2,
            ..a
        }),
        "a re-cut series has to be re-folded",
    );
    assert!(!a.grown_right(&a), "an unchanged key is not a growth");
}

/// The fold reads an anchor the way the paint writes one: an integer bar
/// coordinate is a candle's *centre*. A range dragged from one candle's
/// centre to another's therefore folds both end candles — the rectangle
/// on screen and the bars behind the histogram are the same bars.
#[test]
fn covered_slots_folds_exactly_the_drawn_rectangle() {
    // Centre-to-centre over candles 100..=184 is 85 candles, both ends
    // included. Reading a centre as `slot + 0.5` drops the candle under
    // the right edge and pulls in half of the one left of the box.
    assert_eq!(covered_slots(100.0, 184.0, Some(300)), Some((100, 184)));
    // A coordinate belongs to the candle it lands *on*: candle N owns
    // `[N - 0.5, N + 0.5)`, so 99.5 is candle 100 and 184.4 is candle 184.
    assert_eq!(covered_slots(99.5, 184.4, Some(300)), Some((100, 184)));
}

/// The tool snaps no anchor, so every real drag ends mid-candle. Taking
/// the first and last centres strictly *inside* the span (`ceil`/`floor`)
/// stepped past both endpoints and folded 83 bars for an 85-candle drag —
/// and gave a different count each time the same gesture was repeated.
#[test]
fn covered_slots_folds_the_candles_a_mid_candle_drag_lands_on() {
    // Pressed inside candle 100, released inside candle 184.
    assert_eq!(covered_slots(100.4, 183.6, Some(300)), Some((100, 184)));
    assert_eq!(covered_slots(99.7, 184.3, Some(300)), Some((100, 184)));
    // The same gesture, jittered by a sub-pixel, folds the same bars.
    for (lo, hi) in [(99.6, 183.51), (100.49, 184.49), (99.51, 184.2)] {
        assert_eq!(
            covered_slots(lo, hi, Some(300)),
            Some((100, 184)),
            "drag {lo}..{hi} is the same 85 candles"
        );
    }
}

/// Both anchors on one candle fold that candle. Dropping it left the
/// trader with a rectangle, a "no tape in range" label and a bar that
/// visibly traded.
#[test]
fn covered_slots_folds_the_single_candle_under_a_dot() {
    assert_eq!(covered_slots(5.0, 5.0, Some(300)), Some((5, 5)));
}

/// A range over no candles has no profile. Clamping it onto the nearest
/// slot drew that slot's histogram — real-looking data for a rectangle
/// the trader put nowhere near it.
#[test]
fn covered_slots_rejects_a_range_that_reaches_no_candle() {
    assert_eq!(covered_slots(-50.0, -40.0, Some(300)), None, "left of data");
    assert_eq!(
        covered_slots(400.0, 450.0, Some(300)),
        None,
        "past the tape"
    );
    // A short drag from inside candle 10 to inside candle 11 is those two
    // candles — there is no "between candles" to land in.
    assert_eq!(covered_slots(10.2, 10.8, Some(300)), Some((10, 11)));
    assert_eq!(covered_slots(10.2, 10.4, Some(300)), Some((10, 10)));
    // Left of candle 0 entirely: both ends round to -1.
    assert_eq!(covered_slots(-1.4, -0.6, Some(300)), None, "left of slot 0");
    // The edges still clamp *into* the data when the span overlaps it.
    assert_eq!(covered_slots(-50.0, 2.0, Some(300)), Some((0, 2)));
    assert_eq!(covered_slots(298.0, 450.0, Some(300)), Some((298, 300)));
    assert_eq!(covered_slots(0.0, 10.0, None), None, "no slots exist yet");
}

/// The three levels are one reading, made once. Issue #157: the POC line was
/// drawn at the centre of its row while the POC plate printed the row's low
/// edge, so the chart named a price it had not marked. POC is the centre;
/// VAH tops its row and VAL bottoms its, so a bound hugs the area it bounds.
#[test]
fn level_prices_read_poc_at_the_centre_of_its_row() {
    let mut footprint = quantick_engine::FootprintBuilder::new(dec("10"), DEFAULT_LEVEL_CAP);
    for (price, quantity) in [("95010", "10"), ("95020", "8"), ("95000", "7")] {
        footprint.push(&quantick_engine::Trade {
            agg_id: 1,
            timestamp_ms: 0,
            price: dec(price),
            quantity: dec(quantity),
            side: quantick_engine::Side::Buy,
        });
    }
    let ladder = footprint.close().expect("the fixture traded");
    let profile = VolumeProfile::merge(vec![&ladder], DEFAULT_LEVEL_CAP).expect("one ladder folds");
    let levels = LevelPrices::of(
        &profile,
        ValueArea {
            poc: 9501,
            vah: 9502,
            val: 9500,
        },
    );
    assert_eq!(
        levels.poc,
        dec("95015"),
        "the POC row's centre, not its low edge"
    );
    assert_eq!(levels.vah, dec("95030"), "the top edge of the VAH row");
    assert_eq!(levels.val, dec("95000"), "the bottom edge of the VAL row");
}
