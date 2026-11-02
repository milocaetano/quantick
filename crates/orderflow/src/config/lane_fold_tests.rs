//! Closing a side with the divider: what it resolves to, where the rail sits,
//! what a drag decides and what an old file reads as.

use super::*;
use crate::HeatmapConfig;
use crate::config::{MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE};

const CHART: f32 = 1_000.0;

fn lane(view: LaneView) -> LiveLaneStyle {
    let mut style = LiveLaneStyle::default();
    style.set_view(view);
    style
}

/// Dragged open and let go of at this hand: the tape under it, at the share it
/// says, whatever the lane kept.
fn assert_under_the_hand(style: &LiveLaneStyle, wanted_px: f32) {
    assert_eq!(style.view(), LaneView::Beside);
    let drawn = style.resolved_width_px(CHART);
    assert!(
        (drawn - wanted_px).abs() < 0.05,
        "{drawn} under {wanted_px}"
    );
}

/// A drag begun on the market's rail is no word of the trader's, even when a
/// print releases the hold under the hand; once it has left the rail, it is.
#[test]
fn a_drag_from_the_markets_rail_never_saves_the_held_view() {
    let mut held = LiveLaneStyle::default();
    held.market_hold.held = true;
    let mut drag = LaneDividerDrag::begin(&held, CHART);
    assert!(!drag.drag(&mut held, 1.0, CHART), "a wiggle writes nothing");
    assert_eq!(held.view(), LaneView::Beside);
    assert!(held.market_held(), "and the hold stands");

    held.market_hold.held = false;
    assert!(
        !drag.drag(&mut held, 1.0, CHART),
        "still on the rail's range"
    );
    assert!(!drag.drag(&mut held, -1.0, CHART));
    assert_eq!(held.view(), LaneView::Beside, "the saved view is not held");

    assert!(!drag.drag(&mut held, -300.0, CHART), "open under the hand");
    assert_eq!(held.view(), LaneView::Beside);
    assert!(
        drag.drag(&mut held, 300.0, CHART),
        "back there, it is the hand's"
    );
    assert_eq!(held.view(), LaneView::CandlesOnly);
}

/// A drag off the market's rail stands the hold down and follows the hand.
#[test]
fn a_drag_during_a_hold_stands_it_down_and_follows_the_hand() {
    let mut held = LiveLaneStyle {
        width_share: 0.42,
        ..LiveLaneStyle::default()
    };
    held.market_hold.held = true;
    let mut drag = LaneDividerDrag::begin(&held, CHART);
    assert!(drag.drag(&mut held, -292.0, CHART), "the rail opened");
    assert!(!held.market_held() && held.market_hold.stood_down);
    assert_under_the_hand(&held, LANE_FOLD_RAIL_PX + 292.0);
}

/// A lane beside the candles is the lane every pane always had: the same band at
/// every share it could already hold, with no rail.
#[test]
fn an_open_lane_takes_exactly_the_band_it_always_took() {
    for share in [MIN_LIVE_LANE_SHARE, 0.2, 0.35, 0.5] {
        let open = LiveLaneStyle {
            width_share: share,
            ..LiveLaneStyle::default()
        };
        assert_eq!(open.view(), LaneView::Beside);
        assert!((open.resolved_width_px(CHART) - CHART * share).abs() < 0.01);
        assert!((open.budget_share() - share).abs() < 1e-6);
        assert!(open.fold_rail(CHART).is_none());
    }
}

/// A view reserves what it always did, and the rail stands at the closed edge
/// whichever way it closed. Only candles keep clear of it: the tape alone runs
/// under its rail, its window unmoved.
#[test]
fn one_side_alone_keeps_a_rail_at_the_closed_edge() {
    let candles = lane(LaneView::CandlesOnly);
    assert_eq!(candles.resolved_width_px(CHART), 0.0);
    assert!((candles.candles_clear_px(CHART) - LANE_FOLD_RAIL_PX).abs() < 0.01);
    let tape = lane(LaneView::TapeOnly);
    assert_eq!(tape.resolved_width_px(CHART), CHART);
    assert_eq!(tape.candles_clear_px(CHART), CHART);

    // Never more rail than half a tiny chart, never a rail on no chart.
    assert!((candles.candles_clear_px(10.0) - 5.0).abs() < 0.01);
    assert!(tape.fold_rail(0.0).is_none());
    assert!(candles.fold_rail(f32::NAN).is_none());
    assert_eq!(tape.resolved_width_px(f32::NAN), 0.0);

    let beside = LiveLaneStyle::default();
    assert_eq!(
        beside.candles_clear_px(CHART),
        beside.resolved_width_px(CHART)
    );
}

/// The rail is painted as wide as a canvas pane's and hit wider, reaching
/// into the side that is still open.
#[test]
fn the_rail_sits_on_the_closed_edge_and_is_hit_wider_than_it_paints() {
    let tape = lane(LaneView::CandlesOnly)
        .fold_rail(CHART)
        .expect("a rail");
    assert_eq!(tape.side, LaneSide::Tape);
    assert!((tape.paint_from_px - (CHART - LANE_FOLD_RAIL_PX)).abs() < 0.01);
    assert!((tape.paint_to_px - CHART).abs() < 0.01);
    assert!((tape.hit_from_px - (CHART - LANE_FOLD_HIT_PX)).abs() < 0.01);
    assert!((tape.hit_to_px - CHART).abs() < 0.01);

    let candles = lane(LaneView::TapeOnly).fold_rail(CHART).expect("a rail");
    assert_eq!(candles.side, LaneSide::Trail);
    assert_eq!(candles.paint_from_px, 0.0);
    assert!((candles.paint_to_px - LANE_FOLD_RAIL_PX).abs() < 0.01);
    assert_eq!(candles.hit_from_px, 0.0);
    assert!((candles.hit_to_px - LANE_FOLD_HIT_PX).abs() < 0.01);
    const { assert!(LANE_FOLD_HIT_PX > LANE_FOLD_RAIL_PX) };
}

/// Dragging the divider to the right is the Tape segment's click: the switch
/// itself goes off, so the segment reads unlit, and it clicks back at the default
/// width whatever width the lane had.
#[test]
fn dragging_right_is_tape_off_and_the_segment_reopens_at_the_default() {
    let mut style = LiveLaneStyle {
        width_share: 0.42,
        ..LiveLaneStyle::default()
    };
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    // 420 - 301 = 119 px of tape: past the threshold.
    assert!(drag.drag(&mut style, 301.0, CHART));
    assert_eq!(style.view(), LaneView::CandlesOnly);
    assert!(!style.enabled, "the very switch the segment reads");
    assert!(!style.view().shows(LaneSide::Tape), "the segment is unlit");
    assert_eq!(style.width_share, 0.42, "the share the drag started from");

    let back = style.view().toggled(LaneSide::Tape).expect("a way back");
    assert!(style.set_view(back));
    assert_eq!(style.view(), LaneView::Beside);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
}

/// The mirror: dragging left is tape only, and the Trail segment clicks back.
#[test]
fn dragging_left_is_tape_only_and_the_trail_segment_reopens_at_the_default() {
    let mut style = LiveLaneStyle {
        width_share: 0.42,
        ..LiveLaneStyle::default()
    };
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    // 420 + 461 = 881 px of tape: 119 px of candles left.
    assert!(drag.drag(&mut style, -461.0, CHART));
    assert_eq!(style.view(), LaneView::TapeOnly);
    assert!(style.tape_only, "the very switch the segment reads");
    assert!(!style.view().shows(LaneSide::Trail), "the segment is unlit");

    let back = style.view().toggled(LaneSide::Trail).expect("a way back");
    assert!(style.set_view(back));
    assert_eq!(style.view(), LaneView::Beside);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
}

/// The last-lit rule sees a side closed by hand as it sees one closed by click.
#[test]
fn the_last_lit_rule_sees_a_drag_closed_side() {
    for (delta, last_lit, other) in [
        (10_000.0, LaneSide::Trail, LaneSide::Tape),
        (-10_000.0, LaneSide::Tape, LaneSide::Trail),
    ] {
        let mut style = LiveLaneStyle::default();
        let mut drag = LaneDividerDrag::begin(&style, CHART);
        assert!(drag.drag(&mut style, delta, CHART));
        assert_eq!(style.view().toggled(last_lit), None, "{last_lit:?}");
        assert_eq!(
            style.view().toggled(other),
            Some(LaneView::Beside),
            "{other:?}"
        );
    }
}

/// Closed by button and reopened by the rail, the tape follows the hand: it stays
/// shut through a twitch, then opens where the hand is, never at the default.
#[test]
fn a_button_closed_side_reopens_under_the_hand_with_no_snap() {
    let mut style = lane(LaneView::CandlesOnly);
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    assert!(!drag.drag(&mut style, -50.0, CHART), "a twitch on the rail");
    assert_eq!(style.view(), LaneView::CandlesOnly);
    assert!(drag.drag(&mut style, -250.0, CHART));
    assert_under_the_hand(&style, LANE_FOLD_RAIL_PX + 300.0);
    assert!(drag.drag(&mut style, 300.0, CHART), "and shut again");
    assert_eq!(style.view(), LaneView::CandlesOnly);

    let mut style = lane(LaneView::TapeOnly);
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    assert!(!drag.drag(&mut style, 50.0, CHART), "a twitch on the rail");
    assert_eq!(style.view(), LaneView::TapeOnly);
    assert!(drag.drag(&mut style, 250.0, CHART));
    assert_under_the_hand(&style, CHART - LANE_FOLD_RAIL_PX - 300.0);
}

/// Closed by a drag and reopened by a click on its rail: the default width.
#[test]
fn a_drag_closed_side_reopens_by_rail_click_at_the_default() {
    for delta in [10_000.0, -10_000.0] {
        let mut style = LiveLaneStyle {
            width_share: 0.42,
            ..LiveLaneStyle::default()
        };
        LaneDividerDrag::begin(&style, CHART).drag(&mut style, delta, CHART);
        assert_ne!(style.view(), LaneView::Beside);
        assert!(style.set_fold(LaneFold::Open));
        assert_eq!(style.view(), LaneView::Beside);
        assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
        assert!(!style.set_fold(LaneFold::Open), "a retry moves nothing");
    }
}

/// Every door back beside the candles that is not a hand lands on the default.
#[test]
fn every_door_back_beside_is_the_default_width() {
    let doors: [(LaneView, fn(&mut LiveLaneStyle) -> bool); 5] = [
        (LaneView::CandlesOnly, |s| s.set_view(LaneView::Beside)),
        (LaneView::TapeOnly, |s| s.set_view(LaneView::Beside)),
        (LaneView::CandlesOnly, |s| s.set_enabled(true)),
        (LaneView::TapeOnly, |s| s.set_tape_only(false)),
        (LaneView::TapeOnly, |s| s.set_fold(LaneFold::Open)),
    ];
    for (from, door) in doors {
        let mut style = LiveLaneStyle {
            width_share: 0.6,
            ..lane(from)
        };
        assert!(door(&mut style), "{from:?}");
        assert_eq!(style.view(), LaneView::Beside, "{from:?}");
        assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE, "{from:?}");
    }
}

/// Short of the threshold the divider goes where the hand puts it, past the
/// half the lane used to stop at; dragging back out of a closed side in the same
/// gesture opens the lane again under the hand.
#[test]
fn the_divider_travels_the_whole_chart_short_of_the_edge() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    drag.drag(&mut style, -400.0, CHART);
    assert_eq!(style.view(), LaneView::Beside);
    assert!((style.width_share - 0.75).abs() < 1e-6);
    let mut kept = style.clone();
    kept.sanitize();
    assert_eq!(kept, style, "a share past half is a share the lane keeps");

    // On past the threshold, then back: open again where the hand is.
    drag.drag(&mut style, -200.0, CHART);
    assert_eq!(style.view(), LaneView::TapeOnly);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
    drag.drag(&mut style, 300.0, CHART);
    assert_eq!(style.view(), LaneView::Beside);
    assert!((style.width_share - 0.65).abs() < 1e-6);

    // The threshold is the canvas pane's, measured from the edge.
    let mut edge = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&edge, CHART);
    drag.drag(&mut edge, 350.0 - LANE_FOLD_AT_PX, CHART);
    assert_eq!(edge.view(), LaneView::Beside, "at the threshold holds");
    drag.drag(&mut edge, 1.0, CHART);
    assert_eq!(edge.view(), LaneView::CandlesOnly);
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
    assert_eq!(style.view(), LaneView::Beside);
    assert!((style.resolved_width_px(2_000.0) - 710.0).abs() < 0.01);
}

/// The share the lane can hold now runs nearly to the left edge, so the
/// divider can travel; it still keeps a floor and a ceiling.
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

/// Only the market's hold moves the budget: a side closed by hand spends what a
/// side closed by click spends, which is what a pane alone always spent.
#[test]
fn the_mark_budget_moves_only_for_the_markets_hold() {
    let mut held = LiveLaneStyle::default();
    held.market_hold.held = true;
    assert_eq!(held.budget_share(), 0.0);
    for view in [LaneView::TapeOnly, LaneView::CandlesOnly] {
        let alone = LiveLaneStyle {
            width_share: 0.3,
            ..lane(view)
        };
        assert!((alone.budget_share() - 0.3).abs() < 1e-6, "{view:?}");
    }
}

/// What a file from before the state became one reads as: the side it saved, at
/// the share it saved, and a view other than beside wins, as it won on screen.
#[test]
fn an_old_files_fold_reads_as_the_view_it_showed() {
    let read = |text: &str| toml::from_str::<LiveLaneStyle>(text).expect("reads");
    let tape = read("width_share = 0.42\nfold = \"tape\"\n");
    assert_eq!(tape.view(), LaneView::CandlesOnly);
    assert_eq!(tape.width_share, 0.42);
    let candles = read("width_share = 0.42\nfold = \"candles\"\n");
    assert_eq!(candles.view(), LaneView::TapeOnly);
    assert_eq!(candles.width_share, 0.42);
    assert_eq!(read("fold = \"open\"\n").view(), LaneView::Beside);
    assert_eq!(read("width_share = 0.35\n").view(), LaneView::Beside);

    // Tape only won over the tape's fold, and the tape off over the candles'.
    let alone = read("tape_only = true\nfold = \"tape\"\n");
    assert_eq!(alone.view(), LaneView::TapeOnly);
    let off = read("enabled = false\nfold = \"candles\"\n");
    assert_eq!(off.view(), LaneView::CandlesOnly);
    assert!(!off.tape_only, "not moved");

    assert!(toml::from_str::<LiveLaneStyle>("fold = \"sideways\"\n").is_err());
}

/// The fold is read, never written: the view saves itself, and nothing else.
#[test]
fn a_fold_is_never_written() {
    for view in [LaneView::Beside, LaneView::CandlesOnly, LaneView::TapeOnly] {
        let style = lane(view);
        let text = toml::to_string(&style).unwrap();
        assert!(!text.contains("fold ="), "{text}");
        let read: LiveLaneStyle = toml::from_str(&text).unwrap();
        assert_eq!(read, style, "{view:?} round-trips");
    }
}

/// The divider takes the pointer on the hairline's band beside the candles and
/// on the whole rail in either single view.
#[test]
fn the_divider_is_taken_on_its_band_or_on_its_rail() {
    let open = LiveLaneStyle::default();
    assert_eq!(open.divider_hit(CHART, 5.0), Some((645.0, 655.0)));
    for view in [LaneView::CandlesOnly, LaneView::TapeOnly] {
        let alone = lane(view);
        let rail = alone.fold_rail(CHART).expect("a rail");
        let hit = Some((rail.hit_from_px, rail.hit_to_px));
        assert_eq!(alone.divider_hit(CHART, 5.0), hit, "{view:?}");
    }
    let tape = lane(LaneView::CandlesOnly).fold_rail(CHART).unwrap();
    assert!((tape.edge_px() - (CHART - LANE_FOLD_RAIL_PX)).abs() < 0.01);
    let candles = lane(LaneView::TapeOnly).fold_rail(CHART).unwrap();
    assert!((candles.edge_px() - LANE_FOLD_RAIL_PX).abs() < 0.01);
}

/// A drag says when the view changed, so the caller can keep the view as a
/// setting while the width stays navigation.
#[test]
fn a_drag_says_when_the_view_changed() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    assert!(
        !drag.drag(&mut style, -10.0, CHART),
        "a wider tape is no view"
    );
    assert!(drag.drag(&mut style, 300.0, CHART), "closed the tape");
    assert!(!drag.drag(&mut style, 10.0, CHART), "still closed");
    assert!(drag.drag(&mut style, -300.0, CHART), "open again");
    assert!(!drag.drag(&mut style, f32::NAN, CHART));
}

/// A pane keeps its drag in one slot across frames: the first step begins a
/// drag on the lane as it stands and later steps carry it on, a tape-only pane's
/// rail included.
#[test]
fn a_pane_carries_its_divider_drag_in_one_slot() {
    let mut config = HeatmapConfig::default();
    let mut hand = None;
    assert!(!config.drag_lane_divider(&mut hand, -10.0, CHART));
    assert!(hand.is_some(), "the first step began the drag");
    assert!(config.drag_lane_divider(&mut hand, 10_000.0, CHART));
    assert_eq!(config.live_lane.view(), LaneView::CandlesOnly);

    let mut alone = HeatmapConfig::default();
    alone.live_lane.tape_only = true;
    let mut hand = None;
    assert!(alone.drag_lane_divider(&mut hand, 400.0, CHART));
    assert_eq!(alone.live_lane.view(), LaneView::Beside);
}

/// The wire's words, by this same mapping, retry-safe, from every view.
#[test]
fn the_folds_words_are_the_views() {
    for (fold, view) in [
        (LaneFold::Open, LaneView::Beside),
        (LaneFold::Candles, LaneView::TapeOnly),
        (LaneFold::Tape, LaneView::CandlesOnly),
    ] {
        assert_eq!(fold.view(), view);
        assert_eq!(view.fold(), fold);
        for from in [LaneView::Beside, LaneView::CandlesOnly, LaneView::TapeOnly] {
            let mut style = LiveLaneStyle {
                width_share: 0.42,
                ..lane(from)
            };
            assert_eq!(style.set_fold(fold), from != view, "{from:?} to {view:?}");
            assert_eq!(style.view(), view);
            assert!(!style.set_fold(fold), "a retry moves nothing");
            let share = if view == LaneView::Beside && from != view {
                DEFAULT_LIVE_LANE_SHARE
            } else {
                0.42
            };
            assert_eq!(style.width_share, share, "{from:?} to {view:?}");
        }
    }
}

/// On a wide chart the share's floor and ceiling sit further from the edge
/// than the threshold; the divider stays under the hand all the way to where the
/// side closes rather than stopping at the floor and jumping.
#[test]
fn on_a_wide_chart_the_divider_stays_under_the_hand_until_it_closes() {
    const WIDE: f32 = 3_000.0;
    for (step, closes_to) in [(1.0_f32, LaneView::CandlesOnly), (-1.0, LaneView::TapeOnly)] {
        let mut style = LiveLaneStyle::default();
        let mut drag = LaneDividerDrag::begin(&style, WIDE);
        let mut wanted = style.resolved_width_px(WIDE);
        while style.view() == LaneView::Beside {
            drag.drag(&mut style, step, WIDE);
            wanted -= step;
            if style.view() == LaneView::Beside {
                let drawn = style.resolved_width_px(WIDE);
                assert!((drawn - wanted).abs() < 0.05, "{drawn} under {wanted}");
            }
        }
        assert_eq!(style.view(), closes_to);
        assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
    }
    // The tape closes at its share's floor, 150 px, not 30 px past it.
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, WIDE);
    drag.drag(&mut style, 1_050.0 - 150.0, WIDE);
    assert_eq!(style.view(), LaneView::Beside, "exactly at the floor holds");
    drag.drag(&mut style, 1.0, WIDE);
    assert_eq!(style.view(), LaneView::CandlesOnly);
}

/// A lane kept outside the open range, narrower than the threshold or wider
/// than its mirror, drags under the hand from where it is instead of closing on
/// the first pixel; it closes only once the hand takes it past the share's own
/// floor or ceiling, giving back the share it started from.
#[test]
fn a_lane_kept_outside_the_open_range_drags_from_where_it_is() {
    for (share, chart, toward_edge, closes_to) in [
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
        // hand; then back to where it started, still open.
        for step in [-toward_edge; 40].into_iter().chain([toward_edge; 40]) {
            drag.drag(&mut style, step, chart);
            wanted -= step;
            assert_eq!(style.view(), LaneView::Beside, "{share} on {chart}");
            let drawn = style.resolved_width_px(chart);
            assert!((drawn - wanted).abs() < 0.05, "{drawn} under {wanted}");
        }
        assert!((style.resolved_width_px(chart) - start).abs() < 0.05);
        // One pixel past the share's own floor or ceiling closes.
        drag.drag(&mut style, toward_edge, chart);
        assert_eq!(style.view(), closes_to, "{share} on {chart}");
        assert_eq!(style.width_share, share);
    }
}
