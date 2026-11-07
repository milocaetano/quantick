//! The tape's best bid and best ask lines: bold, in the dots' side colours,
//! and nothing at all while their switch is off.

use super::*;
use quantick_orderflow::projection::BboPath;
use std::sync::Arc;

fn frame() -> HeatmapProjection {
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            Decimal::ONE,
            Decimal::from(100),
        ),
    );
    projection.bbo_lines = Arc::new(vec![
        BboPath {
            side: BookSide::Bid,
            carried: false,
            points: vec![[0.2, 0.6], [0.5, 0.6], [0.5, 0.55], [0.8, 0.55]],
        },
        BboPath {
            side: BookSide::Ask,
            carried: true,
            points: vec![[0.8, 0.45], [0.9, 0.45]],
        },
    ]);
    projection
}

#[test]
fn the_tape_draws_bold_bbo_lines_only_while_their_switch_is_on() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, rect.width());
    let projection = frame();
    let mut config = quantick_orderflow::HeatmapConfig::default();
    config.live_lane.tape_only = true;
    assert!(config.live_lane.bbo_lines, "on by default");
    let style = super::tape_only_tests::style_for(&config);
    let shown = painted(|painter| {
        draw_bbo_lines(
            painter,
            &RenderContext::new(&projection, layout, &style),
            true,
        );
    });
    assert_eq!(
        shown.matches("Path(").count(),
        4,
        "a halo under each line: {shown}"
    );
    assert!(
        shown.contains(&format!("width: {BBO_LINE_WIDTH_PX:?}")),
        "{shown}"
    );

    config.live_lane.bbo_lines = false;
    let off = super::tape_only_tests::style_for(&config);
    let shown_off = crate::orderflow_view::layers::bbo_lines_drawn(&config);
    let shown = painted(|painter| {
        draw_bbo_lines(
            painter,
            &RenderContext::new(&projection, layout, &off),
            shown_off,
        );
    });
    assert_eq!(
        shown, "[]",
        "off, the tape paints exactly what it did before"
    );
}

#[test]
fn the_lines_draw_exactly_when_the_layer_reads_back_drawn() {
    use crate::orderflow_view::layers::bbo_lines_drawn;
    let mut config = quantick_orderflow::HeatmapConfig {
        enabled: true,
        ..quantick_orderflow::HeatmapConfig::default()
    };
    assert!(
        bbo_lines_drawn(&config),
        "on by default while the book is captured"
    );
    config.enabled = false;
    assert!(
        !bbo_lines_drawn(&config),
        "no capture, no lines, as the readback says"
    );
    config.enabled = true;
    config.live_lane.bbo_lines = false;
    assert!(!bbo_lines_drawn(&config));
}

#[test]
fn a_line_from_the_tape_opening_stays_on_the_tape_with_the_candles_panned() {
    let mut viewport = Viewport::new();
    viewport.pan_pixels(400.0, 100);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 100, 96, 4, 200.0);
    let lane = layout.lane_left_x().expect("a tape beside the candles");
    let mut projection = frame();
    let opening = 3.0 / 4.0;
    projection.bbo_lines = Arc::new(vec![BboPath {
        side: BookSide::Bid,
        carried: false,
        points: vec![[opening, 0.5], [0.9, 0.5]],
    }]);
    let config = quantick_orderflow::HeatmapConfig::default();
    let style = super::tape_only_tests::style_for(&config);
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        draw_bbo_lines(
            &painter,
            &RenderContext::new(&projection, layout, &style),
            true,
        );
    });
    let xs: Vec<f32> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Path(path) => Some(path.points.iter().map(|point| point.x)),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(
        (xs[0] - lane).abs() < 0.5 && xs.iter().all(|x| (lane - 0.5..=rect.right()).contains(x)),
        "the opening maps to the divider through the tape, not the candles: {xs:?} vs {lane}"
    );
}
