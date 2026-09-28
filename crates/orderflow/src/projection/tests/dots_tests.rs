//! Golden tests for Bookmap-style volume dots: every print lands in the dot
//! keyed by its bar, a window of market time anchored at exchange epoch 0,
//! and its native price level. Both sides share one dot, drawn as a pie.
//! The key is market data, so a closed window's dot is the same fact on
//! every frame, however the chart rolls, pans, refits or is cut.

use super::*;
use crate::bubble_radius;
use crate::history::AggressorSide;
use crate::projection::{
    DOT_WINDOW_LADDER_MS, PaneGeometry, VolumeDots, dot_window_ms, project_with_dots,
};

/// Dots on, the budget out of the way, and a fixed scale where 10
/// contracts is a full-size dot. The folds dots mode skips are all switched
/// on here, so a test proves they are skipped rather than merely unset.
fn dots_config() -> HeatmapConfig {
    let base = bubbles_only();
    HeatmapConfig {
        bubble_overlap_merge: true,
        bubble_candle_summary: true,
        bubble_region_rows: 3,
        bubble_dust_merge_ms: 3_000,
        bubble_cluster_ms: 500,
        max_aggression_primitives: 1_000_000,
        bubbles: BubbleStyle {
            size_reference: BubbleSizeReference::VisibleMax,
            size_reference_quantity: 10.0,
            readable_min_radius: 6.0,
            ..base.bubbles.clone()
        },
        ..base
    }
}

/// Windows fixed by the test rather than by a zoom: 500 ms on the candles,
/// 250 ms on the tape, and the series' one-second bar opens.
fn windows(candle_window_ms: i64, tape_window_ms: i64) -> VolumeDots {
    VolumeDots {
        candle_window_ms,
        tape_window_ms,
        lane_bar_opens: (0..40).map(|i| i * 1_000).collect(),
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
/// 1 to 12 contracts, some with a decimal part.
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
                format!("{}.{}", 1 + random.next(12), random.next(10)),
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

/// The chart at `now_ms` over one-second bars: every bar that has closed,
/// the one still forming, and a tape `lane_ms` long. `visible` picks the
/// bars on screen, by index; a slice short of the newest is a chart panned
/// into history.
fn chart(now_ms: i64, lane_ms: i64, visible: Option<std::ops::Range<usize>>) -> BarTimeline {
    let forming = (now_ms / 1_000) as usize;
    let mut all: Vec<Bar> = (0..forming as i64)
        .map(|i| bar(i * 1_000, i * 1_000 + 999))
        .collect();
    all.push(bar(forming as i64 * 1_000, now_ms));
    let range = visible.unwrap_or(0..all.len());
    let on_newest_bar = range.end == all.len();
    let shown = &all[range.clone()];
    let (closed, partial) = if on_newest_bar {
        (&shown[..shown.len() - 1], shown.last())
    } else {
        (shown, None)
    };
    BarTimeline::from_bars(
        range.start,
        closed,
        partial,
        Some(crate::LiveEdge {
            now_ms,
            window_ms: lane_ms,
            reference_ms: lane_ms,
            on_newest_bar,
        }),
    )
}

fn frame_at(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    dots: &VolumeDots,
) -> HeatmapProjection {
    project_with_dots(history, timeline, prices, Some(dots))
}

/// A dot as a market fact: everything but where the frame put it on screen.
fn fact(mark: &AggressionPrimitive) -> AggressionPrimitive {
    AggressionPrimitive {
        x: 0.0,
        y: 0.0,
        ..mark.clone()
    }
}

fn facts(
    projection: &HeatmapProjection,
    keep: impl Fn(&AggressionPrimitive) -> bool,
) -> Vec<AggressionPrimitive> {
    let mut facts: Vec<_> = projection
        .aggressions
        .iter()
        .filter(|mark| keep(mark))
        .map(fact)
        .collect();
    facts.sort_by(|a, b| a.agg_ids.cmp(&b.agg_ids));
    facts
}

fn prices(low: &str, high: &str) -> PriceWindow {
    PriceWindow::new(dec(low), dec(high)).unwrap()
}

/// The past does not move. A candle dot whose window has closed, and which
/// the tape has released, is the same fact — down to its ids, its size and
/// its side — as the tape rolls, a bar forms and closes, the seam moves, the
/// chart pans in price or in time, the axis refits and the tape is cut to a
/// different length. So is a closed tape dot both frames still show.
#[test]
fn a_closed_window_is_the_same_dot_on_every_frame() {
    let config = HeatmapConfig {
        // An axis refit may re-bucket the display grouping; dots key on the
        // native level, so it may not re-bucket a dot.
        display_grouping: DisplayGrouping::Adaptive { target_rows: 12 },
        ..dots_config()
    };
    let trades = dense(7, 3_000, 0, 20_000);
    let history = tape(config.clone(), &borrowed(&trades));
    let dots = windows(500, 250);
    let at = |now_ms: i64, lane_ms: i64, visible: Option<std::ops::Range<usize>>, window| {
        frame_at(&history, &chart(now_ms, lane_ms, visible), window, &dots)
    };
    let frames = [
        at(8_300, 1_500, None, prices("90", "110")),
        at(12_700, 1_500, None, prices("90", "110")),
        at(12_700, 3_000, None, prices("90", "110")),
        at(12_700, 1_500, None, prices("88", "108")),
        at(12_700, 1_500, None, prices("70", "130")),
        at(12_700, 1_500, Some(2..10), prices("90", "110")),
    ];
    // Bars 2..6, which every frame shows, and windows the tape of every frame
    // has let go of.
    let closed = |mark: &AggressionPrimitive| {
        !mark.live && mark.first_timestamp_ms >= 2_000 && mark.last_timestamp_ms < 6_000
    };
    let reference = facts(&frames[0], closed);
    assert!(reference.len() > 100, "the fixture draws closed dots");
    assert!(
        reference
            .iter()
            .any(|dot| dot.buy_share > 0.0 && dot.buy_share < 1.0),
        "and some of them are pies"
    );
    for (index, frame) in frames.iter().enumerate() {
        assert!(frame.volume_dots, "frame {index} was built as dots");
        assert_eq!(facts(frame, closed), reference, "frame {index} moved a dot");
    }

    // The tape: two moments 200 ms apart, the same cut. A tape window both
    // show whole, and which has closed in both, is one fact.
    let early = at(12_700, 3_000, None, prices("90", "110"));
    let later = at(12_900, 3_000, None, prices("90", "110"));
    let closed_on_tape = |mark: &AggressionPrimitive| {
        mark.live && mark.first_timestamp_ms >= 10_000 && mark.last_timestamp_ms < 12_500
    };
    let on_tape = facts(&early, closed_on_tape);
    assert!(on_tape.len() > 20, "the tape draws closed dots");
    assert_eq!(facts(&later, closed_on_tape), on_tape);
}

/// Every dot is one bar, one window and one native price level; the folds
/// dots replace — dust, regions, the closed-bar summary — never run, so no
/// contract is drawn twice and every one visible is drawn once. They paint
/// smallest first, so the biggest dot is on top.
#[test]
fn a_dense_frame_keys_every_print_once() {
    let config = dots_config();
    let trades = dense(11, 3_000, 0, 20_000);
    let history = tape(config.clone(), &borrowed(&trades));
    let dots = windows(500, 250);
    let timeline = chart(12_700, 1_500, None);
    let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
    let drawn: Decimal = frame.aggressions.iter().map(|dot| dot.quantity).sum();
    let traded: Decimal = trades
        .iter()
        .filter(|trade| trade.1 <= 12_700)
        .map(|trade| dec(&trade.3))
        .sum();
    assert_eq!(drawn, traded, "every contract, once");
    for dot in &frame.aggressions {
        assert_eq!(dot.price_span, Decimal::ONE, "one native level");
        assert_eq!(dot.folded_marks, 0, "a dot is not a fold");
        assert_eq!(dot.trade_count, dot.agg_ids.len());
        let window = if dot.live { 250 } else { 500 };
        assert_eq!(
            dot.first_timestamp_ms.div_euclid(window),
            dot.last_timestamp_ms.div_euclid(window),
            "one window: {dot:?}"
        );
        assert_eq!(
            dot.first_timestamp_ms.div_euclid(1_000),
            dot.last_timestamp_ms.div_euclid(1_000),
            "one bar: {dot:?}"
        );
        let buys: Decimal = trades
            .iter()
            .filter(|trade| dot.agg_ids.contains(&trade.0) && trade.4 == Side::Buy)
            .map(|trade| dec(&trade.3))
            .sum();
        assert_eq!(dot.buy_quantity, buys, "the bought share is exact");
    }
    assert!(
        frame
            .aggressions
            .windows(2)
            .all(|pair| pair[0].quantity <= pair[1].quantity),
        "the biggest dot paints last"
    );
}

/// A zoom picks the smallest rung of the ladder at least one full dot wide
/// on screen, so the window changes only where a rung starts or stops
/// fitting — and never with anything but the scale.
#[test]
fn a_zoom_changes_the_window_only_at_ladder_steps() {
    let dot_px = 20.0;
    let mut previous = DOT_WINDOW_LADDER_MS[0];
    for step in 1..4_000 {
        let ms_per_px = f64::from(step) * 0.25;
        let window = dot_window_ms(ms_per_px, dot_px);
        assert!(DOT_WINDOW_LADDER_MS.contains(&window), "{window} is a rung");
        assert!(window >= previous, "zooming out never shrinks the window");
        let fits = |rung: i64| rung as f64 / ms_per_px >= dot_px;
        if fits(window) {
            assert!(
                DOT_WINDOW_LADDER_MS
                    .iter()
                    .filter(|rung| **rung < window)
                    .all(|rung| !fits(*rung)),
                "{window} ms at {ms_per_px} ms/px is not the smallest that fits"
            );
        } else {
            assert_eq!(window, *DOT_WINDOW_LADDER_MS.last().unwrap(), "the widest");
        }
        previous = window;
    }

    // 10 px radius: a dot is 20 px. The tape shows 1.5 s over 300 px (5 ms a
    // pixel, 100 ms a dot); a one-second bar is 10 px wide (100 ms a pixel,
    // 2 s a dot), and zoomed in to 40 px a bar, 500 ms a dot.
    let bubbles = BubbleStyle {
        max_radius: 10.0,
        ..BubbleStyle::default()
    };
    let geometry = |px_per_bar: f32| PaneGeometry {
        px_per_bar,
        lane_width_px: 300.0,
        lane_bar_opens: vec![0, 1_000],
    };
    let resolve = |px_per_bar: f32, timeline: &BarTimeline| {
        let dots = VolumeDots::resolve(&geometry(px_per_bar), &bubbles, timeline, 1_000);
        (dots.candle_window_ms, dots.tape_window_ms)
    };
    let now = chart(8_300, 1_500, None);
    assert_eq!(resolve(10.0, &now), (2_000, 100));
    assert_eq!(resolve(40.0, &now), (500, 100));
    // Time passing, a pan and a forming bar are not a zoom.
    for timeline in [
        chart(8_950, 1_500, None),
        chart(15_020, 1_500, None),
        chart(15_020, 1_500, Some(3..9)),
    ] {
        assert_eq!(resolve(10.0, &timeline), (2_000, 100));
    }
}

/// A buy and a sell at one level inside one window are one dot: a pie with
/// the exact quantities, labelled as the cluster it is (`×3`), not a fold.
#[test]
fn both_sides_at_one_level_and_window_are_one_pie() {
    let config = dots_config();
    let history = tape(
        config.clone(),
        &[
            (1, 1_100, "100", "0.3", Side::Buy),
            (2, 1_150, "100", "1.2", Side::Buy),
            (3, 1_180, "100", "2.5", Side::Sell),
            (4, 1_120, "101", "1", Side::Buy),
            (5, 1_300, "100", "1", Side::Sell),
        ],
    );
    let frame = frame_at(
        &history,
        &chart(3_900, 1_500, None),
        prices("90", "110"),
        &windows(250, 250),
    );
    let pie = frame
        .aggressions
        .iter()
        .find(|dot| dot.agg_ids == vec![1, 2, 3])
        .expect("one dot for the three prints");
    assert_eq!(pie.quantity, dec("4.0"));
    assert_eq!(pie.buy_quantity, dec("1.5"));
    assert_eq!(pie.trade_count, 3);
    assert_eq!(pie.folded_marks, 0, "a market fact, not a canvas fold");
    assert_eq!(pie.side, AggressorSide::Sell, "the side that took more");
    assert!((pie.buy_share - 0.375).abs() < 1e-6);
    assert!(!pie.live);
    assert_eq!(pie.y, prices("90", "110").y(dec("100")).unwrap(), "no lean");
    let others: Vec<Vec<u64>> = frame
        .aggressions
        .iter()
        .map(|dot| dot.agg_ids.clone())
        .filter(|ids| ids != &vec![1, 2, 3])
        .collect();
    assert_eq!(
        others.len(),
        2,
        "another level and another window: {others:?}"
    );
}

/// A window that straddles a bar close is one dot per bar, each drawn in its
/// own bar; a tape window that straddles the tape's start leaves the tape
/// whole, to the candles.
#[test]
fn a_window_splits_at_a_bar_close_and_leaves_the_tape_whole() {
    let config = dots_config();
    let history = tape(
        config.clone(),
        &[
            (1, 1_100, "100", "1", Side::Buy),
            (2, 1_200, "100", "1", Side::Buy),
            (3, 2_450, "100", "1", Side::Sell),
            (4, 2_900, "100", "1", Side::Sell),
            (5, 3_100, "100", "1", Side::Buy),
        ],
    );
    // Bars close at 1 130, 2 000, 3 000; the tape shows 2 400..3 900.
    let bars = [bar(0, 1_129), bar(1_130, 1_999), bar(2_000, 2_999)];
    let timeline = BarTimeline::from_bars(
        0,
        &bars,
        Some(&bar(3_000, 3_900)),
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let dots = VolumeDots {
        candle_window_ms: 250,
        tape_window_ms: 1_000,
        lane_bar_opens: vec![0, 1_130, 2_000, 3_000],
    };
    let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
    let dot = |id: u64| {
        frame
            .aggressions
            .iter()
            .find(|dot| dot.agg_ids.contains(&id))
            .expect("drawn")
    };
    assert_eq!(dot(1).agg_ids, vec![1], "the bar closed between them");
    assert_eq!(dot(2).agg_ids, vec![2]);
    for (id, slot) in [(1, 0), (2, 1)] {
        let (left, right) = timeline.slot_bounds(slot);
        assert!(
            dot(id).x >= left && dot(id).x <= right,
            "print {id} is drawn in its own bar"
        );
    }
    // The tape window 2 000..3 000 began before the tape did: both its prints
    // are candle marks, and the tape draws only the window after it.
    assert!(
        !dot(3).live && !dot(4).live,
        "the window left the tape whole"
    );
    assert!(dot(5).live);
}

/// The same prints in any order are the same frame.
#[test]
fn dots_are_order_independent() {
    let config = dots_config();
    let trades = dense(3, 2_000, 0, 12_000);
    let mut shuffled = trades.clone();
    let mut random = Lcg(99);
    for index in (1..shuffled.len()).rev() {
        shuffled.swap(index, random.next(index as u64 + 1) as usize);
    }
    let timeline = chart(11_500, 1_500, None);
    let dots = windows(500, 250);
    let project_all = |trades: &[(u64, i64, String, String, Side)]| {
        let history = tape(config.clone(), &borrowed(trades));
        frame_at(&history, &timeline, prices("90", "110"), &dots)
    };
    assert_eq!(project_all(&shuffled), project_all(&trades));
}

/// The trader's floor hides dots below a fixed quantity, whatever else the
/// session traded, and says how much it hid. It applies to the dot, not to
/// the prints inside it.
#[test]
fn the_min_quantity_floor_is_fixed_and_counted() {
    let config = HeatmapConfig {
        bubbles: BubbleStyle {
            min_quantity: 2.0,
            ..dots_config().bubbles
        },
        ..dots_config()
    };
    let small = [
        (1, 1_100, "100", "1", Side::Buy),
        (2, 1_200, "100", "1", Side::Sell),
        (3, 1_700, "102", "1", Side::Buy),
    ];
    let mut with_a_giant = small.to_vec();
    with_a_giant.push((4, 2_100, "105", "5000", Side::Buy));
    for trades in [small.to_vec(), with_a_giant] {
        let history = tape(config.clone(), &trades);
        let frame = frame_at(
            &history,
            &chart(3_900, 1_500, None),
            prices("90", "110"),
            &windows(500, 250),
        );
        let kept: Vec<_> = frame
            .aggressions
            .iter()
            .filter(|dot| dot.first_timestamp_ms < 2_000)
            .map(|dot| (dot.agg_ids.clone(), dot.quantity, dot.size))
            .collect();
        assert_eq!(
            kept,
            vec![(vec![1, 2], dec("2"), (0.2f64).sqrt() as f32)],
            "two small prints make a dot the floor keeps, sized on the fixed scale"
        );
        assert_eq!(frame.floored_quantity, Decimal::ONE, "the lone contract");
    }
}

/// Size is one fixed session scale: equal quantities are equal dots on the
/// tape and on the candles, in any frame.
#[test]
fn equal_quantities_are_equal_radii_anywhere() {
    let config = dots_config();
    let history = tape(
        config.clone(),
        &[
            (1, 1_100, "100", "3", Side::Buy),
            (2, 3_300, "100", "3", Side::Sell),
            (3, 3_600, "104", "900", Side::Sell),
        ],
    );
    let frame = frame_at(
        &history,
        &chart(3_900, 1_500, None),
        prices("90", "110"),
        &windows(500, 250),
    );
    assert!(frame.volume_dots);
    let dot = |id: u64| {
        frame
            .aggressions
            .iter()
            .find(|dot| dot.agg_ids == vec![id])
            .expect("drawn")
    };
    let (candle, tape_dot) = (dot(1), dot(2));
    assert!(!candle.live && tape_dot.live, "one on each pane");
    assert_eq!(candle.size, tape_dot.size);
    let radius = |mark: &AggressionPrimitive| {
        let (minimum, maximum) =
            config
                .live_lane
                .pane_radii(&config.bubbles, mark.live, frame.volume_dots);
        bubble_radius(mark.size, minimum, maximum)
    };
    assert_eq!(radius(candle), radius(tape_dot));
    assert_eq!(
        config.live_lane.pane_radii(&config.bubbles, true, false),
        config.live_lane.scaled_radii(&config.bubbles),
        "off, the tape keeps its own range"
    );
}

/// Off, the dots never run: the frame is exactly the one built without them.
#[test]
fn off_leaves_the_frame_unchanged() {
    let config = HeatmapConfig {
        bubble_overlap_merge: false,
        ..dots_config()
    };
    let trades = dense(5, 1_500, 0, 12_000);
    let history = tape(config.clone(), &borrowed(&trades));
    let timeline = chart(11_500, 1_500, None);
    let plain = project(&history, &timeline, prices("90", "110"));
    let offered = frame_at(&history, &timeline, prices("90", "110"), &windows(500, 250));
    assert!(!offered.volume_dots);
    assert_eq!(offered, plain);
}
