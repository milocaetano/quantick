//! A pane in tape-only mode: the tape fills the canvas and the candles'
//! marks are not drawn, while the tape's own marks are drawn exactly as a
//! normal pane draws them.

use super::*;
use quantick_orderflow::DotSizing;

/// A volume dot on `live`'s pane holding `quantity` contracts, at `(x, y)`.
pub(super) fn mark(agg_id: u64, live: bool, quantity: i64, x: f64, y: f64) -> AggressionPrimitive {
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

/// The painter's style for `config`, drawing cheap dots only (one circle
/// each) on a fixed cell so the radii are comparable across panes.
pub(super) fn style_for(config: &quantick_orderflow::HeatmapConfig) -> OrderflowRenderStyle {
    let mut style = OrderflowRenderStyle::from_config(config, egui::Color32::BLACK.to_array());
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
/// marks preserve their quantities, with area exactly proportional to volume.
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
    config.show_aggressions = true;
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
    assert_eq!(
        tape_radii,
        vec![5.0, 10.0],
        "four times the volume has twice the radius"
    );
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

#[test]
fn borrowing_all_visible_marks_and_copying_filtered_marks_obey_the_same_visibility_policy() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, rect.width());
    let mut config = quantick_orderflow::HeatmapConfig::default();
    config.volume_dots.enabled = true;
    config.live_lane.tape_only = true;
    config.show_aggressions = true;
    for include_hidden_candle in [false, true] {
        let mut projection = HeatmapProjection::empty(
            true,
            quantick_orderflow::EffectiveGrouping::resolve(
                quantick_orderflow::DisplayGrouping::Native,
                Decimal::ONE,
                Decimal::from(80),
            ),
        );
        projection.volume_dots = true;
        for (id, price, buy_share, live) in [
            (1, 100, 1.0, true),
            (2, 120, 0.0, true),
            (3, 140, 0.5, true),
            (4, 160, 1.0, false),
        ] {
            if !live && !include_hidden_candle {
                continue;
            }
            let mut source = mark(id, live, 4, 0.9, 0.5);
            source.price = Decimal::from(price);
            source.price_bucket = source.price;
            source.buy_share = buy_share;
            source.buy_quantity = Decimal::from(if id == 2 {
                0
            } else if id == 3 {
                2
            } else {
                4
            });
            if id == 2 {
                source.side = Side::Sell;
                source.consumed_side = BookSide::Bid;
            }
            source.first_timestamp_ms = 3_011 + id as i64 * 200;
            source.last_timestamp_ms = source.first_timestamp_ms;
            source.timestamp_quantity = Decimal::from(source.first_timestamp_ms) * source.quantity;
            projection.aggressions.push(source);
        }
        let original = projection.aggressions.clone();
        for (buy, sell, lane, expected) in [
            (true, true, true, vec![1, 2, 3]),
            (true, false, true, vec![1]),
            (false, true, true, vec![2]),
            (false, false, true, vec![]),
            (true, true, false, vec![]),
        ] {
            let mut style = style_for(&config);
            style.show_buy = buy;
            style.show_sell = sell;
            style.lane_aggression_layer = lane;
            let memory =
                std::cell::RefCell::new(quantick_orderflow::projection::TapeDotMemory::default());
            let context = RenderContext::new(&projection, layout, &style)
                .with_tape_time(
                    quantick_orderflow::LiveEdge {
                        now_ms: 5_000,
                        window_ms: 30_000,
                        reference_ms: 30_000,
                        on_newest_bar: true,
                    },
                    100,
                )
                .with_tape_price_range((90.0, 170.0))
                .with_tape_memory(&memory);
            assert_eq!(
                context
                    .bubbles()
                    .map(|mark| mark.agg_id)
                    .collect::<Vec<_>>(),
                expected
            );
            let drawn = radii(&painted(|painter| {
                draw_aggression_bubbles(painter, &context)
            }));
            assert_eq!(
                drawn.len(),
                expected.len(),
                "mixed and hidden-pane marks stay filtered"
            );
            assert_eq!(memory.borrow().retained_group_count(), expected.len());
            assert_eq!(
                projection.aggressions, original,
                "the source frame remains available to other layers"
            );
        }
    }
}

#[test]
fn the_tape_key_fits_the_external_header_without_covering_any_print() {
    let viewport = Viewport::new();
    let projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            Decimal::ONE,
            Decimal::from(100),
        ),
    );
    let mut config = quantick_orderflow::HeatmapConfig::default();
    config.live_lane.tape_only = true;
    config.live_lane.show_aggressions = true;
    config.live_lane.show_depth = false;
    config.bubbles.buy_color = Some([31, 141, 229]);
    config.bubbles.sell_color = Some([247, 117, 24]);
    let mut style = style_for(&config);
    style.bubbles.buy_color = config.bubbles.buy_color;
    style.bubbles.sell_color = config.bubbles.sell_color;
    for (width, height) in [(281.0, 700.0), (500.0, 200.0)] {
        let area = egui::Rect::from_min_size(egui::pos2(30.0, 50.0), egui::vec2(width, height));
        let plot = crate::plot_area::plot_split(area, 0.0, &[]).chart;
        let header = egui::Rect::from_min_max(
            egui::pos2(plot.left(), area.top()),
            plot.left_top() + egui::vec2(plot.width(), 0.0),
        );
        let layout = ProjectedLayout::new(header, &viewport, 2, 0, 2, header.width());
        let ctx = egui::Context::default();
        let mut bounds = None;
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            bounds = draw_compact_legend(
                &ctx.layer_painter(egui::LayerId::background()),
                &RenderContext::new(&projection, layout, &style),
            );
        });
        let bounds = bounds.expect("the narrow tape keeps its buy/sell key visible");
        assert!(header.contains_rect(bounds), "{header:?} / {bounds:?}");
        assert!(
            bounds.bottom() <= plot.top(),
            "the key never covers a print"
        );
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, ["Buy", "Sell"]);
        for color in [
            config.bubbles.buy_color.unwrap(),
            config.bubbles.sell_color.unwrap(),
        ] {
            let expected = egui::Color32::from_rgb(color[0], color[1], color[2]);
            assert!(
                output.shapes.iter().any(|shape| matches!(
                    &shape.shape,
                    egui::epaint::Shape::Circle(circle)
                        if circle.fill == expected || circle.stroke.color == expected
                )),
                "the key follows the trader's bubble colors"
            );
        }
        for shape in &output.shapes {
            let painted = shape
                .shape
                .visual_bounding_rect()
                .intersect(shape.clip_rect);
            if painted.is_positive() {
                assert!(
                    header.contains_rect(painted),
                    "all key ink stays outside the plot"
                );
            }
        }
    }
}
