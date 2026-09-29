//! Cached tape frames must follow the current price axis before overlap merging.
use super::tape_only_tests::{mark, style_for};
use super::*;
use quantick_orderflow::HeatmapConfig;

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
    let layout =
        ProjectedLayout::new(rect, &viewport, 0, 0, 1, rect.width()).with_inverted(inverted);
    let mut config = HeatmapConfig::default();
    config.volume_dots.enabled = true;
    config.live_lane.tape_only = tape_only;
    let style = style_for(&config);
    let context = RenderContext::new(frame, layout, &style).with_tape_price_range((100.0, 120.0));
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw_aggression_bubbles(&ctx.layer_painter(egui::LayerId::background()), &context);
    });
    let mut centers: Vec<_> = output
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Circle(circle) if circle.fill != egui::Color32::TRANSPARENT => {
                Some(circle.center)
            }
            _ => None,
        })
        .collect();
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
    assert_eq!(
        drawn.len(),
        2,
        "old y must not merge separate current prices"
    );
    assert!((drawn[0].y - 100.0).abs() < 0.01, "{drawn:?}");
    assert!((drawn[1].y - 300.0).abs() < 0.01, "{drawn:?}");
    assert_eq!(
        frame.aggressions[0].y, 0.5,
        "cached input remains immutable"
    );
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

#[test]
fn short_tape_axes_keep_extreme_forming_discs_whole_without_moving_their_price() {
    for height in [32.0, 120.0, 400.0, 800.0] {
        for forming_price in [100, 120] {
            for inverted in [false, true] {
                let viewport = Viewport::new();
                let rect =
                    egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(600.0, height));
                let mut config = HeatmapConfig::default();
                config.volume_dots.enabled = true;
                config.live_lane.tape_only = true;
                let mut style = style_for(&config);
                style.bubbles.max_radius = 18.0;
                let geometry = quantick_orderflow::projection::TapeHorizontalGeometry::resolve(
                    rect.width(),
                    rect.height(),
                    &style.bubbles,
                );
                let other_price = 220 - forming_price;
                let mut earlier = mark(1, true, 1, 0.25, 0.5);
                earlier.price = Decimal::from(other_price);
                let mut forming = mark(2, true, 4, 1.0, 0.5);
                forming.price = Decimal::from(forming_price);
                let frame = projection(vec![earlier, forming]);
                let axis = crate::chart::tape_price_window(
                    Some((100.0, 120.0)),
                    Some(f64::from(forming_price)),
                    None,
                    rect.top(),
                    rect.bottom(),
                    geometry.price_inset_px,
                )
                .unwrap()
                .with_inverted(inverted);
                let layout = ProjectedLayout::new(rect, &viewport, 0, 0, 1, rect.width())
                    .with_inverted(inverted);
                let context =
                    RenderContext::new(&frame, layout, &style).with_tape_price_range(axis.range());
                let ctx = egui::Context::default();
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    draw_aggression_bubbles(
                        &ctx.layer_painter(egui::LayerId::background()),
                        &context,
                    );
                });
                let mut circles: Vec<_> = output
                    .shapes
                    .into_iter()
                    .filter_map(|clipped| match clipped.shape {
                        egui::Shape::Circle(circle)
                            if circle.fill != egui::Color32::TRANSPARENT =>
                        {
                            Some(circle)
                        }
                        _ => None,
                    })
                    .collect();
                circles.sort_by(|a, b| a.radius.total_cmp(&b.radius));
                assert_eq!(circles.len(), 2, "both factual prices remain visible");
                assert!(
                    (circles[1].radius / circles[0].radius - 2.0).abs() < 1e-5,
                    "four times the volume must retain four times the disc area"
                );
                for (circle, price) in [(&circles[0], other_price), (&circles[1], forming_price)] {
                    assert!(
                        (circle.center.y - axis.y(f64::from(price))).abs() < 0.01,
                        "padding the axis must not nudge a dot away from its factual price"
                    );
                    assert!(
                        rect.contains_rect(egui::Rect::from_center_size(
                            circle.center,
                            egui::Vec2::splat(circle.radius * 2.0)
                        )),
                        "height={height} forming={forming_price} inverted={inverted}: {circle:?}"
                    );
                }
                if height == 800.0 {
                    assert_eq!(circles[1].radius, 18.0, "normal pane sizes stay unchanged");
                }
            }
        }
    }
}
