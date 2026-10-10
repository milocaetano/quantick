// Pane model integration preserves the existing chart behavior.
use super::*;

/// Two surfaces that must agree: the tag a trader reads off the axis and
/// the slot a control client reads out of the cursor scope are one answer,
/// because they come through one owner. Written as a test rather than
/// trusted from the diff — the two are computed in different files.
#[test]
fn the_axis_tag_and_the_control_cursor_name_one_bar() {
    let mut pane = pane_with_timed_bars(200);
    let ctx = egui::Context::default();
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let areas = test_areas(&pane, TEST_PLOT);
    pane.frame.bands = crate::bands::BandGeometry {
        auto_range: pane.frame.auto_range,
        price_view: &pane.price_view,
        lane_divider_x: pane.frame.lane_divider_x,
        indicators: &pane.indicators,
        price_label: &pane.price_band_label,
    }
    .bands(&areas);
    let right = pane.frame.lane_divider_x.unwrap_or(areas.chart.right());
    let total = pane.slots();
    // Deliberately in the left half of a candle, the half that used to
    // answer with its neighbour.
    let slot = 140_usize;
    let x = pane.model.viewport.x_center(slot, right, total)
        - pane.model.viewport.candle_width() / 2.0
        + 0.5;
    pane.hover_pos = Some(egui::pos2(x, areas.chart.center().y));
    let compass = pane
        .series_read()
        .pointer_bar(&pane.model.viewport, x, right, total)
        .expect("the pointer is on a candle");
    let cursor = pane
        .hit_test()
        .control_pointer_hit()
        .expect("and the control plane can see the same pointer");
    assert_eq!(compass.slot, slot);
    assert_eq!(
        cursor.slot,
        Some(compass.slot),
        "the axis and the cursor scope may not name two different bars"
    );
}
