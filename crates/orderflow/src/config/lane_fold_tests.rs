//! The tape lane folded against either edge of its pane: what it resolves
//! to, where its rail sits, what a divider drag decides and what the file
//! keeps.

use super::*;
use crate::HeatmapConfig;
use crate::config::{MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE};

const CHART: f32 = 1_000.0;

fn lane(fold: LaneFold) -> LiveLaneStyle {
    LiveLaneStyle {
        fold,
        ..LiveLaneStyle::default()
    }
}

/// An open lane is the lane every pane had before folding existed: the same
/// band at every share it could already hold.
#[test]
fn an_open_lane_takes_exactly_the_band_it_always_took() {
    for share in [MIN_LIVE_LANE_SHARE, 0.2, 0.35, 0.5] {
        let open = LiveLaneStyle {
            width_share: share,
            ..LiveLaneStyle::default()
        };
        assert_eq!(open.fold, LaneFold::Open, "open until someone folds it");
        assert!((open.resolved_width_px(CHART) - CHART * share).abs() < 0.01);
        assert!((open.budget_share() - share).abs() < 1e-6);
    }
    assert!(lane(LaneFold::Open).fold_rail(CHART).is_none());
}

/// Folded, the side that gave way keeps a rail to click and the other side
/// takes the rest of the chart.
#[test]
fn a_folded_side_keeps_a_rail_and_the_other_side_takes_the_rest() {
    let candles = lane(LaneFold::Candles);
    assert!((candles.resolved_width_px(CHART) - (CHART - LANE_FOLD_RAIL_PX)).abs() < 0.01);
    let tape = lane(LaneFold::Tape);
    assert!((tape.resolved_width_px(CHART) - LANE_FOLD_RAIL_PX).abs() < 0.01);

    // Never more rail than half a tiny chart, never a lane on no chart.
    assert!((tape.resolved_width_px(10.0) - 5.0).abs() < 0.01);
    assert!((candles.resolved_width_px(10.0) - 5.0).abs() < 0.01);
    assert_eq!(candles.resolved_width_px(0.0), 0.0);
    assert_eq!(tape.resolved_width_px(f32::NAN), 0.0);

    // The tape's own switches still win: off is no band, tape only is the
    // whole chart, whatever the fold says.
    let off = LiveLaneStyle {
        enabled: false,
        ..candles.clone()
    };
    assert_eq!(off.resolved_width_px(CHART), 0.0);
    assert!(off.fold_rail(CHART).is_none());
    let alone = LiveLaneStyle {
        tape_only: true,
        ..tape.clone()
    };
    assert_eq!(alone.resolved_width_px(CHART), CHART);
    assert!(alone.fold_rail(CHART).is_none());
}

/// The rail is painted as wide as a canvas pane's and hit wider, reaching
/// into the side that is still open.
#[test]
fn the_rail_sits_on_the_folded_edge_and_is_hit_wider_than_it_paints() {
    let tape = lane(LaneFold::Tape).fold_rail(CHART).expect("a rail");
    assert_eq!(tape.side, LaneFold::Tape);
    assert!((tape.paint_from_px - (CHART - LANE_FOLD_RAIL_PX)).abs() < 0.01);
    assert!((tape.paint_to_px - CHART).abs() < 0.01);
    assert!((tape.hit_from_px - (CHART - LANE_FOLD_HIT_PX)).abs() < 0.01);
    assert!((tape.hit_to_px - CHART).abs() < 0.01);

    let candles = lane(LaneFold::Candles).fold_rail(CHART).expect("a rail");
    assert_eq!(candles.side, LaneFold::Candles);
    assert_eq!(candles.paint_from_px, 0.0);
    assert!((candles.paint_to_px - LANE_FOLD_RAIL_PX).abs() < 0.01);
    assert_eq!(candles.hit_from_px, 0.0);
    assert!((candles.hit_to_px - LANE_FOLD_HIT_PX).abs() < 0.01);
    const { assert!(LANE_FOLD_HIT_PX > LANE_FOLD_RAIL_PX) };
}

/// Folding never touches the share, so unfolding returns the band the lane
/// had before it was folded.
#[test]
fn unfolding_returns_the_width_the_lane_had_before_the_fold() {
    let mut style = LiveLaneStyle {
        width_share: 0.42,
        ..LiveLaneStyle::default()
    };
    let before = style.resolved_width_px(CHART);
    assert!(style.set_fold(LaneFold::Candles));
    assert!(
        !style.set_fold(LaneFold::Candles),
        "the same fold twice is no change"
    );
    assert_eq!(style.width_share, 0.42);
    assert!(style.set_fold(LaneFold::Open));
    assert!((style.resolved_width_px(CHART) - before).abs() < 0.01);
}

/// Dropping the divider near the left edge folds the candles; the share
/// the lane is given back is the one the drag started from.
#[test]
fn dragging_the_divider_to_the_left_edge_folds_the_candles() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    // Left is a wider tape: 350 + 560 = 910 px, 90 px of candles left.
    drag.drag(&mut style, -560.0, CHART);
    assert_eq!(style.fold, LaneFold::Candles);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
    assert!((style.resolved_width_px(CHART) - (CHART - LANE_FOLD_RAIL_PX)).abs() < 0.01);
}

/// Near the right edge the tape folds instead, by the same rule.
#[test]
fn dragging_the_divider_to_the_right_edge_folds_the_tape() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    // 350 - 250 = 100 px of tape: under the threshold.
    drag.drag(&mut style, 250.0, CHART);
    assert_eq!(style.fold, LaneFold::Tape);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
    assert!((style.resolved_width_px(CHART) - LANE_FOLD_RAIL_PX).abs() < 0.01);
}

/// Short of the threshold the divider goes where the hand puts it, past the
/// half the lane used to stop at; dragging back out of a fold in the same
/// gesture opens the lane again under the hand.
#[test]
fn the_divider_travels_the_whole_chart_short_of_the_fold() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    drag.drag(&mut style, -400.0, CHART);
    assert_eq!(style.fold, LaneFold::Open);
    assert!((style.width_share - 0.75).abs() < 1e-6);
    let mut kept = style.clone();
    kept.sanitize();
    assert_eq!(kept, style, "a share past half is a share the lane keeps");

    // On past the threshold, then back: open again where the hand is.
    drag.drag(&mut style, -200.0, CHART);
    assert_eq!(style.fold, LaneFold::Candles);
    assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
    drag.drag(&mut style, 300.0, CHART);
    assert_eq!(style.fold, LaneFold::Open);
    assert!((style.width_share - 0.65).abs() < 1e-6);

    // The threshold is the canvas pane's, measured from the edge.
    let mut edge = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&edge, CHART);
    drag.drag(&mut edge, 350.0 - LANE_FOLD_AT_PX, CHART);
    assert_eq!(edge.fold, LaneFold::Open, "exactly at the threshold holds");
    drag.drag(&mut edge, 1.0, CHART);
    assert_eq!(edge.fold, LaneFold::Tape);
}

/// A drag that starts on a rail opens the lane where it is let go, and
/// folding it again in the same drag gives back the share it was folded on.
#[test]
fn dragging_out_of_a_rail_opens_the_lane_where_it_is_let_go() {
    let mut style = LiveLaneStyle {
        width_share: 0.3,
        fold: LaneFold::Tape,
        ..LiveLaneStyle::default()
    };
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    drag.drag(&mut style, -50.0, CHART);
    assert_eq!(
        style.fold,
        LaneFold::Tape,
        "a twitch on the rail stays folded"
    );
    drag.drag(&mut style, -250.0, CHART);
    assert_eq!(style.fold, LaneFold::Open);
    let opened = LANE_FOLD_RAIL_PX + 300.0;
    assert!((style.resolved_width_px(CHART) - opened).abs() < 0.01);
    drag.drag(&mut style, 300.0, CHART);
    assert_eq!(style.fold, LaneFold::Tape);
    assert_eq!(style.width_share, 0.3);
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
    assert_eq!(style.fold, LaneFold::Open);
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

/// Folding moves where the marks are spent: the open side takes the
/// budget, the rail keeps the least.
#[test]
fn the_mark_budget_follows_the_fold() {
    assert_eq!(lane(LaneFold::Candles).budget_share(), 1.0);
    assert_eq!(lane(LaneFold::Tape).budget_share(), 0.0);
}

/// A fold is where the lane sits, like its width: the painted tape is
/// kept, never regrouped.
#[test]
fn folding_never_regroups_the_tape() {
    let open = HeatmapConfig::default();
    let mut folded = open.clone();
    folded.live_lane.fold = LaneFold::Candles;
    assert!(open.same_tape_grouping(&folded));
    assert!(folded.same_tape_grouping(&open));
    assert!(
        !open.identical(&folded),
        "a fold is still a different setting"
    );
}

/// A file from before folding opens open, an open lane writes the file it
/// always wrote, and a fold reads back as it was saved.
#[test]
fn a_fold_round_trips_through_its_file() {
    let text = toml::to_string(&LiveLaneStyle::default()).unwrap();
    assert!(!text.contains("fold"), "{text}");
    let old: LiveLaneStyle = toml::from_str("width_share = 0.35\n").unwrap();
    assert_eq!(old.fold, LaneFold::Open);
    for fold in [LaneFold::Candles, LaneFold::Tape] {
        let style = lane(fold);
        let text = toml::to_string(&style).unwrap();
        assert!(text.contains("fold = "), "{text}");
        assert_eq!(toml::from_str::<LiveLaneStyle>(&text).unwrap(), style);
    }
    assert!(toml::from_str::<LiveLaneStyle>("fold = \"sideways\"\n").is_err());
}

/// The divider takes the pointer on the hairline's band while open and on
/// the whole rail while folded; without a lane beside the candles there is
/// nothing to take.
#[test]
fn the_divider_is_taken_on_its_band_or_on_its_rail() {
    let open = LiveLaneStyle::default();
    assert_eq!(open.divider_hit(CHART, 5.0), Some((645.0, 655.0)));
    let tape = lane(LaneFold::Tape);
    let rail = tape.fold_rail(CHART).expect("a rail");
    let hit = Some((rail.hit_from_px, rail.hit_to_px));
    assert_eq!(tape.divider_hit(CHART, 5.0), hit);
    assert!((rail.edge_px() - (CHART - LANE_FOLD_RAIL_PX)).abs() < 0.01);
    let candles = lane(LaneFold::Candles).fold_rail(CHART).expect("a rail");
    assert!((candles.edge_px() - LANE_FOLD_RAIL_PX).abs() < 0.01);
    let alone = LiveLaneStyle {
        tape_only: true,
        ..open.clone()
    };
    assert_eq!(alone.divider_hit(CHART, 5.0), None);
    let off = LiveLaneStyle {
        enabled: false,
        ..open
    };
    assert_eq!(off.divider_hit(CHART, 5.0), None);
}

/// A drag says when it folded or unfolded, so the caller can keep the fold
/// as a setting while the width stays navigation.
#[test]
fn a_drag_says_when_the_fold_changed() {
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, CHART);
    assert!(
        !drag.drag(&mut style, -10.0, CHART),
        "a wider tape is no fold"
    );
    assert!(drag.drag(&mut style, 300.0, CHART), "folded the tape");
    assert!(!drag.drag(&mut style, 10.0, CHART), "still folded");
    assert!(drag.drag(&mut style, -300.0, CHART), "open again");
    assert!(!drag.drag(&mut style, f32::NAN, CHART));
}

/// A pane keeps its drag in one slot across frames: the first step begins a
/// drag on the lane as it stands and later steps carry it on, while a
/// tape-only pane has no divider and its slot never fills.
#[test]
fn a_pane_carries_its_divider_drag_in_one_slot() {
    let mut config = HeatmapConfig::default();
    let mut hand = None;
    assert_eq!(
        config.drag_lane_divider(&mut hand, -10.0, CHART),
        Some(false)
    );
    assert!(hand.is_some(), "the first step began the drag");
    assert_eq!(
        config.drag_lane_divider(&mut hand, 10_000.0, CHART),
        Some(true)
    );
    assert_eq!(config.live_lane.fold, LaneFold::Tape);

    let mut alone = HeatmapConfig::default();
    alone.live_lane.enabled = true;
    alone.live_lane.tape_only = true;
    assert!(alone.tape_only());
    let (before, mut hand) = (alone.clone(), None);
    assert_eq!(alone.drag_lane_divider(&mut hand, -300.0, CHART), None);
    assert_eq!((alone, hand), (before, None), "nothing moved");
}

/// Tape only and the tape's own switch win over a leftover fold in the
/// budget too: such a pane spends the marks it spent before folding existed.
#[test]
fn a_leftover_fold_never_moves_a_tape_only_panes_budget() {
    for fold in [LaneFold::Open, LaneFold::Candles, LaneFold::Tape] {
        let alone = LiveLaneStyle {
            tape_only: true,
            width_share: 0.3,
            ..lane(fold)
        };
        assert!((alone.budget_share() - 0.3).abs() < 1e-6, "{fold:?}");
        let off = LiveLaneStyle {
            enabled: false,
            ..alone.clone()
        };
        assert!((off.budget_share() - 0.3).abs() < 1e-6, "{fold:?}");
    }
}

/// One rule says whether a fold shows: the tape on and beside the candles.
/// The rail, the divider and the control plane all read it.
#[test]
fn a_fold_shows_only_while_the_tape_sits_beside_the_candles() {
    let tape = lane(LaneFold::Tape);
    assert!(tape.beside_candles());
    assert_eq!(tape.shown_fold(), LaneFold::Tape);
    let off = LiveLaneStyle {
        enabled: false,
        ..tape.clone()
    };
    let alone = LiveLaneStyle {
        tape_only: true,
        ..tape
    };
    for hidden in [off, alone] {
        assert!(!hidden.beside_candles(), "{hidden:?}");
        assert_eq!(hidden.shown_fold(), LaneFold::Open);
        assert!(hidden.fold_rail(CHART).is_none());
        assert_eq!(hidden.divider_hit(CHART, 5.0), None);
    }
}

/// The Tape chip brings back a tape, never an 8 px rail: switching the tape
/// on opens a fold it was switched off in, at the share it kept.
#[test]
fn switching_the_tape_on_brings_back_a_tape_not_a_rail() {
    for fold in [LaneFold::Candles, LaneFold::Tape] {
        let mut style = LiveLaneStyle {
            width_share: 0.42,
            ..lane(fold)
        };
        assert!(style.set_enabled(false));
        assert!(!style.set_enabled(false), "already off");
        assert!(style.set_enabled(true));
        assert_eq!(style.fold, LaneFold::Open, "{fold:?}");
        assert_eq!(style.width_share, 0.42);
    }
    let mut on = lane(LaneFold::Tape);
    assert!(!on.set_enabled(true), "already on: nothing to switch");
    assert_eq!(on.fold, LaneFold::Tape, "a tape already on keeps its fold");
}

/// On a wide chart the share's floor and ceiling sit further from the edge
/// than the fold threshold; the divider stays under the hand all the way to
/// where it folds rather than stopping at the floor and jumping.
#[test]
fn on_a_wide_chart_the_divider_stays_under_the_hand_until_it_folds() {
    const WIDE: f32 = 3_000.0;
    for (step, folds_to) in [(1.0_f32, LaneFold::Tape), (-1.0, LaneFold::Candles)] {
        let mut style = LiveLaneStyle::default();
        let mut drag = LaneDividerDrag::begin(&style, WIDE);
        let mut wanted = style.resolved_width_px(WIDE);
        while style.fold.is_open() {
            drag.drag(&mut style, step, WIDE);
            wanted -= step;
            if style.fold.is_open() {
                let drawn = style.resolved_width_px(WIDE);
                assert!((drawn - wanted).abs() < 0.05, "{drawn} under {wanted}");
            }
        }
        assert_eq!(style.fold, folds_to);
        assert_eq!(style.width_share, DEFAULT_LIVE_LANE_SHARE);
    }
    // The tape folds at its share's floor, 150 px, not 30 px past it.
    let mut style = LiveLaneStyle::default();
    let mut drag = LaneDividerDrag::begin(&style, WIDE);
    drag.drag(&mut style, 1_050.0 - 150.0, WIDE);
    assert_eq!(style.fold, LaneFold::Open, "exactly at the floor holds");
    drag.drag(&mut style, 1.0, WIDE);
    assert_eq!(style.fold, LaneFold::Tape);
}

/// A lane kept outside the open range, narrower than the fold threshold or
/// wider than its mirror, drags under the hand from where it is instead of
/// folding on the first pixel; it folds only once the hand takes it past the
/// share's own floor or ceiling, giving back the share it started from.
#[test]
fn a_lane_kept_outside_the_open_range_drags_from_where_it_is() {
    for (share, chart, toward_edge, folds_to) in [
        // 5% of 1000 px: 50 px, inside the 120 px threshold.
        (MIN_LIVE_LANE_SHARE, CHART, 1.0_f32, LaneFold::Tape),
        // 95% of 1000 px: 950 px, past the 880 px mirror.
        (MAX_LIVE_LANE_SHARE, CHART, -1.0, LaneFold::Candles),
        // 150 px kept on a 3000 px monitor, shown on a 1500 px window.
        (MIN_LIVE_LANE_SHARE, 1_500.0, 1.0, LaneFold::Tape),
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
            assert_eq!(style.fold, LaneFold::Open, "{share} on {chart}");
            let drawn = style.resolved_width_px(chart);
            assert!((drawn - wanted).abs() < 0.05, "{drawn} under {wanted}");
        }
        assert!((style.resolved_width_px(chart) - start).abs() < 0.05);
        // One pixel past the share's own floor or ceiling folds.
        drag.drag(&mut style, toward_edge, chart);
        assert_eq!(style.fold, folds_to, "{share} on {chart}");
        assert_eq!(style.width_share, share);
    }
}
