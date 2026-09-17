//! Additive baseline fixtures for named stage owners; no original assertion
//! or fixture is changed. Private stage tests can reuse these expectations.
use quantick_engine::Bar;
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use quantick_orderflow::{
    BarTimeline, DisplayGrouping, HeatmapConfig, LiquidityHistory, LiveEdge, PriceWindow,
    RestingSide, project_live, project_settled,
};
use rust_decimal::Decimal;

fn level(price: u32, quantity: u32) -> BookLevel {
    BookLevel::new(price.into(), quantity.into()).unwrap()
}

fn book(id: u64, bid: u32, ask: u32) -> BookSnapshot {
    BookSnapshot::new(
        id,
        vec![level(100, bid)],
        vec![level(101, ask)],
        BookCoverage::Full,
    )
}

fn config() -> HeatmapConfig {
    HeatmapConfig {
        enabled: true,
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        display_grouping: DisplayGrouping::Native,
        min_unattributed_reduction: 0.0,
        min_unattributed_pull_share: 0.0,
        ..HeatmapConfig::default()
    }
}

fn timeline(bars: i64, span: i64, now: i64, lane: i64) -> BarTimeline {
    let closed: Vec<_> = (0..bars)
        .map(|i| Bar {
            open_time: i * span,
            close_time: (i + 1) * span - 1,
            open: 100.into(),
            high: 101.into(),
            low: 99.into(),
            close: 100.into(),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        })
        .collect();
    BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(LiveEdge {
            now_ms: now,
            window_ms: lane,
            reference_ms: lane,
            on_newest_bar: true,
        }),
    )
}

fn prices() -> PriceWindow {
    PriceWindow::new(99.into(), 103.into()).unwrap()
}

#[test]
fn geometry_counts_a_run_once_even_across_two_hundred_slots_and_lane() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(0, 1, book(1, 1_000, 1)).unwrap();
    for i in 1..=98_u64 {
        history
            .apply_delta(
                i as i64 * 10,
                &BookDelta::new(i + 1, i + 1, vec![], vec![level(101, 1 + (i % 2) as u32)]),
            )
            .unwrap();
    }
    history
        .apply_delta(19_999, &BookDelta::new(100, 100, vec![], vec![]))
        .unwrap();
    let timeline = timeline(200, 100, 19_999, 1_000);
    let output = project_settled(&history, &timeline, prices());
    // Exactly 100 grouped runs: one 1,000 wall, ninety-nine 1/2 ask
    // runs. The nearest-rank P99 is 2; repeated slot drafts must not bias it.
    assert_eq!(output.liquidity_reference, Decimal::from(2));
    assert!(
        output
            .cells
            .iter()
            .filter(|cell| cell.side == RestingSide::Bid)
            .count()
            >= 200
    );
    let mut hidden = history.config().clone();
    hidden.show_liquidity = false;
    hidden.max_visible_cells = 1;
    history.update_config(hidden).unwrap();
    let hidden = project_settled(&history, &timeline, prices());
    assert_eq!(hidden.liquidity_reference, output.liquidity_reference);
    assert!(hidden.cells.is_empty());
    assert_eq!(hidden.dropped_cells, 0);
}

#[test]
fn geometry_weights_partial_presence_and_keeps_latest_generation() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(0, 1, book(1, 4, 1)).unwrap();
    history.mark_gap(500, "fixture_gap").unwrap();
    history.install_snapshot(750, 2, book(2, 8, 1)).unwrap();
    history
        .apply_delta(1_999, &BookDelta::new(3, 3, vec![], vec![]))
        .unwrap();
    let output = project_settled(&history, &timeline(2, 1_000, 1_999, 1_000), prices());
    let first = output
        .cells
        .iter()
        .find(|cell| cell.side == RestingSide::Bid && cell.x0 == 0.0)
        .unwrap();
    assert_eq!(first.quantity, Decimal::from(4));
    assert_eq!(first.generation, 2);
}

#[test]
fn heat_cap_preserves_stable_side_order_when_wall_keys_tie() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        display_grouping: DisplayGrouping::Multiple(10),
        max_visible_cells: 1,
        ..config()
    });
    history.install_snapshot(0, 1, book(1, 2, 2)).unwrap();
    history
        .apply_delta(999, &BookDelta::new(2, 2, vec![], vec![]))
        .unwrap();
    let output = project_settled(
        &history,
        &timeline(1, 1_000, 999, 1_000),
        PriceWindow::new(90.into(), 120.into()).unwrap(),
    );
    assert_eq!(output.cells.len(), 1);
    assert_eq!(output.cells[0].side, RestingSide::Bid);
    assert_eq!(output.cells[0].price_bucket, Decimal::from(100));
    assert_eq!(output.cells[0].quantity, Decimal::from(2));
}

#[test]
fn seam_partition_preserves_ids_and_removed_quantity_at_adjacent_instants() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(0, 1, book(1, 10, 1)).unwrap();
    for (id, time, quantity) in [(2, 1_999, 8), (3, 2_000, 5), (4, 2_001, 1)] {
        history
            .apply_delta(
                time,
                &BookDelta::new(id, id, vec![level(100, quantity)], vec![]),
            )
            .unwrap();
    }
    history
        .apply_delta(3_900, &BookDelta::new(5, 5, vec![], vec![]))
        .unwrap();
    let timeline = timeline(4, 1_000, 3_900, 1_500);
    let settled = project_settled(&history, &timeline, prices());
    assert_eq!(settled.live_from_ms, Some(2_000));
    assert_eq!(
        settled
            .liquidity_events
            .iter()
            .map(|event| (event.event_id, event.timestamp_ms, event.removed))
            .collect::<Vec<_>>(),
        vec![(1, 1_999, Decimal::from(2))]
    );
    assert_eq!(
        settled
            .live_events
            .iter()
            .map(|event| (event.event_id, event.timestamp_ms, event.removed))
            .collect::<Vec<_>>(),
        vec![(2, 2_000, Decimal::from(3)), (3, 2_001, Decimal::from(4))]
    );
    assert!(
        settled
            .live_events
            .iter()
            .all(|event| event.matched_quantity == Decimal::ZERO)
    );
    let live = project_live(&history, &timeline, prices(), &settled);
    let joined = settled.with_live(live, history.config());
    assert_eq!(
        joined
            .liquidity_events
            .iter()
            .map(|event| event.removed)
            .sum::<Decimal>(),
        Decimal::from(9)
    );
    assert_eq!(
        joined
            .liquidity_events
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn gaps_order_leading_absence_before_open_gap_and_respect_visibility() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(250, 1, book(1, 4, 1)).unwrap();
    history.mark_gap(1_500, "fixture_open_gap").unwrap();
    let timeline = timeline(3, 1_000, 2_999, 1_000);
    let output = project_settled(&history, &timeline, prices());
    assert_eq!(output.gaps.len(), 2);
    assert!(output.gaps[0].precedes_capture());
    assert_eq!(output.gaps[0].from_generation, None);
    assert_eq!(output.gaps[0].to_generation, Some(1));
    assert_eq!(output.gaps[1].reason, "fixture_open_gap");
    assert_eq!(output.gaps[1].to_generation, None);
    assert!(
        output
            .gaps
            .iter()
            .all(|gap| gap.x0 >= 0.0 && gap.x1 <= 1.0 && gap.x0 < gap.x1)
    );
    assert!(output.gaps[0].x1 <= output.gaps[1].x0);
    let mut hidden = history.config().clone();
    hidden.show_gaps = false;
    history.update_config(hidden).unwrap();
    assert!(
        project_settled(&history, &timeline, prices())
            .gaps
            .is_empty()
    );
}
