//! Golden tests for the overlap fold: bubbles whose drawn discs would touch
//! become one mark carrying their exact summed quantity, and nothing else
//! about the frame moves.

use super::*;
use crate::PaneGeometry;
use crate::bubble_radius;
use crate::history::{AggressorSide, RestingSide};
use rust_decimal::prelude::ToPrimitive as _;

/// Four closed one-second bars and a tape covering the last 1.5 s of them.
fn timeline() -> BarTimeline {
    let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    )
}

fn prices() -> PriceWindow {
    PriceWindow::new(dec("98"), dec("103")).unwrap()
}

/// Candles four pixels apart — a zoomed-out chart — a 200 px tape, and a
/// 400 px tall chart: 80 px per price unit.
const GEOMETRY: PaneGeometry = PaneGeometry {
    px_per_bar: 4.0,
    lane_width_px: 200.0,
    height_px: 400.0,
};

/// Raw prints, one mark each, on a fixed scale where 5 contracts is a
/// full-size bubble — so every size in these tests is known in advance.
fn merging(on: bool) -> HeatmapConfig {
    let base = bubbles_only();
    HeatmapConfig {
        bubble_overlap_merge: on,
        bubbles: BubbleStyle {
            size_reference: BubbleSizeReference::Fixed,
            size_reference_quantity: 5.0,
            ..base.bubbles.clone()
        },
        ..base
    }
}

fn frame(config: &HeatmapConfig, trades: &[(u64, i64, &str, &str, Side)]) -> HeatmapProjection {
    project(&tape(config.clone(), trades), &timeline(), prices())
}

/// What the painter draws: the fold's own list when there is one.
fn drawn(projection: &HeatmapProjection) -> &[AggressionPrimitive] {
    projection
        .overlap_marks
        .as_deref()
        .unwrap_or(&projection.aggressions)
}

fn total(projection: &HeatmapProjection) -> Decimal {
    drawn(projection).iter().map(|mark| mark.quantity).sum()
}

/// A buy and a sell twenty milliseconds apart on the tape sit under three
/// pixels apart: today they draw as two discs, one on top of the other. With
/// the fold on they are one pie carrying both sides' exact quantities.
#[test]
fn a_buy_and_a_sell_that_overlap_on_the_tape_fold_into_one_pie() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 3_100, "100", "2", Side::Buy),
            (2, 3_120, "100", "3", Side::Sell),
        ],
    );
    assert_eq!(
        projection.aggressions.iter().filter(|m| m.live).count(),
        2,
        "the fixture must start as two tape marks"
    );
    let folded_before = projection.folded_aggressions;

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection).len(), 1, "one pie, not two discs");
    let pie = &drawn(&projection)[0];
    assert!(pie.live, "a tape fold stays on the tape");
    assert_eq!(pie.quantity, dec("5"), "the summed quantity is exact");
    assert_eq!(pie.trade_count, 2);
    assert_eq!(pie.folded_marks, 2, "labelled as a fold of two marks");
    assert_eq!(pie.agg_ids, vec![1, 2]);
    assert!(
        (pie.buy_share - 0.4).abs() < 1e-6,
        "2 bought of 5: the pie is 40% buy, got {}",
        pie.buy_share
    );
    assert_eq!(
        pie.side,
        AggressorSide::Sell,
        "anchored on the heavier side"
    );
    assert_eq!(pie.size, normalized_area_size(dec("5"), dec("5")));
    assert_eq!(pie.first_timestamp_ms, 3_100);
    assert_eq!(pie.last_timestamp_ms, 3_120);
    assert_eq!(
        projection.folded_aggressions, folded_before,
        "the budget's own counter says nothing about the canvas's fold"
    );
}

/// Six half-contract buys in under a tenth of a second pile up as six specks
/// over each other, all inside the first one's disc. They fold into one
/// bigger mark anchored on it.
#[test]
fn small_same_side_prints_close_in_time_fold_into_one_bigger_mark() {
    let config = merging(true);
    let trades: Vec<(u64, i64, &str, &str, Side)> = (0..6)
        .map(|k| (k + 1, 3_100 + 15 * k as i64, "100", "0.5", Side::Buy))
        .collect();
    let mut projection = frame(&config, &trades);
    assert_eq!(projection.aggressions.len(), 6, "six raw marks to start");
    let largest_before = projection
        .aggressions
        .iter()
        .map(|mark| mark.size)
        .fold(0.0_f32, f32::max);

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection).len(), 1);
    let mark = &drawn(&projection)[0];
    assert_eq!(mark.quantity, dec("3"));
    assert_eq!(mark.trade_count, 6);
    assert_eq!(mark.folded_marks, 6);
    assert_eq!(mark.agg_ids, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(mark.buy_share, 1.0, "one side in, no pie");
    assert!(
        mark.size > largest_before,
        "the fold reads bigger than any mark it absorbed"
    );
}

/// Marks that do not touch are exactly what they were: the fold is a
/// drawing fix for a pile-up, not a coarser tape.
#[test]
fn marks_that_do_not_overlap_are_untouched() {
    let config = merging(true);
    let original = frame(
        &config,
        &[
            (1, 1_500, "100", "2", Side::Buy),
            (2, 3_100, "99", "2", Side::Buy),
            (3, 3_800, "102", "3", Side::Sell),
        ],
    );
    let mut projection = original.clone();

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection), original.aggressions.as_slice());
}

/// Off draws today's frame, bit for bit, however crowded it is.
#[test]
fn off_reproduces_the_frame_unchanged() {
    let config = merging(false);
    let original = frame(
        &config,
        &[
            (1, 3_100, "100", "2", Side::Buy),
            (2, 3_120, "100", "3", Side::Sell),
            (3, 1_200, "100", "3", Side::Buy),
            (4, 1_300, "100", "2", Side::Sell),
        ],
    );
    let mut projection = original.clone();

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(projection, original);
}

/// On the candles the fold may cross sides but never a bar: a mark drawn in
/// a bar's slot says that bar traded it, so two bars four pixels apart keep
/// a mark each even though their discs touch — while a buy and a sell inside
/// one bar become its pie.
#[test]
fn candle_marks_fold_inside_a_bar_and_never_across_one() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 1_200, "100", "3", Side::Buy),
            (2, 1_300, "100", "2", Side::Sell),
            (3, 2_100, "100", "2", Side::Buy),
        ],
    );
    assert_eq!(projection.aggressions.len(), 3);
    let before = total(&projection);

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(total(&projection), before, "not a contract is lost");
    assert_eq!(drawn(&projection).len(), 2, "one mark per bar");
    let first = &drawn(&projection)[0];
    assert_eq!(first.quantity, dec("5"));
    assert_eq!(first.agg_ids, vec![1, 2]);
    assert!((first.buy_share - 0.6).abs() < 1e-6);
    assert!(!first.live);
    let second = &drawn(&projection)[1];
    assert_eq!(second.quantity, dec("2"));
    assert_eq!(second.agg_ids, vec![3]);
    assert_eq!(second.folded_marks, 0, "untouched, still a print");
}

/// The fold never mixes the panes: the newest candle mark and the oldest
/// tape mark can sit side by side at the divider, and they stay two marks.
#[test]
fn the_fold_never_crosses_the_divider() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 2_390, "100", "3", Side::Buy),
            (2, 2_410, "100", "3", Side::Buy),
        ],
    );
    assert_eq!(
        projection
            .aggressions
            .iter()
            .map(|mark| mark.live)
            .collect::<Vec<_>>(),
        vec![false, true],
        "the fixture straddles the lane's left edge"
    );
    let original = projection.clone();

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection), original.aggressions.as_slice());
}

/// Same trades in, same marks drawn: the fold puts its input in frame order
/// itself, so the order the marks arrived in never decides a fold.
#[test]
fn the_fold_is_deterministic() {
    let config = merging(true);
    let trades = [
        (1, 3_100, "100", "2", Side::Buy),
        (2, 3_120, "100", "3", Side::Sell),
        (3, 3_130, "101", "1", Side::Buy),
        (4, 1_200, "100", "3", Side::Buy),
        (5, 1_300, "100", "2", Side::Sell),
    ];
    let mut first = frame(&config, &trades);
    let mut second = frame(&config, &trades);
    second.aggressions.reverse();
    first.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);
    second.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);
    assert_eq!(first.overlap_marks, second.overlap_marks);
    assert!(first.overlap_marks.is_some());
}

/// A leg of prints, each touching the next but not the one after: a rally
/// drawn as a string of beads. The fold merges a mark only into a neighbour
/// its own disc touches, so the leg stays a leg — a fold that chained through
/// the string swallowed a whole 180-point rally into one mark and hid where
/// the aggression happened.
#[test]
fn a_chain_of_touching_marks_does_not_fold_into_one() {
    let config = merging(true);
    let (_, lane_max) = config.live_lane.scaled_radii(&config.bubbles);
    // Full-size prints, so every disc is `lane_max` across; one price unit
    // apart at 1.5 radii — a neighbour touches, the next one over does not.
    let radius = f64::from(bubble_radius(1.0, 0.0, lane_max));
    let row_px = 1.5 * radius;
    let span = Decimal::from_f64(f64::from(GEOMETRY.height_px) / row_px)
        .unwrap()
        .round_dp(4);
    let window = PriceWindow::new(dec("99"), dec("99") + span).unwrap();
    let trades: Vec<(u64, i64, String)> = (0..5)
        .map(|k| (k + 1, 3_100, (100 + k).to_string()))
        .collect();
    let trades: Vec<(u64, i64, &str, &str, Side)> = trades
        .iter()
        .map(|(id, ms, price)| (*id, *ms, price.as_str(), "5", Side::Buy))
        .collect();
    let mut projection = project(&tape(config.clone(), &trades), &timeline(), window);
    assert_eq!(projection.aggressions.len(), 5, "five beads to start");

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert!(
        drawn(&projection).len() > 1,
        "the leg must not collapse into one mark"
    );
    assert_eq!(total(&projection), dec("25"), "not a contract is lost");
    let px_per_unit = f64::from(GEOMETRY.height_px) / span.to_f64().unwrap();
    for mark in drawn(&projection) {
        // The members' rows, edge to edge, are the whole band; a direct
        // neighbour of the anchor is at most two radii away from it.
        let reach = (mark.price_span - Decimal::ONE).to_f64().unwrap() * px_per_unit;
        assert!(
            reach < 2.0 * radius,
            "a fold reached {reach:.1}px past its anchor, beyond one disc's touch"
        );
    }
}

/// Raw prints drawn as 5 px discs, nudged 3 px toward their own book half —
/// the numbers the lean bug was reported with.
fn five_pixel_discs() -> HeatmapConfig {
    let config = merging(true);
    HeatmapConfig {
        bubbles: BubbleStyle {
            min_radius: 5.0,
            max_radius: 5.0,
            side_offset: 3.0,
            ..config.bubbles.clone()
        },
        live_lane: LiveLaneStyle {
            radius_scale: 1.0,
            ..config.live_lane.clone()
        },
        ..config
    }
}

/// A 50-point window on a 400 px chart: one price row is 8 px.
fn eight_pixel_rows() -> PriceWindow {
    PriceWindow::new(dec("80"), dec("130")).unwrap()
}

/// The fold measures a disc where the painter draws it. A buy leans up,
/// toward the asks, and a sell leans down: a buy one row above a sell sits
/// 8 + 3 + 3 = 14 px from it, clear of two 5 px radii, so they stay two marks.
#[test]
fn a_buy_a_row_above_a_sell_leans_away_and_does_not_fold() {
    let config = five_pixel_discs();
    let trades = [
        (1, 3_100, "101", "2", Side::Buy),
        (2, 3_100, "100", "2", Side::Sell),
    ];
    let mut projection = project(
        &tape(config.clone(), &trades),
        &timeline(),
        eight_pixel_rows(),
    );
    assert_eq!(projection.aggressions.len(), 2);

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection).len(), 2, "14 px apart, two marks");
}

/// A sell one row above a buy leans toward it: 8 - 3 - 3 = 2 px apart, well
/// inside two 5 px radii. They are drawn over each other, so they are one pie.
#[test]
fn a_sell_a_row_above_a_buy_leans_into_it_and_folds() {
    let config = five_pixel_discs();
    let trades = [
        (1, 3_100, "101", "2", Side::Sell),
        (2, 3_100, "100", "2", Side::Buy),
    ];
    let mut projection = project(
        &tape(config.clone(), &trades),
        &timeline(),
        eight_pixel_rows(),
    );
    assert_eq!(projection.aggressions.len(), 2);

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection).len(), 1, "2 px apart, one pie");
    assert_eq!(drawn(&projection)[0].quantity, dec("4"));
}

/// A mixed fold reports the side that took more, whichever mark anchored it
/// — the rule the cluster fold already follows. A 3-lot sell anchors two
/// 2-lot buys: 4 bought against 3 sold is a buy.
#[test]
fn a_mixed_fold_reports_the_side_that_took_more() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 3_100, "100", "3", Side::Sell),
            (2, 3_110, "100", "2", Side::Buy),
            (3, 3_120, "100", "2", Side::Buy),
        ],
    );
    assert_eq!(projection.aggressions.len(), 3);

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection).len(), 1);
    let pie = &drawn(&projection)[0];
    assert_eq!(pie.quantity, dec("7"));
    assert_eq!(pie.side, AggressorSide::Buy, "4 of 7 were bought");
    assert_eq!(pie.consumed_side, RestingSide::Ask);
}

/// Every reader but the painter sees the frame the fold never touched: the
/// live strip sums quantities per price from `aggressions`, and a fold that
/// rewrote them would move a sell's contracts onto the buy's row.
#[test]
fn the_fold_leaves_the_marks_other_readers_use_untouched() {
    let config = merging(true);
    let original = frame(
        &config,
        &[
            (1, 3_100, "100", "2", Side::Buy),
            (2, 3_120, "100", "3", Side::Sell),
        ],
    );
    let mut projection = original.clone();

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(projection.aggressions, original.aggressions);
    assert_eq!(projection.folded_aggressions, original.folded_aggressions);
    assert_eq!(drawn(&projection).len(), 1, "only the painter's list folds");
}

/// The tape is continuous, but its prints still belong to bars: a buy in the
/// last millisecond of one bar and a sell in the first of the next touch on
/// the tape and stay two marks, so no fold claims volume across a close.
#[test]
fn a_tape_fold_never_spans_two_bars() {
    let config = merging(true);
    let closed = [bar(0, 999), bar(1_000, 1_999)];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 1_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let trades = [
        (1, 999, "100", "2", Side::Buy),
        (2, 1_001, "100", "3", Side::Sell),
    ];
    let mut projection = project(&tape(config.clone(), &trades), &timeline, prices());
    assert_eq!(
        projection
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .count(),
        2,
        "both prints are on the tape"
    );

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline, &config);

    assert_eq!(
        drawn(&projection).iter().filter(|mark| mark.live).count(),
        2,
        "one mark per bar, touching or not"
    );
}

/// Candles panned into history: the tape still shows now, but the bars its
/// prints belong to are off screen, so the timeline cannot say where one
/// ends. Two prints either side of an unseen close stay two marks — a bar
/// the fold cannot see is never guessed, and never clamped onto the last
/// visible slot.
#[test]
fn a_panned_view_never_folds_tape_prints_across_an_unseen_close() {
    let config = merging(true);
    let closed = [bar(0, 999), bar(1_000, 1_999)];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 5_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: false,
        }),
    );
    let trades = [
        (1, 4_999, "100", "2", Side::Buy),
        (2, 5_001, "100", "3", Side::Sell),
    ];
    let mut projection = project(&tape(config.clone(), &trades), &timeline, prices());
    assert_eq!(
        projection
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .count(),
        2,
        "both prints are on the tape"
    );

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline, &config);

    assert_eq!(
        drawn(&projection).iter().filter(|mark| mark.live).count(),
        2,
        "no visible bar to put them in, so no fold"
    );
}

/// A cluster that straddles a close is drawn at its midpoint, inside the
/// later bar — and it folds with that bar's marks, not with the bar its
/// first print happened to open in.
#[test]
fn a_cluster_straddling_a_close_folds_with_the_bar_it_is_drawn_in() {
    let config = HeatmapConfig {
        bubble_cluster_ms: 100,
        ..merging(true)
    };
    let closed = [bar(0, 999), bar(1_000, 1_999)];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 1_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let trades = [
        (1, 990, "100", "1", Side::Buy),
        (2, 1_030, "100", "1", Side::Buy),
        (3, 1_040, "100", "1", Side::Sell),
    ];
    let mut projection = project(&tape(config.clone(), &trades), &timeline, prices());
    let tape_marks: Vec<_> = projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert_eq!(
        tape_marks.len(),
        2,
        "the two buys cluster, the sell does not"
    );
    assert!(
        tape_marks
            .iter()
            .any(|mark| mark.first_timestamp_ms == 990 && mark.trade_count == 2),
        "the buy cluster opens in bar 0 and is drawn at 1 010, in bar 1"
    );

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline, &config);

    let folded: Vec<_> = drawn(&projection).iter().filter(|m| m.live).collect();
    assert_eq!(folded.len(), 1, "drawn in one bar, touching, one pie");
    assert_eq!(folded[0].quantity, dec("3"));
}

/// The side a mixed fold reports is decided on exact quantities. A million
/// sold against 1 000 000.0001 bought is a buy — a margin an f32 share of
/// 0.500000025 rounds to an even split, which would keep the sell anchor.
#[test]
fn a_mixed_fold_decides_its_side_on_exact_quantities() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 3_100, "100", "1000000", Side::Sell),
            (2, 3_110, "100", "500000.00005", Side::Buy),
            (3, 3_120, "100", "500000.00005", Side::Buy),
        ],
    );
    assert_eq!(projection.aggressions.len(), 3);

    projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);

    assert_eq!(drawn(&projection).len(), 1);
    let pie = &drawn(&projection)[0];
    assert_eq!(pie.quantity, dec("2000000.0001"));
    assert_eq!(pie.side, AggressorSide::Buy, "0.0001 more was bought");
    assert_eq!(pie.consumed_side, RestingSide::Ask);
}

/// The painter ignores the fold while a side is hidden or no bubble layer is
/// drawn, so the fold is not built then: nothing to draw, nothing to pay for.
#[test]
fn the_fold_is_not_built_when_the_painter_would_not_draw_it() {
    let trades = [
        (1, 3_100, "100", "2", Side::Buy),
        (2, 3_120, "100", "3", Side::Sell),
    ];
    let one_side_hidden = HeatmapConfig {
        show_sell_aggressions: false,
        ..merging(true)
    };
    let no_bubbles = HeatmapConfig {
        show_aggressions: false,
        live_lane: LiveLaneStyle {
            show_aggressions: false,
            ..merging(true).live_lane
        },
        ..merging(true)
    };
    for config in [one_side_hidden, no_bubbles] {
        let mut projection = frame(&merging(true), &trades);
        projection.merge_overlapping_bubbles(GEOMETRY, &timeline(), &config);
        assert_eq!(projection.overlap_marks, None);
    }
}
