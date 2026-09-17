use super::*;
use crate::DisplayGrouping;
use crate::config::HeatmapConfig;
use crate::history::LiquidityHistory;
use crate::projection::tests::fixtures::*;
use crate::projection::*;
use quantick_orderbook::BookDelta;
use rust_decimal::Decimal;

#[test]
fn drafts_keep_one_reference_for_a_run_drawn_in_many_slots_and_the_lane() {
    let closed = [bar(0, 999), bar(1_000, 1_999), bar(2_000, 2_999)];
    let timeline = BarTimeline::from_bars(0, &closed, None, live(2_999, &closed));
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, dec("5"));
    let run = VisualLiquidityRun {
        generation: 1,
        side: RestingSide::Bid,
        price_bucket: dec("100"),
        quantity: dec("7"),
        start_ms: 0,
        end_ms: 2_999,
    };
    let geometry = project_geometry(
        &[run],
        &timeline,
        PriceWindow::new(dec("99"), dec("104")).unwrap(),
        grouping,
    );
    assert!(geometry.drafts.len() > 1);
    assert_eq!(geometry.run_quantities, vec![dec("7")]);
    let hidden = finish_heat(
        geometry,
        &HeatmapConfig {
            show_liquidity: false,
            max_visible_cells: 1,
            ..config()
        },
    );
    assert_eq!(hidden.liquidity_reference, dec("7"));
    assert!(hidden.cells.is_empty());
    assert_eq!(hidden.dropped_cells, 0);
}

#[test]
fn geometry_intermediate_weights_absence_and_orders_latest_generation_slots() {
    let closed = [bar(0, 999), bar(1_000, 1_999)];
    let timeline = BarTimeline::from_bars(0, &closed, None, live(1_999, &closed));
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, dec("5"));
    let runs = [
        VisualLiquidityRun {
            generation: 1,
            side: RestingSide::Bid,
            price_bucket: dec("100"),
            quantity: dec("4"),
            start_ms: 0,
            end_ms: 500,
        },
        VisualLiquidityRun {
            generation: 2,
            side: RestingSide::Bid,
            price_bucket: dec("100"),
            quantity: dec("8"),
            start_ms: 750,
            end_ms: 1_999,
        },
    ];
    let geometry = project_geometry(
        &runs,
        &timeline,
        PriceWindow::new(dec("99"), dec("104")).unwrap(),
        grouping,
    );
    assert_eq!(geometry.run_quantities, vec![dec("4"), dec("8")]);
    let first = geometry
        .drafts
        .iter()
        .find(|draft| draft.x0 == 0.0)
        .unwrap();
    assert_eq!((first.generation, first.quantity), (2, dec("4")));
}

#[test]
fn heat_finish_preserves_input_order_for_equal_ranked_walls() {
    let draft = |side| DraftCell {
        generation: 2,
        side,
        price_bucket: dec("100"),
        quantity: dec("5"),
        x0: 0.0,
        x1: 1.0,
        y0: 0.0,
        y1: 1.0,
    };
    for sides in [
        [RestingSide::Ask, RestingSide::Bid],
        [RestingSide::Bid, RestingSide::Ask],
    ] {
        let geometry = HeatDrafts {
            drafts: sides.map(draft).into(),
            run_quantities: vec![dec("5"), dec("5")],
        };
        let output = finish_heat(
            geometry,
            &HeatmapConfig {
                max_visible_cells: 1,
                ..config()
            },
        );
        assert_eq!(output.cells[0].side, sides[0]);
        assert_eq!(output.dropped_cells, 1);
        assert_eq!(output.cells[0].intensity, 1.0);
        assert_eq!(output.cells[0].alpha, config().opacity);
    }
}

#[test]
fn percentile_is_robust_to_one_large_outlier() {
    let values = (1..=100)
        .map(Decimal::from)
        .chain(std::iter::once(Decimal::from(1_000_000)));
    assert_eq!(percentile_99(values), Decimal::from(100));
}

#[test]
fn projects_and_clips_runs_in_time_and_price() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history
        .apply_delta(
            800,
            &BookDelta::new(11, 11, vec![level("100", "6")], vec![]),
        )
        .unwrap();
    // A stale event advances display coverage without splitting any run.
    history
        .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
        .unwrap();

    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();
    let projection = project(&history, &timeline, prices);

    assert!(projection.enabled);
    assert!(!projection.cells.is_empty());
    assert!(
        projection
            .cells
            .iter()
            .all(|cell| (0.0..=1.0).contains(&cell.x0)
                && (0.0..=1.0).contains(&cell.x1)
                && (0.0..=1.0).contains(&cell.y0)
                && (0.0..=1.0).contains(&cell.y1))
    );
    let old = projection
        .cells
        .iter()
        .find(|cell| cell.price_bucket == dec("100") && cell.quantity == dec("3"))
        .unwrap();
    assert!((old.x0 - 0.1).abs() < 1e-9);
    assert!((old.x1 - 0.8).abs() < 1e-9);
    // Only the lower half of bucket [100,101] is in [99.5,100.5].
    assert!((old.y0 - 0.0).abs() < 1e-9);
    assert!((old.y1 - 0.5).abs() < 1e-9);
}

/// A bar is timeless: its slot cannot say *when* inside the bar a wall was
/// resting, only how much of the bar it rested for. So with a tape on
/// screen the slots summarize — one band per level, weighted by presence —
/// and the tape keeps the runs themselves.
#[test]
fn a_bar_slot_summarizes_the_book_while_the_tape_keeps_the_runs() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(0, 1, snapshot(10)).unwrap();
    // The 100 bid (quantity 3) is pulled halfway through the first bar.
    history
        .apply_delta(
            5_000,
            &BookDelta::new(11, 11, vec![level("100", "0")], vec![]),
        )
        .unwrap();
    history
        .apply_delta(20_000, &BookDelta::new(12, 12, vec![], vec![]))
        .unwrap();

    let closed = [bar(0, 10_000), bar(10_000, 20_000)];
    let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();
    let of_bucket = |projection: &HeatmapProjection| {
        let mut cells: Vec<_> = projection
            .cells
            .iter()
            .filter(|cell| cell.price_bucket == dec("100"))
            .map(|cell| (cell.x0, cell.x1, cell.quantity))
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells
    };

    // Without a tape the chart keeps drawing the run where it happened:
    // half of the first bar's slot, at its own quantity.
    let alone = project(
        &history,
        &BarTimeline::from_bars(0, &closed, None, None),
        prices,
    );
    assert_eq!(of_bucket(&alone), vec![(0.0, 0.25, dec("3"))]);

    // With a tape, that same run becomes one summary band per bar: the
    // whole slot wide, carrying what was typically resting there — three
    // for half the bar reads as one and a half.
    let summarized = project(
        &history,
        &BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::LiveEdge {
                now_ms: 20_000,
                window_ms: 5_000,
                reference_ms: 5_000,
                on_newest_bar: true,
            }),
        ),
        prices,
    );
    // Three regions: two bar slots and the tape. The run only touches the
    // first bar, and it is long gone by the time the tape's window opens.
    assert_eq!(
        of_bucket(&summarized),
        vec![(0.0, 1.0 / 3.0, dec("1.5"))],
        "one band per bar, weighted by how much of it the wall was there"
    );
}

#[test]
fn hidden_liquidity_clears_cells_but_keeps_reference_and_markers() {
    let projection = project_reductions(HeatmapConfig {
        show_liquidity: false,
        ..event_config()
    });
    assert!(projection.cells.is_empty());
    assert_eq!(
        projection.dropped_cells, 0,
        "hidden cells are a choice, not a cap drop"
    );
    assert_eq!(
        projection.liquidity_events.len(),
        2,
        "markers outlive the heat behind them"
    );
    assert!(
        projection.liquidity_reference > Decimal::ZERO,
        "the depletion floors must not move when the heat is hidden"
    );
}
