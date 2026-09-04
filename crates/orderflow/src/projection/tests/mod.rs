// The `projection.rs` unit tests, moved out of the file so a session
// opening the projection to change one lane no longer reads 3,040 lines of
// tests it did not ask for.
//
// They stay a child module of `crate::projection` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.

use super::*;
use crate::config::{BubbleSizeReference, BubbleStyle, DisplayGrouping, LiveLaneStyle};
use crate::history::LiquidityHistory;
use quantick_engine::{Bar, Side, Trade};
use quantick_orderbook::BookSide;
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use std::str::FromStr as _;

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

/// The live edge as the chart supplies it while the newest bar is on
/// screen: the lane shows the recent bars' typical duration, unzoomed.
fn live(now_ms: i64, closed: &[Bar]) -> Option<crate::LiveEdge> {
    Some(crate::LiveEdge {
        now_ms,
        window_ms: crate::reserved_span_ms(closed),
        reference_ms: crate::reserved_span_ms(closed),
        on_newest_bar: true,
    })
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
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        ..HeatmapConfig::default()
    }
}

/// The reduction cap is a budget for the whole frame too. Each half caps
/// itself where it is built, so without a cap over the join a frame could
/// draw twice the markers the budget allows — one full cap per half.
#[test]
fn the_reduction_cap_is_one_budget_for_both_halves() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        max_visible_cells: 2,
        min_unattributed_reduction: 0.0,
        min_unattributed_pull_share: 0.0,
        ..HeatmapConfig::default()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    // Two pulls each side of the seam at 2 000 ms.
    for (update_id, timestamp_ms, bids, asks) in [
        (11_u64, 300_i64, vec![level("100", "1")], vec![]),
        (12, 900, vec![level("99", "0.5")], vec![]),
        (13, 2_300, vec![], vec![level("101", "1")]),
        (14, 2_900, vec![], vec![level("102", "1")]),
    ] {
        history
            .apply_delta(
                timestamp_ms,
                &BookDelta::new(update_id, update_id, bids, asks),
            )
            .unwrap();
    }
    history
        .apply_delta(3_900, &BookDelta::new(15, 15, vec![], vec![]))
        .unwrap();

    let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );

    assert_eq!(
        projection.liquidity_events.len(),
        2,
        "a cap of two is two markers for the frame, not two per half"
    );
    assert_eq!(projection.dropped_liquidity_events, 2);
}

/// The primitive budget is *split* between the panes, not shared by them.
///
/// One shared budget was the bug this mission exists to fix: the candles'
/// marks each carry a bar and the tape's each carry a print, so ranking
/// them against each other by quantity emptied the tape whenever the
/// candles had more to draw. Each pane now folds against its own share,
/// and — the part that matters to a trader — folding conserves, so the six
/// contracts that traded are still six contracts of ink.
#[test]
fn the_budget_is_split_between_the_panes() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_cluster_ms: 0,
        bubble_dust_merge_ms: 0,
        max_aggression_primitives: 3,
        ..config()
    });
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    // Three prints each side of the seam at 2 000 ms, interleaved by size so
    // the winners cannot be picked by half.
    for (agg_id, timestamp_ms, quantity) in [
        (1_u64, 100_i64, "10"),
        (2, 300, "1"),
        (3, 1_100, "9"),
        (4, 2_100, "2"),
        (5, 3_100, "8"),
        (6, 3_300, "3"),
    ] {
        history.record_aggression(&Trade {
            agg_id,
            timestamp_ms,
            price: dec("101"),
            quantity: dec(quantity),
            side: Side::Buy,
        });
    }
    let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );

    let drawn: Decimal = projection
        .aggressions
        .iter()
        .map(|mark| mark.quantity)
        .sum();
    assert_eq!(
        drawn,
        dec("33"),
        "10 + 1 + 9 + 2 + 8 + 3 traded, and all of it is still on the canvas"
    );
    assert!(
        projection.aggressions.iter().any(|mark| mark.live),
        "the tape keeps its own share however loud the candles are"
    );
    assert!(
        projection.aggressions.iter().any(|mark| !mark.live),
        "and so do the candles"
    );
    // No fold is even possible here, and that is the point: six prints
    // spread one per bar leave every (pane, side, bar) group holding a
    // single mark, and a fold may not cross any of those boundaries. The
    // frame carries more marks than the budget asked for rather than
    // misattribute volume — and it still carries every contract.
    assert_eq!(
        projection.folded_aggressions, 0,
        "nothing could be folded without crossing a bar, so nothing was"
    );
}

/// The regional fold compresses the settled history and never the tape:
/// prints in the live lane keep their own marks — a scalper reads the
/// forming edge print by print — while the same prices behind the seam
/// become one bubble per region carrying the exact summed quantity.
#[test]
fn regions_fold_the_settled_half_and_leave_the_tape_alone() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_cluster_ms: 0,
        bubble_dust_merge_ms: 0,
        bubble_region_rows: 3,
        bubble_region_ms: 5_000,
        ..config()
    });
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    // Two prints in the settled half share the three-row region [99, 102);
    // two prints on the tape land in that same region.
    for (agg_id, timestamp_ms, price) in [
        (1_u64, 100_i64, "100"),
        (2, 300, "101"),
        (3, 3_100, "100"),
        (4, 3_300, "101"),
    ] {
        history.record_aggression(&Trade {
            agg_id,
            timestamp_ms,
            price: dec(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
    }
    let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );

    let settled: Vec<_> = projection
        .aggressions
        .iter()
        .filter(|mark| !mark.live)
        .collect();
    assert_eq!(settled.len(), 1, "one region, one settled mark");
    assert_eq!(settled[0].quantity, dec("2"));
    assert_eq!(settled[0].trade_count, 2);
    let tape: Vec<_> = projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert_eq!(tape.len(), 2, "the tape stays print by print");
    assert!(tape.iter().all(|mark| mark.quantity == Decimal::ONE));
}

/// Two prints in one region but adjacent bars, inside one region window,
/// with the candle summary on: the WIN's tick bars close in seconds, and a
/// fold that crossed the boundary would credit the whole summed quantity
/// to the bar its midpoint lands in — a pie claiming volume the neighbour
/// traded. One mark per bar, each with its own bar's quantity.
#[test]
fn regions_never_move_volume_across_a_bar_boundary() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_cluster_ms: 0,
        bubble_dust_merge_ms: 0,
        bubble_region_rows: 3,
        bubble_region_ms: 5_000,
        bubble_candle_summary: true,
        ..config()
    });
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    for (agg_id, timestamp_ms, price, quantity) in
        [(1_u64, 800_i64, "100", "1"), (2, 1_200, "102", "2")]
    {
        history.record_aggression(&Trade {
            agg_id,
            timestamp_ms,
            price: dec(price),
            quantity: dec(quantity),
            side: Side::Buy,
        });
    }
    let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );

    let mut settled: Vec<_> = projection
        .aggressions
        .iter()
        .filter(|mark| !mark.live)
        .collect();
    settled.sort_by_key(|mark| mark.first_timestamp_ms);
    assert_eq!(settled.len(), 2, "one mark per bar, never one for both");
    assert_eq!(settled[0].quantity, dec("1"));
    assert_eq!(settled[1].quantity, dec("2"));
}

/// The chart is built in two halves that meet at a bar's open time. This is
/// the seam: with the summary on, a bar whose prints landed on both sides of
/// it must still draw *one* pie carrying all of them, never one pie per half
/// — and the lane's raw prints must be neither dropped nor doubled.
#[test]
fn the_seam_between_the_halves_neither_splits_a_bar_nor_doubles_a_print() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_candle_summary: true,
        bubble_cluster_ms: 500,
        bubble_dust_merge_ms: 0,
        ..config()
    });
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    // Four bars of 1 000 ms, one print every 100 ms at one price.
    for step in 0..40_i64 {
        history.record_aggression(&Trade {
            agg_id: step as u64,
            timestamp_ms: step * 100 + 50,
            price: dec("101"),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
    }
    let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    // Walk the live edge across a whole bar, so the seam lands at every
    // offset inside it — including exactly on a bar boundary.
    for now_ms in [3_400_i64, 3_500, 3_600, 3_999, 4_000] {
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::LiveEdge {
                now_ms,
                // A lane wide enough to cover more than one bar, which is
                // what makes the seam fall inside a bar rather than on it.
                window_ms: 1_500,
                reference_ms: 1_500,
                on_newest_bar: true,
            }),
        );
        let projection = project(&history, &timeline, prices);

        let pies: Vec<_> = projection
            .aggressions
            .iter()
            .filter(|mark| !mark.live)
            .collect();
        // A print is carried by exactly one mark. Split the seam wrong and
        // this drops (a print landing in neither half) or doubles (a bar
        // summarized once per half).
        let summarized: Decimal = pies.iter().map(|pie| pie.quantity).sum();
        assert_eq!(
            summarized,
            Decimal::from(
                history
                    .aggressions()
                    .filter(|print| timeline.locate(print.timestamp_ms).is_some())
                    .count()
            ),
            "now_ms={now_ms}: the pies must carry every print exactly once"
        );

        // And they carry it in one place. A summary is drawn in its bar's
        // slot, so the slot it lands in names the bar it is about: two
        // summaries in one slot at one price are one bar counted twice,
        // which is exactly what a seam left mid-bar would produce.
        let regions = timeline.region_count() as f64;
        let mut slots: Vec<(usize, Decimal)> = pies
            .iter()
            .map(|pie| ((pie.x * regions) as usize, pie.price_bucket))
            .collect();
        slots.sort_unstable();
        let mut once = slots.clone();
        once.dedup();
        assert_eq!(
            slots, once,
            "now_ms={now_ms}: a bar was summarized twice, once per half"
        );
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

/// Four prints an order of magnitude apart, so the size ladder is visible.
/// Both readability passes are off — temporal clustering and the dust
/// merge — so these stay four distinct bubbles and the test measures the
/// size mapping alone.
fn history_with_a_size_ladder(config: HeatmapConfig) -> LiquidityHistory {
    size_ladder(config, 0)
}

/// The ladder above with the dust merge armed at `dust_merge_ms`.
fn size_ladder(config: HeatmapConfig, dust_merge_ms: i64) -> LiquidityHistory {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_cluster_ms: 0,
        bubble_dust_merge_ms: dust_merge_ms,
        ..config
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    for (id, quantity) in [(1_u64, "1"), (2, "10"), (3, "50"), (4, "200")] {
        history.record_aggression(&Trade {
            agg_id: id,
            // Spread across time so clustering cannot merge them.
            timestamp_ms: 200 + (id as i64) * 100,
            price: dec("101"),
            quantity: dec(quantity),
            side: Side::Buy,
        });
    }
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    history
}

fn ladder_sizes(config: HeatmapConfig) -> Vec<(Decimal, f32)> {
    let history = history_with_a_size_ladder(config);
    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    let mut sizes: Vec<(Decimal, f32)> = projection
        .aggressions
        .iter()
        .map(|aggression| (aggression.quantity, aggression.size))
        .collect();
    sizes.sort_by_key(|pair| pair.0);
    sizes
}

#[test]
fn the_dust_merge_folds_unreadable_prints_without_losing_quantity() {
    // The same ladder with the dust merge armed. Against a reference of
    // 200 the two smallest prints (1 and 10) fall below the radius where
    // the renderer stops dressing a bubble, so they arrive as one mark
    // carrying both — while the two readable prints are left alone.
    let history = size_ladder(
        HeatmapConfig {
            bubbles: BubbleStyle {
                size_reference: BubbleSizeReference::VisibleMax,
                ..BubbleStyle::default()
            },
            ..config()
        },
        5_000,
    );
    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );

    let mut quantities: Vec<Decimal> = projection
        .aggressions
        .iter()
        .map(|aggression| aggression.quantity)
        .collect();
    quantities.sort();
    assert_eq!(quantities, [dec("11"), dec("50"), dec("200")]);
    assert_eq!(
        quantities.iter().sum::<Decimal>(),
        dec("261"),
        "merging is a drawing decision and never loses quantity"
    );
    let folded = projection
        .aggressions
        .iter()
        .find(|aggression| aggression.quantity == dec("11"))
        .expect("the two dust prints fold into one bubble");
    assert_eq!(folded.trade_count, 2);
    assert_eq!(folded.agg_ids, [1, 2]);
}

#[test]
fn big_prints_read_as_big_bubbles_under_every_size_reference() {
    // The size factor drives radius through area, so it must grow strictly
    // with quantity and reach 1.0 (the maximum radius) for the reference
    // print. This is the "can I see the large trades?" guarantee.
    let sizes = ladder_sizes(HeatmapConfig {
        bubbles: BubbleStyle {
            size_reference: BubbleSizeReference::VisibleMax,
            ..BubbleStyle::default()
        },
        ..config()
    });
    assert_eq!(sizes.len(), 4);
    for pair in sizes.windows(2) {
        assert!(
            pair[1].1 > pair[0].1,
            "size must grow with quantity: {pair:?}"
        );
    }
    assert_eq!(
        sizes.last().unwrap().1,
        1.0,
        "the biggest print is full size"
    );
    // Area proportionality: a quarter of the reference quantity is half the
    // size factor, i.e. a quarter of the drawn area.
    let fifty = sizes.iter().find(|(qty, _)| *qty == dec("50")).unwrap().1;
    assert!(
        (fifty - 0.5).abs() < 1e-6,
        "50/200 must map to 0.5, got {fifty}"
    );

    // A fixed reference pins the scale: the same print keeps the same size
    // no matter what else is visible, and anything above it saturates.
    let fixed = ladder_sizes(HeatmapConfig {
        bubbles: BubbleStyle {
            size_reference: BubbleSizeReference::Fixed,
            size_reference_quantity: 50.0,
            ..BubbleStyle::default()
        },
        ..config()
    });
    assert_eq!(
        fixed.iter().find(|(qty, _)| *qty == dec("50")).unwrap().1,
        1.0
    );
    assert_eq!(
        fixed.last().unwrap().1,
        1.0,
        "a print above the fixed reference clamps instead of overflowing"
    );
    let small = fixed.first().unwrap().1;
    assert!(small > 0.0 && small < 0.2, "1/50 stays a dot, got {small}");
}

#[test]
fn hiding_small_prints_is_display_only_and_keeps_the_scale() {
    let bubbles = BubbleStyle {
        size_reference: BubbleSizeReference::VisibleMax,
        min_quantity: 20.0,
        ..BubbleStyle::default()
    };
    let sizes = ladder_sizes(HeatmapConfig {
        bubbles,
        ..config()
    });
    let quantities: Vec<Decimal> = sizes.iter().map(|(qty, _)| *qty).collect();
    assert_eq!(quantities, vec![dec("50"), dec("200")]);
    // The reference still comes from every visible print, so the surviving
    // bubbles keep the size they had before the floor was raised.
    assert!((sizes[0].1 - 0.5).abs() < 1e-6);
    assert_eq!(sizes[1].1, 1.0);
}

#[test]
fn a_hidden_print_still_explains_the_reduction_it_caused() {
    // The floor is applied after association, so a small print that ate a
    // wall keeps the reduction marked as aggression-aligned even though the
    // bubble itself is not drawn. Hiding noise must never rewrite evidence.
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubbles: BubbleStyle {
            min_quantity: 100.0,
            ..BubbleStyle::default()
        },
        ..config()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history.record_aggression(&Trade {
        agg_id: 7,
        timestamp_ms: 400,
        price: dec("101"),
        quantity: dec("3"),
        side: Side::Buy,
    });
    // Ask 101: 4 -> 1, right after the print.
    history
        .apply_delta(
            450,
            &BookDelta::new(11, 11, vec![], vec![level("101", "1")]),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(12, 12, vec![], vec![]))
        .unwrap();

    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    assert!(
        projection.aggressions.is_empty(),
        "a 3-lot print is under the 100 floor"
    );
    let aligned = projection
        .liquidity_events
        .iter()
        .find(|event| event.price_bucket == dec("101"))
        .expect("the reduction is still projected");
    assert_eq!(aligned.evidence, LiquidityEvidence::AggressionAligned);
    assert!(aligned.matched_quantity > Decimal::ZERO);
}

#[test]
fn unattributed_reduction_floor_hides_small_pulls_only() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    // Bid 100: 3 -> 2 (33% pull, unattributed): under the 50% floor.
    history
        .apply_delta(
            300,
            &BookDelta::new(11, 11, vec![level("100", "2")], vec![]),
        )
        .unwrap();
    // Ask 101: 4 -> 1 (75% pull, unattributed): over the floor.
    history
        .apply_delta(
            500,
            &BookDelta::new(12, 12, vec![], vec![level("101", "1")]),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(13, 13, vec![], vec![]))
        .unwrap();

    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let projection = project(&history, &timeline, prices);
    let buckets: Vec<_> = projection
        .liquidity_events
        .iter()
        .map(|event| event.price_bucket)
        .collect();
    assert!(buckets.contains(&dec("101")), "large pull must display");
    assert!(!buckets.contains(&dec("100")), "small pull must be hidden");

    // The fraction floor alone is not enough: with it at zero the small
    // pull is still gated by its size against the visible reference.
    let mut fraction_only = history.config().clone();
    fraction_only.min_unattributed_reduction = 0.0;
    history.update_config(fraction_only).unwrap();
    let projection = project(&history, &timeline, prices);
    assert!(
        !projection
            .liquidity_events
            .iter()
            .any(|event| event.price_bucket == dec("100")),
        "a small pull must stay hidden while the size gate holds"
    );

    // Lowering both display floors to zero shows the small pull too.
    let mut permissive_config = history.config().clone();
    permissive_config.min_unattributed_reduction = 0.0;
    permissive_config.min_unattributed_pull_share = 0.0;
    history.update_config(permissive_config).unwrap();
    let projection = project(&history, &timeline, prices);
    assert!(
        projection
            .liquidity_events
            .iter()
            .any(|event| event.price_bucket == dec("100"))
    );
}

#[test]
fn compatible_aggression_rescues_a_small_reduction_from_the_floor() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history.record_aggression(&Trade {
        agg_id: 1,
        timestamp_ms: 290,
        price: dec("100"),
        quantity: dec("1"),
        side: Side::Sell,
    });
    // The same 33% bid pull as above, but now a compatible sell hit it.
    history
        .apply_delta(
            300,
            &BookDelta::new(11, 11, vec![level("100", "2")], vec![]),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(12, 12, vec![], vec![]))
        .unwrap();

    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let projection = project(&history, &timeline, prices);
    assert!(
        projection.liquidity_events.iter().any(|event| {
            event.price_bucket == dec("100")
                && matches!(event.evidence, LiquidityEvidence::AggressionAligned)
        }),
        "aligned bites always display regardless of the floor"
    );
}

#[test]
fn event_cap_keeps_aligned_evidence_ahead_of_unattributed_pulls() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        max_visible_cells: 2,
        min_unattributed_reduction: 0.0,
        min_unattributed_pull_share: 0.0,
        ..HeatmapConfig::default()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    // A small SELL print aligns with a small bid reduction at 100...
    history.record_aggression(&Trade {
        agg_id: 1,
        timestamp_ms: 290,
        price: dec("100"),
        quantity: dec("0.5"),
        side: Side::Sell,
    });
    history
        .apply_delta(
            300,
            &BookDelta::new(11, 11, vec![level("100", "2.5")], vec![]),
        )
        .unwrap();
    // ...followed by two much larger unattributed pulls.
    history
        .apply_delta(
            400,
            &BookDelta::new(12, 12, vec![level("99", "0.1")], vec![]),
        )
        .unwrap();
    history
        .apply_delta(
            500,
            &BookDelta::new(13, 13, vec![], vec![level("102", "0.5")]),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(14, 14, vec![], vec![]))
        .unwrap();

    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let projection = project(&history, &timeline, prices);

    assert_eq!(projection.liquidity_events.len(), 2, "cap of two applies");
    assert_eq!(projection.dropped_liquidity_events, 1);
    // The smallest event by removed quantity is the aligned one, yet it
    // must survive the cap: bubbles point at aligned events.
    assert!(
        projection.liquidity_events.iter().any(|event| {
            event.price_bucket == dec("100")
                && matches!(event.evidence, LiquidityEvidence::AggressionAligned)
        }),
        "aligned evidence must outrank bigger unattributed pulls in the cap"
    );
}

/// Clearing the map on the candles may not delete the tape's.
///
/// These cells span the whole normalized x axis, tape included, and the
/// renderer clips them per pane. Gating their *production* on the candles'
/// switch therefore emptied the tape as well: the trader switched off one
/// pane and the other went dark with it, which no amount of correctness in
/// the config layer could fix — the data was never built.
#[test]
fn hiding_the_map_on_the_candles_still_projects_the_tapes() {
    let base = HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        // The candles are clear; the tape keeps both of its layers, which
        // is what a fresh install now opens as.
        show_depth: false,
        ..HeatmapConfig::default()
    };
    assert!(!base.depth_visible(), "the candles draw no map");
    assert!(base.lane_depth_drawn(), "the tape does");

    // Enough book movement to close a run, so the cells below exist for a
    // reason other than the switch under test.
    let fill = |config: HeatmapConfig| {
        let mut history = LiquidityHistory::new(config);
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(
                800,
                &BookDelta::new(11, 11, vec![level("100", "6")], vec![]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
            .unwrap();
        history
    };
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();

    let projection = project(&fill(base.clone()), &timeline, prices);
    assert!(
        !projection.cells.is_empty(),
        "the tape reads these cells, so hiding the candles' map may not stop building them"
    );

    // And with the tape's own switch off too, nobody is reading them.
    let both_off = fill(HeatmapConfig {
        live_lane: LiveLaneStyle {
            show_depth: false,
            ..LiveLaneStyle::default()
        },
        ..base
    });
    assert!(
        project(&both_off, &timeline, prices).cells.is_empty(),
        "with neither pane drawing the map, none is built"
    );
}

#[test]
fn disabled_projection_is_empty_even_with_data() {
    // Every layer off, said out loud: the default config stopped being
    // inert when the tape gained defaults of its own (both layers on), so a
    // test about a *disabled* projection has to disable the tape too.
    let mut history = LiquidityHistory::new(HeatmapConfig {
        live_lane: LiveLaneStyle {
            enabled: false,
            ..LiveLaneStyle::default()
        },
        ..HeatmapConfig::default()
    });
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

/// A bar is timeless: its slot cannot say *when* inside the bar a wall was
/// resting, only how much of the bar it rested for. So with a tape on
/// screen the slots summarize — one band per level, weighted by presence —
/// and the tape keeps the runs themselves.
#[test]
fn a_bar_slot_summarizes_the_book_while_the_tape_keeps_the_runs() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    // The 100 bid (quantity 3) is pulled halfway through the first bar.
    history
        .apply_delta(
            5_000,
            &BookDelta::new(11, 11, vec![level("100", "0")], vec![]),
        )
        .unwrap();
    history
        .apply_delta(20_000, &BookDelta::new(12, 12, vec![], vec![]))
        .unwrap();

    let closed = [bar(0, 10_000), bar(10_000, 20_000)];
    let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();
    let of_bucket = |projection: &HeatmapProjection| {
        let mut cells: Vec<_> = projection
            .cells
            .iter()
            .filter(|cell| cell.price_bucket == dec("100"))
            .map(|cell| (cell.x0, cell.x1, cell.quantity))
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells
    };

    // Without a tape the chart keeps drawing the run where it happened:
    // half of the first bar's slot, at its own quantity.
    let alone = project(
        &history,
        &BarTimeline::from_bars(0, &closed, None, None),
        prices,
    );
    assert_eq!(of_bucket(&alone), vec![(0.0, 0.25, dec("3"))]);

    // With a tape, that same run becomes one summary band per bar: the
    // whole slot wide, carrying what was typically resting there — three
    // for half the bar reads as one and a half.
    let summarized = project(
        &history,
        &BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::LiveEdge {
                now_ms: 20_000,
                window_ms: 5_000,
                reference_ms: 5_000,
                on_newest_bar: true,
            }),
        ),
        prices,
    );
    // Three regions: two bar slots and the tape. The run only touches the
    // first bar, and it is long gone by the time the tape's window opens.
    assert_eq!(
        of_bucket(&summarized),
        vec![(0.0, 1.0 / 3.0, dec("1.5"))],
        "one band per bar, weighted by how much of it the wall was there"
    );
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
        projection.gaps.as_slice(),
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
fn bubbles_track_execution_price_not_a_flat_line() {
    // Regression guard: aggressions at different prices must land at
    // different chart heights (higher price -> higher on chart -> smaller
    // y), so a moving market shows bubbles riding the price, never a flat
    // horizontal band.
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_cluster_ms: 0,
        ..config()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    for (id, price) in [(1_u64, "99.5"), (2, "100.5"), (3, "101.5")] {
        history.record_aggression(&Trade {
            agg_id: id,
            timestamp_ms: 200 + id as i64,
            price: dec(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
    }
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("99"), dec("102")).unwrap(),
    );
    assert_eq!(projection.aggressions.len(), 3);
    let y_at = |bucket: &str| {
        projection
            .aggressions
            .iter()
            .find(|aggression| aggression.price_bucket == dec(bucket))
            .unwrap_or_else(|| panic!("no bubble at bucket {bucket}"))
            .y
    };
    assert!(y_at("101") < y_at("100"), "higher price must sit higher");
    assert!(y_at("100") < y_at("99"), "higher price must sit higher");
}

#[test]
fn bubbles_project_with_l2_capture_off() {
    // The aggression layer only needs the trade stream. With depth capture
    // off there is no map and no coverage story to tell, so cells and gap
    // primitives stay empty while bubbles still project.
    let mut history = LiquidityHistory::new(HeatmapConfig {
        enabled: false,
        bubble_cluster_ms: 0,
        ..config()
    });
    for (id, price) in [(1_u64, "99.5"), (2, "100.5")] {
        history.record_aggression(&Trade {
            agg_id: id,
            timestamp_ms: 200 + id as i64,
            price: dec(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
    }
    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("99"), dec("102")).unwrap(),
    );
    assert!(projection.enabled);
    assert_eq!(projection.aggressions.len(), 2);
    assert!(projection.cells.is_empty());
    assert!(projection.gaps.is_empty());
}

#[test]
fn turning_l2_capture_off_hides_the_map_without_touching_bubbles_or_history() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubble_cluster_ms: 0,
        ..config()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history.record_aggression(&Trade {
        agg_id: 1,
        timestamp_ms: 200,
        price: dec("100.5"),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("99"), dec("102")).unwrap();
    let before = project(&history, &timeline, prices);
    assert!(!before.cells.is_empty());
    assert_eq!(before.aggressions.len(), 1);
    let runs_before = history.runs().count();

    history
        .update_config(HeatmapConfig {
            enabled: false,
            bubble_cluster_ms: 0,
            ..config()
        })
        .unwrap();
    let after = project(&history, &timeline, prices);
    assert!(after.cells.is_empty(), "the map stops drawing");
    assert_eq!(after.aggressions.len(), 1, "bubbles are untouched");
    assert_eq!(
        history.runs().count(),
        runs_before,
        "retained L2 history survives the toggle"
    );
}

/// The depth caps still drop and still report it — a heat cell is a
/// picture of the book, and a frame that cannot draw them all says so. The
/// bubble budget is the one that changed: it folds instead, so it reports
/// folds and the ink still adds up to what traded.
#[test]
fn primitive_caps_report_what_they_dropped_and_what_they_folded() {
    let limited = HeatmapConfig {
        max_visible_cells: 1,
        max_aggression_primitives: 4,
        bubble_cluster_ms: 0,
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
    // Two marks for three prints: the budget folded the two smallest into
    // one and left the biggest — the mark a trader is actually reading —
    // exactly as it was.
    assert_eq!(projection.aggressions.len(), 2);
    assert_eq!(
        projection.folded_aggressions, 1,
        "one mark folded, none lost"
    );
    let drawn: Decimal = projection
        .aggressions
        .iter()
        .map(|mark| mark.quantity)
        .sum();
    assert_eq!(
        drawn,
        dec("6"),
        "prints of 1, 2 and 3 are six contracts of ink"
    );
    let fold = projection
        .aggressions
        .iter()
        .find(|mark| mark.folded_marks > 0)
        .expect("the budget folded something, so a mark must say so");
    assert_eq!(fold.quantity, dec("3"), "the two smallest folded together");
    assert_eq!(fold.folded_marks, 2, "and it says it stands for two");
    assert_eq!(fold.trade_count, 2);
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
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(800, &closed));
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

/// Bubbles only: no book, so nothing but the tape decides what is drawn.
fn tape(config: HeatmapConfig, trades: &[(u64, i64, &str, &str, Side)]) -> LiquidityHistory {
    let mut history = LiquidityHistory::new(config);
    for (agg_id, timestamp_ms, price, quantity, side) in trades {
        history.record_aggression(&Trade {
            agg_id: *agg_id,
            timestamp_ms: *timestamp_ms,
            price: dec(price),
            quantity: dec(quantity),
            side: *side,
        });
    }
    history
}

fn bubbles_only() -> HeatmapConfig {
    HeatmapConfig {
        enabled: false,
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        display_grouping: DisplayGrouping::Native,
        bubble_cluster_ms: 0,
        bubble_dust_merge_ms: 0,
        bubbles: BubbleStyle {
            readable_min_radius: 0.0,
            ..BubbleStyle::default()
        },
        ..HeatmapConfig::default()
    }
}

/// The gap between the newest bubble and the tape's right edge is exactly
/// the distance between the chart's two clocks — and past the lane's own
/// window it swallows the tape whole.
///
/// The lane ends at `max(book clock, print clock)` and bubbles are placed
/// by the print clock alone, so a book running ahead of the tape puts the
/// newest mark that far left of the edge. The depth map, drawn out to the
/// book clock, still reaches the edge — which is why one frame ends in two
/// different places and nothing on the canvas says by how much.
///
/// This pins the arithmetic so a future change cannot quietly turn a
/// delivery delay into a bigger or smaller lie, and pins the part that
/// matters most: the prints are *still on the chart*, in their bar's slot.
/// An empty tape never means nothing traded.
#[test]
fn the_newest_bubble_trails_the_edge_by_the_distance_between_the_clocks() {
    let closed = [bar(0, 10_000), bar(10_000, 20_000)];
    // One print, four seconds before the book's instant.
    let mut history = tape(
        HeatmapConfig {
            enabled: true,
            ..bubbles_only()
        },
        &[(1, 16_000, "100", "3", Side::Buy)],
    );
    history.install_snapshot(20_000, 1, snapshot(10)).unwrap();
    assert_eq!(history.tape_age(), Some(crate::TapeAge::Behind(4_000)));

    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let window_ms = crate::reserved_span_ms(&closed);
    assert_eq!(window_ms, 10_000, "the lane shows one bar's worth of flow");

    let frame_at = |now_ms: i64, history: &LiquidityHistory| {
        let timeline = BarTimeline::from_bars(0, &closed, None, live(now_ms, &closed));
        project(history, &timeline, prices)
    };

    let projected = frame_at(20_000, &history);
    let edge = projected
        .live_now_x
        .expect("the lane ends at the live edge");
    let tape_marks: Vec<_> = projected
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert_eq!(tape_marks.len(), 1, "the print is on the tape");

    // The lane is the last of three equal regions, so one region is a
    // third of the axis and four seconds of a ten-second window is 40% of
    // it: the mark sits 0.4/3 left of the edge.
    let regions = f64::from(u32::try_from(closed.len() + 1).unwrap());
    let expected = edge - (4_000.0 / f64::from(u32::try_from(window_ms).unwrap())) / regions;
    assert!(
        (tape_marks[0].x - expected).abs() < 1e-9,
        "the newest mark should trail the edge by the clocks' distance:              got {}, expected {expected}",
        tape_marks[0].x
    );

    // Twelve more seconds of book with not one print: the gap is now wider
    // than the whole lane.
    history
        .apply_delta(
            32_000,
            &BookDelta::new(11, 11, vec![level("100", "7")], vec![]),
        )
        .unwrap();
    assert_eq!(history.tape_age(), Some(crate::TapeAge::Behind(16_000)));

    let starved = frame_at(32_000, &history);
    assert!(
        !starved.aggressions.iter().any(|mark| mark.live),
        "past the window there is no bubble left on the tape"
    );
    // And this is the half that must never be misread: the print did not
    // vanish from the chart, it went back to the slot of the bar it
    // happened in. The tape is empty; the market was not.
    let in_slots: Decimal = starved
        .aggressions
        .iter()
        .filter(|mark| !mark.live)
        .map(|mark| mark.quantity)
        .sum();
    assert_eq!(in_slots, dec("3"), "the print is still on the chart");
    // The depth map, meanwhile, reaches the edge the bubbles could not.
    assert!(
        !starved.cells.is_empty(),
        "the book is drawn out to its own clock"
    );
}

/// Zooming changes what is on screen, never what a quantity means: the
/// same print maps to the same normalized size through every price window,
/// in every reference mode — the automatic ones included. Only a change in
/// the cluster's own quantity may change its bubble.
#[test]
fn a_prints_bubble_keeps_its_size_when_the_window_zooms_out() {
    let trades = [
        (1_u64, 6_000_i64, "100", "2", Side::Buy),
        (2, 7_000, "200", "100", Side::Sell),
    ];
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let partial = bar(15_000, 20_000);
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
    // Zoomed in only the small print is on screen; zoomed out the large
    // one joins it and, today, silently rescales it.
    let zoomed_in = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let zoomed_out = PriceWindow::new(dec("98"), dec("203")).unwrap();
    for reference in [
        BubbleSizeReference::VisibleP99,
        BubbleSizeReference::VisibleMax,
        BubbleSizeReference::Fixed,
    ] {
        let config = HeatmapConfig {
            bubbles: BubbleStyle {
                size_reference: reference,
                size_reference_quantity: 100.0,
                ..bubbles_only().bubbles
            },
            ..bubbles_only()
        };
        let size_through = |prices: PriceWindow| {
            project(&tape(config.clone(), &trades), &timeline, prices)
                .aggressions
                .iter()
                .find(|bubble| bubble.quantity == dec("2"))
                .map(|bubble| bubble.size)
                .expect("the small print is inside both windows")
        };
        let narrow = size_through(zoomed_in);
        let wide = size_through(zoomed_out);
        assert!(
            (narrow - wide).abs() < 1e-6,
            "{reference:?}: one quantity, one size — got {narrow} zoomed in, {wide} zoomed out"
        );
    }
}

/// The zoom rule holds for the summary tier too: a closed bar's pie maps
/// to the same normalized size through every price window. This is the
/// mark most of a chart is made of once the candle summary is on, so the
/// print scale being stable is not enough — a 1mm zoom that rescales
/// every pie rescales the chart.
#[test]
fn a_pie_keeps_its_size_when_the_window_zooms_out() {
    let trades = [
        (1_u64, 6_000_i64, "100", "2", Side::Buy),
        (2, 6_500, "100", "2", Side::Sell),
        (3, 7_000, "200", "100", Side::Sell),
    ];
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let partial = bar(15_000, 20_000);
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
    // Zoomed in only the small pie is on screen; zoomed out the huge one
    // joins it.
    let zoomed_in = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let zoomed_out = PriceWindow::new(dec("98"), dec("203")).unwrap();
    for reference in [
        BubbleSizeReference::VisibleP99,
        BubbleSizeReference::VisibleMax,
        BubbleSizeReference::Fixed,
    ] {
        let config = HeatmapConfig {
            bubble_candle_summary: true,
            bubbles: BubbleStyle {
                size_reference: reference,
                size_reference_quantity: 100.0,
                ..bubbles_only().bubbles
            },
            ..bubbles_only()
        };
        let pie_size_through = |prices: PriceWindow| {
            project(&tape(config.clone(), &trades), &timeline, prices)
                .aggressions
                .iter()
                .find(|mark| !mark.live && mark.quantity == dec("4"))
                .map(|mark| mark.size)
                .expect("the small bar's pie is inside both windows")
        };
        let narrow = pie_size_through(zoomed_in);
        let wide = pie_size_through(zoomed_out);
        assert!(
            (narrow - wide).abs() < 1e-6,
            "{reference:?}: one pie, one size — got {narrow} zoomed in, {wide} zoomed out"
        );
    }
}

/// The one legitimate way zoom grows a bubble: a coarser grouping merges
/// prints into one cluster, and the merged mark carries their summed
/// quantity — so it may only read *bigger* than the prints it swallowed,
/// never smaller.
#[test]
fn a_cluster_merged_by_zooming_out_reads_bigger_not_smaller() {
    let trades = [
        (1_u64, 6_000_i64, "100", "3", Side::Buy),
        (2, 6_500, "101", "2", Side::Buy),
        (3, 7_000, "150", "50", Side::Sell),
    ];
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let partial = bar(15_000, 20_000);
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
    // Ten target rows: at a 10-wide window prints one price apart keep
    // their own bucket; at a 200-wide window they share one.
    let zoomed_in = PriceWindow::new(dec("98"), dec("108")).unwrap();
    let zoomed_out = PriceWindow::new(dec("50"), dec("250")).unwrap();
    for reference in [
        BubbleSizeReference::VisibleP99,
        BubbleSizeReference::VisibleMax,
        BubbleSizeReference::Fixed,
    ] {
        let config = HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive { target_rows: 10 },
            bubble_cluster_ms: 1_000,
            bubbles: BubbleStyle {
                size_reference: reference,
                size_reference_quantity: 100.0,
                ..bubbles_only().bubbles
            },
            ..bubbles_only()
        };
        let size_of = |prices: PriceWindow, quantity: &str| {
            project(&tape(config.clone(), &trades), &timeline, prices)
                .aggressions
                .iter()
                .find(|bubble| bubble.quantity == dec(quantity))
                .map(|bubble| bubble.size)
                .unwrap_or_else(|| panic!("no bubble of quantity {quantity}"))
        };
        let three = size_of(zoomed_in, "3");
        let two = size_of(zoomed_in, "2");
        let merged = size_of(zoomed_out, "5");
        assert!(
            merged > three && merged > two,
            "{reference:?}: the merged cluster carries more quantity, so it must read \
                 bigger — got {merged} against {three} and {two}"
        );
    }
}

/// Opposing prints of the same bar collapse into one two-sided mark in its
/// slot; the tape's own prints stay separate, because the tape is where
/// flow is read print by print.
#[test]
fn the_summary_folds_a_bar_and_leaves_the_live_lane_alone() {
    // Five-second bars, so the rolling window is five seconds wide and the
    // first bar's prints have long left it.
    let trades = [
        (1_u64, 1_000_i64, "100", "3", Side::Buy),
        (2, 2_000, "100", "1", Side::Sell),
        (3, 16_000, "100", "2", Side::Buy),
        (4, 17_000, "100", "2", Side::Sell),
    ];
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let partial = bar(15_000, 20_000);
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
    assert_eq!(timeline.lane_start_ms(), Some(15_000));
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    let off = project(&tape(bubbles_only(), &trades), &timeline, prices);
    assert_eq!(off.aggressions.len(), 4, "the default draws every print");
    assert!(
        off.aggressions
            .iter()
            .all(|bubble| bubble.buy_share == 1.0 || bubble.buy_share == 0.0),
        "no mark carries both sides until the summary is switched on"
    );

    let summarized = project(
        &tape(
            HeatmapConfig {
                bubble_candle_summary: true,
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        prices,
    );
    // Two pies — the closed bar's and the forming bar's running total —
    // plus the tape's two prints, untouched.
    assert_eq!(summarized.aggressions.len(), 4);
    let pies: Vec<_> = summarized
        .aggressions
        .iter()
        .filter(|bubble| bubble.buy_share > 0.0 && bubble.buy_share < 1.0)
        .collect();
    assert_eq!(pies.len(), 2);
    assert_eq!(pies[0].quantity, dec("4"));
    assert!((pies[0].buy_share - 0.75).abs() < 1e-6);
    assert!(!pies[0].live, "a summary belongs to a bar, not to the tape");
    // The forming bar's pie carries what it has taken so far — the very
    // prints still rolling across the tape.
    assert_eq!(pies[1].quantity, dec("4"));
    assert!((pies[1].buy_share - 0.5).abs() < 1e-6);
    assert_eq!(
        summarized
            .aggressions
            .iter()
            .filter(|bubble| bubble.live)
            .count(),
        2,
        "both live prints keep their own mark"
    );
}

/// A summary is a statement about a finished bar, so it is complete the
/// moment the bar is — even while the prints behind it are still rolling
/// across the tape. That is the one place the two views overlap: the pie is
/// an aggregate of the same flow, not a second copy of a raw print.
#[test]
fn a_bar_gets_its_pie_at_close_without_taking_its_prints_off_the_tape() {
    let trades = [
        (1_u64, 16_000_i64, "100", "3", Side::Buy),
        (2, 17_000, "100", "1", Side::Sell),
    ];
    // The last closed bar runs to 18 s and the window reaches back to 15 s,
    // so both prints are inside a bar that is over *and* on the tape.
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 18_000)];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        Some(&bar(18_000, 20_000)),
        live(20_000, &closed),
    );
    assert_eq!(timeline.lane_start_ms(), Some(15_000));
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    let summarized = project(
        &tape(
            HeatmapConfig {
                bubble_candle_summary: true,
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        prices,
    );
    // Two marks still rolling, plus the finished bar's pie.
    assert_eq!(summarized.aggressions.iter().filter(|b| b.live).count(), 2);
    let pies: Vec<_> = summarized
        .aggressions
        .iter()
        .filter(|bubble| !bubble.live)
        .collect();
    assert_eq!(pies.len(), 1, "the closed bar summarizes immediately");
    assert_eq!(pies[0].quantity, dec("4"));
    assert!((pies[0].buy_share - 0.75).abs() < 1e-6);
    // The pie sits in its bar's slot, left of where the tape begins.
    let lane_left = 3.0 / 4.0;
    assert!(pies[0].x < lane_left, "the pie belongs to the bar's slot");
    assert!(
        summarized
            .aggressions
            .iter()
            .filter(|b| b.live)
            .all(|b| b.x >= lane_left)
    );

    // Without the summary a raw print is drawn exactly once, on the tape.
    let plain = project(&tape(bubbles_only(), &trades), &timeline, prices);
    assert_eq!(plain.aggressions.len(), 2);
    assert!(plain.aggressions.iter().all(|bubble| bubble.live));
}

/// The forming bar summarizes too, and its pie grows with the flow: that is
/// how the compressed left side reports what is happening *now* instead of
/// only what already happened once the bar closed.
#[test]
fn the_forming_bars_pie_grows_with_every_order() {
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        Some(&bar(15_000, 30_000)),
        live(30_000, &closed),
    );
    assert_eq!(timeline.lane_start_ms(), Some(25_000));
    let summarize = HeatmapConfig {
        bubble_candle_summary: true,
        ..bubbles_only()
    };
    let pie_of = |trades: &[(u64, i64, &str, &str, Side)]| {
        let projected = project(
            &tape(summarize.clone(), trades),
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert_eq!(projected.aggressions.len(), 1, "one bar, one mark");
        let pie = projected.aggressions[0].clone();
        assert!(!pie.live, "a summary belongs to a bar, not to the tape");
        (pie.quantity, pie.buy_share, pie.trade_count)
    };

    // One buy so far: the mark is all buy and carries exactly that print.
    let first = [(1_u64, 16_000_i64, "100", "3", Side::Buy)];
    assert_eq!(pie_of(&first), (dec("3"), 1.0, 1));
    // A sell arrives: the same mark grows and the proportion moves with it,
    // without waiting for the bar to close.
    let second = [
        (1_u64, 16_000_i64, "100", "3", Side::Buy),
        (2, 17_000, "100", "1", Side::Sell),
    ];
    assert_eq!(pie_of(&second), (dec("4"), 0.75, 2));
}

/// A summary carries a whole bar's quantity. Sized against the reference
/// single prints set, every one of them would peg at the largest radius
/// and the summaries would stop saying anything about each other — so
/// pies read on their own scale, the session's busiest minute per level,
/// and stay ordered among themselves.
#[test]
fn a_summary_is_sized_against_the_sessions_minutes_and_a_pinned_reference_never_moves() {
    // One busy bar and one quiet one, both closed, plus a live print.
    let mut trades: Vec<(u64, i64, &str, &str, Side)> = Vec::new();
    for index in 0..20_u64 {
        trades.push((
            index + 1,
            1_000 + index as i64 * 500,
            if index % 2 == 0 { "100" } else { "101" },
            "1",
            if index % 2 == 0 {
                Side::Buy
            } else {
                Side::Sell
            },
        ));
    }
    trades.push((100, 21_000, "100", "1", Side::Buy));
    trades.push((101, 21_500, "100", "1", Side::Sell));
    trades.push((200, 41_000, "100", "1", Side::Buy));
    let closed = [bar(0, 20_000), bar(20_000, 40_000)];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        Some(&bar(40_000, 45_000)),
        live(45_000, &closed),
    );
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    let summarized = project(
        &tape(
            HeatmapConfig {
                bubble_candle_summary: true,
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        prices,
    );
    // History is measured against history, the lane against the tape.
    assert!(summarized.summary_reference > summarized.aggression_reference);
    let busiest = summarized
        .aggressions
        .iter()
        .filter(|bubble| !bubble.live)
        .map(|bubble| bubble.size)
        .fold(0.0_f32, f32::max);
    let quietest = summarized
        .aggressions
        .iter()
        .filter(|bubble| !bubble.live)
        .map(|bubble| bubble.size)
        .fold(1.0_f32, f32::min);
    assert!(
        quietest < busiest,
        "summaries must still differ in size: {quietest} vs {busiest}"
    );

    // Without a summary the two regions share one scale, exactly as before.
    let plain = project(&tape(bubbles_only(), &trades), &timeline, prices);
    assert_eq!(plain.summary_reference, plain.aggression_reference);

    // A pinned reference is pinned: the user chose an absolute quantity so
    // that nothing on screen may rescale a bubble.
    let pinned = project(
        &tape(
            HeatmapConfig {
                bubble_candle_summary: true,
                bubbles: BubbleStyle {
                    size_reference: BubbleSizeReference::Fixed,
                    size_reference_quantity: 8.0,
                    readable_min_radius: 0.0,
                    ..BubbleStyle::default()
                },
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        prices,
    );
    assert_eq!(pinned.aggression_reference, dec("8"));
    assert_eq!(pinned.summary_reference, dec("8"));
}

/// The lane clusters on its own window, so the region with room to spare
/// can show detail the compressed history cannot.
#[test]
fn the_lane_clusters_on_its_own_window() {
    let trades = [
        (1_u64, 1_000_i64, "100", "1", Side::Buy),
        (2, 1_050, "100", "1", Side::Buy),
        (3, 16_000, "100", "1", Side::Buy),
        (4, 16_050, "100", "1", Side::Buy),
    ];
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let partial = bar(15_000, 20_000);
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    // History gathers its pair, the lane keeps its prints apart.
    let split = project(
        &tape(
            HeatmapConfig {
                bubble_cluster_ms: 100,
                live_lane: LiveLaneStyle {
                    cluster_ms: Some(0),
                    ..LiveLaneStyle::default()
                },
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        prices,
    );
    assert_eq!(split.aggressions.len(), 3);
    assert_eq!(
        split.aggressions.iter().filter(|b| b.live).count(),
        2,
        "the lane drew both prints"
    );

    // Inheriting means inheriting: the same window on both sides of the
    // boundary gathers both pairs.
    let inherited = project(
        &tape(
            HeatmapConfig {
                bubble_cluster_ms: 100,
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        prices,
    );
    assert_eq!(inherited.aggressions.len(), 2);
}

/// A cluster must never straddle the boundary: half a bubble cannot be
/// both a print still rolling across the tape and a settled summary. The
/// boundary is the rolling window's left edge, so what decides it is a
/// print's age, not which bar it came from.
#[test]
fn no_cluster_spans_the_lane_boundary() {
    let trades = [
        (1_u64, 14_900_i64, "100", "1", Side::Buy),
        (2, 15_100, "100", "1", Side::Buy),
    ];
    // A five-second window ending at 20 s reaches back to 15 s and cuts
    // between the two prints, a fifth of a second apart.
    let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
    let partial = bar(15_000, 20_000);
    let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
    assert_eq!(timeline.lane_start_ms(), Some(15_000));
    let projection = project(
        &tape(
            HeatmapConfig {
                // A window wide enough to swallow both, were it allowed to.
                bubble_cluster_ms: 2_000,
                ..bubbles_only()
            },
            &trades,
        ),
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    assert_eq!(projection.aggressions.len(), 2);
    assert_eq!(projection.aggressions.iter().filter(|b| b.live).count(), 1);
}

/// The live edge is the lane's right edge, so it is the right edge of the
/// chart — and it is only reported for a frame that follows one.
#[test]
fn the_live_edge_is_the_right_edge_of_the_lane() {
    let closed = [bar(0, 20_000), bar(20_000, 40_000)];
    let history = tape(bubbles_only(), &[(1, 41_000, "100", "1", Side::Buy)]);
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    let following = project(
        &history,
        &BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(40_000, 50_000)),
            live(50_000, &closed),
        ),
        prices,
    );
    assert_eq!(
        following.live_now_x,
        Some(1.0),
        "market time always reaches the lane's right edge"
    );

    let settled = project(
        &history,
        &BarTimeline::from_bars(0, &closed, Some(&bar(40_000, 50_000)), None),
        prices,
    );
    assert_eq!(settled.live_now_x, None);
}

#[test]
fn display_grouping_changes_without_resetting_capture_history() {
    let mut history = LiquidityHistory::new(config());
    history
        .install_snapshot(
            100,
            1,
            BookSnapshot::new(
                10,
                vec![level("100", "2"), level("101", "3")],
                vec![level("102", "4")],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let runs_before = history.runs().count();
    let status_before = history.status();
    let next = HeatmapConfig {
        display_grouping: DisplayGrouping::Multiple(2),
        ..history.config().clone()
    };
    history.update_config(next).unwrap();

    assert_eq!(history.runs().count(), runs_before);
    assert_eq!(history.status(), status_before);
    assert!(history.book().is_initialized());

    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("99"), dec("104")).unwrap(),
    );
    assert_eq!(projection.effective_grouping.multiple, 2);
    assert_eq!(projection.effective_grouping.bucket_width, dec("2"));
    assert!(projection.cells.iter().any(|cell| {
        cell.side == BookSide::Bid && cell.price_bucket == dec("100") && cell.quantity == dec("5")
    }));
}

/// Baseline for every evidence-related test.
fn event_config() -> HeatmapConfig {
    HeatmapConfig {
        enabled: true,
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        display_grouping: DisplayGrouping::Native,
        bubble_cluster_ms: 100,
        liquidity_correlation_ms: 250,
        ..HeatmapConfig::default()
    }
}

/// One buy print plus two ask reductions: the one at t=500 has compatible
/// aggression evidence, the full pull at t=800 is depth-only.
fn reduction_history(config: HeatmapConfig) -> LiquidityHistory {
    let mut history = LiquidityHistory::new(config);
    history
        .install_snapshot(
            100,
            1,
            BookSnapshot::new(
                10,
                vec![level("100", "2")],
                vec![level("101", "10")],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    history.record_aggression(&Trade {
        agg_id: 77,
        timestamp_ms: 480,
        price: dec("101"),
        quantity: dec("3"),
        side: Side::Buy,
    });
    history
        .apply_delta(
            500,
            &BookDelta::new(11, 11, vec![], vec![level("101", "6")]),
        )
        .unwrap();
    history
        .apply_delta(
            800,
            &BookDelta::new(12, 12, vec![], vec![level("101", "0")]),
        )
        .unwrap();
    history
}

fn project_reductions(config: HeatmapConfig) -> HeatmapProjection {
    let history = reduction_history(config);
    project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("99"), dec("103")).unwrap(),
    )
}

#[test]
fn projects_partial_and_full_reductions_with_conserved_aggression_evidence() {
    let projection = project_reductions(event_config());
    assert_eq!(projection.liquidity_events.len(), 2);
    let partial = projection
        .liquidity_events
        .iter()
        .find(|event| event.timestamp_ms == 500)
        .unwrap();
    assert_eq!(partial.before, dec("10"));
    assert_eq!(partial.after, dec("6"));
    assert_eq!(partial.removed, dec("4"));
    assert!(!partial.full_removal);
    assert_eq!(partial.matched_quantity, dec("3"));
    assert_eq!(partial.matched_fraction, 0.75);
    assert_eq!(partial.evidence, LiquidityEvidence::AggressionAligned);

    let full = projection
        .liquidity_events
        .iter()
        .find(|event| event.timestamp_ms == 800)
        .unwrap();
    assert_eq!(full.removed, dec("6"));
    assert!(full.full_removal);
    assert_eq!(full.evidence, LiquidityEvidence::DepthOnly);

    let bubble = projection.aggressions.first().unwrap();
    assert_eq!(bubble.trade_count, 1);
    assert_eq!(bubble.agg_ids, [77]);
    assert_eq!(bubble.matched_quantity, dec("3"));
    assert_eq!(bubble.matched_fraction, 1.0);
    assert_eq!(bubble.liquidity_event_ids, [partial.event_id]);
    let total_event_match: Decimal = projection
        .liquidity_events
        .iter()
        .map(|event| event.matched_quantity)
        .sum();
    assert_eq!(total_event_match, bubble.matched_quantity);
}

#[test]
fn evidence_toggles_hide_their_markers_but_keep_bubble_evidence() {
    let no_aligned = project_reductions(HeatmapConfig {
        show_aligned_depletion: false,
        ..event_config()
    });
    assert_eq!(no_aligned.liquidity_events.len(), 1);
    assert_eq!(
        no_aligned.liquidity_events[0].evidence,
        LiquidityEvidence::DepthOnly
    );
    // The hidden marker's evidence still reaches the bubble: association
    // ran, only the marker itself is off screen.
    assert_eq!(no_aligned.aggressions[0].matched_quantity, dec("3"));

    let no_unattributed = project_reductions(HeatmapConfig {
        show_unattributed_reductions: false,
        ..event_config()
    });
    assert_eq!(no_unattributed.liquidity_events.len(), 1);
    assert_eq!(
        no_unattributed.liquidity_events[0].evidence,
        LiquidityEvidence::AggressionAligned
    );

    // With both depletion layers off no association runs at all, and the
    // bubble honestly loses its consumption marks: there is no factual
    // reduction on screen (or off it) for the mark to point at.
    let neither = project_reductions(HeatmapConfig {
        show_aligned_depletion: false,
        show_unattributed_reductions: false,
        ..event_config()
    });
    assert!(neither.liquidity_events.is_empty());
    assert_eq!(neither.aggressions[0].matched_quantity, Decimal::ZERO);
}

#[test]
fn hidden_liquidity_clears_cells_but_keeps_reference_and_markers() {
    let projection = project_reductions(HeatmapConfig {
        show_liquidity: false,
        ..event_config()
    });
    assert!(projection.cells.is_empty());
    assert_eq!(
        projection.dropped_cells, 0,
        "hidden cells are a choice, not a cap drop"
    );
    assert_eq!(
        projection.liquidity_events.len(),
        2,
        "markers outlive the heat behind them"
    );
    assert!(
        projection.liquidity_reference > Decimal::ZERO,
        "the depletion floors must not move when the heat is hidden"
    );
}

#[test]
fn side_toggles_hide_one_side_without_rescaling_the_other() {
    let projection_with = |show_buy: bool, show_sell: bool| {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: 0,
            show_buy_aggressions: show_buy,
            show_sell_aggressions: show_sell,
            bubbles: BubbleStyle {
                size_reference: BubbleSizeReference::VisibleMax,
                ..BubbleStyle::default()
            },
            ..config()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.record_aggression(&Trade {
            agg_id: 1,
            timestamp_ms: 300,
            price: dec("101"),
            quantity: dec("4"),
            side: Side::Buy,
        });
        history.record_aggression(&Trade {
            agg_id: 2,
            timestamp_ms: 400,
            price: dec("100"),
            quantity: dec("1"),
            side: Side::Sell,
        });
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        )
    };

    let both = projection_with(true, true);
    assert_eq!(both.aggressions.len(), 2);
    let sell_size = both
        .aggressions
        .iter()
        .find(|bubble| bubble.side == Side::Sell)
        .expect("the sell bubble draws with both sides on")
        .size;

    // A side switch is a display choice, so the projection still carries
    // both prints: what a frame *draws* is decided in the renderer
    // (`RenderContext::bubbles`), which is what lets the live strip read
    // the same clusters while a side — or the whole bubble layer — is
    // hidden. The invariant this test exists for survives that move: the
    // size reference saw both sides, so hiding the buys must not inflate
    // the sell that stayed.
    let only_sell = projection_with(false, true);
    assert_eq!(only_sell.aggressions.len(), 2);
    // Same clusters, print for print: the live strip buckets these, and a
    // bubble switch may not reshape its histogram.
    assert_eq!(
        both.aggressions
            .iter()
            .map(|a| a.agg_id)
            .collect::<Vec<_>>(),
        only_sell
            .aggressions
            .iter()
            .map(|a| a.agg_id)
            .collect::<Vec<_>>()
    );
    let hidden_buy_sell_size = only_sell
        .aggressions
        .iter()
        .find(|bubble| bubble.side == Side::Sell)
        .expect("the sell print is still a fact of the frame")
        .size;
    assert_eq!(hidden_buy_sell_size, sell_size);
}

#[test]
fn hidden_gaps_emit_no_coverage_primitives() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        show_gaps: false,
        ..config()
    });
    history.install_snapshot(400, 1, snapshot(10)).unwrap();
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    // The same fixture with the flag on yields the
    // "book_unavailable_before_capture" span (see the test above).
    assert!(projection.gaps.is_empty());
}

/// Hiding the map is display-only, so the projection must stop producing
/// depth work while the recorded history stays exactly where it was.
#[test]
fn a_hidden_depth_map_projects_no_depth_primitives() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(400, 1, snapshot(10)).unwrap();
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

    let shown = project(&history, &timeline, prices);
    assert!(!shown.cells.is_empty(), "the visible map has heat cells");
    assert!(!shown.gaps.is_empty(), "and its pre-capture boundary");

    // Hidden on the candles alone is *not* hidden: the tape draws the same
    // cells and the renderer clips them per pane, so the primitives have to
    // survive or the tape goes dark with the chart.
    history
        .update_config(HeatmapConfig {
            show_depth: false,
            ..config()
        })
        .unwrap();
    let candles_only = project(&history, &timeline, prices);
    assert!(
        !candles_only.cells.is_empty(),
        "the tape still draws the map, so its cells are still built"
    );

    // Hidden on both panes is hidden, and that is where the saving is.
    history
        .update_config(HeatmapConfig {
            show_depth: false,
            live_lane: LiveLaneStyle {
                show_depth: false,
                ..LiveLaneStyle::default()
            },
            ..config()
        })
        .unwrap();
    let hidden = project(&history, &timeline, prices);
    assert!(hidden.cells.is_empty());
    assert!(hidden.gaps.is_empty());
    assert!(
        hidden.liquidity_events.is_empty(),
        "no depth primitive survives a map hidden on every pane"
    );
}

// ----------------------------------------------------------------------
// The budget conserves. A frame may fold marks together; it may never
// delete one. And the two panes each answer for their own canvas: what
// the candles draw can not decide what the tape draws, and the tape's
// window can not decide what the candles draw.
// ----------------------------------------------------------------------

/// Total quantity a pane draws, so a test can compare ink against tape.
fn drawn_quantity(projection: &HeatmapProjection, live: bool) -> Decimal {
    projection
        .aggressions
        .iter()
        .filter(|mark| mark.live == live)
        .map(|mark| mark.quantity)
        .sum()
}

/// A ladder of prints either side of the lane boundary, dense enough that
/// any small budget bites.
fn crowded_history(config: HeatmapConfig) -> LiquidityHistory {
    let mut history = LiquidityHistory::new(config);
    for i in 0..60_u64 {
        history.record_aggression(&Trade {
            agg_id: i + 1,
            timestamp_ms: 100 + i as i64 * 300,
            // Spread over price so no two prints share a bucket by luck.
            price: dec("100") + Decimal::from(i % 12),
            quantity: Decimal::from(i % 7 + 1),
            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        });
    }
    history
}

fn crowded_timeline() -> BarTimeline {
    let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
    let partial = bar(18_000, 21_000);
    BarTimeline::from_bars(
        0,
        &closed,
        Some(&partial),
        Some(crate::LiveEdge {
            now_ms: 18_100,
            window_ms: 3_000,
            reference_ms: 3_000,
            on_newest_bar: true,
        }),
    )
}

/// The heart of the mission: a bubble that is too much for the frame is
/// folded into its neighbour, never deleted. A trader reads pressure off
/// these marks, so a quantity that traded must still be on the canvas
/// whatever the budget — carried by a bigger bubble if need be, but
/// carried.
#[test]
fn the_budget_folds_and_never_deletes() {
    let tight = HeatmapConfig {
        max_aggression_primitives: 6,
        ..bubbles_only()
    };
    let history = crowded_history(tight);
    let timeline = crowded_timeline();
    let prices = PriceWindow::new(dec("98"), dec("120")).unwrap();
    let projection = project(&history, &timeline, prices);

    // The budget is a performance target, and correctness outranks it. A
    // fold may not cross a bar — a mark inside a bar's slot is a claim that
    // *that* bar took the volume it carries — so with more (side, bar)
    // groups than the budget has marks, the frame draws the extra marks
    // rather than misattribute volume. Six bars and two sides is a floor of
    // twelve, and the fixture sits on it.
    assert!(
        projection.aggressions.len() <= 12,
        "folded past the point where a fold would have to cross a bar: {} marks",
        projection.aggressions.len()
    );
    assert!(
        projection.aggressions.len() < 60,
        "the budget did not bite at all"
    );
    let recorded: Decimal = history.aggressions().map(|trade| trade.quantity).sum();
    let drawn = drawn_quantity(&projection, true) + drawn_quantity(&projection, false);
    assert_eq!(
        drawn, recorded,
        "every contract that traded is still on the canvas, folded but never dropped"
    );
    assert!(
        projection.folded_aggressions > 0,
        "a frame this far over budget has to report the folds it made"
    );
}

/// A folded mark says so. The trader must be able to tell a bubble that is
/// one print from a bubble the budget squeezed out of several — reading
/// the second as the first is reading a size that never traded at once.
#[test]
fn a_folded_mark_carries_how_many_it_stands_for() {
    let tight = HeatmapConfig {
        max_aggression_primitives: 6,
        ..bubbles_only()
    };
    let history = crowded_history(tight);
    let projection = project(
        &history,
        &crowded_timeline(),
        PriceWindow::new(dec("98"), dec("120")).unwrap(),
    );
    assert!(
        projection
            .aggressions
            .iter()
            .any(|mark| mark.folded_marks > 0),
        "a frame this far over budget has to have folded something"
    );
    for mark in &projection.aggressions {
        if mark.folded_marks > 0 {
            assert!(
                mark.trade_count >= mark.folded_marks as usize,
                "a fold of {} marks can not stand for fewer prints than that",
                mark.folded_marks
            );
        }
    }
}

/// The tape's own budget is its own. Whatever the candles do with theirs —
/// and a summarized chart spends it on pies carrying whole bars — the tape
/// keeps drawing what it drew.
#[test]
fn each_pane_answers_for_its_own_budget() {
    let timeline = crowded_timeline();
    let prices = PriceWindow::new(dec("98"), dec("120")).unwrap();
    let ink_of = |limit: usize| {
        let history = crowded_history(HeatmapConfig {
            max_aggression_primitives: limit,
            ..bubbles_only()
        });
        let projection = project(&history, &timeline, prices);
        (
            drawn_quantity(&projection, true),
            drawn_quantity(&projection, false),
            projection.aggressions.iter().any(|mark| mark.live),
        )
    };
    let (roomy_lane, roomy_chart, roomy_has_lane) = ink_of(400);
    let (tight_lane, tight_chart, tight_has_lane) = ink_of(6);

    assert!(
        roomy_has_lane && tight_has_lane,
        "both frames must have a tape to compare"
    );
    assert_eq!(
        roomy_lane, tight_lane,
        "squeezing the budget changed how much the tape says traded"
    );
    assert_eq!(
        roomy_chart, tight_chart,
        "squeezing the budget changed how much the candles say traded"
    );
}

/// A fold never carries one bar's volume into another bar's slot.
///
/// A bubble drawn inside a bar's slot is a claim about *that* bar. Merging
/// a neighbour's prints into it would be a fabricated fact — the same
/// reason `regionalize_clusters` keeps `bar_index` in its key — so the
/// budget stops folding at that boundary and the frame carries the extra
/// marks instead. Correctness outranks the performance target.
#[test]
fn a_fold_never_moves_volume_into_another_bars_slot() {
    let history = crowded_history(HeatmapConfig {
        max_aggression_primitives: 4,
        ..bubbles_only()
    });
    let timeline = crowded_timeline();
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("120")).unwrap(),
    );

    for mark in projection.aggressions.iter().filter(|mark| !mark.live) {
        let first = timeline
            .locate(mark.first_timestamp_ms)
            .map(|position| position.bar_index);
        let last = timeline
            .locate(mark.last_timestamp_ms)
            .map(|position| position.bar_index);
        assert_eq!(
            first, last,
            "a candle mark spans two bars, so its slot claims volume the                  neighbouring bar traded"
        );
    }
    // And the pane really was squeezed past its budget, so the assertion
    // above was exercised rather than trivially true.
    assert!(
        projection
            .aggressions
            .iter()
            .any(|mark| mark.folded_marks > 1),
        "a budget of four over sixty prints has to have folded something"
    );
}

/// Widening the tape buys it more marks, because marks need room.
///
/// The split is the lane's own width share rather than a constant of its
/// own, so the one control the trader already has over the tape moves both
/// its band and its budget. Without this the number would be a magic
/// constant that disagrees with the canvas the moment the divider moves.
#[test]
fn the_tape_budget_follows_the_room_the_tape_was_given() {
    let lane_of = |share: f32| LiveLaneStyle {
        enabled: true,
        width_share: share,
        ..LiveLaneStyle::default()
    };
    let (narrow_chart, narrow_lane) = pane_budgets(100, &lane_of(MIN_LIVE_LANE_SHARE));
    let (wide_chart, wide_lane) = pane_budgets(100, &lane_of(MAX_LIVE_LANE_SHARE));
    assert!(
        wide_lane > narrow_lane,
        "a wider tape has room for more marks and did not get them"
    );
    assert!(
        wide_chart < narrow_chart,
        "and it takes that room from the candles, not from thin air"
    );
    for share in [
        f32::NAN,
        -1.0,
        5.0,
        MIN_LIVE_LANE_SHARE,
        MAX_LIVE_LANE_SHARE,
    ] {
        let (chart, lane) = pane_budgets(100, &lane_of(share));
        assert!(chart >= 2 && lane >= 2, "every pane can draw both sides");
        assert!(
            chart + lane <= 100,
            "the two shares together overspent the frame's budget"
        );
    }
    // A budget too small to split still leaves both panes able to draw.
    let (chart, lane) = pane_budgets(1, &lane_of(DEFAULT_LIVE_LANE_SHARE));
    assert!(chart >= 2 && lane >= 2);

    // With the tape switched off there is no second pane to protect, and
    // reserving a share for a band nobody is drawing would fold the candles
    // harder for nothing.
    let (all, none) = pane_budgets(
        100,
        &LiveLaneStyle {
            enabled: false,
            ..LiveLaneStyle::default()
        },
    );
    assert_eq!((all, none), (100, 0), "the candles get the whole budget");
}

/// The fold is paid where it costs least, and the rest of the pane is
/// untouched.
///
/// This is the rule the product owner chose over "merge everything
/// evenly": on the tape the newest prints at the right edge are what a
/// scalper is reading right now, so they stay one mark per execution and
/// the left edge — where a print was sliding out anyway — carries the
/// loss. On the candles the big prints carry the story, so the small ones
/// fold and the big ones are left exactly as they were.
#[test]
fn the_fold_spares_the_newest_on_the_tape_and_the_biggest_on_the_candles() {
    // Two budgets, because the sparing rule is about a pane that is *over*
    // its budget rather than swamped: past half over, everything folds and
    // there is no tail left to spare. This one squeezes the tape.
    let tape_pressure = project(
        &crowded_history(HeatmapConfig {
            max_aggression_primitives: 20,
            ..bubbles_only()
        }),
        &crowded_timeline(),
        PriceWindow::new(dec("98"), dec("120")).unwrap(),
    );
    // And this one squeezes only the candles.
    let projection = project(
        &crowded_history(HeatmapConfig {
            max_aggression_primitives: 62,
            ..bubbles_only()
        }),
        &crowded_timeline(),
        PriceWindow::new(dec("98"), dec("120")).unwrap(),
    );

    let lane: Vec<_> = tape_pressure
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert!(lane.len() >= 2, "the tape needs marks to compare");
    let newest = lane
        .iter()
        .max_by_key(|mark| mark.last_timestamp_ms)
        .expect("the tape has a newest mark");
    assert_eq!(
        newest.folded_marks, 0,
        "the newest print on the tape was folded into something else"
    );

    let candles: Vec<_> = projection
        .aggressions
        .iter()
        .filter(|mark| !mark.live)
        .collect();
    // A fold sums, so a fold of four small prints can out-weigh the
    // biggest single one — that is arithmetic, not a lost print. What has
    // to hold is that the biggest *print* is still drawn as itself: the
    // ladder tops out at seven contracts, and seven has to be on the canvas
    // as one untouched mark.
    assert!(candles.len() > 1, "the candles need marks to compare");
    assert_eq!(
        candles
            .iter()
            .filter(|mark| mark.folded_marks == 0)
            .map(|mark| mark.quantity)
            .max(),
        Some(dec("7")),
        "the biggest print on the candles was folded away"
    );
}

/// Zooming the candles is a statement about the candles. Every mark in the
/// lane must come out of the projection identical — same count, same
/// quantities, same folds — because nothing about the tape changed.
#[test]
fn zooming_the_chart_leaves_every_lane_primitive_alone() {
    let history = crowded_history(HeatmapConfig {
        display_grouping: DisplayGrouping::Adaptive { target_rows: 8 },
        ..bubbles_only()
    });
    let timeline = crowded_timeline();
    let lane_of = |prices: PriceWindow| {
        project(&history, &timeline, prices)
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .map(|mark| {
                (
                    mark.side,
                    mark.price_bucket,
                    mark.quantity,
                    mark.trade_count,
                )
            })
            .collect::<Vec<_>>()
    };
    // Both windows hold every price the ladder traded at, so the only
    // thing that differs between the two frames is the adaptive grouping
    // the candles resolved — which is exactly what must not reach the tape.
    let zoomed_in = lane_of(PriceWindow::new(dec("95"), dec("135")).unwrap());
    let zoomed_out = lane_of(PriceWindow::new(dec("90"), dec("240")).unwrap());
    assert!(!zoomed_in.is_empty(), "the tape must have marks to compare");
    assert_eq!(
        zoomed_in, zoomed_out,
        "the candles' zoom decided what the tape shows"
    );
}

/// And the mirror: the tape's window is a statement about the tape. Widen
/// it and the candles must draw exactly what they drew — the prints behind
/// the seam are the same prints, and they belong to the same bars.
#[test]
fn changing_the_tape_window_leaves_every_chart_primitive_alone() {
    let history = crowded_history(bubbles_only());
    let prices = PriceWindow::new(dec("98"), dec("120")).unwrap();
    let chart_of = |window_ms: i64| {
        let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
        let partial = bar(18_000, 21_000);
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&partial),
            Some(crate::LiveEdge {
                now_ms: 18_100,
                window_ms,
                reference_ms: 3_000,
                on_newest_bar: true,
            }),
        );
        project(&history, &timeline, prices)
            .aggressions
            .iter()
            .map(|mark| (mark.live, mark.quantity))
            .collect::<Vec<_>>()
    };
    let quick = chart_of(2_000);
    let slow = chart_of(9_000);
    assert!(
        !quick.is_empty() && !slow.is_empty(),
        "the candles must have marks to compare"
    );
    // Prints legitimately *move* between the panes as the tape's window
    // grows — that is what the window means. What may never happen is a
    // print leaving the canvas: whatever the speed, the two panes together
    // still account for every contract that traded.
    assert_eq!(
        quick.iter().map(|mark| mark.1).sum::<Decimal>(),
        slow.iter().map(|mark| mark.1).sum::<Decimal>(),
        "changing the tape's speed changed how much the frame says traded"
    );
}

/// The mission's invariant, over the whole grid a trader can put the chart
/// in: every grouping mode crossed with every tape speed, and both budget
/// regimes.
///
/// For each cell: the contracts drawn, plus the contracts an explicit
/// display floor removed, equal the contracts that traded inside the
/// window. Nothing else may go missing, whatever the zoom or the speed.
#[test]
fn conservation_holds_across_every_grouping_and_every_tape_speed() {
    let groupings = [
        DisplayGrouping::Native,
        DisplayGrouping::Multiple(4),
        DisplayGrouping::Adaptive { target_rows: 8 },
        DisplayGrouping::Adaptive { target_rows: 160 },
    ];
    let speeds = [1_500_i64, 3_000, 9_000, 21_000];
    // Roomy enough that nothing folds, and tight enough that everything
    // does — the invariant may not notice the difference.
    let budgets = [4_usize, 400];
    let prices = PriceWindow::new(dec("90"), dec("140")).unwrap();

    for display_grouping in groupings {
        for window_ms in speeds {
            for max_aggression_primitives in budgets {
                let history = crowded_history(HeatmapConfig {
                    display_grouping,
                    max_aggression_primitives,
                    ..bubbles_only()
                });
                let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
                let partial = bar(18_000, 21_000);
                let timeline = BarTimeline::from_bars(
                    0,
                    &closed,
                    Some(&partial),
                    Some(crate::LiveEdge {
                        now_ms: 18_100,
                        window_ms,
                        reference_ms: 3_000,
                        on_newest_bar: true,
                    }),
                );
                let projection = project(&history, &timeline, prices);
                let cell = format!(
                    "grouping {display_grouping:?}, window {window_ms} ms, budget \
                         {max_aggression_primitives}"
                );

                // Every print of the fixture is inside this price window and
                // inside the timeline, so the tape is the whole retained set.
                let traded: Decimal = history.aggressions().map(|trade| trade.quantity).sum();
                let drawn: Decimal = projection
                    .aggressions
                    .iter()
                    .map(|mark| mark.quantity)
                    .sum();
                assert_eq!(
                    drawn + projection.floored_quantity,
                    traded,
                    "contracts went missing at {cell}"
                );
                assert_eq!(
                    projection.floored_quantity,
                    Decimal::ZERO,
                    "no floor is set in this fixture, so nothing may be floored at {cell}"
                );
                // And a fold never crossed a bar to get there.
                for mark in projection.aggressions.iter().filter(|mark| !mark.live) {
                    assert_eq!(
                        timeline
                            .locate(mark.first_timestamp_ms)
                            .map(|position| position.bar_index),
                        timeline
                            .locate(mark.last_timestamp_ms)
                            .map(|position| position.bar_index),
                        "a candle mark spans two bars at {cell}"
                    );
                }
            }
        }
    }
}

/// The one discard left is the trader's own, and the frame says how big it
/// is — in contracts, because what matters is the size of what is missing
/// and not the number of dots.
#[test]
fn the_display_floor_is_the_only_thing_missing_and_it_is_declared() {
    let floored = project(
        &crowded_history(HeatmapConfig {
            bubbles: BubbleStyle {
                min_quantity: 5.0,
                readable_min_radius: 0.0,
                ..BubbleStyle::default()
            },
            ..bubbles_only()
        }),
        &crowded_timeline(),
        PriceWindow::new(dec("90"), dec("140")).unwrap(),
    );
    let history = crowded_history(bubbles_only());
    let traded: Decimal = history.aggressions().map(|trade| trade.quantity).sum();
    let drawn: Decimal = floored.aggressions.iter().map(|mark| mark.quantity).sum();

    assert!(
        floored.floored_quantity > Decimal::ZERO,
        "a floor of five over a ladder of ones has to remove something"
    );
    assert_eq!(
        drawn + floored.floored_quantity,
        traded,
        "the floor's residue does not account for what is off the canvas"
    );
    for mark in &floored.aggressions {
        assert!(
            mark.quantity >= dec("5"),
            "a mark under the floor survived it"
        );
    }
}

/// Reversibility, on both axes. The projection is a function of the window
/// it is handed, so a trader who zooms out to look around — or slows the
/// tape down and speeds it back up — finds exactly the frame they left.
#[test]
fn returning_to_a_window_or_a_speed_returns_its_frame() {
    let history = crowded_history(HeatmapConfig {
        display_grouping: DisplayGrouping::Adaptive { target_rows: 8 },
        max_aggression_primitives: 12,
        ..bubbles_only()
    });
    let frame_at = |window_ms: i64, prices: PriceWindow| {
        let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
        let partial = bar(18_000, 21_000);
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&partial),
            Some(crate::LiveEdge {
                now_ms: 18_100,
                window_ms,
                reference_ms: 3_000,
                on_newest_bar: true,
            }),
        );
        project(&history, &timeline, prices).aggressions
    };
    let home = PriceWindow::new(dec("99"), dec("104")).unwrap();
    let away = PriceWindow::new(dec("90"), dec("140")).unwrap();

    let first = frame_at(3_000, home);
    let _ = frame_at(3_000, away);
    assert_eq!(
        first,
        frame_at(3_000, home),
        "the same price window gave a different frame the second time"
    );

    let quick = frame_at(1_500, away);
    let _ = frame_at(21_000, away);
    assert_eq!(
        quick,
        frame_at(1_500, away),
        "the same tape speed gave a different frame the second time"
    );
}

/// No hysteresis: the projection is a function of the window it is given,
/// so coming back to a window comes back to its frame. A trader who zooms
/// out to look around and zooms back in must find the tape they left.
#[test]
fn returning_to_a_window_returns_its_frame() {
    let history = crowded_history(HeatmapConfig {
        display_grouping: DisplayGrouping::Adaptive { target_rows: 8 },
        max_aggression_primitives: 12,
        ..bubbles_only()
    });
    let timeline = crowded_timeline();
    let home = PriceWindow::new(dec("99"), dec("104")).unwrap();
    let away = PriceWindow::new(dec("90"), dec("140")).unwrap();
    let first = project(&history, &timeline, home);
    let _ = project(&history, &timeline, away);
    let back = project(&history, &timeline, home);
    assert_eq!(
        first.aggressions, back.aggressions,
        "the same window gave a different frame the second time"
    );
}

/// Wall-clock cost of one projection over a dense tape, printed for the
/// mission's performance gate. Ignored by default: it is a measurement,
/// not an assertion.
#[test]
#[ignore]
fn bench_projection_over_a_dense_tape() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        display_grouping: DisplayGrouping::Adaptive { target_rows: 128 },
        bubble_candle_summary: true,
        ..bubbles_only()
    });
    // 40 000 prints over 200 bars: a busy session, well past the budget.
    for i in 0..40_000_u64 {
        history.record_aggression(&Trade {
            agg_id: i + 1,
            timestamp_ms: 100 + i as i64 * 25,
            price: dec("100") + Decimal::from(i % 400),
            quantity: Decimal::from(i % 23 + 1),
            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        });
    }
    let closed: Vec<Bar> = (0..200)
        .map(|i| bar(i * 5_000, i * 5_000 + 4_999))
        .collect();
    let partial = bar(1_000_000, 1_005_000);
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        Some(&partial),
        Some(crate::LiveEdge {
            now_ms: 1_000_100,
            window_ms: 5_000,
            reference_ms: 5_000,
            on_newest_bar: true,
        }),
    );
    let prices = PriceWindow::new(dec("90"), dec("520")).unwrap();

    // Warm the caches the allocator and the CPU keep.
    for _ in 0..3 {
        let _ = project(&history, &timeline, prices);
    }
    let runs = 30;
    let started = std::time::Instant::now();
    let mut marks = 0;
    for _ in 0..runs {
        marks = project(&history, &timeline, prices).aggressions.len();
    }
    let per_run = started.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
    eprintln!("BENCH projection_ms_per_frame={per_run:.3} marks={marks}");
}

/// What one frame of the tape actually costs, under the shipped preset the
/// trader reported it lagging on.
///
/// The bench above times a whole projection; the app never pays that per
/// frame. It keeps the finished half cached and rebuilds only the moving
/// one, so `project_live` is the figure that decides whether the drawing
/// can keep up with the prints. Separated because the two answer different
/// questions and mixing them hides the one that matters at 60 Hz.
///
/// Ignored by default: a measurement, not an assertion.
#[test]
#[ignore]
fn bench_the_live_half_under_the_live_lane_pie_preset() {
    // `live lane pie`, from crates/app/config/bubbles.toml: the candle
    // summary on, half-second clustering on the candles and a tenth on the
    // tape, dust merged over a second and a half.
    let config = HeatmapConfig {
        enabled: true,
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        display_grouping: DisplayGrouping::Adaptive { target_rows: 128 },
        bubble_candle_summary: true,
        bubble_cluster_ms: 500,
        bubble_dust_merge_ms: 1_500,
        live_lane: LiveLaneStyle {
            enabled: true,
            cluster_ms: Some(100),
            ..LiveLaneStyle::default()
        },
        ..HeatmapConfig::default()
    };
    let mut history = LiquidityHistory::new(config);
    // 40 000 prints over 200 bars — a full retention window of a busy WIN
    // session, which is the load the tape has to draw at 60 Hz.
    for i in 0..40_000_u64 {
        history.record_aggression(&Trade {
            agg_id: i + 1,
            timestamp_ms: 100 + i as i64 * 25,
            price: dec("100") + Decimal::from(i % 400),
            quantity: Decimal::from(i % 23 + 1),
            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        });
    }
    let closed: Vec<Bar> = (0..200)
        .map(|i| bar(i * 5_000, i * 5_000 + 4_999))
        .collect();
    let partial = bar(1_000_000, 1_005_000);
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        Some(&partial),
        Some(crate::LiveEdge {
            now_ms: 1_000_100,
            window_ms: 5_000,
            reference_ms: 5_000,
            on_newest_bar: true,
        }),
    );
    let prices = PriceWindow::new(dec("90"), dec("520")).unwrap();

    // The half the app keeps: built once here, exactly as the cache does.
    let settled = project_settled(&history, &timeline, prices);
    for _ in 0..3 {
        let _ = project_live(&history, &timeline, prices, &settled);
    }
    let runs = 60;
    let started = std::time::Instant::now();
    let mut marks = 0;
    for _ in 0..runs {
        marks = project_live(&history, &timeline, prices, &settled)
            .aggressions
            .len();
    }
    let per_run = started.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
    eprintln!("BENCH live_ms_per_frame={per_run:.3} marks={marks}");
}
