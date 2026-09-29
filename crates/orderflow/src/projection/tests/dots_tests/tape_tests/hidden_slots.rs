//! A tape-only pane never constructs its hidden candle-slot counterpart.

use super::*;
use crate::projection::dots::DotHorizon;
use crate::projection::tiers::{TierCut, TierGrouping, cluster_tier, refine_tier, tier_primitives};
use crate::projection::{PendingTape, project_settled};

fn live_marks(frame: &HeatmapProjection) -> Vec<AggressionPrimitive> {
    frame
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .cloned()
        .collect()
}

/// Preserve the former two-view collection as the reference, then apply the
/// factual tape fold/placement. This path deliberately retains slot work.
fn dual_collection_reference(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    dots: &VolumeDots,
) -> Vec<AggressionPrimitive> {
    let config = history.config();
    let mixed = VolumeDots {
        native_tape: false,
        ..dots.clone()
    };
    let grouping = crate::projection::dots::native_grouping(config);
    let collected = cluster_tier(
        history,
        timeline,
        prices("90", "110"),
        &[],
        TierGrouping {
            slots: grouping,
            lane: grouping,
        },
        TierCut {
            range: (timeline.live_boundary_ms(), None),
            tape_from_ms: timeline.lane_start_ms(),
            reach_ms: None,
            dots: Some(&mixed),
        },
        false,
    );
    assert!(
        !collected.slot.is_empty(),
        "the reference includes the old hidden work"
    );
    let (refined, _) = refine_tier(
        collected,
        config,
        dec("10"),
        timeline,
        grouping,
        false,
        Some((dots, DotHorizon::of(history))),
    );
    let mut marks: Vec<_> = tier_primitives(
        refined,
        timeline,
        prices("90", "110"),
        dec("10"),
        dec("10"),
        Some(dots),
    )
    .into_iter()
    .filter(|mark| mark.live)
    .collect();
    marks.sort_by_key(|mark| mark.quantity);
    marks
}

#[test]
fn tape_only_omits_hidden_slots_without_changing_any_live_primitive() {
    let history = recorded(
        tape_config(),
        &[
            (1, 501, "98", "7", Side::Sell),
            (2, 2_501, "100", "0.1", Side::Buy),
            (3, 2_509, "100", "0.3", Side::Sell),
            (4, 3_130, "105", "2", Side::Buy),
        ],
    );
    let timeline = chart(3_400, 1_000, None);
    let dots = tape_dots(100, 1);
    let expected = dual_collection_reference(&history, &timeline, &dots);
    let settled = project_settled(&history, &timeline, prices("90", "110"), Some(&dots));
    let actual = frame_at(&history, &timeline, prices("90", "110"), &dots);
    assert_eq!(
        live_marks(&actual),
        expected,
        "coordinates, Decimal facts and sizes stay exact"
    );
    assert_eq!(
        expected.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        dec("2.4")
    );
    assert!(
        settled.aggressions.is_empty(),
        "no settled candle slots in a tape-only pane"
    );
    assert!(
        actual.aggressions.iter().all(|mark| mark.live),
        "no hidden live-bar slots either"
    );
    let facts = actual
        .tape_facts
        .as_ref()
        .expect("native prefix survives slot omission");
    assert_eq!(
        facts
            .clusters
            .iter()
            .map(|mark| mark.quantity)
            .sum::<Decimal>(),
        dec("2.4")
    );
    assert_eq!(facts.opening_bursts, history.opening_bursts());
}

#[test]
fn ordinary_dot_slots_survive_a_tape_mode_roundtrip_with_identical_output() {
    let ordinary = dots_config();
    let mut history = recorded(
        ordinary.clone(),
        &[
            (1, 501, "98", "7", Side::Sell),
            (2, 2_501, "100", "0.1", Side::Buy),
            (3, 2_509, "100", "0.3", Side::Sell),
            (4, 3_130, "105", "2", Side::Buy),
        ],
    );
    let timeline = chart(3_400, 1_000, None);
    let before = frame_at(&history, &timeline, prices("90", "110"), &coarse(100, 1));
    assert!(
        before
            .aggressions
            .iter()
            .any(|mark| !mark.live && mark.agg_ids.contains(&1))
    );
    assert!(
        before
            .aggressions
            .iter()
            .any(|mark| !mark.live && mark.agg_ids.contains(&4))
    );
    history.update_config(tape_config()).unwrap();
    let taped = frame_at(&history, &timeline, prices("90", "110"), &tape_dots(100, 1));
    assert!(taped.aggressions.iter().all(|mark| mark.live));
    assert_eq!(
        live_marks(&taped),
        dual_collection_reference(&history, &timeline, &tape_dots(100, 1))
    );
    history.update_config(ordinary).unwrap();
    let restored = frame_at(&history, &timeline, prices("90", "110"), &coarse(100, 1));
    assert_eq!(
        restored, before,
        "ordinary pane behavior and retained source facts are unchanged"
    );
}

#[test]
fn slot_omission_keeps_subfloor_pending_prefix_and_same_frame_handoff_exact() {
    let mut config = tape_config();
    config.bubbles.min_quantity = 10.0;
    let mut history = recorded(
        config.clone(),
        &[
            (1, 501, "98", "7", Side::Sell),
            (2, 3_011, "100", "6", Side::Buy),
        ],
    );
    let timeline = chart(3_060, 1_500, None);
    let dots = tape_dots(100, 1);
    let prefix = frame_at(&history, &timeline, prices("90", "110"), &dots);
    assert!(live_marks(&prefix).is_empty());
    assert_eq!(
        prefix.tape_facts.as_ref().unwrap().clusters[0].quantity,
        dec("6")
    );
    let suffix = Trade {
        agg_id: 3,
        timestamp_ms: 3_052,
        price: dec("100"),
        quantity: dec("6"),
        side: Side::Sell,
    };
    let mut pending = PendingTape::default();
    let ordinal = pending.record(&suffix, &config);
    let immediate = pending.project(
        Some(&prefix),
        &config,
        &timeline,
        prices("90", "110"),
        &dots,
    );
    history.record_aggression(&suffix);
    let published = frame_at(&history, &timeline, prices("90", "110"), &dots);
    assert_eq!(immediate.aggressions, live_marks(&published));
    assert_eq!(immediate.tape_facts, published.tape_facts);
    let dot = &immediate.aggressions[0];
    assert_eq!(
        (dot.quantity, dot.buy_quantity, dot.timestamp_quantity),
        (dec("12"), dec("6"), dec("36378"))
    );
    assert_eq!(
        dot.x, 1.0,
        "the forming suffix is visible on the accepting frame"
    );
    pending.acknowledge(ordinal);
    let adopted = pending.project(
        Some(&published),
        &config,
        &timeline,
        prices("90", "110"),
        &dots,
    );
    assert_eq!(
        adopted.aggressions, immediate.aggressions,
        "publication cannot double or hide the suffix"
    );
    assert!(adopted.aggressions.iter().all(|mark| mark.live));
    assert_eq!(adopted.floored_quantity, Decimal::ZERO);
}
