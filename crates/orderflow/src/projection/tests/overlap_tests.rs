//! Golden tests for the overlap grid: each pane is cut into cells no wider
//! than a full-size disc and never across a bar, every mark in a cell folds
//! into one carrying the exact summed quantity, and nothing drawn overlaps.

use std::collections::BTreeMap;

use super::*;
use crate::PaneGeometry;
use crate::history::{AggressorSide, RestingSide};
use crate::{bubble_center_offset, bubble_radius};

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

/// Every chart in these tests is 400 px tall.
const HEIGHT_PX: f32 = 400.0;

fn geometry(px_per_bar: f32, lane_width_px: f32, lane_bar_opens: Vec<i64>) -> PaneGeometry {
    PaneGeometry {
        px_per_bar,
        lane_width_px,
        height_px: HEIGHT_PX,
        lane_bar_opens,
    }
}

/// Candles four pixels apart — a zoomed-out chart — and a 200 px tape over
/// the four one-second bars of [`timeline`].
fn small() -> PaneGeometry {
    geometry(4.0, 200.0, vec![0, 1_000, 2_000, 3_000])
}

/// Raw prints, one mark each, on a fixed scale where 5 contracts is a
/// full-size bubble — so every size in these tests is known in advance. The
/// budget is out of the way, so every mark is one print and its bar is known.
fn merging(on: bool) -> HeatmapConfig {
    let base = bubbles_only();
    HeatmapConfig {
        bubble_overlap_merge: on,
        max_aggression_primitives: 1_000_000,
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

/// What the painter draws: the grid's own list when there is one.
fn drawn(projection: &HeatmapProjection) -> &[AggressionPrimitive] {
    projection
        .overlap_marks
        .as_deref()
        .unwrap_or(&projection.aggressions)
}

fn total(projection: &HeatmapProjection) -> Decimal {
    drawn(projection).iter().map(|mark| mark.quantity).sum()
}

/// One disc as the painter draws it, in pane pixels, upright.
#[derive(Debug, Clone, Copy)]
struct Disc {
    live: bool,
    x: f64,
    y: f64,
    radius: f64,
}

/// The painter's own arithmetic: `layout.x`/`layout.y` without the offsets
/// every disc of a pane shares, the lean, and the radius range per pane,
/// held under the grid's cap.
fn discs(
    projection: &HeatmapProjection,
    geometry: &PaneGeometry,
    timeline: &BarTimeline,
    config: &HeatmapConfig,
) -> Vec<Disc> {
    let regions = timeline.region_count() as f64;
    let lane = config.live_lane.scaled_radii(&config.bubbles);
    let candle = (config.bubbles.min_radius, config.bubbles.max_radius);
    drawn(projection)
        .iter()
        .map(|mark| {
            let region = mark.x * regions;
            let x = if mark.live {
                (region - (regions - 1.0)) * f64::from(geometry.lane_width_px)
            } else {
                region * f64::from(geometry.px_per_bar)
            };
            let lean = bubble_center_offset(mark.buy_share, config.bubbles.side_offset, false);
            let (minimum, maximum) = if mark.live { lane } else { candle };
            let radius = bubble_radius(mark.size, minimum, maximum)
                .min(mark.radius_cap_px.unwrap_or(f32::INFINITY));
            Disc {
                live: mark.live,
                x,
                y: mark.y * f64::from(geometry.height_px) + f64::from(lean),
                radius: f64::from(radius),
            }
        })
        .collect()
}

/// No two discs of one pane overlap. The panes are clipped apart, so a tape
/// disc and a candle disc cannot.
fn assert_apart(discs: &[Disc]) {
    for (index, a) in discs.iter().enumerate() {
        for b in &discs[index + 1..] {
            if a.live != b.live {
                continue;
            }
            let distance = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            assert!(
                distance + 1e-3 >= a.radius + b.radius,
                "discs overlap: {a:?} and {b:?}, {distance:.2}px apart"
            );
        }
    }
}

/// A seeded linear congruential generator: random-looking, same every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

/// `count` prints over `from_ms..to_ms`, on whole prices 91..=109, both sides,
/// 1 to 12 contracts.
fn dense(seed: u64, count: u64, from_ms: i64, to_ms: i64) -> Vec<(u64, i64, String, String, Side)> {
    let mut random = Lcg(seed);
    let span = (to_ms - from_ms) as u64;
    let mut trades: Vec<_> = (0..count)
        .map(|id| {
            let side = if random.next(2) == 0 {
                Side::Buy
            } else {
                Side::Sell
            };
            (
                id + 1,
                from_ms + random.next(span) as i64,
                (91 + random.next(19)).to_string(),
                (1 + random.next(12)).to_string(),
                side,
            )
        })
        .collect();
    trades.sort_by_key(|trade| (trade.1, trade.0));
    trades
}

fn borrowed(trades: &[(u64, i64, String, String, Side)]) -> Vec<(u64, i64, &str, &str, Side)> {
    trades
        .iter()
        .map(|(id, ms, price, quantity, side)| (*id, *ms, price.as_str(), quantity.as_str(), *side))
        .collect()
}

/// A 20-point window on the 400 px chart: 20 px per price row.
fn wide_prices() -> PriceWindow {
    PriceWindow::new(dec("90"), dec("110")).unwrap()
}

/// `bars` closed one-second bars; the visible slice is `visible` of them from
/// the first, and the tape's `window_ms` ends in the last one.
fn seconds(bars: i64, visible: std::ops::Range<i64>, window_ms: i64) -> (BarTimeline, Vec<i64>) {
    let all: Vec<Bar> = (0..bars).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let shown = &all[visible.start as usize..visible.end as usize];
    let timeline = BarTimeline::from_bars(
        visible.start as usize,
        shown,
        None,
        Some(crate::LiveEdge {
            now_ms: bars * 1_000 - 100,
            window_ms,
            reference_ms: window_ms,
            on_newest_bar: visible.end == bars,
        }),
    );
    (timeline, all.iter().map(|bar| bar.open_time).collect())
}

/// The bar a print at `ms` belongs to, among one-second bars.
fn second_of(ms: i64) -> i64 {
    ms.div_euclid(1_000)
}

/// The per-bar invariants of a dense frame: nothing overlaps, every mark
/// holds prints of one bar only, and each bar keeps every contract per pane.
fn assert_dense_frame_is_clean(
    projection: &HeatmapProjection,
    geometry: &PaneGeometry,
    timeline: &BarTimeline,
    config: &HeatmapConfig,
    trades: &[(u64, i64, String, String, Side)],
) {
    assert_apart(&discs(projection, geometry, timeline, config));
    let bar_of: BTreeMap<u64, i64> = trades
        .iter()
        .map(|(id, ms, ..)| (*id, second_of(*ms)))
        .collect();
    let per_bar = |marks: &[AggressionPrimitive]| {
        let mut sums: BTreeMap<(bool, i64), Decimal> = BTreeMap::new();
        for mark in marks {
            let bars: Vec<i64> = mark.agg_ids.iter().map(|id| bar_of[id]).collect();
            assert!(
                bars.windows(2).all(|pair| pair[0] == pair[1]),
                "a mark mixes bars {bars:?}"
            );
            *sums.entry((mark.live, bars[0])).or_default() += mark.quantity;
        }
        sums
    };
    assert_eq!(
        per_bar(drawn(projection)),
        per_bar(&projection.aggressions),
        "every bar keeps its exact quantity, per pane"
    );
}

/// The trader's scene, in numbers: hundreds of prints, both panes crowded.
/// At every zoom the grid leaves no two discs over each other, never mixes
/// two bars in one mark, and loses not a contract.
#[test]
fn a_dense_frame_draws_no_overlapping_discs_at_any_zoom() {
    let config = merging(true);
    let (timeline, opens) = seconds(20, 0..20, 4_000);
    let trades = dense(7, 600, 0, 19_900);
    let original = project(
        &tape(config.clone(), &borrowed(&trades)),
        &timeline,
        wide_prices(),
    );
    assert!(original.aggressions.iter().any(|mark| mark.live));
    assert!(original.aggressions.iter().any(|mark| !mark.live));
    for (px_per_bar, lane_width_px) in [(4.0, 200.0), (12.0, 300.0), (40.0, 600.0)] {
        let geometry = geometry(px_per_bar, lane_width_px, opens.clone());
        let mut projection = original.clone();
        projection.merge_overlapping_bubbles(&geometry, &timeline, wide_prices(), &config);
        assert!(
            drawn(&projection).len() < original.aggressions.len(),
            "a crowded frame folds at {px_per_bar} px per bar"
        );
        assert_dense_frame_is_clean(&projection, &geometry, &timeline, &config, &trades);
    }
}

/// Zoomed out the way the trader was: bars two pixels wide — narrower than
/// the smallest disc — and a 16 s tape. A cell never spans a bar, so each
/// slot is one cell and its disc is held to the slot; nothing overlaps.
#[test]
fn a_zoomed_out_dense_frame_draws_no_overlapping_discs() {
    let config = merging(true);
    let (timeline, opens) = seconds(40, 0..40, 16_000);
    let trades = dense(11, 1_200, 0, 39_900);
    let geometry = geometry(2.0, 600.0, opens);
    let mut projection = project(
        &tape(config.clone(), &borrowed(&trades)),
        &timeline,
        wide_prices(),
    );
    let before = projection.aggressions.len();

    projection.merge_overlapping_bubbles(&geometry, &timeline, wide_prices(), &config);

    assert!(drawn(&projection).len() < before / 2, "the pile-up folded");
    for mark in drawn(&projection).iter().filter(|mark| !mark.live) {
        assert!(
            mark.radius_cap_px.is_some_and(|cap| cap <= 1.0 + 1e-6),
            "a disc in a 2 px slot is held to it: {:?}",
            mark.radius_cap_px
        );
    }
    assert_dense_frame_is_clean(&projection, &geometry, &timeline, &config, &trades);
}

/// Candles panned into history: the tape still shows now, but the bars its
/// prints belong to are off screen. The grid keys them by the true bar from
/// the series, so the lane is still binned — nothing overlaps and no mark
/// crosses a close the candles cannot see.
#[test]
fn a_panned_dense_frame_still_bins_the_tape_by_its_true_bars() {
    let config = merging(true);
    let (timeline, opens) = seconds(20, 0..5, 4_000);
    let trades = dense(3, 600, 0, 19_900);
    let geometry = geometry(12.0, 300.0, opens);
    let mut projection = project(
        &tape(config.clone(), &borrowed(&trades)),
        &timeline,
        wide_prices(),
    );
    let tape_before = projection.aggressions.iter().filter(|m| m.live).count();
    assert!(tape_before > 50, "the tape is crowded");

    projection.merge_overlapping_bubbles(&geometry, &timeline, wide_prices(), &config);

    let tape_after = drawn(&projection).iter().filter(|m| m.live).count();
    assert!(tape_after < tape_before, "the panned tape folded too");
    assert_dense_frame_is_clean(&projection, &geometry, &timeline, &config, &trades);
}

/// Two prints either side of a close the candles cannot see stay two marks;
/// three inside one unseen bar become one.
#[test]
fn a_panned_tape_folds_inside_an_unseen_bar_and_never_across_its_close() {
    let config = merging(true);
    let (timeline, opens) = seconds(6, 0..2, 1_500);
    let trades = [
        (1, 4_990, "100", "2", Side::Buy),
        (2, 5_010, "100", "3", Side::Sell),
        (3, 5_020, "100", "1", Side::Buy),
        (4, 5_030, "100", "1", Side::Sell),
    ];
    let mut projection = project(&tape(config.clone(), &trades), &timeline, prices());
    assert_eq!(projection.aggressions.iter().filter(|m| m.live).count(), 4);

    projection.merge_overlapping_bubbles(&geometry(4.0, 200.0, opens), &timeline, prices(), &config);

    let mut tape: Vec<_> = drawn(&projection).iter().filter(|m| m.live).collect();
    tape.sort_by_key(|mark| mark.agg_id);
    assert_eq!(tape.len(), 2, "one mark per unseen bar");
    assert_eq!(tape[0].agg_ids, vec![1]);
    assert_eq!(tape[1].agg_ids, vec![2, 3, 4]);
    assert_eq!(tape[1].quantity, dec("5"));
}

/// A buy and a sell twenty milliseconds apart on the tape share a cell: one
/// pie carrying both sides' exact quantities.
#[test]
fn a_buy_and_a_sell_in_one_cell_fold_into_one_pie() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 3_100, "100", "2", Side::Buy),
            (2, 3_120, "100", "3", Side::Sell),
        ],
    );
    assert_eq!(projection.aggressions.iter().filter(|m| m.live).count(), 2);
    let folded_before = projection.folded_aggressions;

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(drawn(&projection).len(), 1, "one pie, not two discs");
    let pie = &drawn(&projection)[0];
    assert!(pie.live, "a tape fold stays on the tape");
    assert_eq!(pie.quantity, dec("5"), "the summed quantity is exact");
    assert_eq!(pie.buy_quantity, dec("2"), "and so is the bought share");
    assert_eq!(pie.trade_count, 2);
    assert_eq!(pie.folded_marks, 2, "labelled as a fold of two marks");
    assert_eq!(pie.agg_ids, vec![1, 2]);
    assert!((pie.buy_share - 0.4).abs() < 1e-6, "{}", pie.buy_share);
    assert_eq!(pie.side, AggressorSide::Sell, "3 of 5 were sold");
    assert_eq!(pie.size, normalized_area_size(dec("5"), dec("5")));
    assert_eq!(pie.first_timestamp_ms, 3_100);
    assert_eq!(pie.last_timestamp_ms, 3_120);
    assert_eq!(
        projection.folded_aggressions, folded_before,
        "the budget's own counter says nothing about the canvas's fold"
    );
}

/// Six half-contract buys in under a tenth of a second share a cell and
/// become one bigger mark.
#[test]
fn small_same_cell_prints_fold_into_one_bigger_mark() {
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

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(drawn(&projection).len(), 1);
    let mark = &drawn(&projection)[0];
    assert_eq!(mark.quantity, dec("3"));
    assert_eq!(mark.buy_quantity, dec("3"));
    assert_eq!(mark.trade_count, 6);
    assert_eq!(mark.folded_marks, 6);
    assert_eq!(mark.agg_ids, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(mark.buy_share, 1.0, "one side in, no pie");
    assert!(mark.size > largest_before, "the fold reads bigger");
}

/// A mark alone in its cell keeps its data: it is not a fold, only held
/// inside the cell so it cannot reach a neighbour's.
#[test]
fn a_lone_mark_keeps_its_data() {
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

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    let marks = drawn(&projection);
    assert_eq!(marks.len(), 3);
    for (after, before) in marks.iter().zip(&original.aggressions) {
        assert_eq!(after.agg_ids, before.agg_ids);
        assert_eq!(after.quantity, before.quantity);
        assert_eq!(after.side, before.side);
        assert_eq!(after.size, before.size);
        assert_eq!(after.folded_marks, 0, "still a print");
        assert!(after.radius_cap_px.is_some(), "held to its cell");
    }
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

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(projection, original);
}

/// On the candles a cell may hold both sides but never two bars: a buy and a
/// sell inside one bar become its pie, the next bar keeps its own mark.
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

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(total(&projection), before, "not a contract is lost");
    assert_eq!(drawn(&projection).len(), 2, "one mark per bar");
    let first = &drawn(&projection)[0];
    assert_eq!(first.quantity, dec("5"));
    assert_eq!(first.buy_quantity, dec("3"));
    assert_eq!(first.agg_ids, vec![1, 2]);
    assert!((first.buy_share - 0.6).abs() < 1e-6);
    assert!(!first.live);
    let second = &drawn(&projection)[1];
    assert_eq!(second.quantity, dec("2"));
    assert_eq!(second.agg_ids, vec![3]);
    assert_eq!(second.folded_marks, 0, "alone, still a print");
}

/// The grid never mixes the panes: the newest candle mark and the oldest
/// tape mark sit side by side at the divider and stay two marks.
#[test]
fn the_grid_never_crosses_the_divider() {
    let config = merging(true);
    let mut projection = frame(
        &config,
        &[
            (1, 2_390, "100", "3", Side::Buy),
            (2, 2_410, "100", "3", Side::Buy),
        ],
    );
    let panes = |marks: &[AggressionPrimitive]| marks.iter().map(|m| m.live).collect::<Vec<_>>();
    assert_eq!(panes(&projection.aggressions), vec![false, true]);

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(panes(drawn(&projection)), vec![false, true]);
}

/// Same trades in, same marks drawn, whatever order the marks arrived in.
#[test]
fn the_grid_is_order_independent() {
    let config = merging(true);
    let (timeline, opens) = seconds(20, 0..20, 4_000);
    let trades = dense(5, 300, 0, 19_900);
    let geometry = geometry(12.0, 300.0, opens);
    let mut first = project(
        &tape(config.clone(), &borrowed(&trades)),
        &timeline,
        wide_prices(),
    );
    let mut second = first.clone();
    second.aggressions.reverse();
    first.merge_overlapping_bubbles(&geometry, &timeline, wide_prices(), &config);
    second.merge_overlapping_bubbles(&geometry, &timeline, wide_prices(), &config);
    assert!(first.overlap_marks.is_some());
    assert_eq!(first.overlap_marks, second.overlap_marks);
}

/// A mixed fold reports the side that took more, whichever mark anchored it.
/// A 3-lot sell with two 2-lot buys: 4 bought against 3 sold is a buy.
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

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(drawn(&projection).len(), 1);
    let pie = &drawn(&projection)[0];
    assert_eq!(pie.quantity, dec("7"));
    assert_eq!(pie.buy_quantity, dec("4"));
    assert_eq!(pie.side, AggressorSide::Buy, "4 of 7 were bought");
    assert_eq!(pie.consumed_side, RestingSide::Ask);
}

/// Every reader but the painter sees the frame the grid never touched: the
/// live strip sums quantities per price from `aggressions`.
#[test]
fn the_grid_leaves_the_marks_other_readers_use_untouched() {
    let config = merging(true);
    let original = frame(
        &config,
        &[
            (1, 3_100, "100", "2", Side::Buy),
            (2, 3_120, "100", "3", Side::Sell),
        ],
    );
    let mut projection = original.clone();

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(projection.aggressions, original.aggressions);
    assert_eq!(projection.folded_aggressions, original.folded_aggressions);
    assert_eq!(drawn(&projection).len(), 1, "only the painter's list folds");
}

/// A cluster that straddles a close is drawn at its midpoint, inside the
/// later bar — and it folds with that bar's marks.
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
    let tape_marks = projection.aggressions.iter().filter(|m| m.live).count();
    assert_eq!(tape_marks, 2, "the two buys cluster, the sell does not");

    projection.merge_overlapping_bubbles(
        &geometry(4.0, 200.0, vec![0, 1_000]),
        &timeline,
        prices(),
        &config,
    );

    let folded: Vec<_> = drawn(&projection).iter().filter(|m| m.live).collect();
    assert_eq!(folded.len(), 1, "drawn in one bar and one cell, one pie");
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

    projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);

    assert_eq!(drawn(&projection).len(), 1);
    let pie = &drawn(&projection)[0];
    assert_eq!(pie.quantity, dec("2000000.0001"));
    assert_eq!(pie.side, AggressorSide::Buy, "0.0001 more was bought");
    assert_eq!(pie.consumed_side, RestingSide::Ask);
}

/// The painter ignores the grid while a side is hidden or no bubble layer is
/// drawn, so the grid is not built then: nothing to draw, nothing to pay for.
#[test]
fn the_grid_is_not_built_when_the_painter_would_not_draw_it() {
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
        projection.merge_overlapping_bubbles(&small(), &timeline(), prices(), &config);
        assert_eq!(projection.overlap_marks, None);
    }
}
