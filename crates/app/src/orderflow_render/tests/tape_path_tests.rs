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

#[test]
fn a_tape_path_clips_an_offscreen_excursion_without_reconnecting_visible_dots() {
    use quantick_orderflow::projection::TapeDotMemory;
    use std::cell::RefCell;

    for retained in [false, true] {
        for inverted in [false, true] {
            for outside_price in [80, 140] {
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
                projection.aggressions = [
                    (500, 110, 4),
                    (1_500, outside_price, 10_000),
                    (2_500, 110, 1),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (time, price, quantity))| {
                    let mut mark =
                        super::tape_only_tests::mark(index as u64 + 1, true, quantity, 0.5, 0.5);
                    mark.price = Decimal::from(price);
                    mark.price_bucket = mark.price;
                    mark.first_timestamp_ms = time;
                    mark.last_timestamp_ms = time;
                    mark.timestamp_quantity = Decimal::from(time) * mark.quantity;
                    mark
                })
                .collect();
                let mut config = quantick_orderflow::HeatmapConfig::default();
                config.volume_dots.enabled = true;
                config.live_lane.tape_only = true;
                let style = super::tape_only_tests::style_for(&config);
                let layout = ProjectedLayout::new(rect, &viewport, 0, 0, 1, rect.width())
                    .with_inverted(inverted);
                let memory = RefCell::new(TapeDotMemory::default());
                let context = RenderContext::new(&projection, layout, &style)
                    .with_tape_price_range((100.0, 120.0))
                    .with_tape_time(
                        quantick_orderflow::LiveEdge {
                            now_ms: 3_000,
                            window_ms: 3_000,
                            reference_ms: 3_000,
                            on_newest_bar: true,
                        },
                        100,
                    );
                let context = if retained {
                    context.with_tape_memory(&memory)
                } else {
                    context
                };
                let ctx = egui::Context::default();
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    draw_aggression_bubbles(
                        &ctx.layer_painter(egui::LayerId::background()),
                        &context,
                    );
                });
                let mut segments = Vec::new();
                let mut circles = Vec::new();
                for clipped in output.shapes {
                    match clipped.shape {
                        egui::Shape::LineSegment { points, .. } => {
                            assert_eq!(
                                clipped.clip_rect, rect,
                                "the actual painter clips to the tape"
                            );
                            segments.push(points);
                        }
                        egui::Shape::Circle(circle)
                            if circle.fill != egui::Color32::TRANSPARENT =>
                        {
                            circles.push(circle)
                        }
                        _ => {}
                    }
                }
                assert_eq!(
                    segments.len(),
                    2,
                    "retain both factual segments across the excursion: retained={retained}, inverted={inverted}, outside={outside_price}"
                );
                let raw_y = (120.0 - f64::from(outside_price)) / 20.0;
                let outside_y = (if inverted { 1.0 - raw_y } else { raw_y }) as f32 * rect.height();
                for segment in &segments {
                    assert!(
                        segment
                            .iter()
                            .any(|point| (point.y - outside_y).abs() < 0.01),
                        "each segment keeps the raw offscreen endpoint, never a bridge along the boundary: {segment:?}"
                    );
                    assert!(segment.iter().any(|point| (point.y - 200.0).abs() < 0.01));
                }
                assert_eq!(
                    circles.len(),
                    2,
                    "the offscreen endpoint adds no phantom dot"
                );
                circles.sort_by(|a, b| a.radius.total_cmp(&b.radius));
                assert_eq!(circles[0].radius, 5.0);
                assert_eq!(
                    circles[1].radius, 10.0,
                    "offscreen volume cannot set the visible reference or collision cap"
                );
            }
        }
    }
}
