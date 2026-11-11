use super::*;
use quantick_chart::flow_execution::{FlowDisc, FlowRegionRole, FlowRegionVisual};

#[test]
fn tick_chrome_keeps_live_l2_wrapping_and_header_stacks_at_the_preferred_top() {
    for width in [240.0, 600.0] {
        for inset in [
            LEGEND_HEADER_CLEARANCE_PX,
            LEGEND_HEADER_CLEARANCE_PX + 60.0,
        ] {
            let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 400.0));
            let viewport = Viewport::new();
            let layout = ProjectedLayout::new(area, &viewport, 2, 0, 2, 0.0);
            let projection = HeatmapProjection::empty(
                true,
                quantick_orderflow::EffectiveGrouping::resolve(
                    quantick_orderflow::DisplayGrouping::Native,
                    Decimal::ONE,
                    Decimal::from(100),
                ),
            );
            let style = OrderflowRenderStyle {
                legend_top_inset: inset,
                aggression_layer: false,
                lane_aggression_layer: false,
                ..OrderflowRenderStyle::default()
            };
            let disc = FlowDisc {
                center: [width / 2.0, inset + 220.0],
                radius: 24.0,
            };
            let regions = [FlowRegionVisual {
                dot_index: 0,
                source_disc: disc,
                disc,
                role: FlowRegionRole::Peak,
                unresolved_overlap: false,
            }];
            let mut key = None;
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                key = draw_compact_legend_avoiding(
                    &painter,
                    &RenderContext::new(&projection, layout, &style),
                    Some(&regions),
                );
            });
            let key = key.expect("the live L2 key remains visible");
            assert_eq!(
                key.top(),
                inset + LEGEND_OUTER_MARGIN_PX - LEGEND_BORDER_WIDTH_PX / 2.0
            );
            assert!(area.contains_rect(key));
            assert!(!disc.visible([key.min.into(), key.max.into()]));
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("liquidity"))));
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("FLOW"))));
            assert_eq!(regions[0].disc, disc);
        }
    }
}

#[test]
fn tick_chrome_omits_a_blocked_l2_key() {
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 80.0));
    let viewport = Viewport::new();
    let layout = ProjectedLayout::new(area, &viewport, 2, 0, 2, 0.0);
    let projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            Decimal::ONE,
            Decimal::from(100),
        ),
    );
    let style = OrderflowRenderStyle {
        aggression_layer: false,
        lane_aggression_layer: false,
        ..OrderflowRenderStyle::default()
    };
    let disc = FlowDisc {
        center: [100.0, 40.0],
        radius: 24.0,
    };
    let regions = [FlowRegionVisual {
        dot_index: 0,
        source_disc: disc,
        disc,
        role: FlowRegionRole::Peak,
        unresolved_overlap: false,
    }];
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        assert!(
            draw_compact_legend_avoiding(
                &painter,
                &RenderContext::new(&projection, layout, &style),
                Some(&regions)
            )
            .is_none()
        );
    });
    assert!(
        output
            .shapes
            .iter()
            .all(|shape| !matches!(&shape.shape, egui::Shape::Text(_)))
    );
}
