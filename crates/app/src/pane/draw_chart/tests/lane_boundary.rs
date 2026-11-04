//! Requested tape layout does not depend on market data arriving.
use crate::pane::ChartPane;
use crate::state::BarSpec;
use eframe::egui;

#[test]
fn the_requested_tape_boundary_survives_missing_book_and_clock() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "WINV26".to_owned());
    let chart = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(1000.0, 600.0));
    let flow = pane
        .orderflow
        .as_mut()
        .expect("the flow pane owns its tape");
    flow.set_lane_enabled(true);
    let width = flow.lane_width_px(chart.width());
    assert!(width > 0.0);
    assert!(pane.lay_out_lane(chart).is_none());
    assert_eq!(pane.frame.lane_divider_x, Some(chart.right() - width));

    let narrow = chart.with_max_x(600.0);
    let flow = pane.orderflow.as_mut().unwrap();
    flow.resize_live_lane(-20.0, narrow.width());
    let width = flow.lane_width_px(narrow.width());
    assert!(pane.lay_out_lane(narrow).is_none());
    assert_eq!(pane.frame.lane_divider_x, Some(narrow.right() - width));

    pane.orderflow.as_mut().unwrap().set_lane_enabled(false);
    pane.lay_out_lane(narrow);
    // Only the rail's band is left for the candles to keep clear of, and no lane:
    // the time strip and a click on the rail are the candles'.
    let rail = quantick_orderflow::LANE_FOLD_RAIL_PX;
    assert_eq!(pane.frame.lane_divider_x, None);
    assert_eq!(pane.frame.candles_right, Some(narrow.right() - rail));
    assert!(!pane.frame.click_on_tape(narrow.right() - 1.0));
    let (candles_strip, tape_strip) =
        crate::plot_area::split_time_strip(narrow, pane.frame.lane_divider_x);
    assert_eq!((candles_strip, tape_strip), (narrow, None));
    assert!(
        crate::pane::lane_rail::lane_handle(&pane, narrow).is_some(),
        "and the rail still takes the pointer"
    );
    let flow = pane.orderflow.as_mut().unwrap();
    flow.set_lane_enabled(true);
    let width = flow.lane_width_px(narrow.width());
    pane.lay_out_lane(narrow);
    assert_eq!(pane.frame.lane_divider_x, Some(narrow.right() - width));
}

/// A pane too short for its chrome clamps the side row under the band; the
/// divider's grab band reaches up through that row as carved and never into the
/// band above it. Without a carved row it stays on the chart.
#[test]
fn the_divider_grab_stops_at_the_carved_side_row() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "WINV26".to_owned());
    pane.orderflow.as_mut().unwrap().set_lane_enabled(true);
    for height in [40.0, 60.0, 600.0] {
        let body = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(1000.0, height));
        let areas = crate::pane::split_flow_pane(body, 1.0);
        pane.frame.area = Some(areas.chart);
        pane.frame.side_row = None;
        pane.lay_out_lane(areas.chart);
        let handle = crate::pane::lane_rail::lane_handle(&pane, areas.chart).unwrap();
        assert_eq!(
            handle.top(),
            areas.chart.top(),
            "{height}: no row, no reach"
        );
        pane.frame.side_row = Some(areas.side_row);
        let handle = crate::pane::lane_rail::lane_handle(&pane, areas.chart).unwrap();
        assert_eq!(handle.top(), areas.side_row.top(), "{height}");
        assert!(
            handle.top() >= areas.band.bottom(),
            "{height}: into the band"
        );
    }
}
