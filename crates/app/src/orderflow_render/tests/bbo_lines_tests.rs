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
        draw_bbo_lines(painter, &RenderContext::new(&projection, layout, &style));
    });
    assert_eq!(shown.matches("Path(").count(), 2, "{shown}");
    assert!(
        shown.contains(&format!("width: {BBO_LINE_WIDTH_PX:?}")),
        "{shown}"
    );

    config.live_lane.bbo_lines = false;
    let off = super::tape_only_tests::style_for(&config);
    let shown = painted(|painter| {
        draw_bbo_lines(painter, &RenderContext::new(&projection, layout, &off));
    });
    assert_eq!(
        shown, "[]",
        "off, the tape paints exactly what it did before"
    );
}
