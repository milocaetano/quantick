use super::*;
use crate::{DisplayGrouping, LiveEdge};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(price.into(), quantity.into()).unwrap()
}

fn fixture() -> LiquidityHistory {
    let mut history = LiquidityHistory::new(crate::HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        max_visible_cells: 3,
        ..crate::HeatmapConfig::default()
    });
    history
        .install_snapshot(
            1_000,
            1,
            BookSnapshot::new(
                10,
                vec![level(95, 100), level(99, 1)],
                vec![level(101, 1)],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    for (id, time, quantity) in [(11, 1_100, 110), (12, 1_200, 120), (13, 1_900, 130)] {
        history
            .apply_delta(
                time,
                &BookDelta::new(id, id, vec![level(95, quantity)], vec![]),
            )
            .unwrap();
    }
    history
}

fn projection(history: &LiquidityHistory) -> (Vec<HeatmapCell>, usize) {
    projection_at(history, 2_000)
}

fn projection_at(history: &LiquidityHistory, now_ms: i64) -> (Vec<HeatmapCell>, usize) {
    let timeline = BarTimeline::from_bars(
        0,
        &[bar()],
        None,
        Some(LiveEdge {
            now_ms,
            window_ms: 1_000,
            reference_ms: 50,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage();
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, 8.into());
    project_lane_heat(
        history,
        &timeline,
        PriceWindow::new(94.into(), 102.into()).unwrap(),
        grouping,
        130.into(),
    )
}

#[test]
fn strongest_wall_cap_omits_confirmed_near_spread_depth_without_a_feed_gap() {
    let history = fixture();
    assert!(history.is_synchronized());
    assert_eq!(history.latest_book_ms(), Some(1_900));
    assert_eq!(history.coverage_gaps().count(), 0);
    let (cells, dropped) = projection(&history);
    assert!(dropped > 0);
    assert!(
        cells
            .iter()
            .all(|cell| cell.price_bucket == Decimal::from(95))
    );
    assert!(
        history
            .runs_intersecting(1_500, 1_900)
            .any(|run| run.price_bucket == Decimal::from(99) && run.quantity == Decimal::ONE)
    );
}

fn bar() -> quantick_engine::Bar {
    quantick_engine::Bar {
        open_time: 1_000,
        close_time: 2_000,
        open: 100.into(),
        high: 101.into(),
        low: 99.into(),
        close: 100.into(),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

#[test]
fn averaged_lane_retains_current_near_spread_depth_and_its_provenance() {
    let mut history = fixture();
    let mut config = history.config().clone();
    config.live_lane.depth_time_averaging = true;
    history.update_config(config).unwrap();
    let (cells, dropped) = projection(&history);
    assert!(dropped > 0, "the cap reports unavoidable omitted history");
    for (side, price) in [
        (crate::RestingSide::Bid, 99),
        (crate::RestingSide::Ask, 101),
    ] {
        let cell = cells
            .iter()
            .find(|cell| cell.side == side && cell.price_bucket == Decimal::from(price))
            .unwrap();
        assert_eq!(cell.quantity, Decimal::ONE);
        assert!(!cell.time_averaged);
        assert!(
            cell.carried,
            "the freshest publication carries honestly to NOW"
        );
    }
}

#[test]
fn current_preview_reads_latest_coherent_quantity_and_excludes_deleted_rows() {
    let mut history = fixture();
    let mut config = history.config().clone();
    config.live_lane.depth_time_averaging = true;
    config.max_visible_cells = 8;
    history.update_config(config).unwrap();
    history
        .apply_delta(
            1_950,
            &BookDelta::new(14, 14, vec![level(99, 0)], vec![level(101, 7)]),
        )
        .unwrap();
    let (cells, _) = projection(&history);
    let preview: Vec<_> = cells
        .iter()
        .filter(|cell| super::super::current_book_preview(cell, true))
        .collect();
    assert!(
        !preview
            .iter()
            .any(|cell| cell.price_bucket == Decimal::from(99))
    );
    let ask = preview
        .iter()
        .find(|cell| cell.price_bucket == Decimal::from(101))
        .unwrap();
    assert_eq!(ask.side, crate::RestingSide::Ask);
    assert_eq!(ask.quantity, Decimal::from(7));
    assert_eq!(history.latest_book_ms(), Some(1_950));
    history.mark_gap(1_975, "transport_closed").unwrap();
    assert!(
        projection(&history)
            .0
            .iter()
            .all(|cell| !super::super::current_book_preview(cell, true))
    );
}

#[test]
fn current_book_ignores_rle_churn_tolerance_and_survives_exact_confirmation() {
    let mut history = fixture();
    let mut config = history.config().clone();
    config.live_lane.depth_time_averaging = true;
    config.max_visible_cells = 32;
    history.update_config(config).unwrap();
    history
        .apply_delta(
            1_950,
            &BookDelta::new(14, 14, vec![level(95, 131)], vec![level(101, 0)]),
        )
        .unwrap();
    assert_eq!(history.book().last_update_id(), Some(14));
    assert_eq!(history.latest_book_ms(), Some(1_950));
    for now_ms in [1_950, 2_000, 2_450] {
        let cells = projection_at(&history, now_ms).0;
        let current: Vec<_> = cells
            .iter()
            .filter(|cell| super::super::current_book_preview(cell, true))
            .collect();
        let bid = current
            .iter()
            .find(|cell| cell.price_bucket == Decimal::from(95))
            .unwrap();
        assert_eq!(bid.quantity, Decimal::from(131));
        assert_eq!(bid.side, crate::RestingSide::Bid);
        assert_eq!(bid.generation, 1);
        assert_eq!(
            bid.current_book_source,
            Some(crate::projection::BookSource {
                update_id: Some(14),
                event_ms: 1_950
            })
        );
        assert_eq!(bid.carried, now_ms > 1_950);
        assert!(
            !current
                .iter()
                .any(|cell| cell.price_bucket == Decimal::from(101))
        );
    }
    assert!(
        projection_at(&history, 2_451)
            .0
            .iter()
            .all(|cell| !cell.current_book)
    );
    history.mark_gap(2_000, "sequence_gap").unwrap();
    assert!(
        projection_at(&history, 2_050)
            .0
            .iter()
            .all(|cell| !cell.current_book)
    );
}
