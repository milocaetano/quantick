//! An optional size reference excludes the first recorded native burst,
//! while every execution remains a factual part of its rendered dot.

use super::*;
use crate::projection::{TapeDotFrame, TapeDotMemory, TapeDotView};

const DAY_MS: i64 = 86_400_000;

mod reference_tests;

#[test]
fn recorded_opening_metadata_saturates_unrepresentable_timestamp_floors() {
    let mut openings = crate::history::RecordedOpenings::default();
    openings.observe(i64::MIN);
    openings.observe(i64::MIN + 200);
    assert_eq!(openings.windows(), &[i64::MIN]);
    openings.observe(i64::MAX);
    assert_eq!(openings.windows(), &[i64::MAX.div_euclid(100) * 100]);
}

fn native(id: u64, time: i64, price: i64, quantity: i64, buy: i64) -> AggressionPrimitive {
    let (_, _, source) = fixture(3_400);
    let mut mark = source[0].clone();
    mark.agg_id = id;
    mark.agg_ids = vec![id];
    mark.quantity = Decimal::from(quantity);
    mark.buy_quantity = Decimal::from(buy);
    mark.buy_share = buy as f32 / quantity as f32;
    mark.side = if buy >= quantity - buy {
        Side::Buy
    } else {
        Side::Sell
    };
    mark.consumed_side = if buy >= quantity - buy {
        BookSide::Ask
    } else {
        BookSide::Bid
    };
    mark.price = Decimal::from(price);
    mark.price_bucket = mark.price;
    mark.price_span = Decimal::ONE;
    mark.first_timestamp_ms = time;
    mark.last_timestamp_ms = time;
    mark.timestamp_quantity = Decimal::from(time) * mark.quantity;
    mark.trade_count = 1;
    mark
}

fn project(
    memory: &mut TapeDotMemory,
    marks: &[AggressionPrimitive],
    openings: &[i64],
    typed_full: Option<Decimal>,
) -> TapeDotFrame {
    let config = tape_config();
    memory.project(
        marks,
        TapeDotView {
            now_ms: 5_000,
            window_ms: 30_000,
            dot_window_ms: 100,
            evicted_through_ms: None,
            prices: prices("90", "190"),
            geometry: TapeDotGeometry {
                left_x: 0.0,
                right_x: 1.0,
                width_px: 640.0,
                height_px: 400.0,
            },
        },
        DotSizing {
            tape_column_px: 1.0,
            candle_column_px: 1.0,
            px_per_price: 1.0,
            typed_full,
        },
        &config.bubbles,
        &config.live_lane,
        openings,
    )
}

fn with_id(frame: &TapeDotFrame, id: u64) -> &AggressionPrimitive {
    frame
        .marks
        .iter()
        .find(|mark| mark.agg_ids.contains(&id))
        .unwrap()
}

fn factual_marks(frame: &TapeDotFrame) -> Vec<AggressionPrimitive> {
    frame
        .marks
        .iter()
        .map(|mark| AggressionPrimitive {
            size: 0.0,
            ..mark.clone()
        })
        .collect()
}

#[test]
fn the_opening_burst_option_defaults_off() {
    assert!(!VolumeDotStyle::default().ignore_opening_burst_in_scale);
}

#[test]
fn excluding_the_opening_native_window_changes_only_the_size_reference() {
    // 74,365 is the real first 100 ms price-187225 native total from the
    // WIN recording. Prices/times here are translated to keep the fixture small.
    let native = [
        native(1, 717, 100, 74_365, 74_365),
        native(2, 751, 110, 35, 0),
        native(3, 1_201, 140, 100, 75),
        native(4, 3_201, 170, 25, 5),
    ];
    let off = project(&mut TapeDotMemory::default(), &native, &[], None);
    let on = project(&mut TapeDotMemory::default(), &native, &[700], None);
    assert_eq!(off.full_quantity, Decimal::from(74_365));
    assert_eq!(on.full_quantity, Decimal::from(100));
    assert_eq!(factual_marks(&on), factual_marks(&off));
    assert_eq!(
        on.marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        Decimal::from(74_525)
    );
    assert_eq!(
        on.marks
            .iter()
            .map(|mark| mark.buy_quantity)
            .sum::<Decimal>(),
        Decimal::from(74_445)
    );
    assert_eq!(
        with_id(&on, 1).size,
        1.0,
        "the opening burst remains visible, capped at full radius"
    );
    assert_eq!(with_id(&on, 3).size, 1.0);
    assert_eq!(
        with_id(&on, 4).size,
        0.5,
        "ordinary dot area still follows quantity"
    );
    assert!(with_id(&on, 4).size > with_id(&off, 4).size * 20.0);
    assert_eq!(on.max_radius, 15.0);
}

#[test]
fn a_mixed_group_excludes_only_its_opening_quantity_from_the_reference() {
    let native = [
        native(1, 717, 100, 74_365, 74_365),
        native(2, 801, 100, 100, 0),
        native(3, 3_201, 170, 25, 5),
    ];
    let on = project(&mut TapeDotMemory::default(), &native, &[700], None);
    let mixed = with_id(&on, 1);
    assert_eq!(mixed.agg_ids, [1, 2]);
    assert_eq!(mixed.quantity, Decimal::from(74_465));
    assert_eq!(mixed.buy_quantity, Decimal::from(74_365));
    assert_eq!(
        mixed.timestamp_quantity,
        Decimal::from(717) * Decimal::from(74_365) + Decimal::from(80_100)
    );
    assert_eq!(
        on.full_quantity,
        Decimal::from(100),
        "later ordinary quantity in the same rendered group still supplies the reference"
    );
    assert_eq!(with_id(&on, 3).size, 0.5);
}

#[test]
fn collision_protection_uses_the_same_opening_adjusted_reference_as_drawing() {
    let native = [
        native(1, 717, 100, 74_365, 74_365),
        native(2, 1_201, 140, 100, 75),
        native(3, 1_501, 145, 25, 5),
    ];
    let on = project(&mut TapeDotMemory::default(), &native, &[700], None);
    assert_eq!(
        on.full_quantity,
        Decimal::from(125),
        "the ordinary reference grows with a frontier merge"
    );
    assert_eq!(with_id(&on, 2).agg_ids, [2, 3]);
    assert!(on.max_radius > 0.0 && on.max_radius <= 15.0);
    for (index, a) in on.marks.iter().enumerate() {
        for b in &on.marks[index + 1..] {
            let ra = f64::from(on.max_radius * a.size);
            let rb = f64::from(on.max_radius * b.size);
            let distance = ((a.x - b.x) * 640.0).hypot((a.y - b.y) * 400.0);
            assert!(distance + 1e-5 >= ra + rb - 0.1 * ra.min(rb));
        }
    }
}

#[test]
fn dense_ordinary_frontier_uses_eligible_volume_before_the_final_collision_cap() {
    let mut marks = vec![native(1, 717, 100, 74_365, 74_365)];
    for index in 0..12 {
        marks.push(native(index + 2, 1_201 + index as i64 * 100, 160, 25, 10));
    }
    marks.push(native(20, 3_201, 175, 75, 30));
    let off = project(&mut TapeDotMemory::default(), &marks, &[], None);
    let on = project(&mut TapeDotMemory::default(), &marks, &[700], None);
    let ordinary_radii = |frame: &TapeDotFrame| {
        let mut radii: Vec<_> = frame
            .marks
            .iter()
            .filter(|mark| !mark.agg_ids.contains(&1))
            .map(|mark| frame.max_radius * mark.size)
            .collect();
        radii.sort_by(f32::total_cmp);
        radii
    };
    let before = ordinary_radii(&off);
    let after = ordinary_radii(&on);
    assert_eq!(on.full_quantity, Decimal::from(300));
    assert_eq!(
        after.len(),
        2,
        "the normal 1.2 s burst is folded while entering the frontier"
    );
    assert!(
        after[0] >= 7.0 && after[1] >= 14.0,
        "the shared cap cannot cancel the readable ordinary scale: {after:?}"
    );
    assert!(after[after.len() / 2] >= before[before.len() / 2] * 10.0);
    assert!(
        (after[0] / after[1] - 0.5).abs() < 1e-6,
        "75:300 normal volume retains 1:4 area"
    );
    assert_eq!(
        on.marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        Decimal::from(74_740)
    );
    assert_eq!(
        on.marks
            .iter()
            .map(|mark| mark.buy_quantity)
            .sum::<Decimal>(),
        Decimal::from(74_515)
    );
    assert_eq!(
        with_id(&on, 2).timestamp_quantity,
        marks[1..13]
            .iter()
            .map(|mark| mark.timestamp_quantity)
            .sum::<Decimal>()
    );
}

#[test]
fn an_offscreen_opening_is_never_replaced_by_the_first_visible_dot() {
    let native = [native(2, 1_201, 140, 100, 75), native(3, 3_201, 170, 25, 5)];
    let expected = project(&mut TapeDotMemory::default(), &native, &[], None);
    let actual = project(&mut TapeDotMemory::default(), &native, &[700], None);
    assert_eq!(actual.marks, expected.marks);
    assert_eq!(actual.full_quantity, Decimal::from(100));
    assert_eq!(actual.max_radius, expected.max_radius);
}

#[test]
fn a_typed_reference_and_a_burst_only_frame_remain_well_defined() {
    let native = [
        native(1, 717, 100, 74_365, 74_365),
        native(2, 3_201, 170, 25, 5),
    ];
    let typed = project(
        &mut TapeDotMemory::default(),
        &native,
        &[700],
        Some(Decimal::from(400)),
    );
    assert_eq!(typed.full_quantity, Decimal::from(400));
    assert_eq!(with_id(&typed, 2).size, 0.25);
    let only_opening = project(&mut TapeDotMemory::default(), &native[..1], &[700], None);
    assert_eq!(
        only_opening.full_quantity,
        Decimal::from(74_365),
        "until an ordinary reference exists, use the factual maximum"
    );
    assert_eq!(only_opening.marks[0].size, 1.0);
}

fn record(history: &mut LiquidityHistory, time: i64) {
    history.record_aggression(&Trade {
        agg_id: 1,
        timestamp_ms: time,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
}

#[test]
fn first_recorded_bursts_survive_retention_grouping_and_reused_source_ids() {
    let mut config = tape_config();
    config.max_aggressions = 2;
    let mut history = LiquidityHistory::new(config.clone());
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    for time in [2_717, 3_001, 4_001, 5_001] {
        record(&mut history, time);
    }
    assert_eq!(
        history.opening_bursts(),
        &[2_700],
        "book start and retained front are not the first recorded execution"
    );
    history.reset_price_grouping(Decimal::from(5)).unwrap();
    record(&mut history, 6_001);
    assert_eq!(
        history.opening_bursts(),
        &[2_700],
        "a capture regroup is not a new source day"
    );
    record(&mut history, 1_217);
    assert_eq!(
        history.opening_bursts(),
        &[1_200],
        "earlier accepted backfill refines the recorded prefix"
    );
    let mut reset = LiquidityHistory::new(config);
    record(&mut reset, 6_001);
    assert_eq!(
        reset.opening_bursts(),
        &[6_000],
        "a new source starts its own recorded prefix"
    );
}

#[test]
fn daily_opening_metadata_is_bounded_and_does_not_follow_the_viewport() {
    let mut history = LiquidityHistory::new(tape_config());
    for day in 0..40 {
        record(&mut history, day * DAY_MS + 10_717);
        record(&mut history, day * DAY_MS + 20_001);
        assert!(history.opening_bursts().len() <= 8);
    }
    assert_eq!(
        history.opening_bursts(),
        &(32..40)
            .map(|day| day * DAY_MS + 10_700)
            .collect::<Vec<_>>()
    );
    record(&mut history, 39 * DAY_MS + 9_001);
    assert_eq!(
        history.opening_bursts().last(),
        Some(&(39 * DAY_MS + 9_000))
    );
}
