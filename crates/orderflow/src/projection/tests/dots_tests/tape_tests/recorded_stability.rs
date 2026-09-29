//! Closed tape groups in an unchanged, contiguous real WIN replay slice.

use super::*;
use crate::projection::{TapeDotFrame, TapeDotMemory, TapeDotView};
use rust_decimal::prelude::ToPrimitive as _;
use std::collections::BTreeSet;

const BEFORE_MS: i64 = 1_790_078_860_000;
const AFTER_MS: i64 = 1_790_078_860_083;
const WINDOW_MS: i64 = 30_000;
const FIRST_SOURCE_ID: u64 = 62_392;

fn recording() -> Vec<Trade> {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/win_2026_09_22_historical_tape.csv"
    ))
    .lines()
    .filter(|line| line.starts_with("2026-09-22,"))
    .enumerate()
    .map(|(index, line)| {
        let fields: Vec<_> = line.split(',').collect();
        let time: Vec<i64> = fields[1]
            .split([':', '.'])
            .map(|part| part.parse().expect("recorded civil-time component"))
            .collect();
        Trade {
            agg_id: FIRST_SOURCE_ID + index as u64,
            timestamp_ms: 1_790_046_000_000
                + ((time[0] * 60 + time[1]) * 60 + time[2]) * 1_000
                + time[3],
            price: dec(fields[2]),
            quantity: dec(fields[5]),
            side: match fields[6] {
                "B" => Side::Buy,
                "S" => Side::Sell,
                _ => panic!("recorded venue side"),
            },
        }
    })
    .collect()
}

fn view(now_ms: i64) -> TapeDotView {
    TapeDotView {
        now_ms,
        window_ms: WINDOW_MS,
        dot_window_ms: 100,
        evicted_through_ms: None,
        prices: prices("187400", "187550"),
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 640.0,
            height_px: 400.0,
        },
    }
}

fn native_prefix(prints: &[Trade], now_ms: i64) -> Vec<AggressionPrimitive> {
    let mut history = LiquidityHistory::new(tape_config());
    let mut partial = Bar::opened_by(&prints[0]);
    for (index, trade) in prints.iter().enumerate() {
        history.record_aggression(trade);
        if index > 0 {
            partial.extend(trade);
        }
    }
    let timeline = BarTimeline::from_bars(
        0,
        &[],
        Some(&partial),
        Some(crate::LiveEdge {
            now_ms,
            window_ms: WINDOW_MS,
            reference_ms: WINDOW_MS,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage();
    frame_at(&history, &timeline, view(now_ms).prices, &tape_dots(100, 1))
        .aggressions
        .into_iter()
        .filter(|mark| mark.live)
        .collect()
}

fn draw(memory: &mut TapeDotMemory, native: &[AggressionPrimitive], now_ms: i64) -> TapeDotFrame {
    let config = tape_config();
    memory.project(
        native,
        view(now_ms),
        DotSizing {
            tape_column_px: 1.0,
            candle_column_px: 1.0,
            px_per_price: 1.0,
            typed_full: None,
        },
        &config.bubbles,
        &config.live_lane,
    )
}

fn identities(marks: &[AggressionPrimitive]) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for mark in marks {
        for id in &mark.agg_ids {
            assert!(ids.insert(*id), "a source execution cannot appear twice");
        }
    }
    ids
}

fn assert_exact_source_facts(marks: &[AggressionPrimitive], prints: &[Trade]) {
    for mark in marks {
        let members: Vec<_> = mark.agg_ids.iter()
            .map(|id| &prints[(*id - FIRST_SOURCE_ID) as usize]).collect();
        let quantity: Decimal = members.iter().map(|trade| trade.quantity).sum();
        let buys: Decimal = members.iter().filter(|trade| trade.side == Side::Buy)
            .map(|trade| trade.quantity).sum();
        let time: Decimal = members.iter()
            .map(|trade| Decimal::from(trade.timestamp_ms) * trade.quantity).sum();
        let price: Decimal = members.iter().map(|trade| trade.price * trade.quantity).sum();
        assert_eq!(mark.quantity, quantity);
        assert_eq!(mark.buy_quantity, buys);
        assert_eq!(mark.timestamp_quantity, time);
        assert_eq!(mark.price, price / quantity);
    }
}

#[test]
fn real_win_prefixes_do_not_regroup_ten_second_old_history_when_the_left_maximum_changes() {
    let prints = recording();
    assert_eq!(prints.len(), 1_879);
    let prefix_len = prints.partition_point(|trade| trade.timestamp_ms <= BEFORE_MS);
    assert_eq!(prefix_len, 1_844);
    let before_native = native_prefix(&prints[..prefix_len], BEFORE_MS);
    let after_native = native_prefix(&prints, AFTER_MS);
    let mut memory = TapeDotMemory::default();
    let before = draw(&mut memory, &before_native, BEFORE_MS);
    let after = draw(&mut memory, &after_native, AFTER_MS);
    assert_eq!(identities(&before.marks), identities(&before_native));

    let left = Decimal::from(AFTER_MS - WINDOW_MS);
    let mut expected_ids = identities(&after_native);
    for old in &before.marks {
        if old.timestamp_quantity / old.quantity >= left {
            expected_ids.extend(old.agg_ids.iter().copied());
        }
    }
    for expired in before.marks.iter()
        .filter(|old| old.timestamp_quantity / old.quantity < left)
    {
        for id in &expired.agg_ids {
            expected_ids.remove(id);
        }
    }
    assert_eq!(identities(&after.marks), expected_ids,
        "retain or expire whole left-edge groups without losing or duplicating executions");
    assert_exact_source_facts(&before.marks, &prints);
    assert_exact_source_facts(&after.marks, &prints);

    let closed: Vec<_> = before.marks.iter().filter(|mark| {
        mark.last_timestamp_ms < BEFORE_MS - 2_000
            && mark.timestamp_quantity / mark.quantity >= left
    }).collect();
    assert!(closed.iter().any(|mark| mark.agg_ids.contains(&63_755)));
    assert!(closed.iter().any(|mark| mark.agg_ids.contains(&63_761)));
    for old in closed {
        let current = after.marks.iter().find(|mark| mark.agg_ids == old.agg_ids)
            .expect("closed source membership survives unrelated arrivals and expiration");
        assert_eq!((current.quantity, current.buy_quantity, current.timestamp_quantity, current.price),
            (old.quantity, old.buy_quantity, old.timestamp_quantity, old.price));
        let expected_x = ((current.timestamp_quantity / current.quantity - left)
            / Decimal::from(WINDOW_MS)).to_f64().unwrap();
        assert!((current.x - expected_x).abs() < 1e-12, "only ordinary time scrolling is allowed");
        assert_eq!(current.y, view(AFTER_MS).prices.y_unclamped(current.price).unwrap());
    }
}
