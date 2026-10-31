//! The flow pane's three views: how they derive from the lane's two switches,
//! how a pane moves between them, what a divider drag decides and what a file
//! from the days of the rail reads as.

use super::*;
use crate::HeatmapConfig;
use crate::config::{MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE};

const CHART: f32 = 1_000.0;

fn lane(view: LaneView) -> LiveLaneStyle {
    let mut style = LiveLaneStyle::default();
    style.set_view(view);
    style
}

/// The view is the two switches read together: no third state to disagree
/// with them, and a tape switched off is candles only whatever `tape_only`
/// says.
#[test]
fn the_view_is_derived_from_the_two_switches() {
    for (enabled, tape_only, view) in [
        (true, false, LaneView::Beside),
        (true, true, LaneView::TapeOnly),
        (false, false, LaneView::CandlesOnly),
        (false, true, LaneView::CandlesOnly),
    ] {
        let style = LiveLaneStyle {
            enabled,
            tape_only,
            ..LiveLaneStyle::default()
        };
        assert_eq!(style.view(), view, "{enabled} {tape_only}");
    }
    assert_eq!(LiveLaneStyle::default().view(), LaneView::Beside);
}

/// Setting a view writes the switches the view reads back from, says whether
/// anything changed, and leaves the share alone.
#[test]
fn setting_a_view_is_idempotent_and_keeps_the_share() {
    for view in [LaneView::Beside, LaneView::CandlesOnly, LaneView::TapeOnly] {
        for from in [LaneView::Beside, LaneView::CandlesOnly, LaneView::TapeOnly] {
            let mut style = LiveLaneStyle {
                width_share: 0.42,
                ..lane(from)
            };
            assert_eq!(style.set_view(view), from != view, "{from:?} to {view:?}");
            assert_eq!(style.view(), view);
            assert!(!style.set_view(view), "the same view twice is no change");
            assert_eq!(style.width_share, 0.42);
        }
    }
}

/// Candles only leaves no stale `tape_only` behind, or the tape's own switch
/// would bring back a tape-only pane nobody asked for.
#[test]
fn candles_only_clears_tape_only_so_the_tape_switch_returns_beside() {
    let mut style = lane(LaneView::TapeOnly);
    style.set_view(LaneView::CandlesOnly);
    assert!(!style.tape_only);
    assert!(style.set_enabled(true));
    assert_eq!(style.view(), LaneView::Beside);
}

/// One button walks the three views in one order and comes home.
#[test]
fn the_view_button_cycles_through_the_three_views() {
    assert_eq!(LaneView::Beside.next(), LaneView::CandlesOnly);
    assert_eq!(LaneView::CandlesOnly.next(), LaneView::TapeOnly);
    assert_eq!(LaneView::TapeOnly.next(), LaneView::Beside);
}

/// The key goes to tape only and back to beside; there is no remembered view,
/// so candles only returns through tape only.
#[test]
fn the_tape_key_goes_to_tape_only_and_from_there_beside() {
    assert_eq!(LaneView::Beside.tape_key(), LaneView::TapeOnly);
    assert_eq!(LaneView::CandlesOnly.tape_key(), LaneView::TapeOnly);
    assert_eq!(LaneView::TapeOnly.tape_key(), LaneView::Beside);
}

/// Beside the candles the lane takes the band its share says, at every share
/// it can hold; alone it is the chart, off it is nothing.
#[test]
fn the_band_follows_the_view() {
    for share in [MIN_LIVE_LANE_SHARE, 0.2, 0.35, 0.5] {
        let beside = LiveLaneStyle {
            width_share: share,
            ..LiveLaneStyle::default()
        };
        assert!((beside.resolved_width_px(CHART) - CHART * share).abs() < 0.01);
        assert!((beside.open_share() - share).abs() < 1e-6);
    }
    assert_eq!(lane(LaneView::TapeOnly).resolved_width_px(CHART), CHART);
    assert_eq!(lane(LaneView::CandlesOnly).resolved_width_px(CHART), 0.0);
    assert_eq!(lane(LaneView::Beside).resolved_width_px(0.0), 0.0);
    assert_eq!(lane(LaneView::Beside).resolved_width_px(f32::NAN), 0.0);
}

/// Let go near the left edge and the tape takes the pane; the share is the one
/// the drag started from, ready for the way back.
#[test]
fn releasing_the_divider_at_the_left_edge_is_tape_only() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    // Left is a wider tape: 350 + 560 = 910 px, 90 px of candles left.
    drag.drag(&mut style, -560.0, CHART);
    assert_eq!(style.view(), LaneView::Beside, "only a release decides");
    assert!(drag.release(&mut style), "the view changed");
    assert_eq!(style.view(), LaneView::TapeOnly);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
}

/// Near the right edge the candles take the pane instead, by the same rule.
#[test]
fn releasing_the_divider_at_the_right_edge_is_candles_only() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    // 350 - 250 = 100 px of tape: under the threshold.
    drag.drag(&mut style, 250.0, CHART);
    assert!(drag.release(&mut style));
    assert_eq!(style.view(), LaneView::CandlesOnly);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
}

/// Short of the threshold the divider goes where the hand puts it and a
/// release keeps it there; the threshold itself holds, a pixel past it does
/// not.
#[test]
fn the_divider_travels_the_whole_chart_short_of_the_threshold() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    drag.drag(&mut style, -400.0, CHART);
    assert!((style.width_share - 0.75).abs() < 1e-6);
    let mut kept = style.clone();
    kept.sanitize();
    assert_eq!(kept, style, "a share past half is a share the lane keeps");

    // On past the threshold and back: under the hand again, nothing decided.
    drag.drag(&mut style, -200.0, CHART);
    drag.drag(&mut style, 300.0, CHART);
    assert!((style.width_share - 0.65).abs() < 1e-6);
    assert!(!drag.release(&mut style), "released in the open");
    assert_eq!(style.view(), LaneView::Beside);
    assert!((style.width_share - 0.65).abs() < 1e-6);

    // The threshold is the canvas pane's, measured from the edge.
    let mut edge = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&edge, CHART);
    drag.drag(&mut edge, 350.0 - LANE_FOLD_AT_PX, CHART);
    assert!(!drag.release(&mut edge.clone()), "at it holds");
    drag.drag(&mut edge, 1.0, CHART);
    assert!(drag.release(&mut edge));
    assert_eq!(edge.view(), LaneView::CandlesOnly);
}

/// Past the threshold the divider waits at the end of the open range instead
/// of following the hand off the pane, and comes back under it on the way out.
#[test]
fn past_the_threshold_the_divider_stops_at_the_open_range() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    drag.drag(&mut style, -10_000.0, CHART);
    assert!((style.resolved_width_px(CHART) - (CHART - LANE_FOLD_AT_PX)).abs() < 0.01);
    drag.drag(&mut style, 10_000.0, CHART);
    assert!((style.resolved_width_px(CHART) - 350.0).abs() < 0.01);
}

/// Bad input moves nothing; a chart that changed width mid-drag rebases the
/// drag on the band now on screen instead of jumping.
#[test]
fn a_drag_ignores_what_it_cannot_measure() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    let before = style.clone();
    drag.drag(&mut style, f32::NAN, CHART);
    drag.drag(&mut style, -10.0, 0.0);
    drag.drag(&mut style, -10.0, f32::INFINITY);
    assert_eq!(style, before);

    drag.drag(&mut style, -10.0, 2_000.0);
    assert!((style.resolved_width_px(2_000.0) - 710.0).abs() < 0.01);
}

/// The share the lane can hold runs nearly to the left edge, so the divider
/// can travel; it still keeps a floor and a ceiling.
#[test]
fn the_share_ceiling_is_lifted_but_still_a_ceiling() {
    const { assert!(MAX_LIVE_LANE_SHARE > 0.5) };
    let mut wide = LiveLaneStyle {
        width_share: 1.5,
        ..LiveLaneStyle::default()
    };
    wide.sanitize();
    assert_eq!(wide.width_share, MAX_LIVE_LANE_SHARE);
    let mut narrow = LiveLaneStyle {
        width_share: 0.0,
        ..LiveLaneStyle::default()
    };
    narrow.sanitize();
    assert_eq!(narrow.width_share, MIN_LIVE_LANE_SHARE);
}

/// A file that kept a fold opens beside the candles whatever the fold said,
/// a value nobody ever wrote included, and the next save drops the key. The
/// share and both switches are still the file's.
#[test]
fn a_saved_fold_of_any_value_loads_beside_the_candles() {
    for fold in ["open", "candles", "tape", "sideways"] {
        let text = format!("width_share = 0.42\nfold = \"{fold}\"\n");
        let style: LiveLaneStyle = toml::from_str(&text).expect("the file still loads");
        assert_eq!(style.view(), LaneView::Beside, "{fold}");
        assert_eq!(style.width_share, 0.42);
        let saved = toml::to_string(&style).unwrap();
        assert!(!saved.contains("fold"), "{saved}");
    }
    let alone: LiveLaneStyle = toml::from_str("tape_only = true\nfold = \"tape\"\n").unwrap();
    assert_eq!(alone.view(), LaneView::TapeOnly, "the file's own switch");
    let off: LiveLaneStyle = toml::from_str("enabled = false\nfold = \"candles\"\n").unwrap();
    assert_eq!(off.view(), LaneView::CandlesOnly, "the file's own switch");
    let number: LiveLaneStyle = toml::from_str("fold = 3\n").unwrap();
    assert_eq!(number.view(), LaneView::Beside);
}

/// The divider takes the pointer on the hairline's band; with no lane beside
/// the candles there is nothing to take.
#[test]
fn only_a_lane_beside_the_candles_has_a_divider() {
    let beside = lane(LaneView::Beside);
    assert!(beside.beside_candles());
    assert_eq!(beside.divider_hit(CHART, 5.0), Some((645.0, 655.0)));
    for view in [LaneView::TapeOnly, LaneView::CandlesOnly] {
        let hidden = lane(view);
        assert!(!hidden.beside_candles(), "{view:?}");
        assert_eq!(hidden.divider_hit(CHART, 5.0), None, "{view:?}");
    }
}

/// A pane keeps its drag in one slot across frames: the first step begins a
/// drag on the lane as it stands and later steps carry it on, the release
/// empties it, and a tape-only pane has no divider and its slot never fills.
#[test]
fn a_pane_carries_its_divider_drag_in_one_slot() {
    let mut config = HeatmapConfig::default();
    let mut hand = None;
    assert!(config.drag_lane_divider(&mut hand, -10.0, CHART));
    assert!(hand.is_some(), "the first step began the drag");
    assert!(config.drag_lane_divider(&mut hand, 10_000.0, CHART));
    assert_eq!(
        config.live_lane.view(),
        LaneView::Beside,
        "not yet released"
    );
    assert!(config.release_lane_divider(&mut hand), "the view changed");
    assert!(hand.is_none(), "a release empties the slot");
    assert_eq!(config.live_lane.view(), LaneView::CandlesOnly);
    assert!(!config.release_lane_divider(&mut hand), "nothing in hand");

    let mut alone = HeatmapConfig::default();
    alone.live_lane.set_view(LaneView::TapeOnly);
    assert!(alone.tape_only());
    let (before, mut hand) = (alone.clone(), None);
    assert!(!alone.drag_lane_divider(&mut hand, -300.0, CHART));
    assert_eq!((alone, hand), (before, None), "nothing moved");
}

/// On a wide chart the share's floor and ceiling sit further from the edge
/// than the threshold; the divider stays under the hand all the way there
/// rather than stopping at the floor, and only then does a release decide.
#[test]
fn on_a_wide_chart_the_divider_stays_under_the_hand_until_the_threshold() {
    const WIDE: f32 = 3_000.0;
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, WIDE);
    let mut wanted = style.resolved_width_px(WIDE);
    while wanted > 150.0 {
        drag.drag(&mut style, 1.0, WIDE);
        wanted -= 1.0;
        let drawn = style.resolved_width_px(WIDE);
        assert!((drawn - wanted).abs() < 0.05, "{drawn} under {wanted}");
    }
    // The threshold is its share's floor there, 150 px.
    assert!(!drag.release(&mut style.clone()), "at it holds");
    drag.drag(&mut style, 1.0, WIDE);
    assert!(drag.release(&mut style));
    assert_eq!(style.view(), LaneView::CandlesOnly);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
}

/// A lane kept outside the open range, narrower than the threshold or wider
/// than its mirror, drags under the hand from where it is instead of deciding
/// on the first pixel; only past the share's own floor or ceiling does a
/// release change the view, giving back the share it started from.
#[test]
fn a_lane_kept_outside_the_open_range_drags_from_where_it_is() {
    for (share, chart, toward_edge, lands_on) in [
        // 5% of 1000 px: 50 px, inside the 120 px threshold.
        (MIN_LIVE_LANE_SHARE, CHART, 1.0_f32, LaneView::CandlesOnly),
        // 95% of 1000 px: 950 px, past the 880 px mirror.
        (MAX_LIVE_LANE_SHARE, CHART, -1.0, LaneView::TapeOnly),
        // 150 px kept on a 3000 px monitor, shown on a 1500 px window.
        (MIN_LIVE_LANE_SHARE, 1_500.0, 1.0, LaneView::CandlesOnly),
    ] {
        let mut style = LiveLaneStyle {
            width_share: share,
            ..LiveLaneStyle::default()
        };
        let start = style.resolved_width_px(chart);
        let mut drag = LaneDividerDrag::begin(&style, chart);
        let mut wanted = start;
        // Away from the edge, pixel by pixel, the divider stays under the
        // hand; then back to where it started, and a release there holds.
        for step in [-toward_edge; 40].into_iter().chain([toward_edge; 40]) {
            drag.drag(&mut style, step, chart);
            wanted -= step;
            let drawn = style.resolved_width_px(chart);
            assert!((drawn - wanted).abs() < 0.05, "{drawn} under {wanted}");
        }
        assert!((style.resolved_width_px(chart) - start).abs() < 0.05);
        assert!(!drag.release(&mut style.clone()), "{share}");
        // One pixel past the share's own floor or ceiling decides.
        drag.drag(&mut style, toward_edge, chart);
        assert!(drag.release(&mut style));
        assert_eq!(style.view(), lands_on, "{share} on {chart}");
        assert_eq!(style.width_share, share);
    }
}

/// A switched-off tape can carry a stale `tape_only` (a preset, the layer's
/// setter): it reads candles only, clearing it counts as a change, and turning
/// the tape on lands beside the candles rather than silently on tape only.
#[test]
fn a_stale_tape_only_never_decides_a_view() {
    let stale = || LiveLaneStyle {
        enabled: false,
        tape_only: true,
        ..LiveLaneStyle::default()
    };
    assert_eq!(stale().view(), LaneView::CandlesOnly);

    let mut cleared = stale();
    assert!(cleared.set_view(LaneView::CandlesOnly), "a field moved");
    assert!(!cleared.tape_only);
    assert!(
        !cleared.set_view(LaneView::CandlesOnly),
        "and now none does"
    );

    let mut switched = stale();
    assert!(switched.set_enabled(true));
    assert_eq!(switched.view(), LaneView::Beside);

    // Off from tape only and on again is the same path.
    let mut alone = lane(LaneView::TapeOnly);
    assert!(alone.set_enabled(false));
    assert!(alone.set_enabled(true));
    assert_eq!(alone.view(), LaneView::Beside);
    // A tape already on keeps its view.
    let mut on = lane(LaneView::TapeOnly);
    assert!(!on.set_enabled(true));
    assert_eq!(on.view(), LaneView::TapeOnly);
}

/// A drag given up returns the share and decides no view, wherever the hand was.
#[test]
fn an_abandoned_drag_restores_the_share_and_decides_nothing() {
    let mut config = HeatmapConfig::default();
    let share = config.live_lane.width_share;
    let mut hand = None;
    assert!(config.drag_lane_divider(&mut hand, -5_000.0, CHART));
    assert_ne!(config.live_lane.width_share, share);
    config.abandon_lane_divider(&mut hand);
    assert!(hand.is_none());
    assert_eq!(config.live_lane.width_share, share);
    assert_eq!(config.live_lane.view(), LaneView::Beside);
    config.abandon_lane_divider(&mut hand);
    assert_eq!(config.live_lane.width_share, share, "an empty hand is idle");
}
