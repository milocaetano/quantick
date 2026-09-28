//! A pane in tape-only mode: the tape fills the canvas and the candles'
//! marks are not drawn, while the tape's own marks are drawn exactly as a
//! normal pane draws them.

use super::*;
use quantick_orderflow::DotSizing;

/// A volume dot on `live`'s pane holding `quantity` contracts, at `(x, y)`.
fn mark(agg_id: u64, live: bool, quantity: i64, x: f64, y: f64) -> AggressionPrimitive {
    AggressionPrimitive {
        agg_id,
        agg_ids: vec![agg_id],
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

/// The painter's style for `config`, drawing cheap dots only (one circle
/// each) on a fixed cell so the radii are comparable across panes.
fn style_for(config: &quantick_orderflow::HeatmapConfig) -> OrderflowRenderStyle {
    let mut style = OrderflowRenderStyle::from_config(config, egui::Color32::BLACK);
    style.bubbles = BubbleStyle {
        min_radius: 2.0,
        max_radius: 10.0,
        detail_min_radius: 100.0,
        hollow_small_buys: false,
        halo_strength: 0.0,
        trail_length: 0.0,
        show_quantity_labels: false,
        show_trade_count: false,
        ..BubbleStyle::default()
    };
    style.dot_sizing = Some(DotSizing {
        tape_column_px: 100.0,
        candle_column_px: 100.0,
        px_per_price: 100.0,
        typed_full: None,
    });
    style
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

/// The tape-only pane draws no candle mark and no candle dot; its tape
/// marks are the ones a normal pane draws, at the same sizes.
#[test]
fn a_tape_only_pane_draws_the_tape_and_no_candle_marks() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
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
        mark(1, false, 2, 0.3, 0.3),
        mark(2, false, 8, 0.3, 0.7),
        mark(3, true, 5, 0.85, 0.3),
        mark(4, true, 20, 0.85, 0.7),
    ];
    let mut config = quantick_orderflow::HeatmapConfig::default();
    config.volume_dots.enabled = true;
    let normal = style_for(&config);
    config.live_lane.tape_only = true;
    let tape_only = style_for(&config);
    assert!(!tape_only.aggression_layer, "no candle marks in tape only");
    assert!(!tape_only.depth_layer, "and no depth behind candles");
    assert!(tape_only.lane_aggression_layer, "the tape still draws");

    let normal_layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    // Tape only: the lane is the whole chart.
    let tape_layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, rect.width());
    assert_eq!(tape_layout.lane_left_x(), Some(rect.left()));
    assert_eq!(tape_layout.lane_rect(), rect, "the tape fills the canvas");

    let listed = |style: &OrderflowRenderStyle, layout| {
        RenderContext::new(&projection, layout, style)
            .bubbles()
            .map(|mark| (mark.agg_id, mark.live))
            .collect::<Vec<_>>()
    };
    let normal_live: Vec<_> = listed(&normal, normal_layout)
        .into_iter()
        .filter(|(_, live)| *live)
        .collect();
    assert_eq!(listed(&tape_only, tape_layout), normal_live);
    assert_eq!(normal_live, vec![(3, true), (4, true)]);

    let paint = |style: &OrderflowRenderStyle, layout| {
        radii(&painted(|painter| {
            draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, style));
        }))
    };
    let normal_radii = paint(&normal, normal_layout);
    let tape_radii = paint(&tape_only, tape_layout);
    assert_eq!(normal_radii.len(), 4, "{normal_radii:?}");
    assert_eq!(tape_radii, normal_radii[2..].to_vec(), "the tape's own sizes");
}

/// The divider sits on the chart's left edge when the tape takes the whole
/// width, and a lane wider than the chart is still no lane.
#[test]
fn a_whole_width_lane_puts_the_divider_on_the_left_edge() {
    let rect = egui::Rect::from_min_max(egui::pos2(60.0, 0.0), egui::pos2(1060.0, 400.0));
    assert_eq!(lane_divider_x(rect, 1_000.0), Some(60.0));
    assert_eq!(lane_divider_x(rect, 1_000.5), None);
    assert_eq!(lane_divider_x(rect, 350.0), Some(710.0));
}
