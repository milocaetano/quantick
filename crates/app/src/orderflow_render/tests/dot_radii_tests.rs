//! The painter's volume-dot radii: each pane sizes its dots against its own
//! biggest dot, and a dot never outgrows its cell.

use super::*;
use quantick_orderflow::DotSizing;

/// A volume dot on `live`'s pane, holding `quantity` contracts one tick
/// tall, at `(x, y)`.
fn dot(live: bool, quantity: i64, x: f64, y: f64) -> AggressionPrimitive {
    AggressionPrimitive {
        agg_id: 1,
        agg_ids: vec![1],
        generation: None,
        side: Side::Buy,
        consumed_side: BookSide::Ask,
        quantity: Decimal::from(quantity),
        buy_share: 1.0,
        live,
        price_bucket: Decimal::ONE,
        price_span: Decimal::ONE,
        price: Decimal::ONE,
        trade_count: 1,
        first_timestamp_ms: 0,
        last_timestamp_ms: 0,
        timestamp_quantity: Decimal::ZERO,
        matched_quantity: Decimal::ZERO,
        buy_quantity: Decimal::from(quantity),
        matched_fraction: 0.0,
        liquidity_event_ids: Vec::new(),
        x,
        y,
        size: 1.0,
        folded_marks: 0,
    }
}

/// Every circle radius a paint emitted, ascending.
fn radii(shapes: &str) -> Vec<f32> {
    let mut radii: Vec<f32> = shapes
        .split("radius: ")
        .skip(1)
        .filter_map(|rest| {
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(rest.len());
            rest[..end].parse().ok()
        })
        .collect();
    radii.sort_by(f32::total_cmp);
    radii
}

fn dots_style(sizing: DotSizing) -> OrderflowRenderStyle {
    OrderflowRenderStyle {
        bubbles: BubbleStyle {
            min_radius: 2.0,
            max_radius: 10.0,
            detail_min_radius: 100.0, // cheap dots only: one circle each
            hollow_small_buys: false,
            halo_strength: 0.0,
            trail_length: 0.0,
            show_quantity_labels: false,
            show_trade_count: false,
            ..BubbleStyle::default()
        },
        dot_sizing: Some(sizing),
        ..OrderflowRenderStyle::default()
    }
}

fn dots_frame(marks: Vec<AggressionPrimitive>) -> HeatmapProjection {
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

/// With the automatic scale the biggest dot of each pane is drawn at that
/// pane's largest radius — the tape's full radius, the candles' share of it —
/// and a dot a quarter of its volume visibly smaller. All four differ.
#[test]
fn the_biggest_dot_of_each_pane_is_clearly_the_biggest() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let style = dots_style(DotSizing {
        native_tape: false,
        tape_column_px: 100.0,
        candle_column_px: 100.0,
        px_per_price: 100.0,
        typed_full: None,
    });
    let projection = dots_frame(vec![
        dot(false, 2, 0.3, 0.3),
        dot(false, 8, 0.3, 0.7),
        dot(true, 5, 0.85, 0.3),
        dot(true, 20, 0.85, 0.7),
    ]);
    let drawn = radii(&painted(|painter| {
        draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &style));
    }));
    // Candles: 2..4 px (0.4 of 10). Tape: 2..10 px. Area follows quantity.
    let expected = [7.0_f32.sqrt(), 4.0, 28.0_f32.sqrt(), 10.0];
    assert_eq!(drawn.len(), 4, "one circle a dot: {drawn:?}");
    for (drawn, expected) in drawn.iter().zip(expected) {
        assert!((drawn - expected).abs() < 1e-3, "{drawn} vs {expected}");
    }
}

/// A dot never outgrows its cell: half the tape's column and half its
/// level's height on screen. Squeezing the price axis shrinks every dot of
/// the pane by one share, so they stay in proportion.
#[test]
fn a_squeezed_axis_shrinks_the_dots_in_proportion() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let projection = dots_frame(vec![dot(true, 5, 0.85, 0.3), dot(true, 20, 0.85, 0.7)]);
    let paint = |px_per_price: f64| {
        let style = dots_style(DotSizing {
            native_tape: false,
            tape_column_px: 100.0,
            candle_column_px: 100.0,
            px_per_price,
            typed_full: None,
        });
        radii(&painted(|painter| {
            draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &style));
        }))
    };
    let roomy = paint(100.0);
    // A one-tick level 10 px tall: the largest radius is 5, half of it.
    let squeezed = paint(10.0);
    assert!((squeezed[1] - 5.0).abs() < 1e-3, "{squeezed:?}");
    assert!(
        ((squeezed[0] / squeezed[1]) - (roomy[0] / roomy[1])).abs() < 1e-3,
        "in proportion: {squeezed:?} vs {roomy:?}"
    );
}

fn stable_native_style(spheres: bool) -> OrderflowRenderStyle {
    let mut style = dots_style(DotSizing {
        native_tape: true,
        tape_column_px: 100.0,
        candle_column_px: 100.0,
        px_per_price: 100.0,
        typed_full: Some(Decimal::ONE),
    });
    style.native_tape = true;
    style.live_lane.native_tape = true;
    style.live_lane.stable_size = true;
    style.bubbles.max_radius = 15.0;
    style.bubbles.outline_width = 0.0;
    style.bubbles.show_impact_ring = false;
    style.bubbles.size_reference = quantick_orderflow::BubbleSizeReference::Fixed;
    style.bubbles.size_reference_quantity = 8.0;
    if spheres {
        style.bubbles.detail_min_radius = 0.0;
        style.bubbles.render_mode = BubbleRenderMode::Sphere;
    }
    style
}

/// Read actual circle fills or shaded mesh vertices emitted by the painter.
/// A sphere's separator gives its exact centre; the mesh's farthest vertex
/// gives its filled radius without depending on tessellation segment counts.
fn native_painted_radii(style: &OrderflowRenderStyle, spheres: bool) -> (Vec<f32>, Vec<f32>) {
    use quantick_orderflow::projection::{
        TapeDotGeometry, TapeDotMemory, TapeDotView, TapeHorizontalGeometry,
    };
    use std::cell::RefCell;

    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let mut marks = Vec::new();
    for (index, quantity) in [1, 4, 8].into_iter().enumerate() {
        let y = 0.2 + index as f64 * 0.3;
        let mut tape = dot(true, quantity, 0.8, y);
        tape.agg_id = index as u64 + 1;
        tape.agg_ids = vec![tape.agg_id];
        tape.price = Decimal::from(80 - index as i64 * 30);
        tape.price_bucket = tape.price;
        tape.first_timestamp_ms = 1_000 + index as i64 * 2_000;
        tape.last_timestamp_ms = tape.first_timestamp_ms;
        tape.timestamp_quantity = Decimal::from(tape.first_timestamp_ms) * tape.quantity;
        let mut candle = dot(false, quantity, 0.3, y);
        candle.agg_id = index as u64 + 10;
        candle.agg_ids = vec![candle.agg_id];
        if index == 1 {
            for mark in [&mut tape, &mut candle] {
                mark.side = Side::Sell;
                mark.consumed_side = BookSide::Bid;
                mark.buy_quantity = Decimal::ZERO;
                mark.buy_share = 0.0;
            }
        }
        marks.extend([tape, candle]);
    }
    let original = marks.clone();
    let projection = dots_frame(marks);
    let memory = RefCell::new(TapeDotMemory::default());
    let edge = quantick_orderflow::LiveEdge {
        now_ms: 6_000,
        window_ms: 6_000,
        reference_ms: 6_000,
        on_newest_bar: true,
    };
    let context = RenderContext::new(&projection, layout, style)
        .with_tape_price_range((0.0, 100.0))
        .with_tape_time(edge, 100)
        .with_tape_memory(&memory, None);
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..egui::RawInput::default()
        },
        |ctx| {
            draw_aggression_bubbles(&ctx.layer_painter(egui::LayerId::background()), &context);
        },
    );
    assert_eq!(
        projection.aggressions, original,
        "rendering retains source facts"
    );
    assert_eq!(
        memory.borrow().retained_group_count(),
        3,
        "separated native groups"
    );
    let geometry = TapeHorizontalGeometry::resolve(300.0, 400.0, &style.bubbles);
    let retained = memory.borrow().draw_retained(
        &projection.aggressions,
        TapeDotView {
            now_ms: edge.now_ms,
            window_ms: edge.window_ms,
            dot_window_ms: 100,
            evicted_through_ms: None,
            prices: quantick_orderflow::PriceWindow::new(Decimal::ZERO, Decimal::from(100))
                .unwrap(),
            geometry: TapeDotGeometry {
                left_x: 0.75,
                right_x: 1.0,
                width_px: geometry.span_px,
                height_px: 400.0,
            },
        },
        style.dot_sizing.unwrap(),
        &style.bubbles,
        &style.live_lane,
        &[],
    );
    assert_eq!(
        retained.full_quantity,
        Decimal::ONE,
        "grouping keeps its scale"
    );
    for source in original.iter().filter(|mark| mark.live) {
        let group = retained
            .marks
            .iter()
            .find(|mark| mark.agg_ids == source.agg_ids)
            .expect("each native membership remains separate");
        assert_eq!(group.quantity, source.quantity);
        assert_eq!(group.buy_quantity, source.buy_quantity);
        assert_eq!(group.side, source.side);
        assert_eq!(group.trade_count, source.trade_count);
    }
    let mut separator_center = None;
    let mut tape = Vec::new();
    let mut candles = Vec::new();
    for clipped in output.shapes {
        let (center, radius) = match clipped.shape {
            egui::Shape::Circle(circle) if circle.fill != egui::Color32::TRANSPARENT => {
                assert!(!spheres, "the dressed sphere uses its shaded mesh");
                (circle.center, circle.radius)
            }
            egui::Shape::Circle(circle) => {
                separator_center = Some(circle.center);
                continue;
            }
            egui::Shape::Mesh(mesh) => {
                assert!(spheres, "the cheap dot uses one circle");
                let center = separator_center.take().expect("a sphere has its separator");
                let radius = mesh
                    .vertices
                    .iter()
                    .map(|vertex| vertex.pos.distance(center))
                    .fold(0.0, f32::max);
                (center, radius)
            }
            _ => continue,
        };
        assert!(clipped.clip_rect.contains(center));
        if center.x >= 700.0 {
            tape.push(radius);
        } else {
            candles.push(radius);
        }
    }
    tape.sort_by(f32::total_cmp);
    candles.sort_by(f32::total_cmp);
    assert_eq!(tape.len(), 3, "one drawn disc for each tape group");
    assert_eq!(candles.len(), 3, "one drawn disc for each candle mark");
    (tape, candles)
}

#[test]
fn stable_native_fixed_display_reference_reaches_circle_and_sphere_radii() {
    for spheres in [false, true] {
        let style = stable_native_style(spheres);
        let (tape, candles) = native_painted_radii(&style, spheres);
        for (radius, quantity) in tape.iter().zip([1.0_f32, 4.0, 8.0]) {
            let expected = bubble_radius((quantity / 8.0).sqrt(), 2.0, 15.0);
            assert!(
                (radius - expected).abs() < 0.001,
                "drawn={radius}, expected={expected}"
            );
            assert!(*radius >= style.bubbles.min_radius);
        }
        assert!(tape.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(candles.iter().all(|radius| (radius - 6.0).abs() < 0.001));
    }
}

#[test]
fn fixed_native_small_prints_keep_a_visible_radius_and_quantity_order() {
    let mut style = stable_native_style(false);
    style.bubbles.size_reference_quantity = 800_000.0;
    let (tape, _) = native_painted_radii(&style, false);
    assert!(tape[0] >= style.bubbles.min_radius);
    assert!(tape[2] < style.bubbles.min_radius + 0.01);
    assert!(tape.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn legacy_native_reference_policies_keep_their_painted_radii() {
    for spheres in [false, true] {
        for stable in [false, true] {
            let mut style = stable_native_style(spheres);
            style.live_lane.stable_size = stable;
            if stable {
                style.bubbles.size_reference = quantick_orderflow::BubbleSizeReference::VisibleP99;
            }
            let (tape, candles) = native_painted_radii(&style, spheres);
            assert!(tape.iter().all(|radius| (radius - 15.0).abs() < 0.001));
            assert!(candles.iter().all(|radius| (radius - 6.0).abs() < 0.001));
        }
    }
}
