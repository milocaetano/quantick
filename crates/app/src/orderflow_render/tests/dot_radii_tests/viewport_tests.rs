//! Read the actual painted discs after cached Tape groups meet a new camera.

use super::*;
use quantick_orderflow::ViewportBubbleScale;
use quantick_orderflow::projection::{TapeBubbleDisplay, TapeDotMemory, TapeHorizontalGeometry};
use std::cell::RefCell;

#[test]
fn visible_native_display_reaches_circle_and_sphere_radii_without_changing_candles() {
    for spheres in [false, true] {
        let mut style = stable_native_style(spheres);
        let (_, old_candles) = native_painted_radii(&style, spheres);
        style.bubbles.viewport_scale = Some(ViewportBubbleScale {
            x_units_per_pixel: 200.0,
            price_units_per_pixel: 1.0,
        });
        let geometry = TapeHorizontalGeometry::resolve(300.0, 400.0, &style.bubbles);
        let marks = [
            dot(true, 1, 0.8, 0.2),
            dot(true, 4, 0.8, 0.5),
            dot(true, 8, 0.8, 0.8),
        ];
        let display = TapeBubbleDisplay::resolve(
            &style.bubbles,
            &marks,
            f64::from(geometry.span_px) / 6_000.0,
            4.0,
            geometry.max_radius,
        )
        .unwrap();
        let (tape, candles) = native_painted_radii(&style, spheres);
        assert_eq!(
            candles, old_candles,
            "Tape presentation never changes candle marks"
        );
        for (radius, quantity) in tape.iter().zip([1, 4, 8]) {
            let expected = display.draw(quantity.into()).1;
            assert!(
                (radius - expected).abs() < 0.001,
                "painted={radius}, readback={expected}"
            );
        }
    }
}

#[test]
fn cached_native_frame_uses_current_camera_and_visible_groups_for_every_paint() {
    let mut style = stable_native_style(false);
    style.bubbles.viewport_scale = Some(ViewportBubbleScale {
        x_units_per_pixel: 200.0,
        price_units_per_pixel: 1.0,
    });
    let mut marks = Vec::new();
    for (index, (quantity, price)) in [(1, 80), (4, 50), (8, 20), (1000, 1000)]
        .into_iter()
        .enumerate()
    {
        let mut mark = dot(true, quantity, 0.8, 0.5);
        mark.agg_id = index as u64 + 1;
        mark.agg_ids = vec![mark.agg_id];
        mark.price = price.into();
        mark.price_bucket = mark.price;
        mark.first_timestamp_ms = 1_000 + index as i64 * 1_000;
        mark.last_timestamp_ms = mark.first_timestamp_ms;
        mark.timestamp_quantity = Decimal::from(mark.first_timestamp_ms) * mark.quantity;
        marks.push(mark);
    }
    let original = marks.clone();
    let projection = dots_frame(marks);
    let memory = RefCell::new(TapeDotMemory::default());
    let viewport = Viewport::new();
    let paint = |window_ms, price_high, lane_width| {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0));
        let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, lane_width);
        let edge = quantick_orderflow::LiveEdge {
            now_ms: 6_000,
            window_ms,
            reference_ms: 6_000,
            on_newest_bar: true,
        };
        let context = RenderContext::new(&projection, layout, &style)
            .with_tape_price_range((0.0, price_high))
            .with_tape_time(edge, 100)
            .with_tape_memory(&memory, None);
        radii(&painted(|painter| {
            draw_aggression_bubbles(painter, &context)
        }))
    };
    let a = paint(6_000, 100.0, 300.0);
    assert_eq!(a.len(), 3, "the offscreen whale does not draw");
    assert_eq!(memory.borrow().retained_group_count(), 4);
    assert!((a[1] / a[0] - 2.0).abs() < 0.001);
    for b in [
        paint(12_000, 100.0, 300.0),
        paint(6_000, 200.0, 300.0),
        paint(6_000, 100.0, 150.0),
        paint(6_000, 100.0, 20.0),
    ] {
        assert_eq!(b.len(), 3);
        assert!(
            b[2] < a[2],
            "each camera compression shrinks the common radius"
        );
        for index in 0..3 {
            assert!((b[index] / a[index] - b[2] / a[2]).abs() < 0.001);
        }
        assert_eq!(
            paint(6_000, 100.0, 300.0),
            a,
            "same cached frame and camera restore exactly"
        );
    }
    assert!(
        paint(6_000, 100.0, 20.0)[2] <= 10.0,
        "one shared envelope fits a narrow Tape"
    );
    let with_whale = paint(6_000, 1100.0, 300.0);
    assert_eq!(with_whale.len(), 4);
    assert!(
        with_whale[2] / with_whale[3] < 0.1,
        "visible quantity ratios remain readable"
    );
    assert_eq!(
        paint(6_000, 100.0, 300.0),
        a,
        "a hidden whale stops setting the reference"
    );
    assert_eq!(
        projection.aggressions, original,
        "drawing cannot rewrite source facts"
    );
}
