use super::*;
use crate::LiveLaneStyle;
use crate::config::HeatmapConfig;
use crate::history::LiquidityHistory;
use crate::projection::tests::fixtures::*;
use crate::projection::*;
use quantick_orderbook::BookDelta;
use rust_decimal::Decimal;

#[test]
fn resolution_distinguishes_disabled_empty_and_known_book_extent() {
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let empty = BarTimeline::from_bars(0, &[], None, None);
    let disabled = LiquidityHistory::new(HeatmapConfig {
        live_lane: LiveLaneStyle {
            enabled: false,
            ..LiveLaneStyle::default()
        },
        ..HeatmapConfig::default()
    });
    assert!(matches!(
        resolve_window(&disabled, &empty, prices),
        WindowResolution::Disabled(_)
    ));
    let mut history = LiquidityHistory::new(config());
    assert!(matches!(
        resolve_window(&history, &empty, prices),
        WindowResolution::Empty(_)
    ));
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history
        .apply_delta(500, &BookDelta::new(11, 11, vec![], vec![]))
        .unwrap();
    let timeline = BarTimeline::from_bars(0, &[bar(0, 999)], None, None);
    let WindowResolution::Ready(window) = resolve_window(&history, &timeline, prices) else {
        panic!("enabled nonempty window");
    };
    assert_eq!(window.retained_start, 0);
    assert_eq!(window.open_run_end_ms, 500);
    assert!(window.depth_enabled);
    let liquidity = sweep_window(&history, &window, prices);
    assert_eq!(liquidity.coverage.len(), 1);
    assert!(!liquidity.grouped.runs.is_empty());
    assert!(liquidity.grouped.runs.iter().all(|run| run.end_ms <= 500));
}

/// Clearing the map on the candles may not delete the tape's.
///
/// These cells span the whole normalized x axis, tape included, and the
/// renderer clips them per pane. Gating their *production* on the candles'
/// switch therefore emptied the tape as well: the trader switched off one
/// pane and the other went dark with it, which no amount of correctness in
/// the config layer could fix — the data was never built.
#[test]
fn hiding_the_map_on_the_candles_still_projects_the_tapes() {
    let base = HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        // The candles are clear; the tape keeps both of its layers, which
        // is what a fresh install now opens as.
        show_depth: false,
        ..HeatmapConfig::default()
    };
    assert!(!base.depth_visible(), "the candles draw no map");
    assert!(base.lane_depth_drawn(), "the tape does");

    // Enough book movement to close a run, so the cells below exist for a
    // reason other than the switch under test.
    let fill = |config: HeatmapConfig| {
        let mut history = LiquidityHistory::new(config);
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(
                800,
                &BookDelta::new(11, 11, vec![level("100", "6")], vec![]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
            .unwrap();
        history
    };
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();

    let projection = project(&fill(base.clone()), &timeline, prices);
    assert!(
        !projection.cells.is_empty(),
        "the tape reads these cells, so hiding the candles' map may not stop building them"
    );

    // And with the tape's own switch off too, nobody is reading them.
    let both_off = fill(HeatmapConfig {
        live_lane: LiveLaneStyle {
            show_depth: false,
            ..LiveLaneStyle::default()
        },
        ..base
    });
    assert!(
        project(&both_off, &timeline, prices).cells.is_empty(),
        "with neither pane drawing the map, none is built"
    );
}

#[test]
fn disabled_projection_is_empty_even_with_data() {
    // Every layer off, said out loud: the default config stopped being
    // inert when the tape gained defaults of its own (both layers on), so a
    // test about a *disabled* projection has to disable the tape too.
    let mut history = LiquidityHistory::new(HeatmapConfig {
        live_lane: LiveLaneStyle {
            enabled: false,
            ..LiveLaneStyle::default()
        },
        ..HeatmapConfig::default()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
    let projection = project(&history, &timeline, prices);
    assert!(!projection.enabled);
    assert!(projection.cells.is_empty());
    assert!(projection.aggressions.is_empty());
}
