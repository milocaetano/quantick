use super::*;
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::config::theme::OrderflowRenderStyle;
use quantick_orderflow::projection::TapeHorizontalGeometry;
use rust_decimal::Decimal;

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> FlowRect {
    FlowRect {
        left,
        top,
        right,
        bottom,
    }
}

/// The depth map is switched per pane by the region it may paint, not by
/// dropping cells: a resting level that has been there since before the
/// tape's window opened is one continuous band across the divider, and
/// hiding the map over the candles has to cut it there rather than lose it.
#[test]
fn the_depth_map_is_cut_at_the_divider_rather_than_dropped() {
    let viewport = Viewport::new();
    let chart = rect(0.0, 0.0, 1000.0, 400.0);
    let layout = ProjectedLayout::new(chart, &viewport, 3, 0, 4, 300.0);
    let divider = layout.lane_left_x().expect("a lane has a boundary");

    assert_eq!(layout.layer_clip(true, true), Some(layout.chart_rect));
    assert_eq!(layout.layer_clip(true, false), Some(layout.history_rect()));
    assert_eq!(layout.layer_clip(false, true), Some(layout.lane_rect()));
    assert_eq!(layout.layer_clip(false, false), None);
    // The two regions meet at the divider and neither reaches past it, so
    // a band crossing it is cut, never doubled or dropped.
    assert!(layout.layer_clip(true, false).unwrap().right <= divider);
    assert!(layout.layer_clip(false, true).unwrap().left >= divider);

    // A canvas with no tape has one pane, and it is the candles'. The
    // lane's switch cannot blank a chart that has no lane.
    let laneless = ProjectedLayout::new(chart, &viewport, 3, 0, 4, 0.0);
    assert!(laneless.lane_left_x().is_none());
    assert_eq!(laneless.layer_clip(true, false), Some(chart));
    assert_eq!(laneless.layer_clip(true, true), Some(chart));
    assert_eq!(laneless.layer_clip(false, true), None);
}

/// The lane is a pane of its own: a fixed band on the right edge of the
/// chart, with the candles' pane ending exactly where it opens.
#[test]
fn the_lane_is_a_fixed_band_on_the_right_edge() {
    let viewport = Viewport::new(); // candle_width 8, following
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    // Four regions: three bar slots and the lane, 300 px wide.
    let layout = ProjectedLayout::new(chart, &viewport, 3, 0, 4, 300.0);
    let boundary = layout.lane_left_x().expect("a lane has a boundary");
    assert!(
        (boundary - 700.0).abs() < 0.01,
        "the band is the last 300 px"
    );

    // Bars keep their candle width, and the last one ends at the divider.
    let bar_width = (layout.x(1.0 / 4.0) - layout.x(0.0)).abs();
    assert!((bar_width - viewport.candle_width()).abs() < 0.01);
    assert!(
        (layout.x(3.0 / 4.0) - boundary).abs() < 0.01,
        "the candles' pane ends exactly where the lane opens"
    );
    // ...and the lane spans the band, whatever the candles are worth.
    assert!((layout.x(1.0) - chart.right).abs() < 0.01);
    assert_eq!(layout.lane_rect(), rect(700.0, 0.0, 1000.0, 100.0));

    let no_lane = ProjectedLayout::new(chart, &viewport, 4, 0, 4, 0.0);
    assert_eq!(no_lane.lane_left_x(), None);
    let flat_width = (no_lane.x(1.0) - no_lane.x(3.0 / 4.0)).abs();
    assert!((flat_width - viewport.candle_width()).abs() < 0.01);
    assert!((no_lane.x(1.0) - chart.right).abs() < 0.01);
}

#[test]
fn the_lane_is_the_only_region_wider_than_a_candle() {
    let viewport = Viewport::new(); // candle_width 8, following
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    // 3 regions: 2 bar slots and the lane, 32 px wide — four candles' worth
    // at this zoom, and still 32 px at any other.
    let layout = ProjectedLayout::new(chart, &viewport, 2, 0, 3, 32.0);
    let closed_w = (layout.x(1.0 / 3.0) - layout.x(0.0)).abs();
    let live_w = (layout.x(1.0) - layout.x(2.0 / 3.0)).abs();
    assert!(closed_w > 0.0);
    assert!(
        (live_w - closed_w * 4.0).abs() < 0.01,
        "the lane should be 4x a bar slot: bar={closed_w} lane={live_w}"
    );
}

/// The divider sits on the chart's left edge when the tape takes the whole
/// width, and a lane wider than the chart is still no lane.
#[test]
fn a_whole_width_lane_puts_the_divider_on_the_left_edge() {
    let chart = rect(60.0, 0.0, 1060.0, 400.0);
    assert_eq!(lane_divider_x(chart, 1_000.0), Some(60.0));
    assert_eq!(lane_divider_x(chart, 1_000.5), None);
    assert_eq!(lane_divider_x(chart, 350.0), Some(710.0));
    assert_eq!(lane_divider_x(chart, f32::NAN), None);
    let viewport = Viewport::new();
    let tape = ProjectedLayout::new(chart, &viewport, 3, 0, 4, chart.width());
    assert_eq!(tape.lane_rect(), chart, "the tape fills the canvas");
}

/// The point of the pane: every chart movement leaves the tape alone.
#[test]
fn panning_and_zooming_the_candles_never_move_the_lane() {
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    let mut panned = Viewport::new();
    panned.pan_pixels(240.0, 60); // 30 candles into history
    let mut zoomed = Viewport::new();
    zoomed.zoom(4.0);

    let still = Viewport::new();
    let reference = ProjectedLayout::new(chart, &still, 60, 0, 5, 300.0);
    for viewport in [&panned, &zoomed] {
        let moved = ProjectedLayout::new(chart, viewport, 60, 0, 5, 300.0);
        assert_eq!(moved.lane_left_x(), reference.lane_left_x());
        // Same instant on the tape, same pixel — however the candles moved.
        for position in [0.85_f64, 0.9, 1.0] {
            assert!(
                (moved.x(position) - reference.x(position)).abs() < 0.01,
                "the tape moved with the candles at {position}"
            );
        }
    }
}

/// A bar panned off the right of its own pane scrolls out of sight instead
/// of being drawn over the tape.
#[test]
fn a_candle_panned_behind_the_tape_is_clipped_to_its_own_pane() {
    let mut viewport = Viewport::new();
    viewport.pan_pixels(80.0, 20); // 10 candles into history
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    let layout = ProjectedLayout::new(chart, &viewport, 20, 0, 21, 300.0);
    let divider = layout.lane_left_x().expect("a lane has a boundary");

    // The newest bar is now right of the divider, and its pane stops there.
    let newest = 19.5 / 21.0;
    assert!(layout.x(newest) > divider);
    assert!(layout.pane(newest).right <= divider + 0.01);
    // A band entirely past the divider is clipped away to nothing...
    assert!(!layout.band(newest, newest, 0.1, 0.2, 1.0).is_positive());
    // ...while the tape's own pane starts there and reaches the edge.
    assert!((layout.pane(1.0).left - divider).abs() < 0.01);
    assert!((layout.pane(1.0).right - chart.right).abs() < 0.01);
}

/// A band crossing the divider is clipped by neither pane; one whose history
/// end has scrolled off the candles' pane keeps only the tape's part.
#[test]
fn a_band_across_the_divider_spans_both_panes_until_its_history_scrolls_off() {
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    let still = Viewport::new();
    let layout = ProjectedLayout::new(chart, &still, 60, 0, 5, 300.0);
    assert_eq!(layout.span_pane(0.5, 0.9), chart);
    assert_eq!(layout.span_pane(0.9, 1.0), layout.lane_rect());
    assert_eq!(layout.span_pane(0.1, 0.2), layout.history_rect());

    let mut panned = Viewport::new();
    panned.pan_pixels(80.0, 60); // the newest bars behind the tape
    let ahead = ProjectedLayout::new(chart, &panned, 60, 56, 5, 300.0);
    assert!(ahead.x(0.7) >= ahead.lane_left_x().unwrap());
    assert_eq!(ahead.span_pane(0.7, 0.9), ahead.lane_rect());
}

/// A band thinner than the readable minimum grows about its centre, and the
/// orientation flip turns it over with the chart.
#[test]
fn a_thin_band_grows_to_the_minimum_about_its_centre_and_flips_with_the_chart() {
    let viewport = Viewport::new();
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    let layout = ProjectedLayout::new(chart, &viewport, 3, 0, 4, 300.0);
    let band = layout.band(0.0, 0.25, 0.5, 0.51, 4.0);
    assert!((band.height() - 4.0).abs() < 1e-4, "{band:?}");
    assert!(((band.top + band.bottom) / 2.0 - 50.5).abs() < 1e-4);
    let flipped = layout.with_inverted(true).band(0.0, 0.25, 0.1, 0.2, 1.0);
    assert!((flipped.top - 80.0).abs() < 1e-4 && (flipped.bottom - 90.0).abs() < 1e-4);
    assert!(layout.band(f64::NAN, 0.25, 0.1, 0.2, 1.0).is_finite());
}

#[test]
fn ordinary_lane_opening_never_stretches_old_depth_across_forward_space() {
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    let mut viewport = Viewport::new();
    viewport.pan_pixels(-160.0, 60);
    for native in [false, true] {
        for inverted in [false, true] {
            let mut layout =
                ProjectedLayout::new(chart, &viewport, 60, 56, 5, 300.0).with_inverted(inverted);
            if native {
                layout.tape_geometry = Some(TapeHorizontalGeometry::resolve(
                    300.0,
                    100.0,
                    &quantick_orderflow::BubbleStyle::default(),
                ));
            }
            let divider = layout.lane_left_x().unwrap();
            let band = layout.band(0.8, 0.9, 0.2, 0.3, 1.0);
            let opening = layout.with_lane_only().x(0.8);
            assert!((band.left - opening).abs() < 0.01);
            assert!(band.left >= divider);
            let preview = layout.current_book_preview_rect(0.2, 0.3);
            assert!(preview.is_positive());
            assert!((preview.right - divider).abs() < 0.01);
            assert!(preview.left < divider);
            assert!(preview.intersect(band).width() <= 0.01);
            assert_eq!(preview.top, layout.y(0.2).min(layout.y(0.3)));
        }
    }
}

/// A live strip on the candles' side of the divider: the candles' pane, and every
/// bar mapped into it, ends where the strip starts; the tape still opens on the
/// divider, as wide as it was.
#[test]
fn a_trail_strip_ends_the_candles_short_of_the_divider() {
    let chart = rect(0.0, 0.0, 1_000.0, 600.0);
    let viewport = Viewport::new();
    let plain = ProjectedLayout::new(chart, &viewport, 10, 0, 11, 300.0);
    let mut layout = plain;
    layout.trail_strip_px = 84.0;
    assert_eq!(plain.history_rect().right, 700.0);
    assert_eq!(layout.history_rect().right, 616.0);
    assert_eq!(layout.lane_rect(), plain.lane_rect());
    assert_eq!(layout.lane_left_x(), Some(700.0));
    let newest = |layout: ProjectedLayout<'_>| layout.x(9.5 / 11.0);
    assert!((newest(plain) - newest(layout) - 84.0).abs() < 1e-3);
}

/// The native tape's book spans the whole lane: a lead band from the lane's
/// left edge, a live current row to its right edge, a stale one only to NOW.
#[test]
fn the_tape_book_reaches_both_lane_edges() {
    let viewport = Viewport::new();
    let chart = rect(20.0, 30.0, 620.0, 430.0);
    let mut config = HeatmapConfig::default();
    config.volume_dots.enabled = true;
    config.live_lane.enabled = true;
    config.live_lane.tape_only = true;
    config.live_lane.depth_time_averaging = true;
    let mut style = OrderflowRenderStyle::from_config(&config, [0, 0, 0, 255]);
    style.bubbles.max_radius = 48.0;
    let mut layout = ProjectedLayout::new(chart, &viewport, 1, 0, 2, chart.width());
    layout.tape_geometry =
        TapeHorizontalGeometry::from_style(&style, layout.lane_width_px, chart.height());
    let geometry = layout.tape_geometry.expect("an averaged native tape");
    assert!(geometry.inset_px > 0.0);
    let cell = |x0: f64, lead_span: Option<(f64, f64)>, current_book: bool| HeatmapCell {
        generation: 1,
        side: quantick_orderflow::RestingSide::Bid,
        price_bucket: Decimal::from(99),
        quantity: Decimal::from(5),
        x0,
        x1: if lead_span.is_some() { 0.5 } else { 1.0 },
        y0: 0.2,
        y1: 0.3,
        intensity: 0.8,
        alpha: 0.8,
        carried: false,
        time_averaged: false,
        current_book,
        lead_span,
    };
    let lead = layout.heat_cell_rect(&cell(0.5, Some((-1.0, -0.05)), false), 1.0, true);
    assert!((lead.left - chart.left).abs() < 0.001, "{lead:?}");
    let older = chart.left + geometry.x(-0.05);
    assert!(older > chart.left && lead.right > lead.left);
    assert!((lead.right - older).abs() < 0.001, "on the dots' clock");
    let cut = layout.heat_cell_rect(&cell(0.5, Some((-1.0, -0.9)), false), 1.0, true);
    assert_eq!(
        cut,
        FlowRect::NOTHING,
        "a lead cut at the edge is no sliver"
    );
    let current = cell(0.75, None, true);
    let live = layout.heat_cell_rect(&current, 1.0, true);
    assert!((live.right - chart.right).abs() < 0.001, "{live:?}");
    assert!((live.left - (chart.left + geometry.x(0.5))).abs() < 0.001);
    let stale = layout.heat_cell_rect(&current, 1.0, false);
    assert!((stale.right - (chart.left + geometry.x(1.0))).abs() < 0.001);
    let observed = layout.heat_cell_rect(&cell(0.75, None, false), 1.0, true);
    assert_eq!(
        observed,
        layout.band(0.75, 1.0, 0.2, 0.3, 1.0),
        "the rest stays put"
    );
}

/// Off the tape a current-book row that only previews the book covers nothing;
/// a historical cell is its band.
#[test]
fn a_heat_cell_off_the_tape_is_its_band_and_a_bare_preview_row_is_nothing() {
    let viewport = Viewport::new();
    let chart = rect(0.0, 0.0, 1000.0, 100.0);
    let layout = ProjectedLayout::new(chart, &viewport, 3, 0, 4, 300.0);
    let mut cell = HeatmapCell {
        generation: 1,
        side: quantick_orderflow::RestingSide::Ask,
        price_bucket: Decimal::from(101),
        quantity: Decimal::from(2),
        x0: 0.1,
        x1: 0.4,
        y0: 0.6,
        y1: 0.7,
        intensity: 0.5,
        alpha: 0.5,
        carried: false,
        time_averaged: false,
        current_book: false,
        lead_span: None,
    };
    assert_eq!(
        layout.heat_cell_rect(&cell, 1.0, true),
        layout.band(0.1, 0.4, 0.6, 0.7, 1.0)
    );
    cell.current_book = true;
    cell.x1 = cell.x0;
    assert_eq!(layout.heat_cell_rect(&cell, 1.0, true), FlowRect::NOTHING);
}
