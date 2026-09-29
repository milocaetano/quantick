//! Historical tape membership survives changes elsewhere on the screen.

use super::*;
use crate::projection::{TapeDotFrame, TapeDotMemory, TapeDotView};
use rust_decimal::prelude::ToPrimitive as _;

const WIN_TIME_MS: i64 = 1_790_078_859_000;
const WINDOW_MS: i64 = 30_000;

fn mark(id: u64, time: i64, price: i64, quantity: i64) -> AggressionPrimitive {
    let (_, _, source) = fixture(3_400);
    let mut mark = source[0].clone();
    mark.agg_id = id;
    mark.agg_ids = vec![id];
    mark.quantity = Decimal::from(quantity);
    mark.buy_quantity = mark.quantity;
    mark.buy_share = 1.0;
    mark.side = Side::Buy;
    mark.consumed_side = BookSide::Ask;
    mark.price = Decimal::from(price);
    mark.price_bucket = mark.price;
    mark.price_span = Decimal::from(5);
    mark.first_timestamp_ms = time;
    mark.last_timestamp_ms = time;
    mark.timestamp_quantity = Decimal::from(time) * mark.quantity;
    mark.trade_count = 1;
    mark
}

/// Actual rows 64198, 64206 and 64264 of WINV26 2026-09-22, with no
/// synthetic volume amplification. The selected rows are not contiguous.
fn real_win_prints() -> Vec<AggressionPrimitive> {
    vec![
        mark(64_198, WIN_TIME_MS + 122, 187_505, 1),
        mark(64_206, WIN_TIME_MS + 336, 187_510, 1),
        mark(64_264, WIN_TIME_MS + 1_076, 187_535, 105),
    ]
}

fn view(now_ms: i64, low: &str, high: &str) -> TapeDotView {
    TapeDotView {
        now_ms,
        window_ms: WINDOW_MS,
        dot_window_ms: 100,
        evicted_through_ms: None,
        prices: prices(low, high),
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 640.0,
            height_px: 400.0,
        },
    }
}

fn draw(
    memory: &mut TapeDotMemory,
    marks: &[AggressionPrimitive],
    view: TapeDotView,
) -> TapeDotFrame {
    let config = tape_config();
    let sizing = DotSizing {
        tape_column_px: 1.0,
        candle_column_px: 1.0,
        px_per_price: 1.0,
        typed_full: None,
    };
    memory.project(marks, view, sizing, &config.bubbles, &config.live_lane)
}

fn source_facts(mark: &AggressionPrimitive) -> (Vec<u64>, Decimal, Decimal, Decimal, Decimal) {
    (
        mark.agg_ids.clone(),
        mark.quantity,
        mark.buy_quantity,
        mark.timestamp_quantity,
        mark.price,
    )
}

fn represented<'a>(frame: &'a TapeDotFrame, ids: &[u64]) -> &'a AggressionPrimitive {
    frame
        .marks
        .iter()
        .find(|mark| mark.agg_ids == ids)
        .expect("the same closed membership remains represented")
}

fn assert_current_coordinates(mark: &AggressionPrimitive, view: &TapeDotView) {
    let mean = mark.timestamp_quantity / mark.quantity;
    let expected_x = ((mean - Decimal::from(view.now_ms - view.window_ms))
        / Decimal::from(view.window_ms))
    .to_f64()
    .unwrap();
    assert!((mark.x - expected_x).abs() < 1e-12);
    let expected_y = view.prices.y_unclamped(mark.price).unwrap();
    assert!((mark.y - expected_y).abs() < 1e-12);
}

fn assert_proportional_clearance(frame: &TapeDotFrame, geometry: TapeDotGeometry) {
    let full = frame.marks.iter().map(|mark| mark.quantity).max().unwrap();
    let radius = |mark: &AggressionPrimitive| {
        f64::from(frame.max_radius) * (mark.quantity / full).to_f64().unwrap().sqrt()
    };
    assert!(frame.max_radius > 0.0 && frame.max_radius <= 15.0);
    for (index, a) in frame.marks.iter().enumerate() {
        for b in &frame.marks[index + 1..] {
            let (ra, rb) = (radius(a), radius(b));
            let distance = ((a.x - b.x) * f64::from(geometry.width_px))
                .hypot((a.y - b.y) * f64::from(geometry.height_px));
            assert!(distance + 1e-5 >= ra + rb - 0.1 * ra.min(rb));
            let area_ratio = (ra / rb).powi(2);
            let quantity_ratio = (a.quantity / b.quantity).to_f64().unwrap();
            assert!((area_ratio - quantity_ratio).abs() < 1e-9);
        }
    }
}

#[test]
fn a_real_new_win_maximum_cannot_split_a_closed_historical_dot() {
    let mut memory = TapeDotMemory::default();
    let prints = real_win_prints();
    let first_view = view(WIN_TIME_MS + 400, "187450", "187550");
    let first = draw(&mut memory, &prints[..2], first_view);
    let old = represented(&first, &[64_198, 64_206]);
    assert_eq!(old.quantity, Decimal::TWO);
    assert_eq!(old.price, dec("187507.5"));
    assert_eq!(old.timestamp_quantity, dec("3580157718458"));
    let original = source_facts(old);

    // Seal it beyond the maximum-diameter frontier before the later receipt.
    let _ = draw(
        &mut memory,
        &prints[..2],
        view(WIN_TIME_MS + 2_000, "187450", "187550"),
    );
    let next_view = view(WIN_TIME_MS + 2_100, "187450", "187550");
    let next = draw(&mut memory, &prints, next_view);
    let old = represented(&next, &[64_198, 64_206]);
    assert_eq!(source_facts(old), original);
    assert_current_coordinates(old, &next_view);
    assert_proportional_clearance(&next, next_view.geometry);
    assert!(
        next.max_radius >= 12.0,
        "this ordinary recorded three-print sample must retain readable largest dots"
    );
}

#[test]
fn a_delayed_publication_cannot_rewrite_already_settled_membership() {
    let mut memory = TapeDotMemory::default();
    let prints = real_win_prints();
    let first = draw(
        &mut memory,
        &prints[..2],
        view(WIN_TIME_MS + 2_000, "187450", "187550"),
    );
    let original = source_facts(represented(&first, &[64_198, 64_206]));
    let next_view = view(WIN_TIME_MS + 2_100, "187450", "187550");
    let next = draw(&mut memory, &prints, next_view);
    let old = represented(&next, &[64_198, 64_206]);
    assert_eq!(source_facts(old), original);
    assert_current_coordinates(old, &next_view);
}

#[test]
fn an_expiring_maximum_does_not_merge_previously_separate_closed_dots() {
    let mut memory = TapeDotMemory::default();
    let mut prints = vec![mark(1, WIN_TIME_MS - 28_000, 187_450, 105)];
    prints.extend(real_win_prints().into_iter().take(2));
    let first = draw(
        &mut memory,
        &prints,
        view(WIN_TIME_MS + 1_800, "187450", "187550"),
    );
    let a = source_facts(represented(&first, &[64_198]));
    let b = source_facts(represented(&first, &[64_206]));
    let next_view = view(WIN_TIME_MS + 2_100, "187450", "187550");
    let next = draw(&mut memory, &prints[1..], next_view);
    assert_eq!(next.marks.len(), 2);
    assert_eq!(source_facts(represented(&next, &[64_198])), a);
    assert_eq!(source_facts(represented(&next, &[64_206])), b);
    assert!(
        next.max_radius < 15.0,
        "one shared cap prevents new occlusion"
    );
    assert_proportional_clearance(&next, next_view.geometry);
}

#[test]
fn the_left_edge_expires_whole_groups_instead_of_rewriting_their_centroids() {
    let mut memory = TapeDotMemory::default();
    let prints = real_win_prints();
    let first = draw(
        &mut memory,
        &prints[..2],
        view(WIN_TIME_MS + 2_000, "187450", "187550"),
    );
    let original = source_facts(represented(&first, &[64_198, 64_206]));
    // The first native cell has left the input, but the aggregate mean at
    // 39.229 remains visible until the mean itself crosses the left edge.
    let edge_view = view(WIN_TIME_MS + WINDOW_MS + 200, "187450", "187550");
    let edge = draw(&mut memory, &prints[1..2], edge_view);
    let old = represented(&edge, &[64_198, 64_206]);
    assert_eq!(source_facts(old), original);
    assert_current_coordinates(old, &edge_view);
    assert_eq!(
        memory.price_range(edge_view.now_ms, edge_view.window_ms),
        Some((187_507.5, 187_507.5)),
        "the retained factual centroid still participates in the tape price fit"
    );
    let gone = draw(
        &mut memory,
        &[],
        view(WIN_TIME_MS + WINDOW_MS + 400, "187450", "187550"),
    );
    assert!(gone.marks.is_empty(), "the complete group leaves together");
}

#[test]
fn an_expired_whole_group_cannot_return_as_its_later_native_constituent() {
    let mut memory = TapeDotMemory::default();
    let prints = real_win_prints();
    let first = draw(
        &mut memory,
        &prints[..2],
        view(WIN_TIME_MS + 2_000, "187450", "187550"),
    );
    assert_eq!(first.marks.len(), 1);
    // The group's exact mean is 39.229; its second native cell starts at
    // 39.300 and is still supplied after the aggregate itself leaves.
    for offset in [250, 260, 270] {
        let frame = draw(
            &mut memory,
            &prints[1..2],
            view(WIN_TIME_MS + WINDOW_MS + offset, "187450", "187550"),
        );
        assert!(
            frame.marks.is_empty(),
            "a consumed native cell cannot reappear"
        );
    }
}

#[test]
fn automatic_price_transforms_preserve_membership_and_use_one_area_scale() {
    let mut memory = TapeDotMemory::default();
    let prints = [
        mark(1, 1_011, 100, 1),
        mark(2, 1_211, 110, 4),
        mark(3, 4_011, 180, 16),
    ];
    let first = draw(&mut memory, &prints, view(5_000, "90", "190"));
    assert_eq!(first.marks.len(), 3);
    let originals: Vec<_> = first.marks.iter().map(source_facts).collect();
    let changed_view = view(5_040, "0", "1000");
    let changed = draw(&mut memory, &prints, changed_view);
    assert_eq!(
        changed.marks.len(),
        3,
        "axis changes are not regrouping requests"
    );
    for original in originals {
        let mark = represented(&changed, &original.0);
        assert_eq!(source_facts(mark), original);
        assert_current_coordinates(mark, &changed_view);
    }
    assert!(changed.max_radius < first.max_radius);
    assert_proportional_clearance(&changed, changed_view.geometry);
}

#[test]
fn native_watermarks_make_the_partition_independent_of_frame_batching() {
    let prints = real_win_prints();
    let initial_view = view(WIN_TIME_MS, "187450", "187550");
    let mut one_batch = TapeDotMemory::default();
    let mut individual = TapeDotMemory::default();
    // The display epoch has the same explicit grouping metric in each run.
    let _ = draw(&mut one_batch, &[], initial_view);
    let _ = draw(&mut individual, &[], initial_view);
    for end in 1..=prints.len() {
        let _ = draw(
            &mut individual,
            &prints[..end],
            view(prints[end - 1].last_timestamp_ms, "187450", "187550"),
        );
        let closed_ms = prints[end - 1].last_timestamp_ms.div_euclid(100) * 100 + 100;
        let _ = draw(
            &mut individual,
            &prints[..end],
            view(closed_ms, "187450", "187550"),
        );
    }
    let final_view = view(WIN_TIME_MS + 2_500, "187450", "187550");
    let incremental = draw(&mut individual, &prints, final_view);
    let mut reversed = prints;
    reversed.reverse();
    let batched = draw(&mut one_batch, &reversed, final_view);
    assert_eq!(batched.marks, incremental.marks);
    assert_eq!(batched.max_radius, incremental.max_radius);
}

#[test]
fn resetting_the_display_epoch_forgets_groups_even_when_source_ids_repeat() {
    let prints = real_win_prints();
    let mut memory = TapeDotMemory::default();
    let _ = draw(
        &mut memory,
        &prints,
        view(WIN_TIME_MS + 2_500, "187450", "187550"),
    );
    memory.clear();
    let restarted = [mark(64_198, 101, 200, 7)];
    let reset_view = view(200, "190", "210");
    let reset = draw(&mut memory, &restarted, reset_view);
    let fresh = draw(&mut TapeDotMemory::default(), &restarted, reset_view);
    assert_eq!(reset.marks, fresh.marks);
    assert_eq!(reset.max_radius, fresh.max_radius);
    assert_eq!(reset.marks.len(), 1);
    assert_eq!(reset.marks[0].quantity, Decimal::from(7));
}

#[test]
fn retained_group_storage_expires_with_the_visible_tape() {
    let mut memory = TapeDotMemory::default();
    for index in 0..100_u64 {
        let time = 10_000 + index as i64 * 2_000;
        let print = mark(index + 1, time, 187_500, 1);
        let shown = draw(
            &mut memory,
            &[print],
            view(time + 1_600, "187450", "187550"),
        );
        assert!(shown.marks.len() <= 16);
        assert!(
            memory.retained_group_count() <= 17,
            "neither retired groups nor their membership cache may accumulate"
        );
    }
    let empty = draw(&mut memory, &[], view(250_000, "187450", "187550"));
    assert!(empty.marks.is_empty());
    assert_eq!(memory.retained_group_count(), 0);
}

#[test]
fn ordinary_lanes_keep_the_existing_projection_and_do_not_read_tape_memory() {
    let mut memory = TapeDotMemory::default();
    let prints = real_win_prints();
    let current = view(WIN_TIME_MS + 2_500, "187450", "187550");
    let _ = draw(&mut memory, &prints, current);
    let mut config = tape_config();
    config.live_lane.tape_only = false;
    let sizing = DotSizing {
        tape_column_px: 1.0,
        candle_column_px: 1.0,
        px_per_price: 1.0,
        typed_full: None,
    };
    let legacy = memory.project(&prints, current, sizing, &config.bubbles, &config.live_lane);
    assert_eq!(legacy.marks, prints);
    assert_eq!(legacy.max_radius, config.bubbles.max_radius);
}

#[test]
fn a_reconnected_trade_id_is_not_a_global_membership_identity() {
    let mut memory = TapeDotMemory::default();
    let first = mark(1, 1_001, 100, 2);
    let _ = draw(
        &mut memory,
        std::slice::from_ref(&first),
        view(3_000, "90", "130"),
    );
    let reconnected = mark(1, 4_001, 120, 3);
    let shown = draw(&mut memory, &[first, reconnected], view(4_100, "90", "130"));
    assert_eq!(shown.marks.len(), 2);
    assert_eq!(
        shown
            .marks
            .iter()
            .map(|mark| mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(5)
    );
    assert!(shown.marks.iter().all(|mark| mark.agg_ids == [1]));
}

#[test]
fn a_late_factual_native_update_changes_its_group_once_without_rewriting_others() {
    let mut memory = TapeDotMemory::default();
    let first = mark(1, 1_001, 100, 2);
    let unrelated = mark(3, 2_001, 180, 7);
    let before = draw(
        &mut memory,
        &[first.clone(), unrelated.clone()],
        view(5_000, "90", "190"),
    );
    let stable = source_facts(represented(&before, &[3]));
    let mut updated = first;
    updated.agg_ids = vec![1, 2];
    updated.quantity = Decimal::from(5);
    updated.buy_quantity = Decimal::TWO;
    updated.buy_share = 0.4;
    updated.last_timestamp_ms = 1_017;
    updated.timestamp_quantity = Decimal::from(5_053);
    updated.trade_count = 2;
    let native = [updated, unrelated];
    let next = draw(&mut memory, &native, view(5_100, "90", "190"));
    let changed = represented(&next, &[1, 2]);
    assert_eq!(changed.quantity, Decimal::from(5));
    assert_eq!(changed.buy_quantity, Decimal::TWO);
    assert_eq!(changed.timestamp_quantity, Decimal::from(5_053));
    assert_eq!(source_facts(represented(&next, &[3])), stable);
    let repeated = draw(&mut memory, &native, view(5_100, "90", "190"));
    assert_eq!(
        repeated.marks, next.marks,
        "publication is replacement, not another trade"
    );
}

#[test]
fn canonical_eviction_removes_only_invalid_native_facts_from_a_retained_group() {
    let mut memory = TapeDotMemory::default();
    let prints = real_win_prints();
    let before = draw(
        &mut memory,
        &prints,
        view(WIN_TIME_MS + 2_500, "187450", "187550"),
    );
    assert_eq!(
        represented(&before, &[64_198, 64_206]).quantity,
        Decimal::TWO
    );
    let stable = source_facts(represented(&before, &[64_264]));
    let mut after_view = view(WIN_TIME_MS + 2_600, "187450", "187550");
    after_view.evicted_through_ms = Some(WIN_TIME_MS + 150);
    // Even a stale source slice cannot resurrect an authoritatively evicted cell.
    let after = draw(&mut memory, &prints, after_view);
    assert_eq!(after.marks.len(), 2);
    assert_eq!(
        source_facts(represented(&after, &[64_206])),
        source_facts(&prints[1])
    );
    assert_eq!(source_facts(represented(&after, &[64_264])), stable);
    assert_eq!(
        after
            .marks
            .iter()
            .map(|mark| mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(106)
    );
}

#[test]
fn an_exactly_coincident_late_execution_joins_facts_without_collapsing_every_radius() {
    let mut memory = TapeDotMemory::default();
    let first = mark(1, 1_001, 100, 1);
    let mut second = mark(2, 1_201, 110, 1);
    second.side = Side::Sell;
    second.consumed_side = BookSide::Bid;
    second.buy_quantity = Decimal::ZERO;
    second.buy_share = 0.0;
    let unrelated = mark(9, 1_001, 200, 1);
    let mut prints = vec![first, second, unrelated];
    let before = draw(&mut memory, &prints, view(4_000, "50", "250"));
    let old = represented(&before, &[1, 2]);
    assert_eq!(old.price, Decimal::from(105));
    assert_eq!(old.timestamp_quantity, Decimal::from(2_202));
    let stable = source_facts(represented(&before, &[9]));

    // A different native key arrives late at the exact factual centroid.
    prints.push(mark(3, 1_101, 105, 3));
    let current_view = view(4_100, "50", "250");
    let after = draw(&mut memory, &prints, current_view);
    assert_eq!(after.marks.len(), 2);
    let combined = represented(&after, &[1, 2, 3]);
    assert_eq!(combined.quantity, Decimal::from(5));
    assert_eq!(combined.buy_quantity, Decimal::from(4));
    assert_eq!(combined.buy_share, 0.8);
    assert_eq!(combined.price, Decimal::from(105));
    assert_eq!(combined.timestamp_quantity, Decimal::from(5_505));
    assert_current_coordinates(combined, &current_view);
    assert_eq!(source_facts(represented(&after, &[9])), stable);
    assert_eq!(
        after.marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        Decimal::from(6)
    );
    assert_proportional_clearance(&after, current_view.geometry);
}
