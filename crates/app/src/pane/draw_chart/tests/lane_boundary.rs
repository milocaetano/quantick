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
    assert!(pane.lay_out_lane(chart, false).is_none());
    assert_eq!(pane.frame.lane_divider_x, Some(chart.right() - width));

    let narrow = chart.with_max_x(600.0);
    let flow = pane.orderflow.as_mut().unwrap();
    flow.resize_live_lane(-20.0, narrow.width());
    let width = flow.lane_width_px(narrow.width());
    assert!(pane.lay_out_lane(narrow, false).is_none());
    assert_eq!(pane.frame.lane_divider_x, Some(narrow.right() - width));

    pane.orderflow.as_mut().unwrap().set_lane_enabled(false);
    pane.lay_out_lane(narrow, false);
    assert_eq!(pane.frame.lane_divider_x, None);
    pane.orderflow.as_mut().unwrap().set_lane_enabled(true);
    pane.lay_out_lane(narrow, false);
    assert_eq!(pane.frame.lane_divider_x, Some(narrow.right() - width));
}
