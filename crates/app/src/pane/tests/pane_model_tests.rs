// Pane model integration preserves the existing chart behavior.
use super::*;
use quantick_chart_interaction::pane::CanvasHit;
use quantick_chart_interaction::viewport::{
    DEFAULT_PX_PER_BAR, MAX_CANDLE_WIDTH, OVERVIEW_BARS, OVERVIEW_MIN_PX_PER_BAR,
};

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

/// Lay out `pane`'s bands on [`TEST_PLOT`] the way a drawn frame would,
/// so the price-band hit tests have a band to find.
fn lay_out_bands(pane: &mut ChartPane) -> PlotAreas {
    let areas = test_areas(pane, TEST_PLOT);
    pane.frame.bands = crate::bands::BandGeometry {
        auto_range: pane.frame.auto_range,
        price_view: &pane.price_view,
        lane_divider_x: pane.frame.lane_divider_x,
        indicators: &pane.indicators,
        price_label: &pane.price_band_label,
    }
    .bands(&areas);
    areas
}

/// The candles' canvas ends at the lane divider: the right lane — a book
/// or a heatmap as much as the tape — is not the candles, even inside the
/// price band, so a double click there neither opens the Candles settings
/// nor reads as empty canvas.
#[test]
fn the_bare_candles_end_at_the_lane_divider() {
    let mut pane = pane_with_timed_bars(200);
    let ctx = egui::Context::default();
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let divider = TEST_PLOT.center().x;
    pane.frame.lane_divider_x = Some(divider);
    let areas = lay_out_bands(&mut pane);
    let y = areas.chart.center().y;
    let hit = pane.hit_test();
    let history_px = divider - TEST_PLOT.left();
    assert_eq!(
        hit.canvas_hit(egui::pos2(divider - 20.0, y)),
        Some(CanvasHit::Empty { history_px }),
        "left of the divider is the candles' canvas"
    );
    assert_eq!(
        hit.canvas_hit(egui::pos2(divider + 20.0, y)),
        None,
        "right of the divider is the lane, not the candles"
    );
}

/// A pane panned into history at `px_per_bar`, laid out and banded, with
/// the projection its hit tests read through.
fn panned_pane(ctx: &egui::Context, px_per_bar: f32) -> (ChartPane, PlotAreas) {
    let mut pane = pane_with_timed_bars(200);
    pane.model.viewport.set_px_per_bar(px_per_bar);
    pane.model.viewport.pan_pixels(4.0 * px_per_bar, 200);
    let _ = drive_navigation(&mut pane, ctx, TEST_PLOT, Vec::new());
    let areas = lay_out_bands(&mut pane);
    assert!(!pane.model.viewport.follows_live(), "the fixture is in history");
    (pane, areas)
}

/// On a candle the double click asks for the chart settings and leaves the
/// view exactly where the trader put it: no snap, no zoom.
#[test]
fn a_double_click_on_a_candle_opens_the_chart_settings_and_keeps_the_view() {
    let ctx = egui::Context::default();
    let (mut pane, _) = panned_pane(&ctx, 40.0);
    let (_, right, total, scale) = pane.hit_test().last_projection().expect("drawn");
    let before = (
        pane.model.viewport.px_per_bar(),
        pane.model.viewport.right_edge_bar(total),
    );
    let slot = pane.model.viewport.right_edge_bar(total) as usize - 3;
    // Every fixture bar spans 1..2; 1.5 is inside its body.
    let on_the_candle = egui::pos2(pane.model.viewport.x_center(slot, right, total), scale.y(1.5));

    assert_eq!(
        double_click_asking(&mut pane, &ctx, TEST_PLOT, on_the_candle),
        (None, true),
        "a candle double click opens the chart settings"
    );
    assert_eq!(
        (
            pane.model.viewport.px_per_bar(),
            pane.model.viewport.right_edge_bar(total)
        ),
        before,
        "and moves nothing"
    );
    assert!(!pane.model.viewport.follows_live());
}

/// On empty canvas the double click opens nothing and returns to the live
/// edge at the overview spacing — from max zoom-in as from zoom-out, and a
/// second one changes nothing.
#[test]
fn a_double_click_on_empty_canvas_returns_to_a_live_overview() {
    let ctx = egui::Context::default();
    let (mut pane, areas) = panned_pane(&ctx, MAX_CANDLE_WIDTH);
    let chart = pane.frame.chart_area.expect("laid out");
    let history_px = pane.frame.lane_divider_x.unwrap_or(chart.right()) - chart.left();
    let (_, _, _, scale) = pane.hit_test().last_projection().expect("drawn");
    // Far above every fixture candle (1..2) on a 0..100 axis.
    let open_canvas = egui::pos2(areas.chart.center().x, scale.y(80.0));
    let overview =
        (history_px / OVERVIEW_BARS as f32).clamp(OVERVIEW_MIN_PX_PER_BAR, DEFAULT_PX_PER_BAR);

    assert_eq!(
        double_click_asking(&mut pane, &ctx, TEST_PLOT, open_canvas),
        (None, false),
        "empty canvas opens no dialog"
    );
    assert!(pane.model.viewport.follows_live(), "it returns to live");
    assert_eq!(pane.model.viewport.px_per_bar(), overview, "at the overview cap");

    // Idle frames, so the next click is not read as a third.
    for _ in 0..30 {
        let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    }
    let _ = double_click_asking(&mut pane, &ctx, TEST_PLOT, open_canvas);
    assert_eq!(pane.model.viewport.px_per_bar(), overview, "a second one changes nothing");

    // Zoomed out already, the view keeps its width: the overview never zooms in.
    let (mut wide, areas) = panned_pane(&ctx, 2.0);
    let (_, _, _, scale) = wide.hit_test().last_projection().expect("drawn");
    let open_canvas = egui::pos2(areas.chart.center().x, scale.y(80.0));
    assert_eq!(
        double_click_asking(&mut wide, &ctx, TEST_PLOT, open_canvas),
        (None, false)
    );
    assert!(wide.model.viewport.follows_live());
    assert_eq!(wide.model.viewport.px_per_bar(), 2.0);
}
