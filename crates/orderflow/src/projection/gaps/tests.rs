use super::*;
use crate::config::HeatmapConfig;
use crate::history::LiquidityHistory;
use crate::projection::tests::fixtures::*;
use crate::projection::*;
use quantick_orderbook::BookDelta;

#[test]
fn gap_stage_clips_open_gaps_and_orders_leading_absence() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(250, 1, snapshot(10)).unwrap();
    history.mark_gap(1_500, "stage_open_gap").unwrap();
    let timeline = BarTimeline::from_bars(
        0,
        &[bar(0, 999), bar(1_000, 1_999), bar(2_000, 2_999)],
        None,
        None,
    );
    let output = project_gaps(&history, &timeline, 0, 2_000, true);
    assert_eq!(output.len(), 2);
    assert!(output[0].precedes_capture());
    assert_eq!(output[1].reason, "stage_open_gap");
    assert_eq!(
        output[1].x1,
        timeline.locate_clamped(2_000).unwrap().normalized
    );
    assert!(output[0].x1 <= output[1].x0);
    assert!(project_gaps(&history, &timeline, 0, 2_000, false).is_empty());
}

#[test]
fn marks_history_before_first_snapshot_as_unavailable() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(400, 1, snapshot(10)).unwrap();
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    let unavailable = projection
        .gaps
        .iter()
        .find(|gap| gap.reason == "book_unavailable_before_capture")
        .unwrap();
    assert_eq!(unavailable.x0, 0.0);
    assert!((unavailable.x1 - 0.4).abs() < 1e-9);
    assert_eq!(unavailable.to_generation, Some(1));
}

#[test]
fn an_unsynchronized_history_marks_the_whole_timeline_unavailable() {
    let history = LiquidityHistory::new(config());
    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    assert_eq!(
        projection.gaps.as_slice(),
        [GapPrimitive {
            from_generation: None,
            to_generation: None,
            x0: 0.0,
            x1: 1.0,
            reason: "book_unavailable_before_capture".to_owned(),
        }]
    );
}

#[test]
fn resync_gap_is_a_primitive_and_runs_do_not_bridge_it() {
    let mut history = LiquidityHistory::new(config());
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history.mark_gap(300, "sequence_gap").unwrap();
    history.install_snapshot(600, 2, snapshot(50)).unwrap();
    history
        .apply_delta(900, &BookDelta::new(50, 50, vec![], vec![]))
        .unwrap();

    let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
    let projection = project(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    let gap = projection
        .gaps
        .iter()
        .find(|gap| gap.reason == "sequence_gap")
        .unwrap();
    assert!((gap.x0 - 0.3).abs() < 1e-9);
    assert!((gap.x1 - 0.6).abs() < 1e-9);
    assert!(
        projection
            .cells
            .iter()
            .all(|cell| { cell.x1 <= gap.x0 || cell.x0 >= gap.x1 })
    );
}

#[test]
fn hidden_gaps_emit_no_coverage_primitives() {
    let mut history = LiquidityHistory::new(HeatmapConfig {
        show_gaps: false,
        ..config()
    });
    history.install_snapshot(400, 1, snapshot(10)).unwrap();
    history
        .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
        .unwrap();
    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    // The same fixture with the flag on yields the
    // "book_unavailable_before_capture" span (see the test above).
    assert!(projection.gaps.is_empty());
}
