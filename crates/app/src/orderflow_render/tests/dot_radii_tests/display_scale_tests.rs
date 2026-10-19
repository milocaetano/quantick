use super::*;
use quantick_orderflow::BubbleDisplayScale;

/// The viewport camera's radius gain over the tape `native_painted_radii`
/// paints: its time span is the one the Tape gain's NOW inset leaves.
fn camera_gain(style: &OrderflowRenderStyle) -> f32 {
    style.bubbles.viewport_scale.map_or(1.0, |camera| {
        let geometry = quantick_orderflow::projection::TapeHorizontalGeometry::resolve(
            300.0,
            400.0,
            &style.bubbles,
        );
        camera.radius_gain(f64::from(geometry.span_px) / 6_000.0, 400.0 / 100.0)
    })
}

#[test]
fn manual_native_gains_scale_actual_meshes_and_circles_independently() {
    for spheres in [false, true] {
        for viewport in [false, true] {
            let mut style = stable_native_style(spheres);
            if viewport {
                style.bubbles.viewport_scale = Some(quantick_orderflow::ViewportBubbleScale {
                    x_units_per_pixel: 200.0,
                    price_units_per_pixel: 1.0,
                });
            }
            let original = style.bubbles.clone();
            let (base_tape, base_candles) = native_painted_radii(&style);
            let base_camera = camera_gain(&style);
            for gain in [0.25, 3.0] {
                style.bubbles.display_scale = BubbleDisplayScale {
                    tape: gain,
                    candle: 1.0,
                };
                let (tape, candles) = native_painted_radii(&style);
                assert_eq!(candles, base_candles);
                // The gain widens the NOW inset, and a viewport camera reads
                // its density on the span that leaves: that is its only
                // other effect on the Tape's radii.
                let camera = camera_gain(&style) / base_camera;
                for (scaled, base) in tape.iter().zip(&base_tape) {
                    assert!((scaled - base * gain * camera).abs() < 0.001);
                }
                style.bubbles.display_scale = BubbleDisplayScale {
                    tape: 1.0,
                    candle: gain,
                };
                let (tape, candles) = native_painted_radii(&style);
                assert_eq!(tape, base_tape);
                for (scaled, base) in candles.iter().zip(&base_candles) {
                    assert!((scaled - base * gain).abs() < 0.001);
                }
            }
            style.bubbles.display_scale = BubbleDisplayScale::default();
            assert_eq!(style.bubbles, original);
            assert_eq!(native_painted_radii(&style), (base_tape, base_candles));
        }
    }
}

#[test]
fn manual_legacy_gain_does_not_move_edge_clamped_centers_or_require_volume_dots() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let mut style = dots_style(DotSizing {
        native_tape: false,
        tape_column_px: 100.0,
        candle_column_px: 100.0,
        px_per_price: 100.0,
        typed_full: None,
    });
    let mut projection = dots_frame(vec![dot(false, 8, 0.69, 0.3), dot(true, 20, 0.999, 0.7)]);
    let original = projection.aggressions.clone();
    let paint = |style: &OrderflowRenderStyle, projection: &HeatmapProjection| {
        let ctx = egui::Context::default();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ctx| {
                draw_aggression_bubbles(
                    &ctx.layer_painter(egui::LayerId::background()),
                    &RenderContext::new(projection, layout, style),
                );
            },
        )
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Circle(circle) if circle.fill != egui::Color32::TRANSPARENT => {
                Some((circle.center, circle.radius))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
    };
    for dots in [true, false] {
        projection.volume_dots = dots;
        if !dots {
            style.dot_sizing = None;
        }
        style.bubbles.display_scale = BubbleDisplayScale::default();
        let before = paint(&style, &projection);
        assert_eq!(before.len(), 2);
        style.bubbles.display_scale = BubbleDisplayScale {
            tape: 3.0,
            candle: 0.25,
        };
        let after = paint(&style, &projection);
        assert_eq!(after.len(), before.len());
        for ((center, radius), (old_center, old_radius)) in after.iter().zip(&before) {
            assert_eq!(center, old_center);
            let gain = if center.x >= 700.0 { 3.0 } else { 0.25 };
            assert!((radius - old_radius * gain).abs() < 0.001);
        }
    }
    assert_eq!(projection.aggressions, original);
}

/// The Tape gain magnifies the disc after sizing, so the inset at NOW has to
/// hold the magnified disc: the newest, biggest print stays whole against the
/// lane's right edge at the shipped BTC gain and at the slider's top, its
/// centre stepping left to make the room.
#[test]
fn the_newest_dot_stays_whole_at_now_under_the_tape_gain() {
    use quantick_orderflow::projection::TapeDotMemory;
    use std::cell::RefCell;

    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let mut newest = dot(true, 8, 1.0, 0.5);
    newest.price = Decimal::from(50);
    newest.price_bucket = newest.price;
    newest.first_timestamp_ms = 6_000;
    newest.last_timestamp_ms = 6_000;
    newest.timestamp_quantity = Decimal::from(6_000) * newest.quantity;
    let projection = dots_frame(vec![newest]);
    let edge = quantick_orderflow::LiveEdge {
        now_ms: 6_000,
        window_ms: 6_000,
        reference_ms: 6_000,
        on_newest_bar: true,
    };
    let painted_dot = |gain: f32| {
        let mut style = stable_native_style(false);
        style.bubbles.display_scale = BubbleDisplayScale {
            tape: gain,
            candle: 1.0,
        };
        let memory = RefCell::new(TapeDotMemory::default());
        let context = RenderContext::new(&projection, layout, &style)
            .with_tape_price_range((0.0, 100.0))
            .with_tape_time(edge, 100)
            .with_tape_memory(&memory, None);
        let shapes = egui::Context::default()
            .run(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..egui::RawInput::default()
                },
                |ctx| {
                    draw_aggression_bubbles(
                        &ctx.layer_painter(egui::LayerId::background()),
                        &context,
                    );
                },
            )
            .shapes;
        let lane = layout.lane_rect();
        let mut bounds = egui::Rect::NOTHING;
        for clipped in shapes {
            let drawn = clipped.shape.visual_bounding_rect();
            if drawn.is_positive() && clipped.clip_rect == lane {
                bounds = bounds.union(drawn);
            }
        }
        assert!(bounds.is_positive(), "a dot at NOW under gain {gain}");
        bounds
    };
    let plain = painted_dot(1.0);
    let lane_right = layout.lane_rect().right();
    for gain in [1.37, 3.0] {
        let dot = painted_dot(gain);
        assert!(
            dot.right() <= lane_right + 0.001,
            "gain {gain}: the dot at NOW ends at {} past the lane's {lane_right}",
            dot.right()
        );
        assert!(
            dot.height() > plain.height() * gain * 0.9,
            "gain {gain} magnifies the disc: {dot:?} against {plain:?}"
        );
        assert!(
            dot.center().x < plain.center().x,
            "gain {gain} widens the inset"
        );
    }
}
