//! Stable past navigation keeps the exact groups the trader already saw.

use super::*;
use crate::projection::{TapeDotFrame, TapeDotMemory};
use std::collections::BTreeSet;

#[path = "engine_past_stable_perf_tests.rs"]
mod performance;

fn past_at_window(trades: &[Trade], end_ms: i64, window_ms: i64) -> Arc<PastTape> {
    let mut engine = engine(trades);
    let mut config = engine.history.config().clone();
    config.live_lane.window = crate::LaneWindow::Fixed { ms: window_ms };
    engine.apply_visual_config(config);
    engine.set_tape_end(Some(end_ms));
    engine.project_at(&request(trades), Instant::now()).unwrap();
    engine.published().past_tape.unwrap()
}

fn stable_style() -> (DotSizing, crate::BubbleStyle, crate::LiveLaneStyle) {
    let sizing = DotSizing {
        typed_full: Some(Decimal::from(100)),
        ..sizing()
    };
    let lane = crate::LiveLaneStyle {
        stable_size: true,
        ..Default::default()
    };
    (sizing, crate::BubbleStyle::default(), lane)
}

fn stable_frame(memory: &mut PastTapeMemory, tape: &PastTape, view: TapeDotView) -> TapeDotFrame {
    let (sizing, bubbles, lane) = stable_style();
    memory.project(
        &tape.projection.aggressions,
        tape,
        view,
        sizing,
        &bubbles,
        &lane,
        &[],
    )
}

fn assert_same_facts(before: &AggressionPrimitive, after: &AggressionPrimitive) {
    let mut expected = before.clone();
    expected.x = after.x;
    expected.y = after.y;
    assert_eq!(&expected, after, "only execution coordinates may move");
}

fn assert_shared_groups(before: &TapeDotFrame, after: &TapeDotFrame, view: TapeDotView) {
    let mut checked = 0;
    for old in &before.marks {
        let in_window = old.timestamp_quantity
            >= Decimal::from(view.now_ms - view.window_ms) * old.quantity
            && old.timestamp_quantity <= Decimal::from(view.now_ms) * old.quantity;
        if !in_window {
            continue;
        }
        let next = after
            .marks
            .iter()
            .find(|mark| mark.agg_ids == old.agg_ids)
            .expect("a displayed group keeps every native member");
        assert_same_facts(old, next);
        checked += 1;
    }
    assert!(checked > 0, "the fixture must retain overlapping groups");
    assert_eq!(before.max_radius, after.max_radius);
    assert_eq!(before.full_quantity, after.full_quantity);
    let mut ids = BTreeSet::new();
    for mark in &after.marks {
        for id in &mark.agg_ids {
            assert!(ids.insert(*id), "a native execution has one owner");
        }
    }
}

#[test]
fn stable_past_keeps_groups_through_zoom_resize_uncovered_pan_and_return() {
    let trades = tape();
    let end = 202_000;
    let original = past_at_window(&trades, end, WINDOW_MS);
    let mut memory = PastTapeMemory::default();
    let before = stable_frame(&mut memory, &original, view(end, &original));
    assert!(!before.marks.is_empty());

    for (window, width, height, low, high) in [
        (3_000, 90.0, 120.0, 60, 150),
        (60_000, 1_000.0, 900.0, 80, 120),
        (WINDOW_MS, 300.0, 500.0, 90, 115),
    ] {
        let changed = past_at_window(&trades, end, window);
        let mut transformed = view(end, &changed);
        transformed.geometry.width_px = width;
        transformed.geometry.height_px = height;
        transformed.prices = crate::PriceWindow::new(low.into(), high.into()).unwrap();
        let after = stable_frame(&mut memory, &changed, transformed);
        assert_shared_groups(&before, &after, transformed);
    }

    let distant = past_at_window(&trades, 72_000, 3_000);
    let far = stable_frame(&mut memory, &distant, view(72_000, &distant));
    assert!(!far.marks.is_empty(), "a new short-window block must load");
    let returned = stable_frame(&mut memory, &original, view(end, &original));
    assert_eq!(returned.marks, before.marks);
    assert_eq!(returned.max_radius, before.max_radius);
    assert!(memory.cached_block_count() <= 64);
}

#[test]
fn stable_past_initial_grouping_matches_the_existing_typed_partition() {
    let trades = tape();
    let end = 202_000;
    let tape = past_at_window(&trades, end, WINDOW_MS);
    let (sizing, bubbles, mut lane) = stable_style();
    let stable = stable_frame(&mut PastTapeMemory::default(), &tape, view(end, &tape));
    lane.stable_size = false;
    let legacy = PastTapeMemory::default().project(
        &tape.projection.aggressions,
        &tape,
        view(end, &tape),
        sizing,
        &bubbles,
        &lane,
        &[],
    );
    assert_eq!(stable.marks, legacy.marks);
}

#[test]
fn stable_live_past_live_transfer_keeps_memberships_and_accepts_later_facts_once() {
    let trades = tape();
    let end = 202_000;
    let tape = past_at_window(&trades, end, WINDOW_MS);
    let input: Vec<_> = tape
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.last_timestamp_ms < end - 4_000)
        .cloned()
        .collect();
    let current = view(end, &tape);
    let (sizing, bubbles, lane) = stable_style();
    let mut live = TapeDotMemory::default();
    let before = live.project(&input, current, sizing, &bubbles, &lane, &[]);
    let mut past = PastTapeMemory::default();
    past.hold_live(live);
    let held = stable_frame(&mut past, &tape, current);
    assert_shared_groups(&before, &held, current);
    let mut live = past.resume_live().expect("the same memory returns to live");
    let later_tape = past_at_window(&trades, end + 6_000, WINDOW_MS);
    let later_input: Vec<_> = later_tape
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.last_timestamp_ms <= end + 6_000)
        .cloned()
        .collect();
    let later_view = view(end + 6_000, &later_tape);
    let later = live.project(&later_input, later_view, sizing, &bubbles, &lane, &[]);
    assert_shared_groups(&before, &later, later_view);
    let repeated = live.project(&later_input, later_view, sizing, &bubbles, &lane, &[]);
    assert_eq!(
        later.marks, repeated.marks,
        "publication never adds a trade twice"
    );
    assert!(past.resume_live().is_none(), "ownership transfers once");
}

#[test]
fn stable_navigation_rejects_an_older_cell_and_accepts_a_genuine_late_execution() {
    let trades = vec![
        print(1, 1_001, 100, 2, Side::Buy),
        print(2, 1_017, 100, 3, Side::Sell),
        print(3, 10_000, 110, 1, Side::Buy),
    ];
    let complete = past_at_window(&trades, 5_000, 5_000);
    let older = past_at_window(&[trades[0].clone(), trades[2].clone()], 5_000, 5_000);
    let current = view(5_000, &complete);
    let mut memory = PastTapeMemory::default();
    let before = stable_frame(&mut memory, &complete, current);
    assert_eq!(before.marks.len(), 1);
    assert_eq!(before.marks[0].quantity, Decimal::from(5));
    let stale = stable_frame(&mut memory, &older, current);
    assert_eq!(
        stale.marks, before.marks,
        "worker lag cannot undo accepted prints"
    );
    let mut live = memory.resume_live().unwrap();
    let (sizing, bubbles, lane) = stable_style();
    let stale_live = live.project(
        &older.projection.aggressions,
        current,
        sizing,
        &bubbles,
        &lane,
        &[],
    );
    assert_eq!(stale_live.marks, before.marks);
    let mut later = trades;
    later.push(print(4, 1_033, 100, 7, Side::Buy));
    let published = past_at_window(&later, 5_000, 5_000);
    let updated = live.project(
        &published.projection.aggressions,
        current,
        sizing,
        &bubbles,
        &lane,
        &[],
    );
    assert_eq!(updated.marks.len(), 1);
    assert_eq!(updated.marks[0].agg_ids, [1, 2, 4]);
    assert_eq!(updated.marks[0].quantity, Decimal::from(12));
    assert_eq!(updated.marks[0].buy_quantity, Decimal::from(9));
    assert_eq!(updated.marks[0].trade_count, 3);
}

#[test]
fn stable_cache_keeps_every_visible_bucket_when_a_small_window_expands_a_hundredfold() {
    let trades = tape();
    let end = 303_000;
    let narrow = past_at_window(&trades, end, 3_000);
    let mut narrow_view = view(end, &narrow);
    narrow_view.geometry.width_px = 90_000.0;
    let mut memory = PastTapeMemory::default();
    let before = stable_frame(&mut memory, &narrow, narrow_view);
    let wide = past_at_window(&trades, end, 300_000);
    let mut wide_view = view(end, &wide);
    wide_view.geometry.width_px = 90_000.0;
    let after = stable_frame(&mut memory, &wide, wide_view);
    assert_shared_groups(&before, &after, wide_view);
    let visible: Vec<_> = trades
        .iter()
        .filter(|trade| (end - 300_000..=end).contains(&trade.timestamp_ms))
        .collect();
    assert_eq!(
        after.marks.len(),
        visible.len(),
        "visible facts cannot be evicted by a cache budget"
    );
    assert_eq!(
        after
            .marks
            .iter()
            .map(|mark| mark.quantity)
            .sum::<Decimal>(),
        visible.iter().map(|trade| trade.quantity).sum::<Decimal>()
    );
    assert_eq!(
        after
            .marks
            .iter()
            .map(|mark| mark.buy_quantity)
            .sum::<Decimal>(),
        visible
            .iter()
            .filter(|trade| trade.side == Side::Buy)
            .map(|trade| trade.quantity)
            .sum::<Decimal>()
    );
    assert!(
        memory.cached_block_count() > 64,
        "visible buckets are outside the offscreen budget"
    );
    let repeated = stable_frame(&mut memory, &wide, wide_view);
    assert_eq!(after.marks, repeated.marks);
    let mut returned_live = memory.resume_live().unwrap();
    let (sizing, bubbles, lane) = stable_style();
    returned_live.project(
        &[],
        TapeDotView {
            now_ms: end + 600_000,
            window_ms: 3_000,
            ..wide_view
        },
        sizing,
        &bubbles,
        &lane,
        &[],
    );
    memory.hold_live(returned_live);
    assert!(
        memory.cached_block_count() <= 64,
        "live keeps the offscreen budget"
    );
}

#[test]
fn stable_cache_applies_canonical_eviction_without_resurrecting_a_source_cell() {
    let trades = vec![
        print(1, 1_001, 100, 2, Side::Buy),
        print(2, 1_017, 100, 3, Side::Sell),
        print(5, 3_001, 110, 7, Side::Sell),
        print(3, 10_000, 105, 1, Side::Buy),
    ];
    let tape = past_at_window(&trades, 5_000, 5_000);
    let current = view(5_000, &tape);
    let mut memory = PastTapeMemory::default();
    let before = stable_frame(&mut memory, &tape, current);
    assert_eq!(before.marks.len(), 2);
    let mut evicted = current;
    evicted.evicted_through_ms = Some(1_050);
    let after = stable_frame(&mut memory, &tape, evicted);
    assert_eq!(after.marks.len(), 1);
    assert_eq!(after.marks[0].agg_ids, [5]);
    assert_eq!(after.marks[0].quantity, Decimal::from(7));
    assert_eq!(after.marks[0].buy_quantity, Decimal::ZERO);
    let repeated = stable_frame(&mut memory, &tape, evicted);
    assert_eq!(repeated.marks, after.marks);
}

#[test]
fn stable_far_live_return_keeps_a_group_that_crosses_past_fetch_blocks() {
    let trades = vec![
        print(1, 2_999, 100, 100, Side::Buy),
        print(2, 3_001, 100, 100, Side::Sell),
        print(3, 1_000_000, 110, 1, Side::Buy),
    ];
    let source = past_at_window(&trades, 3_500, 3_000);
    let current = view(3_500, &source);
    let (sizing, bubbles, lane) = stable_style();
    let mut live = TapeDotMemory::default();
    let before = live.project(
        &source.projection.aggressions,
        current,
        sizing,
        &bubbles,
        &lane,
        &[],
    );
    assert_eq!(before.marks.len(), 1);
    assert_eq!(before.marks[0].agg_ids, [1, 2]);
    let fresh = stable_frame(&mut PastTapeMemory::default(), &source, current);
    assert_eq!(fresh.marks.len(), 2, "a cold pass partitions fetch blocks");
    let mut ordinary = live.clone();
    let mut past = PastTapeMemory::default();
    past.hold_live(live);
    assert_eq!(
        stable_frame(&mut past, &source, current).marks,
        before.marks
    );
    let far = TapeDotView {
        now_ms: 1_000_000,
        ..current
    };
    let mut live = past.resume_live().unwrap();
    assert!(
        live.project(&[], far, sizing, &bubbles, &lane, &[])
            .marks
            .is_empty()
    );
    assert_eq!(
        live.retained_group_count(),
        1,
        "distance is not cache eviction"
    );
    // A never-transferred ordinary lane keeps its original temporal expiry.
    let legacy_lane = crate::LiveLaneStyle {
        stable_size: false,
        ..lane.clone()
    };
    ordinary.project(&[], far, sizing, &bubbles, &legacy_lane, &[]);
    assert_eq!(ordinary.retained_group_count(), 0);
    past.hold_live(live);
    assert_eq!(
        stable_frame(&mut past, &source, current).marks,
        before.marks
    );
    let evicted = TapeDotView {
        evicted_through_ms: Some(2_999),
        ..current
    };
    let after = stable_frame(&mut past, &source, evicted);
    assert_eq!(after.marks.len(), 1);
    assert_eq!(after.marks[0].agg_ids, [2]);
    assert_eq!(after.marks[0].quantity, Decimal::from(100));
    assert_eq!(after.marks[0].buy_quantity, Decimal::ZERO);
}
