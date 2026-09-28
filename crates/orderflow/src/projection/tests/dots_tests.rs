//! Golden tests for Bookmap-style volume dots. On the candles every print
//! lands in the dot keyed by its bar and a price level of the candle axis; on
//! the tape, by a window of market time anchored at exchange epoch 0 and a
//! price level of the tape's own span. Both sides share one dot, drawn as a
//! pie. The key is market data, so a closed dot is the same fact on every
//! frame, however the chart rolls, pans, refits or is cut.

use super::*;
use crate::bubble_radius;
use crate::config::VolumeDotStyle;
use crate::history::AggressorSide;
use crate::projection::{
    DOT_LEVEL_LADDER_TICKS, DOT_WINDOW_CELL_PX, DOT_WINDOW_LADDER_MS, DotRungMemory, PaneGeometry,
    VolumeDots, dot_level_ticks, dot_window_ms, hold_rung, project_with_dots,
};

mod sizing_tests;
mod tape_tests;

/// Dots on, the budget out of the way, and a fixed scale where 10
/// contracts is a full-size dot. The folds dots mode skips are all switched
/// on here, so a test proves they are skipped rather than merely unset.
fn dots_config() -> HeatmapConfig {
    let base = bubbles_only();
    HeatmapConfig {
        volume_dots: VolumeDotStyle {
            enabled: true,
            full_quantity: 10.0,
            ..VolumeDotStyle::default()
        },
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

/// Dots keyed by the test rather than by a zoom: `tape_window_ms` on the
/// tape, one-tick levels on both panes, over one-second bars.
fn dots_at(tape_window_ms: i64) -> VolumeDots {
    coarse(tape_window_ms, 1)
}

/// [`dots_at`], on price levels `level_ticks` native ticks tall on both
/// panes.
fn coarse(tape_window_ms: i64, level_ticks: i64) -> VolumeDots {
    levels(tape_window_ms, level_ticks, level_ticks)
}

/// [`dots_at`], with the tape's and the candles' levels apart.
fn levels(tape_window_ms: i64, tape_level_ticks: i64, candle_level_ticks: i64) -> VolumeDots {
    VolumeDots {
        tape_only: false,
        tape_window_ms,
        tape_level_ticks,
        candle_level_ticks,
        bars: (0..40).map(|i| (i * 1_000, i * 1_000 + 999)).collect(),
        forming: None,
    }
}

/// A history of `trades` that began recording well before the chart's first
/// bar: a sentinel print at -5 s, off every timeline here, so the recording
/// start cuts no window a test looks at.
fn recorded(config: HeatmapConfig, trades: &[(u64, i64, &str, &str, Side)]) -> LiquidityHistory {
    let mut all = vec![(u64::MAX, -5_000, "100", "1", Side::Buy)];
    all.extend_from_slice(trades);
    tape(config, &all)
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
    let history = recorded(config.clone(), &borrowed(&trades));
    let dots = dots_at(250);
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
    // Bars 2..6, which every frame shows, windows the tape of every frame has
    // let go of, and levels every price window shows.
    let closed = |mark: &AggressionPrimitive| {
        !mark.live
            && mark.first_timestamp_ms >= 2_000
            && mark.last_timestamp_ms < 6_000
            && mark.price_bucket <= dec("107")
    };
    let reference = facts(&frames[0], closed);
    assert!(reference.len() > 50, "the fixture draws closed dots");
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

/// The mark budget never folds a dot. A fold over budget picks by size and
/// age, so which dots it joins changes as prints arrive and the chart pans:
/// on a dense tape that was the blinking past. The ladder already bounds how
/// many dots a zoom can draw, so dots are drawn as they are, every one.
#[test]
fn the_budget_never_folds_a_dot() {
    let config = HeatmapConfig {
        max_aggression_primitives: 40,
        ..dots_config()
    };
    let trades = dense(11, 3_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let dots = dots_at(250);
    for now_ms in [9_100, 12_700, 15_300] {
        let frame = frame_at(
            &history,
            &chart(now_ms, 1_500, None),
            prices("90", "110"),
            &dots,
        );
        assert!(frame.aggressions.len() > 40, "over the budget at {now_ms}");
        assert_eq!(
            frame.folded_aggressions, 0,
            "the budget folded dots at {now_ms}"
        );
        assert!(
            frame.aggressions.iter().all(|mark| mark.folded_marks <= 1),
            "a dot drawn as a fold at {now_ms}"
        );
    }
}

/// The tape is a zoom of its own: with volume dots on, the candles draw every
/// print of their bars whatever the tape's length, so squeezing the tape
/// never empties or changes the tick chart.
#[test]
fn the_tape_zoom_never_changes_the_candle_dots() {
    let config = dots_config();
    let trades = dense(17, 3_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let dots = dots_at(250);
    let candles = |lane_ms: i64| {
        let timeline = chart(12_700, lane_ms, None);
        let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
        let facts: Vec<_> = frame
            .aggressions
            .iter()
            .filter(|dot| !dot.live)
            .map(|dot| {
                (
                    dot.first_timestamp_ms,
                    dot.price_bucket,
                    dot.quantity,
                    dot.buy_quantity,
                )
            })
            .collect();
        facts
    };
    let short = candles(1_500);
    assert_eq!(
        candles(12_000),
        short,
        "a long tape leaves the candles alone"
    );
    let drawn: Decimal = short.iter().map(|dot| dot.2).sum();
    let traded: Decimal = trades
        .iter()
        .filter(|trade| trade.1 <= 12_700)
        .map(|trade| dec(&trade.3))
        .sum();
    assert_eq!(
        drawn, traded,
        "the candles hold every contract of their bars"
    );
}

/// Every dot is one bar and one native price level, and on the tape one
/// window of market time; the folds
/// dots replace — dust, regions, the closed-bar summary — never run, so no
/// contract is drawn twice and every one visible is drawn once. They paint
/// smallest first, so the biggest dot is on top.
#[test]
fn a_dense_frame_keys_every_print_once() {
    let config = dots_config();
    let trades = dense(11, 3_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let dots = dots_at(250);
    let timeline = chart(12_700, 1_500, None);
    let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
    let drawn: Decimal = frame
        .aggressions
        .iter()
        .filter(|dot| !dot.live)
        .map(|dot| dot.quantity)
        .sum();
    let traded: Decimal = trades
        .iter()
        .filter(|trade| trade.1 <= 12_700)
        .map(|trade| dec(&trade.3))
        .sum();
    assert_eq!(drawn, traded, "every contract, once on the candles");
    for dot in &frame.aggressions {
        assert_eq!(dot.price_span, Decimal::ONE, "one native level");
        assert_eq!(dot.folded_marks, 0, "a dot is not a fold");
        assert_eq!(dot.trade_count, dot.agg_ids.len());
        if dot.live {
            assert_eq!(
                dot.first_timestamp_ms.div_euclid(250),
                dot.last_timestamp_ms.div_euclid(250),
                "one tape window: {dot:?}"
            );
        }
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

    // The long end of the ladder.
    assert_eq!(dot_window_ms(3_600_000.0, 20.0), 300_000);
    assert_eq!(dot_window_ms(1_000.0, 20.0), 30_000);
}

/// The view holds its rungs: an ideal that differs only just is not enough
/// to move one. A rung moves when its dot would be over 150 % of its cell, or
/// under 70 % of the next smaller cell — so an autoscale wobbling across a
/// boundary never flips the levels back and forth.
#[test]
fn rungs_hold_through_a_wobble_at_a_boundary() {
    let ladder = DOT_LEVEL_LADDER_TICKS;
    // 20 px dots: 10 px a tick is the boundary between 2 and 5 ticks.
    let mut level = hold_rung(&ladder, None, 9.9, 20.0);
    assert_eq!(level, 5);
    for px_per_tick in [10.1, 9.9, 10.4, 9.6, 11.0] {
        level = hold_rung(&ladder, Some(level), px_per_tick, 20.0);
        assert_eq!(level, 5, "held at {px_per_tick} px a tick");
    }
    level = hold_rung(&ladder, Some(level), 15.0, 20.0);
    assert_eq!(
        level, 2,
        "a 2-tick cell is 30 px: the dot is under 70 % of it"
    );
    for px_per_tick in [9.9, 10.1, 8.0] {
        level = hold_rung(&ladder, Some(level), px_per_tick, 20.0);
        assert_eq!(level, 2, "held at {px_per_tick} px a tick");
    }
    level = hold_rung(&ladder, Some(level), 6.0, 20.0);
    assert_eq!(
        level, 5,
        "a 2-tick cell is 12 px: the dot is over 150 % of it"
    );

    // The memory the view keeps does the same with a wobbling autoscale, for
    // both panes' levels and for the tape's window. With no tape span to go
    // on, the tape's level falls back to the axis.
    let config = HeatmapConfig {
        bubbles: BubbleStyle {
            max_radius: 10.0,
            ..BubbleStyle::default()
        },
        ..dots_config()
    };
    let geometry = |lane_window_ms: i64| PaneGeometry {
        px_per_bar: 40.0,
        lane_width_px: 300.0,
        lane_window_ms,
        height_px: 400.0,
        lane_bars: vec![(0, 999), (1_000, 1_500)],
    };
    let mut memory = DotRungMemory::default();
    // 40 ticks over 400 px: 10 px a tick, the 2/5 boundary.
    let first = memory.choose(geometry(1_500), &config, (60.0, 100.0), None);
    for (low, high, lane) in [
        (60.0, 100.5, 1_450),
        (59.5, 100.0, 1_550),
        (60.0, 99.5, 1_500),
    ] {
        let zoom = memory.choose(geometry(lane), &config, (low, high), None);
        assert_eq!(
            (
                zoom.tape_level_ticks,
                zoom.candle_level_ticks,
                zoom.tape_window_ms
            ),
            (
                first.tape_level_ticks,
                first.candle_level_ticks,
                first.tape_window_ms
            ),
            "a wobble to {low}..{high} and a {lane} ms tape moved a rung"
        );
    }
    assert_eq!(first.lane_bars, vec![(0, 999), (1_000, 1_500)]);
}

/// A tape dot's window of market time is a thin column of screen, not the
/// width of the biggest dot: at the default zoom a dot sits near the moment
/// its prints traded, instead of every print of five seconds piling into one
/// column far from the next. The column is at least the largest radius wide,
/// half a full dot, so a dot fitted to it stays readable; only squeezing the
/// time axis widens the window.
#[test]
fn time_windows_follow_a_thin_column_not_the_biggest_dot() {
    assert_eq!(DOT_WINDOW_CELL_PX, 8.0);
    let geometry = |lane_window_ms: i64| PaneGeometry {
        px_per_bar: 108.0,
        lane_width_px: 300.0,
        lane_window_ms,
        height_px: 400.0,
        lane_bars: vec![(0, 59_999)],
    };
    // (max radius, rungs at 15 s, a minute and twenty minutes over 300 px)
    for (max_radius, rungs) in [(6.0, [500, 2_000, 60_000]), (15.0, [1_000, 5_000, 60_000])] {
        let config = HeatmapConfig {
            bubbles: BubbleStyle {
                max_radius,
                ..dots_config().bubbles
            },
            ..dots_config()
        };
        let tape = |lane_window_ms: i64| {
            DotRungMemory::default()
                .choose(geometry(lane_window_ms), &config, (60.0, 100.0), None)
                .tape_window_ms
        };
        // 8 px or the radius, whichever is wider: 400 or 750 ms at 15 s,
        // 1.6 or 3 s at a minute, 32 or 60 s at twenty minutes.
        assert_eq!(
            [tape(15_000), tape(60_000), tape(1_200_000)],
            rungs,
            "max radius {max_radius}"
        );
    }
}

/// With dots on, squeezing the tape reaches twenty minutes and more, not the
/// automatic zoom's floor: the first squeeze pins the window it resolves to,
/// and the pinned window goes as far as the lane allows.
#[test]
fn dots_let_the_tape_squeeze_to_twenty_minutes() {
    use crate::config::{LaneWindow, MAX_LIVE_LANE_WINDOW_MS};
    const { assert!(MAX_LIVE_LANE_WINDOW_MS >= 1_200_000) };
    let mut config = dots_config();
    config.live_lane.window = LaneWindow::Auto { zoom: 1.0 };
    config.zoom_lane_window(0.5);
    assert_eq!(config.live_lane.window, LaneWindow::Fixed { ms: 30_000 });
    for _ in 0..8 {
        config.zoom_lane_window(0.5);
    }
    assert_eq!(config.lane_window_ms(1_000), MAX_LIVE_LANE_WINDOW_MS);

    // Dots off, the gesture keeps speaking the automatic zoom.
    let mut off = dots_config();
    off.volume_dots.enabled = false;
    off.live_lane.window = LaneWindow::Auto { zoom: 1.0 };
    off.zoom_lane_window(0.5);
    assert_eq!(off.live_lane.window, LaneWindow::Auto { zoom: 0.5 });
}

/// The price ladder works like the time ladder: the smallest level, in
/// native ticks, at least one full dot tall on screen.
#[test]
fn a_zoom_changes_the_level_only_at_ladder_steps() {
    let dot_px = 20.0;
    let mut previous = DOT_LEVEL_LADDER_TICKS[0];
    for step in (1..4_000).rev() {
        let px_per_tick = f64::from(step) * 0.05;
        let ticks = dot_level_ticks(px_per_tick, dot_px);
        assert!(DOT_LEVEL_LADDER_TICKS.contains(&ticks), "{ticks} is a rung");
        assert!(ticks >= previous, "zooming out never shrinks the level");
        let fits = |rung: i64| rung as f64 * px_per_tick >= dot_px;
        if fits(ticks) {
            assert!(
                DOT_LEVEL_LADDER_TICKS
                    .iter()
                    .filter(|rung| **rung < ticks)
                    .all(|rung| !fits(*rung)),
                "{ticks} ticks at {px_per_tick} px a tick is not the smallest that fits"
            );
        } else {
            assert_eq!(ticks, *DOT_LEVEL_LADDER_TICKS.last().unwrap(), "the widest");
        }
        previous = ticks;
    }
}

/// An axis refit inside one ladder step changes no level, so it moves no
/// dot; zooming the axis out past a step coarsens the levels.
#[test]
fn a_refit_inside_a_ladder_step_keeps_every_dot() {
    let config = HeatmapConfig {
        bubbles: BubbleStyle {
            max_radius: 10.0,
            ..dots_config().bubbles
        },
        ..dots_config()
    };
    let trades = dense(21, 3_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let timeline = chart(12_700, 1_500, None);
    let closed_bars: Vec<Bar> = (0..12).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let forming = bar(12_000, 12_700);
    let geometry = PaneGeometry {
        px_per_bar: 40.0,
        lane_width_px: 300.0,
        lane_window_ms: 1_500,
        height_px: 400.0,
        lane_bars: vec![(11_000, 11_999), (12_000, 12_700)],
    };
    let memory = std::cell::RefCell::new(DotRungMemory::default());
    let at = |low: &str, high: &str| {
        let window = prices(low, high);
        let range = (low.parse::<f64>().unwrap(), high.parse::<f64>().unwrap());
        let zoom = memory
            .borrow_mut()
            .choose(geometry.clone(), &config, range, None);
        let dots = VolumeDots::resolve(&zoom, &closed_bars, Some(&forming));
        (
            dots.candle_level_ticks,
            frame_at(&history, &timeline, window, &dots),
        )
    };
    // 70 and 75 ticks over 400 px: 5.7 and 5.3 px a tick. A candle dot is
    // 8 px (0.4 of a 20 px dot): two ticks a level. The tape, with no span of
    // its own here, reads the axis at its full 20 px dot: five ticks.
    let (ticks, before) = at("60", "130");
    let (refit_ticks, refit) = at("58", "133");
    assert_eq!((ticks, refit_ticks), (2, 2));
    let closed = |mark: &AggressionPrimitive| !mark.live && mark.last_timestamp_ms < 6_000;
    assert!(facts(&before, closed).len() > 20);
    assert_eq!(facts(&refit, closed), facts(&before, closed));
    for dot in &before.aggressions {
        let span = if dot.live { dec("5") } else { dec("2") };
        assert_eq!(dot.price_span, span, "each pane's own levels: {dot:?}");
        assert_eq!(dot.price_bucket % span, Decimal::ZERO, "anchored at zero");
    }
    // 200 ticks over 400 px: 2 px a tick, five ticks a candle dot.
    let (zoomed_out, _) = at("0", "200");
    assert_eq!(zoomed_out, 5);
}

/// A coarse level holds every native tick inside it. The dot is drawn at
/// its level's centre, so neighbouring levels never crowd, and carries its
/// quantity-weighted price rounded to the tick, inside the level. Its
/// `size` reads the typed full size: full at `volume_dot_full_quantity`.
#[test]
fn a_coarse_level_places_at_its_weighted_tick_and_sizes_by_quantity() {
    let config = dots_config();
    let history = recorded(
        config.clone(),
        &[
            (1, 1_100, "100", "1", Side::Buy),
            (2, 1_200, "103", "3", Side::Sell),
            (3, 1_300, "105", "25", Side::Buy),
            (4, 3_300, "100", "12.5", Side::Sell),
        ],
    );
    let window = prices("90", "110");
    let frame = frame_at(
        &history,
        &chart(3_900, 1_500, None),
        window,
        &coarse(250, 5),
    );
    let dot = |id: u64| {
        frame
            .aggressions
            .iter()
            .filter(|dot| dot.agg_ids.contains(&id))
            // The tape's copy when the tape holds the print.
            .max_by_key(|dot| dot.live)
            .expect("drawn")
    };
    let level = dot(1);
    assert_eq!(
        level.agg_ids,
        vec![1, 2],
        "100 and 103 share the 100..105 level"
    );
    assert_eq!(level.price_bucket, dec("100"));
    assert_eq!(level.price_span, dec("5"));
    assert_eq!(level.buy_quantity, dec("1"));
    // (100 × 1 + 103 × 3) / 4 = 102.25, on the 102 tick; drawn at the
    // level's centre, 102.5.
    assert_eq!(level.price, dec("102"), "weighted, on a tick");
    assert_eq!(
        level.y,
        window.y(dec("102.5")).unwrap(),
        "the level's centre"
    );
    // 10 contracts is a full-size dot, whatever the level or the window.
    assert_eq!(level.size, (4.0f64 / 10.0).sqrt() as f32);
    assert_eq!(dot(3).price_bucket, dec("105"), "the next level up");
    assert_eq!(dot(3).size, 1.0, "25 contracts are past full size");
    assert!(dot(4).live);
    assert_eq!(dot(4).size, 1.0, "12.5 contracts are past full size");
}

/// A buy and a sell at one level inside one bar are one candle dot: a pie
/// with the exact quantities, labelled as the cluster it is (`×4`), not a
/// fold.
#[test]
fn both_sides_at_one_level_and_window_are_one_pie() {
    let config = dots_config();
    let history = recorded(
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
        &dots_at(250),
    );
    let pie = frame
        .aggressions
        .iter()
        .find(|dot| dot.agg_ids == vec![1, 2, 3, 5])
        .expect("one dot for the bar's four prints at 100");
    assert_eq!(pie.quantity, dec("5.0"));
    assert_eq!(pie.buy_quantity, dec("1.5"));
    assert_eq!(pie.trade_count, 4);
    assert_eq!(pie.folded_marks, 0, "a market fact, not a canvas fold");
    assert_eq!(pie.side, AggressorSide::Sell, "the side that took more");
    assert!((pie.buy_share - 0.3).abs() < 1e-6);
    assert!(!pie.live);
    assert_eq!(
        pie.y,
        prices("90", "110").y(dec("100.5")).unwrap(),
        "the level's centre, no lean"
    );
    let others: Vec<Vec<u64>> = frame
        .aggressions
        .iter()
        .map(|dot| dot.agg_ids.clone())
        .filter(|ids| ids != &vec![1, 2, 3, 5])
        .collect();
    assert_eq!(others, vec![vec![4]], "another level");
}

/// Prints either side of a bar close are one dot per bar, each drawn in its
/// own bar.
#[test]
fn a_window_splits_at_a_bar_close() {
    let config = dots_config();
    let history = recorded(
        config.clone(),
        &[
            (1, 1_100, "100", "1", Side::Buy),
            (2, 1_200, "100", "1", Side::Buy),
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
        tape_only: false,
        tape_window_ms: 1_000,
        tape_level_ticks: 1,
        candle_level_ticks: 1,
        bars: vec![(0, 1_129), (1_130, 1_999), (2_000, 2_999), (3_000, 3_900)],
        forming: None,
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
}

/// A print is the tape's while its tape window starts at or after the tape;
/// otherwise it is in its candle window's dot. A candle window wholly older
/// than the tape never changes again, and the tape never piles prints at its
/// left edge: every tape dot sits inside the tape, right of where it starts.
#[test]
fn deep_history_never_changes_and_the_tape_never_piles_at_its_edge() {
    let config = dots_config();
    let trades = dense(17, 4_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let dots = dots_at(250);
    let at = |now_ms: i64| {
        let timeline = chart(now_ms, 1_500, None);
        let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
        (timeline, frame)
    };
    // Candle windows ending by 9 000 are older than every tape below.
    let deep = |mark: &AggressionPrimitive| !mark.live && mark.last_timestamp_ms < 9_000;
    let (_, reference) = at(10_600);
    let reference = facts(&reference, deep);
    assert!(reference.len() > 50, "the fixture draws deep dots");
    for now_ms in [10_620, 10_900, 11_350, 12_700, 15_020] {
        let (timeline, frame) = at(now_ms);
        assert_eq!(
            facts(&frame, deep),
            reference,
            "deep history moved at {now_ms}"
        );
        let (lane_start, _) = timeline.lane_bounds_ms().unwrap();
        let edge = timeline.locate(lane_start).unwrap().normalized;
        let on_tape: Vec<_> = frame.aggressions.iter().filter(|mark| mark.live).collect();
        assert!(!on_tape.is_empty(), "the tape draws dots at {now_ms}");
        for mark in on_tape {
            assert!(
                mark.first_timestamp_ms >= lane_start,
                "a print older than the tape was drawn on it at {now_ms}: {mark:?}"
            );
            assert!(
                mark.x > edge,
                "a tape dot piled at the tape's left edge at {now_ms}: {mark:?}"
            );
        }
    }
}

/// Where a dot sits never depends on the pan or on the latest print. A
/// candle dot sits at its bar's slot centre, however the slice is cut; a
/// forming tape window's dot sits at the window's own centre, not at the
/// newest print, so it does not slide as prints arrive.
#[test]
fn dots_sit_still_through_a_pan_and_a_new_print() {
    let config = dots_config();
    let history = recorded(
        config.clone(),
        &[
            (1, 2_100, "100", "1", Side::Buy),
            (2, 3_050, "101", "1", Side::Buy),
        ],
    );
    let whole = dots_at(250);
    for visible in [None, Some(1..3), Some(2..4)] {
        let timeline = chart(3_900, 1_500, visible.clone());
        let frame = frame_at(&history, &timeline, prices("90", "110"), &whole);
        let dot = frame
            .aggressions
            .iter()
            .find(|dot| dot.agg_ids == vec![1])
            .expect("the candle dot");
        let slot = timeline.slot_at(2_100).unwrap();
        let (left, right) = timeline.slot_bounds(slot.index);
        assert!(
            (dot.x - (left + right) / 2.0).abs() < 1e-9,
            "the candle dot left its slot's centre with {visible:?}"
        );
    }
    // The forming bar has run to 3 150 and then to 3 180: its tape dot, in
    // the window 3 000..3 250, stays where it was.
    let forming = |close_ms: i64| {
        let mut dots = dots_at(250);
        dots.bars.truncate(3);
        dots.bars.push((3_000, close_ms));
        let frame = frame_at(
            &history,
            &chart(3_900, 1_500, None),
            prices("90", "110"),
            &dots,
        );
        frame
            .aggressions
            .iter()
            .find(|dot| dot.agg_ids == vec![2])
            .expect("the forming dot")
            .x
    };
    assert_eq!(forming(3_150), forming(3_180));
}

/// A candle dot is its whole bar at one level, whatever the epoch grid does
/// and however long the bar ran, and sits at its slot's centre; no candle dot
/// is pinned at a slot's edge.
#[test]
fn a_candle_dot_is_one_bar_and_level_at_its_slot_centre() {
    let config = dots_config();
    let trades = dense(29, 3_000, 0, 9_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    // Bars 300 to 900 ms long, opening off the epoch's second grid.
    let lengths = [
        700, 450, 900, 600, 800, 550, 300, 850, 650, 750, 500, 900, 600,
    ];
    let mut opens = vec![0_i64];
    for length in lengths {
        opens.push(opens.last().unwrap() + length);
    }
    let bars: Vec<Bar> = opens
        .windows(2)
        .map(|pair| bar(pair[0], pair[1] - 1))
        .collect();
    let timeline = BarTimeline::from_bars(0, &bars, None, None);
    let dots = VolumeDots {
        tape_only: false,
        tape_window_ms: 250,
        tape_level_ticks: 1,
        candle_level_ticks: 1,
        bars: bars
            .iter()
            .map(|bar| (bar.open_time, bar.close_time))
            .collect(),
        forming: None,
    };
    let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
    assert!(frame.aggressions.len() > 100, "the fixture draws dots");
    let mut seen = std::collections::BTreeSet::new();
    for dot in &frame.aggressions {
        assert!(!dot.live, "no tape in this chart");
        let slot = timeline.slot_at(dot.first_timestamp_ms).unwrap();
        assert!(
            seen.insert((slot.index, dot.price_bucket)),
            "two dots for one bar and level: {dot:?}"
        );
        let (left, right) = timeline.slot_bounds(slot.index);
        assert!(
            (dot.x - (left + right) / 2.0).abs() < 1e-9,
            "a dot off its slot's centre: {dot:?} in {left}..{right}"
        );
    }
}

/// Retention evicts the oldest prints one by one as time passes. A bar that
/// has lost any print to it draws no dots at all, so eviction never shrinks
/// a dot: across an eviction every dot is either the same or gone.
#[test]
fn eviction_never_shrinks_a_dot() {
    let config = HeatmapConfig {
        retention_ms: 4_000,
        ..dots_config()
    };
    let trades = dense(31, 4_000, 0, 10_000);
    let up_to = |now_ms: i64| {
        let so_far: Vec<_> = trades
            .iter()
            .filter(|trade| trade.1 <= now_ms)
            .cloned()
            .collect();
        recorded(config.clone(), &borrowed(&so_far))
    };
    let dots = dots_at(250);
    let (before, after) = (up_to(8_000), up_to(9_300));
    assert!(
        after.counters().aggressions_evicted > before.counters().aggressions_evicted,
        "the later history evicted more"
    );
    let early = frame_at(
        &before,
        &chart(8_000, 1_500, None),
        prices("90", "110"),
        &dots,
    );
    let late = frame_at(
        &after,
        &chart(9_300, 1_500, None),
        prices("90", "110"),
        &dots,
    );
    // The bar forming at 8 s still grows; every closed bar's dot is final.
    let candles = facts(&early, |mark| !mark.live && mark.first_timestamp_ms < 8_000);
    let late_facts = facts(&late, |mark| !mark.live);
    let (mut same, mut gone) = (0, 0);
    for dot in &candles {
        let sharing: Vec<_> = late_facts
            .iter()
            .filter(|later| later.agg_ids.iter().any(|id| dot.agg_ids.contains(id)))
            .collect();
        match sharing.as_slice() {
            [] => gone += 1,
            [later] if *later == dot => same += 1,
            _ => panic!("eviction changed a dot: {dot:?} became {sharing:?}"),
        }
    }
    assert!(same > 0 && gone > 0, "{same} kept and {gone} gone");
}

/// Eviction drops a dot only when its own window may have lost prints. With
/// the horizon inside the bar before the forming one, the tape keeps drawing
/// that bar's windows after the horizon, instead of going blank from its
/// open on.
#[test]
fn eviction_drops_only_the_windows_it_reached() {
    let config = HeatmapConfig {
        retention_ms: 1_000,
        ..dots_config()
    };
    let trades: Vec<_> = dense(41, 4_000, 0, 10_000)
        .into_iter()
        .filter(|trade| trade.1 <= 9_300)
        .collect();
    let history = recorded(config.clone(), &borrowed(&trades));
    let horizon = history.evicted_through_ms().expect("the history evicted");
    assert!(
        horizon > 8_000 && horizon < 8_400,
        "the horizon cuts bar 8: {horizon}"
    );
    let dots = dots_at(250);
    let frame = frame_at(
        &history,
        &chart(9_300, 3_000, None),
        prices("90", "110"),
        &dots,
    );
    assert!(
        frame
            .aggressions
            .iter()
            .any(|dot| dot.live && (8_500..9_000).contains(&dot.first_timestamp_ms)),
        "the tape keeps the cut bar's windows after the horizon"
    );
    assert!(
        frame
            .aggressions
            .iter()
            .any(|dot| dot.first_timestamp_ms >= 9_000),
        "and the forming bar"
    );
    for dot in &frame.aggressions {
        // A candle dot's window is its whole one-second bar.
        let width = if dot.live { 250 } else { 1_000 };
        let start = dot.first_timestamp_ms.div_euclid(width) * width;
        assert!(
            start > horizon,
            "a window the eviction reached was drawn: {dot:?}"
        );
    }
}

/// A price-grouping reset drops every print; the horizon moves to the newest
/// one dropped, so nothing recorded before it can come back as a short dot.
#[test]
fn a_grouping_reset_moves_the_horizon_to_the_newest_dropped_print() {
    let mut history = recorded(
        dots_config(),
        &[
            (1, 1_100, "100", "1", Side::Buy),
            (2, 1_700, "101", "2", Side::Sell),
        ],
    );
    history.reset_price_grouping(dec("0.5")).unwrap();
    assert_eq!(history.evicted_through_ms(), Some(1_700));
}

/// Recording starts mid-bar: the bar it starts in holds only the prints seen
/// since, so its candle dot is not drawn; the next bar's is.
#[test]
fn the_window_recording_started_in_is_not_drawn() {
    let config = dots_config();
    let history = tape(
        config.clone(),
        &[
            (1, 1_100, "100", "1", Side::Buy),
            (2, 2_100, "100", "1", Side::Sell),
        ],
    );
    let frame = frame_at(
        &history,
        &chart(3_900, 1_500, None),
        prices("90", "110"),
        &dots_at(250),
    );
    let ids: Vec<Vec<u64>> = frame
        .aggressions
        .iter()
        .map(|dot| dot.agg_ids.clone())
        .collect();
    assert_eq!(ids, vec![vec![2]], "only the bar after the recording start");
}

/// A full-size quantity that is not a number falls back to the dots' own
/// default, never to a constant of its own.
#[test]
fn a_broken_dot_scale_falls_back_to_the_default() {
    let mut config = dots_config();
    config.volume_dots.full_quantity = f64::NAN;
    assert_eq!(
        crate::projection::dot_full_quantity(&config),
        Decimal::from_f64(crate::config::DEFAULT_VOLUME_DOT_FULL_QUANTITY).unwrap()
    );
}

/// With dots on the tape never follows the bars, so it never rescales when a
/// bar closes: automatic is 15 s at zoom 1, scaled by the zoom alone, and a
/// pinned window is the trader's. Squeezing the tape's time axis still shows
/// more market time, which the tape rung then groups into wider windows.
#[test]
fn dots_keep_the_tape_off_the_bars_but_let_it_zoom() {
    use crate::config::{DOT_TAPE_WINDOW_MS, LaneWindow};
    assert_eq!(DOT_TAPE_WINDOW_MS, 15_000);
    let with = |window: LaneWindow| {
        let mut config = dots_config();
        config.live_lane.window = window;
        config
    };
    for reference_ms in [1_000, 4_000, 90_000] {
        let auto = with(LaneWindow::Auto { zoom: 1.0 });
        assert_eq!(auto.lane_window_ms(reference_ms), DOT_TAPE_WINDOW_MS);
        let squeezed = with(LaneWindow::Auto { zoom: 0.25 });
        assert_eq!(squeezed.lane_window_ms(reference_ms), 60_000);
        let pinned = with(LaneWindow::Fixed { ms: 120_000 });
        assert_eq!(pinned.lane_window_ms(reference_ms), 120_000);
        let mut dragged = with(LaneWindow::Auto { zoom: 1.0 });
        dragged.live_lane.window.zoom_by(0.5);
        assert_eq!(dragged.lane_window_ms(reference_ms), 30_000);
        for window in [
            LaneWindow::Auto { zoom: 1.0 },
            LaneWindow::Fixed { ms: 60_000 },
        ] {
            let off = HeatmapConfig {
                volume_dots: VolumeDotStyle {
                    enabled: false,
                    ..dots_config().volume_dots
                },
                ..with(window)
            };
            assert_eq!(off.lane_window(), window);
            assert_eq!(
                off.lane_window_ms(reference_ms),
                off.live_lane.window_ms(reference_ms)
            );
        }
    }
}

/// The price window is a view: every print in the time range is keyed, and
/// a level the edge of the chart cuts through is the same dot either way.
#[test]
fn a_price_pan_never_changes_a_dot() {
    let config = dots_config();
    let history = recorded(
        config.clone(),
        &[
            (1, 1_100, "107", "1", Side::Buy),
            (2, 1_150, "109", "3", Side::Sell),
        ],
    );
    let timeline = chart(3_900, 1_500, None);
    let dots = coarse(250, 5);
    let cut = frame_at(&history, &timeline, prices("90", "108"), &dots);
    let whole = frame_at(&history, &timeline, prices("90", "112"), &dots);
    let facts_of = |frame: &HeatmapProjection| facts(frame, |_| true);
    assert_eq!(facts_of(&cut), facts_of(&whole));
    assert_eq!(
        facts_of(&cut)[0].agg_ids,
        vec![1, 2],
        "the 109 print is in it"
    );
    assert_eq!(facts_of(&cut)[0].quantity, dec("4"));
}

/// Dots keep the evidence: prints are matched to the reductions they explain
/// one by one, as they are without dots, and only then folded, so a dot
/// carries its prints' matched quantity and every event they point at.
#[test]
fn dots_carry_the_evidence_of_their_prints() {
    let frame = |dots_on: bool| {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            volume_dots: VolumeDotStyle {
                enabled: dots_on,
                ..dots_config().volume_dots
            },
            bubble_cluster_ms: 0,
            ..config()
        });
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        for (agg_id, timestamp_ms, quantity) in [(7, 400, "1"), (8, 420, "2")] {
            history.record_aggression(&Trade {
                agg_id,
                timestamp_ms,
                price: dec("101"),
                quantity: dec(quantity),
                side: Side::Buy,
            });
        }
        // Ask 101: 4 -> 1, right after the prints.
        history
            .apply_delta(
                450,
                &BookDelta::new(11, 11, vec![], vec![level("101", "1")]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(12, 12, vec![], vec![]))
            .unwrap();
        let dots = VolumeDots {
            tape_only: false,
            tape_window_ms: 1_000,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            bars: vec![(0, 1_000)],
            forming: None,
        };
        project_with_dots(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
            Some(&dots),
        )
    };
    let (plain, dots) = (frame(false), frame(true));
    let evidence = |frame: &HeatmapProjection| {
        let matched: Decimal = frame
            .aggressions
            .iter()
            .map(|mark| mark.matched_quantity)
            .sum();
        let mut ids: Vec<u64> = frame
            .aggressions
            .iter()
            .flat_map(|mark| mark.liquidity_event_ids.clone())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        (matched, ids)
    };
    assert!(dots.volume_dots);
    assert_eq!(dots.aggressions.len(), 1, "one dot");
    assert!(evidence(&plain).0 > Decimal::ZERO, "the prints explain it");
    assert!(!evidence(&plain).1.is_empty());
    assert_eq!(evidence(&dots), evidence(&plain));
    assert_eq!(dots.liquidity_events, plain.liquidity_events);
}

/// A bar is one dot a level and zoomed out a level is many ticks, so the dot count
/// is bounded by the bars on screen times the levels in the price window,
/// whatever the tape did.
#[test]
fn the_dot_count_is_bounded_by_bars_and_levels() {
    assert!(DOT_LEVEL_LADDER_TICKS.ends_with(&[500, 1_000, 2_000, 5_000]));
    let config = dots_config();
    let trades = dense(13, 6_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let timeline = chart(12_700, 1_500, None);
    // Every bar is one dot a level; 10-tick levels over 91..=109.
    let dots = VolumeDots {
        tape_only: false,
        tape_window_ms: 1_000,
        tape_level_ticks: 10,
        candle_level_ticks: 10,
        bars: (0..13).map(|i| (i * 1_000, i * 1_000 + 999)).collect(),
        forming: None,
    };
    let frame = frame_at(&history, &timeline, prices("90", "110"), &dots);
    let bars = 13;
    let tape_windows = 3;
    let levels = 3;
    assert!(
        frame.aggressions.len() <= (bars + tape_windows) * levels,
        "{} dots for {bars} bars and {levels} levels",
        frame.aggressions.len()
    );
    assert!(frame.aggressions.len() >= bars, "and every bar is drawn");
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
    let dots = dots_at(250);
    let project_all = |trades: &[(u64, i64, String, String, Side)]| {
        let history = recorded(config.clone(), &borrowed(trades));
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
        let history = recorded(config.clone(), &trades);
        let frame = frame_at(
            &history,
            &chart(3_900, 1_500, None),
            prices("90", "110"),
            &dots_at(250),
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

/// With a typed full size, size is one absolute scale for the whole
/// session: equal quantities are equal dots on one pane, in any bar, at any
/// rung and any level, in any frame, whatever else traded. The candles draw
/// that scale smaller. Radii are compared within a pane.
#[test]
fn equal_quantities_are_equal_radii_anywhere() {
    let mut config = dots_config();
    config.volume_dots.auto_full = false;
    let prints = [
        (1, 1_100, "100", "3", Side::Buy),
        (2, 1_600, "95", "3", Side::Sell),
        (3, 3_300, "100", "3", Side::Sell),
        (4, 3_700, "104", "3", Side::Buy),
    ];
    let mut with_a_giant = prints.to_vec();
    with_a_giant.push((5, 3_500, "108", "9000", Side::Sell));
    let mut radii = Vec::new();
    for trades in [prints.to_vec(), with_a_giant] {
        let history = recorded(config.clone(), &trades);
        for dots in [dots_at(250), coarse(100, 5), coarse(1_000, 2)] {
            let frame = frame_at(
                &history,
                &chart(3_900, 1_500, None),
                prices("90", "110"),
                &dots,
            );
            assert!(frame.volume_dots);
            for id in 1..=4 {
                let alone: Vec<_> = frame
                    .aggressions
                    .iter()
                    .filter(|dot| dot.agg_ids == vec![id])
                    .collect();
                assert!(!alone.is_empty(), "print {id} drawn alone");
                for dot in alone {
                    let (minimum, maximum) =
                        config
                            .live_lane
                            .pane_radii(&config.bubbles, dot.live, frame.volume_dots);
                    radii.push((id, dot.live, bubble_radius(dot.size, minimum, maximum)));
                }
            }
        }
    }
    assert!(radii.iter().any(|(_, live, _)| *live) && radii.iter().any(|(_, live, _)| !live));
    for pane in [true, false] {
        let mut sizes = radii.iter().filter(|(_, live, _)| *live == pane);
        let first = sizes.next().expect("a dot on each pane").2;
        for (id, live, radius) in sizes {
            assert_eq!(
                *radius, first,
                "print {id} (tape: {live}) drawn at another size"
            );
        }
    }
    assert_eq!(
        config.live_lane.pane_radii(&config.bubbles, true, false),
        config.live_lane.scaled_radii(&config.bubbles),
        "off, the tape keeps its own range"
    );
}

/// Zooming the price axis out merges levels, and a merged dot holds the
/// exact sum of the dots it replaced, so it is drawn at least as big as each
/// of them — bigger whenever it merged two and was not already at full size.
/// Grouping is never a size the market did not trade.
#[test]
fn a_merged_level_is_the_sum_and_never_smaller() {
    // A full-size dot at 1 000 contracts, so the fixture's dots stay below it
    // and a merge has room to grow.
    let config = HeatmapConfig {
        volume_dots: VolumeDotStyle {
            enabled: true,
            full_quantity: 1_000.0,
            auto_full: false,
        },
        ..dots_config()
    };
    let trades = dense(37, 3_000, 0, 12_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let timeline = chart(11_500, 1_500, None);
    let fine = frame_at(&history, &timeline, prices("90", "110"), &coarse(250, 1));
    let merged = frame_at(&history, &timeline, prices("90", "110"), &coarse(250, 2));
    let radius = |mark: &AggressionPrimitive| {
        let (minimum, maximum) = config
            .live_lane
            .pane_radii(&config.bubbles, mark.live, true);
        bubble_radius(mark.size, minimum, maximum)
    };
    let mut grew = 0;
    for dot in &merged.aggressions {
        let parts: Vec<&AggressionPrimitive> = fine
            .aggressions
            .iter()
            .filter(|part| {
                part.live == dot.live && part.agg_ids.iter().all(|id| dot.agg_ids.contains(id))
            })
            .collect();
        let covered: usize = parts.iter().map(|part| part.agg_ids.len()).sum();
        assert_eq!(
            covered,
            dot.agg_ids.len(),
            "the merge is a union of dots: {dot:?}"
        );
        let sum: Decimal = parts.iter().map(|part| part.quantity).sum();
        assert_eq!(dot.quantity, sum, "the exact sum");
        for part in &parts {
            assert!(
                radius(dot) >= radius(part),
                "a merged dot drew smaller: {dot:?}"
            );
        }
        let biggest = parts.iter().map(|part| part.size).fold(0.0_f32, f32::max);
        if parts.len() > 1 && biggest < 1.0 {
            assert!(
                dot.size > biggest,
                "a merge of {} dots did not grow: {dot:?}",
                parts.len()
            );
            grew += 1;
        }
    }
    assert!(grew > 20, "the fixture merges levels ({grew})");
}

/// Dots are sized against their own full-size quantity, not the prints'
/// `size_reference_quantity`. Changing it rescales every dot alike: the ratio
/// between two dots' radii holds, and the preset's print scale plays no part.
#[test]
fn the_dot_scale_rescales_every_dot_alike() {
    let sizes = |full_quantity: f64, print_reference: f64| {
        let config = HeatmapConfig {
            volume_dots: VolumeDotStyle {
                enabled: true,
                full_quantity,
                auto_full: false,
            },
            bubbles: BubbleStyle {
                min_radius: 0.0,
                size_reference: BubbleSizeReference::Fixed,
                size_reference_quantity: print_reference,
                ..dots_config().bubbles
            },
            ..dots_config()
        };
        let history = recorded(
            config.clone(),
            &[
                (1, 1_100, "100", "4", Side::Buy),
                (2, 1_600, "101", "9", Side::Sell),
            ],
        );
        let frame = frame_at(
            &history,
            &chart(3_900, 1_500, None),
            prices("90", "110"),
            &dots_at(250),
        );
        let radius = |id: u64| {
            let dot = frame
                .aggressions
                .iter()
                .find(|dot| dot.agg_ids == vec![id])
                .expect("drawn");
            let (minimum, maximum) = config.live_lane.pane_radii(&config.bubbles, dot.live, true);
            bubble_radius(dot.size, minimum, maximum)
        };
        (radius(1), radius(2))
    };
    let (small_100, big_100) = sizes(100.0, 100.0);
    let (small_1000, big_1000) = sizes(1_000.0, 100.0);
    assert!(
        small_1000 < small_100 && big_1000 < big_100,
        "a bigger scale draws smaller"
    );
    assert!(
        ((small_100 / big_100) - (small_1000 / big_1000)).abs() < 1e-6,
        "alike"
    );
    assert!(
        ((small_100 / big_100) - 2.0 / 3.0).abs() < 1e-6,
        "area follows quantity"
    );
    assert_eq!(
        sizes(100.0, 5.0),
        sizes(100.0, 100_000.0),
        "the print scale plays no part"
    );
}

/// Off, the dots never run: the frame is exactly the one built without them.
#[test]
fn off_leaves_the_frame_unchanged() {
    let config = HeatmapConfig {
        volume_dots: VolumeDotStyle {
            enabled: false,
            ..dots_config().volume_dots
        },
        ..dots_config()
    };
    let trades = dense(5, 1_500, 0, 12_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let timeline = chart(11_500, 1_500, None);
    let plain = project(&history, &timeline, prices("90", "110"));
    let offered = frame_at(&history, &timeline, prices("90", "110"), &dots_at(250));
    assert!(!offered.volume_dots);
    assert_eq!(offered, plain);
}
