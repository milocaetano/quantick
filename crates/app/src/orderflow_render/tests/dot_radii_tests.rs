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
