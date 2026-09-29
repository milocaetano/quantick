//! Cached tape frames must follow the current price axis before overlap merging.
use super::*;
use super::tape_only_tests::{mark, style_for};

fn projection(marks: Vec<AggressionPrimitive>) -> HeatmapProjection {
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            Decimal::ONE,
            Decimal::from(100),
        ),
    );
    projection.volume_dots = true;
    projection.aggressions = marks;
    projection
}

fn centers(frame: &HeatmapProjection, tape_only: bool, inverted: bool) -> Vec<egui::Pos2> {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 0, 0, 1, rect.width())
        .with_inverted(inverted);
    let mut config = HeatmapConfig::default();
    config.volume_dots.enabled = true;
    config.live_lane.tape_only = tape_only;
    let style = style_for(&config);
    let context = RenderContext::new(frame, layout, &style)
        .with_tape_price_range((100.0, 120.0));
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw_aggression_bubbles(&ctx.layer_painter(egui::LayerId::background()), &context);
    });
    let mut centers: Vec<_> = output.shapes.into_iter().filter_map(|clipped| {
        match clipped.shape {
            egui::Shape::Circle(circle) if circle.fill != egui::Color32::TRANSPARENT => {
                Some(circle.center)
            }
            _ => None,
        }
    }).collect();
    centers.sort_by(|a, b| a.y.total_cmp(&b.y));
    centers
}

#[test]
fn cached_tape_prices_follow_the_current_axis_before_merging() {
    // The worker's stale mapping places both at one pixel, while the current
    // axis separates their factual prices by 200 px. They must stay two dots.
    let mut lower = mark(1, true, 10, 0.5, 0.5);
    lower.price = Decimal::from(105);
    let mut upper = mark(2, true, 40, 0.5, 0.5);
    upper.price = Decimal::from(115);
    let frame = projection(vec![lower, upper]);
    let drawn = centers(&frame, true, false);
    assert_eq!(drawn.len(), 2, "old y must not merge separate current prices");
    assert!((drawn[0].y - 100.0).abs() < 0.01, "{drawn:?}");
    assert!((drawn[1].y - 300.0).abs() < 0.01, "{drawn:?}");
    assert_eq!(frame.aggressions[0].y, 0.5, "cached input remains immutable");
}

#[test]
fn a_cached_offscreen_dot_reappears_at_its_current_price_in_both_orientations() {
    let mut dot = mark(1, true, 10, 0.5, 2.0);
    dot.price = Decimal::from(105);
    let frame = projection(vec![dot]);
    for (inverted, expected_y) in [(false, 300.0), (true, 100.0)] {
        let drawn = centers(&frame, true, inverted);
        assert_eq!(drawn.len(), 1, "visibility must use the current axis");
        assert!((drawn[0].y - expected_y).abs() < 0.01, "{drawn:?}");
    }
}

#[test]
fn an_ordinary_btc_lane_retains_its_published_price_mapping() {
    let mut dot = mark(1, true, 10, 0.5, 0.2);
    dot.price = Decimal::from(105);
    let drawn = centers(&projection(vec![dot]), false, false);
    assert_eq!(drawn.len(), 1);
    assert!((drawn[0].y - 80.0).abs() < 0.01, "{drawn:?}");
}
