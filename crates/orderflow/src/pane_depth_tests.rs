//! Regressions consuming a real worker publication without rebuilding it.

use std::sync::Arc;

use quantick_engine::Bar;
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use rust_decimal::Decimal;

use crate::engine::{ProjectionRequest, VisibleOrderflow};
use crate::pane_tape::PaneTape;
use crate::projection::{
    CellHitContext, FlowCellHit, current_book_preview, depth_caption, project_live, project_settled,
};
use crate::{BarTimeline, HeatmapConfig, LiquidityHistory, LiveEdge, PriceWindow};

fn fixture(native: bool) -> (HeatmapConfig, LiquidityHistory, ProjectionRequest) {
    let mut config = HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        ..Default::default()
    };
    config.live_lane.enabled = true;
    config.live_lane.depth_time_averaging = true;
    config.live_lane.native_tape = native;
    config.volume_dots.enabled = true;
    let level = |price| BookLevel::new(price, Decimal::ONE).unwrap();
    let mut history = LiquidityHistory::new(config.clone());
    history
        .install_snapshot(
            0,
            1,
            BookSnapshot::new(
                10,
                vec![level(99.into())],
                vec![level(101.into())],
                BookCoverage::Limited { levels_per_side: 1 },
            ),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
        .unwrap();
    let bar = Bar {
        open_time: 0,
        close_time: 1_000,
        open: 100.into(),
        high: 101.into(),
        low: 99.into(),
        close: 100.into(),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    };
    let request = ProjectionRequest {
        timeline_revision: 1,
        first_bar_index: 0,
        closed: vec![bar],
        partial: None,
        lane: true,
        on_newest_bar: true,
        lane_reference_ms: Some(50),
        lane_now_ms: Some(1_000),
        price_range: (94.0, 102.0),
        dot_zoom: Some(crate::projection::DotZoom {
            native_tape: native,
            tape_window_ms: crate::projection::DOT_WINDOW_LADDER_MS[0],
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: Vec::new(),
        }),
    };
    (config, history, request)
}

fn publish(history: &LiquidityHistory, request: &ProjectionRequest) -> Arc<VisibleOrderflow> {
    let edge = LiveEdge {
        now_ms: 1_000,
        window_ms: 1_000,
        reference_ms: 50,
        on_newest_bar: true,
    };
    let timeline =
        BarTimeline::from_bars(0, &request.closed, None, Some(edge)).with_full_lane_coverage();
    let prices = PriceWindow::new(94.into(), 102.into()).unwrap();
    let dots = crate::projection::VolumeDots::resolve(
        request.dot_zoom.as_ref().unwrap(),
        &request.closed,
        None,
    );
    let settled = project_settled(history, &timeline, prices, Some(&dots));
    let live = project_live(history, &timeline, prices, &settled, Some(&dots));
    let mut projection = settled.with_live(live, history.config());
    if let Some(facts) = projection.tape_facts.as_mut() {
        crate::projection::seal_tape(
            Arc::make_mut(facts),
            None,
            crate::projection::SealInputs {
                revision: 1,
                window_ms: dots.tape_window_ms,
                native_width: Decimal::ONE,
                seal_from_ms: Some(900),
                lane_from_ms: timeline.lane_start_ms(),
                recorded: 0,
            },
        );
    }
    Arc::new(VisibleOrderflow {
        projection: Arc::new(projection),
        first_bar_index: 0,
        slot_count: timeline.region_count(),
        live_edge: Some(edge),
        volume_dots: None,
        tape_overlay: None,
    })
}

#[test]
fn delayed_publication_expires_shared_preview_policy_at_market_clock_boundary() {
    for pending in [false, true] {
        let (config, history, mut request) = fixture(true);
        let published = publish(&history, &request);
        let mut pane = PaneTape::default();
        if pending {
            pane.record(
                &quantick_engine::Trade {
                    agg_id: 1,
                    timestamp_ms: 1_000,
                    price: 100.into(),
                    quantity: Decimal::ONE,
                    side: quantick_engine::Side::Buy,
                },
                &config,
            );
        }
        for (now_ms, fresh) in [(1_400, true), (1_401, false), (2_000, false), (1_400, true)] {
            request.lane_now_ms = Some(now_ms);
            let frame = pane
                .complete_frame(&config, &request, Some(&published))
                .unwrap();
            if pending {
                assert!(
                    frame.tape_overlay.is_some(),
                    "exercise the cached overlay fast path"
                );
            }
            assert_eq!(
                frame
                    .projection
                    .cells
                    .iter()
                    .any(|cell| current_book_preview(cell, true)),
                fresh
            );
            assert_eq!(
                depth_caption(&frame.projection.cells, true)
                    .is_some_and(|caption| caption.contains("current book preview")
                        || caption.contains("Current book preview")),
                fresh
            );
            for original in published
                .projection
                .cells
                .iter()
                .filter(|cell| cell.current_book)
            {
                let cell = frame
                    .projection
                    .cells
                    .iter()
                    .find(|cell| {
                        cell.current_book_source == original.current_book_source
                            && cell.price_bucket == original.price_bucket
                    })
                    .unwrap();
                assert_eq!(
                    (
                        cell.x0,
                        cell.x1,
                        cell.y0,
                        cell.y1,
                        cell.quantity,
                        cell.current_book_source
                    ),
                    (
                        original.x0,
                        original.x1,
                        original.y0,
                        original.y1,
                        original.quantity,
                        original.current_book_source
                    )
                );
                let hit = FlowCellHit::from_cell(
                    cell,
                    Decimal::ONE,
                    CellHitContext {
                        first_bar_index: 0,
                        slot_count: frame.slot_count,
                        has_lane: true,
                        held_tape_end_ms: None,
                        held: false,
                        current_book_preview: current_book_preview(cell, true),
                    },
                )
                .unwrap();
                assert_eq!(hit.current_book_preview, fresh);
                assert_eq!(hit.current_book_source, original.current_book_source);
            }
            for cell in published
                .projection
                .cells
                .iter()
                .filter(|cell| !cell.current_book)
            {
                assert!(
                    frame.projection.cells.contains(cell),
                    "observed history survives expiry"
                );
            }
        }
    }
}

#[test]
fn gap_publication_replaces_cached_preview_without_relabelling_its_source() {
    let (config, mut history, mut request) = fixture(true);
    let published = publish(&history, &request);
    let mut pane = PaneTape::default();
    assert!(
        pane.complete_frame(&config, &request, Some(&published))
            .unwrap()
            .projection
            .cells
            .iter()
            .any(|cell| cell.current_book)
    );
    history.mark_gap(950, "sequence_gap").unwrap();
    let gapped = publish(&history, &request);
    request.lane_now_ms = Some(1_100);
    let frame = pane
        .complete_frame(&config, &request, Some(&gapped))
        .unwrap();
    assert!(
        frame
            .projection
            .cells
            .iter()
            .all(|cell| !current_book_preview(cell, true))
    );
    assert!(!frame.projection.gaps.is_empty());
}

#[test]
fn supplied_monotonic_clock_expires_and_reuses_the_cached_read_model() {
    let (config, history, mut request) = fixture(true);
    let published = publish(&history, &request);
    let mut pane = PaneTape::default();
    request.lane_now_ms = None;
    pane.follow_live(true, Some(900), 10_000);
    pane.follow_live(true, Some(900), 10_501);
    let expired = pane
        .complete_frame(&config, &request, Some(&published))
        .unwrap();
    assert!(
        expired
            .projection
            .cells
            .iter()
            .all(|cell| !cell.current_book)
    );
    let again = pane
        .complete_frame(&config, &request, Some(&published))
        .unwrap();
    assert!(
        Arc::ptr_eq(&expired, &again),
        "expiry copies once per publication"
    );
    assert!(
        published
            .projection
            .cells
            .iter()
            .any(|cell| cell.current_book),
        "the worker publication stays immutable"
    );
}

#[test]
fn ordinary_preview_expires_on_market_and_elapsed_time_without_moving_lane_geometry() {
    for pending in [false, true] {
        let (config, history, mut request) = fixture(false);
        request.lane_now_ms = None;
        request.dot_zoom.as_mut().unwrap().native_tape = false;
        let published = publish(&history, &request);
        let mut pane = PaneTape::default();
        if pending {
            pane.record(
                &quantick_engine::Trade {
                    agg_id: 1,
                    timestamp_ms: 1_000,
                    price: 100.into(),
                    quantity: Decimal::ONE,
                    side: quantick_engine::Side::Buy,
                },
                &config,
            );
        }
        for (applied, monotonic, fresh) in [
            (1_000, 10_000, true),
            (1_000, 10_400, true),
            (1_000, 10_401, false),
            (2_000, 10_401, false),
        ] {
            request.closed[0].close_time = applied;
            pane.follow_live(false, Some(applied), monotonic);
            assert_eq!(pane.lane_now_ms(false), None);
            let frame = pane
                .complete_frame(&config, &request, Some(&published))
                .unwrap();
            assert_eq!(
                frame
                    .projection
                    .cells
                    .iter()
                    .any(|cell| current_book_preview(cell, true)),
                fresh
            );
            for original in published.projection.cells.iter() {
                let cell = frame
                    .projection
                    .cells
                    .iter()
                    .find(|cell| {
                        cell.price_bucket == original.price_bucket
                            && cell.x0 == original.x0
                            && cell.x1 == original.x1
                            && cell.current_book_source == original.current_book_source
                    })
                    .unwrap();
                let mut expected = original.clone();
                if !fresh {
                    expected.current_book = false;
                }
                assert_eq!(cell, &expected, "only current eligibility changes");
            }
            let again = pane
                .complete_frame(&config, &request, Some(&published))
                .unwrap();
            assert!(
                Arc::ptr_eq(&frame, &again),
                "ordinary cached frames are reused"
            );
        }
    }
}

#[test]
fn ordinary_preview_replay_clamps_rewinds_and_resets_source_clock() {
    let (config, history, mut request) = fixture(false);
    request.lane_now_ms = None;
    let published = publish(&history, &request);
    let mut pane = PaneTape::default();
    for (position, applied, next, fresh) in [
        (2_000, 1_000, Some(1_400), true),
        (2_000, 1_000, Some(1_401), false),
        (1_400, 1_000, None, true),
        (1_000, 2_000, None, false),
    ] {
        pane.follow_replay(false, position, Some(applied), next);
        assert_eq!(pane.lane_now_ms(false), None);
        let frame = pane
            .complete_frame(&config, &request, Some(&published))
            .unwrap();
        assert_eq!(
            frame.projection.cells.iter().any(|cell| cell.current_book),
            fresh
        );
    }
    pane.reset_clock();
    pane.follow_live(false, Some(1_000), 50_000);
    assert!(Arc::ptr_eq(
        &published,
        &pane
            .complete_frame(&config, &request, Some(&published))
            .unwrap()
    ));
    pane.follow_live(false, None, 60_000);
    pane.follow_live(false, Some(1_000), 70_000);
    assert!(Arc::ptr_eq(
        &published,
        &pane
            .complete_frame(&config, &request, Some(&published))
            .unwrap()
    ));
}
