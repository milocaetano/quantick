//! What the redesigned volume dots promise the trader: a candle dot is its
//! bar's volume at its level, the tape never follows the candle view, no
//! drawn dot hides another, and each pane sizes its dots against its own
//! biggest.

use rust_decimal::prelude::ToPrimitive as _;

use super::*;
use crate::projection::{DotSizing, candle_dot_px, lane_bars};

/// A candle dot is its bar's volume at its level: one dot per bar and level,
/// holding exactly what that bar traded inside the level, so the level where
/// the footprint shows the most volume is the bar's biggest dot.
#[test]
fn a_candle_dot_is_its_bars_volume_at_its_level() {
    let config = dots_config();
    let trades = dense(43, 4_000, 0, 12_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let timeline = chart(12_700, 1_500, None);
    let frame = frame_at(&history, &timeline, prices("90", "110"), &levels(250, 1, 2));
    let candles: Vec<&AggressionPrimitive> =
        frame.aggressions.iter().filter(|dot| !dot.live).collect();
    assert!(candles.len() > 100, "the fixture draws candle dots");
    let traded_in = |bar: i64, low: Decimal, span: Decimal| -> Decimal {
        trades
            .iter()
            .filter(|trade| trade.1.div_euclid(1_000) == bar)
            .filter(|trade| dec(&trade.2) >= low && dec(&trade.2) < low + span)
            .map(|trade| dec(&trade.3))
            .sum()
    };
    let mut seen = std::collections::BTreeSet::new();
    for dot in &candles {
        let bar = dot.first_timestamp_ms.div_euclid(1_000);
        assert_eq!(dot.last_timestamp_ms.div_euclid(1_000), bar, "one bar");
        assert!(
            seen.insert((bar, dot.price_bucket)),
            "two dots for one bar and level: {dot:?}"
        );
        assert_eq!(dot.price_span, dec("2"), "the candles' own level");
        assert_eq!(
            dot.quantity,
            traded_in(bar, dot.price_bucket, dot.price_span),
            "the bar's exact volume in its level: {dot:?}"
        );
    }
    let sizing = DotSizing {
        tape_column_px: 100.0,
        candle_column_px: 100.0,
        px_per_price: 100.0,
        typed_full: None,
    };
    let full = sizing.full_quantity(candles.iter().copied(), false);
    let radius =
        |dot: &AggressionPrimitive| sizing.radius(&config.bubbles, &config.live_lane, dot, full);
    for bar in 0..12 {
        let of_bar: Vec<&&AggressionPrimitive> = candles
            .iter()
            .filter(|dot| dot.first_timestamp_ms.div_euclid(1_000) == bar)
            .collect();
        let busiest = (90..110)
            .step_by(2)
            .map(|low| traded_in(bar, Decimal::from(low), dec("2")))
            .max()
            .expect("levels");
        let biggest = of_bar
            .iter()
            .max_by(|a, b| radius(a).total_cmp(&radius(b)))
            .expect("the bar draws dots");
        assert_eq!(
            biggest.quantity, busiest,
            "bar {bar}: the busiest level is the biggest dot"
        );
    }
}

/// The tape is a view of its own: two frames that differ in the candles'
/// visible slice, bar width, price axis and whether the newest bar is on
/// screen, but share the tape's geometry, draw the same tape dots on the same
/// rungs. A price axis that leaves the live price out of sight still returns
/// every tape dot; the painter clips.
#[test]
fn the_tape_never_follows_the_candle_view() {
    let config = dots_config();
    let trades = dense(47, 4_000, 0, 20_000);
    let history = recorded(config.clone(), &borrowed(&trades));
    let (now_ms, lane_ms) = (12_700, 3_000);
    let all: Vec<Bar> = (0..12).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
    let forming = bar(12_000, now_ms);
    let reach = lane_bars(&all, Some(&forming), lane_ms);
    let tape_span = Some(18.0);
    let tape = |visible: std::ops::Range<usize>, px_per_bar: f32, low: &str, high: &str| {
        let on_newest_bar = visible.end == all.len() + 1;
        let (closed, partial) = if on_newest_bar {
            (&all[visible.start..], Some(&forming))
        } else {
            (&all[visible.clone()], None)
        };
        let geometry = PaneGeometry {
            px_per_bar,
            lane_width_px: 300.0,
            lane_window_ms: lane_ms,
            height_px: 400.0,
            lane_bars: reach.clone(),
        };
        let range = (low.parse::<f64>().unwrap(), high.parse::<f64>().unwrap());
        let zoom = DotRungMemory::default().choose(geometry, &config, range, tape_span);
        let dots = VolumeDots::resolve(&zoom, closed, partial);
        let timeline = chart(now_ms, lane_ms, Some(visible));
        let frame = frame_at(&history, &timeline, prices(low, high), &dots);
        let mut marks: Vec<_> = frame
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .map(|mark| {
                (
                    mark.first_timestamp_ms,
                    mark.last_timestamp_ms,
                    mark.price_bucket,
                    mark.price_span,
                    mark.quantity,
                    mark.buy_quantity,
                )
            })
            .collect();
        marks.sort();
        ((zoom.tape_window_ms, zoom.tape_level_ticks), marks)
    };
    let (rungs, reference) = tape(0..13, 40.0, "90", "110");
    assert!(reference.len() > 20, "the fixture draws tape dots");
    let whole_windows: Decimal = trades
        .iter()
        .filter(|trade| trade.1 >= now_ms - lane_ms + rungs.0 && trade.1 <= now_ms)
        .map(|trade| dec(&trade.3))
        .sum();
    let drawn: Decimal = reference.iter().map(|mark| mark.4).sum();
    assert!(
        drawn >= whole_windows,
        "every print of the tape's whole windows is drawn"
    );
    for (visible, px_per_bar, low, high) in [
        (3..9, 12.0, "60", "130"),
        (5..13, 80.0, "88", "112"),
        (0..13, 40.0, "20", "60"),
    ] {
        assert_eq!(
            tape(visible.clone(), px_per_bar, low, high),
            (rungs, reference.clone()),
            "the candles at {visible:?}, {px_per_bar} px a bar, {low}..{high} moved the tape"
        );
    }
}

/// No drawn dot hides another. With the painter's radius function, and every
/// dot at its cell's centre, no two tape discs intersect and no two candle
/// discs intersect, on a dense tape squeezed to 1, 5 and 15 minutes and with
/// the price axis squeezed tenfold. The dots still differ in size, even on
/// a uniform random tape whose cells trade much alike.
#[test]
fn no_two_drawn_dots_overlap() {
    let config = HeatmapConfig {
        bubbles: BubbleStyle {
            min_radius: 2.0,
            max_radius: 15.0,
            ..dots_config().bubbles
        },
        ..dots_config()
    };
    let (now_ms, bar_ms) = (1_002_500_i64, 5_000_i64);
    let trades = dense(53, 20_000, 0, now_ms);
    let history = recorded(config.clone(), &borrowed(&trades));
    let closed: Vec<Bar> = (0..now_ms / bar_ms)
        .map(|i| bar(i * bar_ms, i * bar_ms + bar_ms - 1))
        .collect();
    let forming = bar(now_ms / bar_ms * bar_ms, now_ms);
    let (px_per_bar, lane_px, axis) = (12.0_f32, 300.0_f32, (90.0, 110.0));
    for lane_ms in [60_000_i64, 300_000, 900_000] {
        for height in [600.0_f32, 60.0] {
            let timeline = BarTimeline::from_bars(
                0,
                &closed,
                Some(&forming),
                Some(crate::LiveEdge {
                    now_ms,
                    window_ms: lane_ms,
                    reference_ms: lane_ms,
                    on_newest_bar: true,
                }),
            );
            let on_tape: Vec<f64> = trades
                .iter()
                .filter(|trade| trade.1 > now_ms - lane_ms)
                .map(|trade| trade.2.parse::<f64>().unwrap())
                .collect();
            let tape_span = on_tape.iter().copied().fold(f64::MIN, f64::max)
                - on_tape.iter().copied().fold(f64::MAX, f64::min);
            let geometry = PaneGeometry {
                px_per_bar,
                lane_width_px: lane_px,
                lane_window_ms: lane_ms,
                height_px: height,
                lane_bars: lane_bars(&closed, Some(&forming), lane_ms),
            };
            let mut memory = DotRungMemory::default();
            let zoom = memory.choose(geometry, &config, axis, Some(tape_span));
            let dots = VolumeDots::resolve(&zoom, &closed, Some(&forming));
            let window = prices("90", "110");
            let frame = frame_at(&history, &timeline, window, &dots);
            let sizing = memory
                .sizing(&dots.scale(&config, axis), height)
                .expect("a zoom was chosen");
            let px_per_ms = f64::from(lane_px) / lane_ms as f64;
            for live in [true, false] {
                let pane = if live { "tape" } else { "candle" };
                let marks: Vec<&AggressionPrimitive> = frame
                    .aggressions
                    .iter()
                    .filter(|mark| mark.live == live)
                    .collect();
                assert!(marks.len() > 10, "{lane_ms} ms, {height} px: {pane} dots");
                let full = sizing.full_quantity(marks.iter().copied(), live);
                let mut discs: Vec<(f64, f64, f64)> = marks
                    .iter()
                    .map(|mark| {
                        let centre = mark.price_bucket + mark.price_span / Decimal::TWO;
                        assert_eq!(
                            mark.y,
                            window.y(centre).unwrap(),
                            "drawn at its level's centre: {mark:?}"
                        );
                        let x = if live {
                            let width = zoom.tape_window_ms;
                            let start = mark.first_timestamp_ms.div_euclid(width) * width;
                            (start + width / 2 - (now_ms - lane_ms)) as f64 * px_per_ms
                        } else {
                            let slot = mark.first_timestamp_ms.div_euclid(bar_ms);
                            (slot as f64 + 0.5) * f64::from(px_per_bar)
                        };
                        let y = (axis.1 - centre.to_f64().unwrap()) / (axis.1 - axis.0)
                            * f64::from(height);
                        let radius = sizing.radius(&config.bubbles, &config.live_lane, mark, full);
                        (x, y, f64::from(radius))
                    })
                    .collect();
                discs.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (index, a) in discs.iter().enumerate() {
                    for b in &discs[index + 1..] {
                        if b.0 - a.0 >= a.2 + b.2 {
                            break;
                        }
                        let distance = (a.0 - b.0).hypot(a.1 - b.1);
                        assert!(
                            distance >= a.2 + b.2 - 1e-3,
                            "tape {lane_ms} ms, {height} px: two {pane} dots overlap: {a:?} {b:?}"
                        );
                    }
                }
                let largest = discs.iter().map(|disc| disc.2).fold(0.0, f64::max);
                let smallest = discs.iter().map(|disc| disc.2).fold(f64::MAX, f64::min);
                assert!(
                    largest > 1.1 * smallest,
                    "tape {lane_ms} ms, {height} px: every {pane} dot the same size \
                     ({smallest}..{largest})"
                );
            }
        }
    }
}

/// With the automatic scale each pane sizes its dots against its own biggest:
/// that dot is drawn at the pane's largest radius and one with a quarter of
/// its quantity visibly smaller. The candles' sizes never follow the tape's
/// zoom, and the tape's never follow the candles'. A typed full size is the
/// scale on both panes.
#[test]
fn each_pane_sizes_its_dots_against_its_own_biggest() {
    let config = HeatmapConfig {
        bubbles: BubbleStyle {
            min_radius: 2.0,
            max_radius: 10.0,
            ..dots_config().bubbles
        },
        ..dots_config()
    };
    assert!(config.volume_dots.auto_full, "automatic by default");
    let history = recorded(
        config.clone(),
        &[
            (1, 1_100, "100", "8", Side::Buy),
            (2, 1_300, "104", "2", Side::Sell),
            (3, 3_100, "100", "20", Side::Buy),
            (4, 3_600, "104", "5", Side::Sell),
        ],
    );
    let roomy = DotSizing {
        tape_column_px: 100.0,
        candle_column_px: 100.0,
        px_per_price: 100.0,
        typed_full: None,
    };
    let radii = |frame: &HeatmapProjection, sizing: &DotSizing, live: bool| {
        let marks: Vec<&AggressionPrimitive> = frame
            .aggressions
            .iter()
            .filter(|mark| mark.live == live)
            .collect();
        let full = sizing.full_quantity(marks.iter().copied(), live);
        let mut radii: Vec<(Vec<u64>, Decimal, f32)> = marks
            .iter()
            .map(|mark| {
                let radius = sizing.radius(&config.bubbles, &config.live_lane, mark, full);
                (mark.agg_ids.clone(), mark.quantity, radius)
            })
            .collect();
        radii.sort_by(|a, b| a.0.cmp(&b.0));
        (full, radii)
    };
    let frame = frame_at(
        &history,
        &chart(3_900, 1_500, None),
        prices("90", "110"),
        &dots_at(250),
    );
    for live in [true, false] {
        let (full, radii) = radii(&frame, &roomy, live);
        assert_eq!(full, dec("20"), "the pane's own biggest (tape: {live})");
        let (_, pane_max) = config.live_lane.pane_radii(&config.bubbles, live, true);
        let largest = radii
            .iter()
            .find(|dot| dot.1 == dec("20"))
            .expect("the biggest")
            .2;
        let quarter = radii
            .iter()
            .find(|dot| dot.1 == dec("5"))
            .expect("a quarter")
            .2;
        assert!((largest - pane_max).abs() < 1e-5, "full size: {largest}");
        assert!(
            quarter < 0.75 * largest,
            "a quarter of the volume is visibly smaller: {quarter} vs {largest}"
        );
    }

    // The tape's zoom leaves the candles' sizes alone.
    let wide_tape = frame_at(
        &history,
        &chart(3_900, 1_500, None),
        prices("90", "110"),
        &dots_at(1_000),
    );
    assert_eq!(
        radii(&wide_tape, &roomy, false),
        radii(&frame, &roomy, false),
        "a tape zoom resized the candles"
    );
    // The candles' view — slice, bar width, levels — leaves the tape's alone.
    let moved = frame_at(
        &history,
        &chart(3_900, 1_500, Some(2..4)),
        prices("90", "110"),
        &levels(250, 1, 5),
    );
    let thin = DotSizing {
        candle_column_px: 7.0,
        ..roomy
    };
    assert_eq!(
        radii(&moved, &thin, true),
        radii(&frame, &roomy, true),
        "a candle move resized the tape"
    );

    // Typed, the full size is the trader's number on both panes.
    let typed = DotSizing {
        typed_full: Some(dec("10")),
        ..roomy
    };
    for live in [true, false] {
        assert_eq!(radii(&frame, &typed, live).0, dec("10"));
    }
}

/// A candle dot is `2 × max radius × CANDLE_DOT_RADIUS_SHARE` across: the
/// size the candles' level rung is chosen for.
#[test]
fn a_candle_dot_is_a_share_of_a_full_dot() {
    let bubbles = BubbleStyle {
        max_radius: 15.0,
        ..BubbleStyle::default()
    };
    assert!((candle_dot_px(&bubbles) - 12.0).abs() < 1e-5);
}
