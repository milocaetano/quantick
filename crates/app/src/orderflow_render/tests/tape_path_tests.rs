//! The mini-index tape joins factual dot centres with a quiet price path.

use super::*;

#[test]
fn the_tape_price_path_is_behind_the_dots_and_opt_in() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            Decimal::ONE,
            Decimal::from(100),
        ),
    );
    projection.volume_dots = true;
    projection.aggressions = vec![
        super::tape_only_tests::mark(1, true, 5, 0.82, 0.4),
        super::tape_only_tests::mark(2, true, 20, 0.92, 0.6),
    ];
    let mut config = quantick_orderflow::HeatmapConfig::default();
    config.volume_dots.enabled = true;
    config.live_lane.tape_only = true;
    let style = super::tape_only_tests::style_for(&config);
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, rect.width());
    let shown = painted(|painter| {
        draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &style));
    });
    let path = shown
        .find("LineSegment")
        .expect("a subtle line joins the tape dots");
    let dot = shown.find("Circle").expect("the dots remain visible");
    assert!(path < dot, "paint the price path behind the volume dots");

    config.live_lane.tape_only = false;
    let legacy = super::tape_only_tests::style_for(&config);
    let shown = painted(|painter| {
        draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &legacy));
    });
    assert!(
        !shown.contains("LineSegment"),
        "the existing BTC tape is unchanged"
    );
}
