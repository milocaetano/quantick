//! The dot ladders: a zoom or a refit moves a dot's time window or price
//! level only where a rung starts or stops fitting, and a held rung rides
//! out a wobble at its boundary.

use super::*;

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
