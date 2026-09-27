//! Golden tests for the overlap fold: bubbles whose drawn discs would touch
//! become one mark carrying their exact summed quantity, and nothing else
//! about the frame moves.

use super::*;
use crate::PaneGeometry;
use crate::history::AggressorSide;

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

fn total(projection: &HeatmapProjection) -> Decimal {
    projection
        .aggressions
        .iter()
        .map(|mark| mark.quantity)
        .sum()
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

    assert_eq!(projection.aggressions.len(), 1, "one pie, not two discs");
    let pie = &projection.aggressions[0];
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
        projection.folded_aggressions,
        folded_before + 1,
        "the frame counts the mark it folded"
    );
}

/// Six half-contract buys in under a tenth of a second pile up as six specks
/// over each other. They fold into one bigger mark — and the merge keeps
/// going as the fold grows, so it is one mark, not three pairs.
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

    assert_eq!(projection.aggressions.len(), 1);
    let mark = &projection.aggressions[0];
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

    assert_eq!(projection, original);
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
    assert_eq!(projection.aggressions.len(), 2, "one mark per bar");
    let first = &projection.aggressions[0];
    assert_eq!(first.quantity, dec("5"));
    assert_eq!(first.agg_ids, vec![1, 2]);
    assert!((first.buy_share - 0.6).abs() < 1e-6);
    assert!(!first.live);
    let second = &projection.aggressions[1];
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

    assert_eq!(projection, original);
}

/// Same trades in, same frame out: the fold is ordered by the marks
/// themselves, never by the order they arrived in.
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
    assert_eq!(first, second);
}
