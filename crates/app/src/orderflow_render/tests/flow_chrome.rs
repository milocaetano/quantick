use super::*;
use quantick_chart::flow_execution::{FlowDisc, FlowRegionRole, FlowRegionVisual};

#[test]
fn tick_chrome_avoids_spheres_with_live_l2_wrapping_and_header_stacks() {
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
                center: [width / 2.0, inset + 18.0],
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
            let mut caption = None;
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                key = draw_compact_legend_avoiding(
                    &painter,
                    &RenderContext::new(&projection, layout, &style),
                    Some(&regions),
                );
                caption = flow_caption(
                    &painter,
                    area,
                    key.unwrap().bottom() + 4.0,
                    None,
                    quantick_orderflow::projection::flow_tape::FlowProgress::default(),
                    true,
                    Some(&regions),
                );
            });
            let key = key.expect("the live L2 key remains visible");
            let caption = caption.expect("the FLOW caption remains visible");
            assert!(key.top() >= inset);
            assert!(area.contains_rect(key) && area.contains_rect(caption));
            assert!(!key.intersects(caption));
            for bounds in [key, caption] {
                assert!(!disc.visible([bounds.min.into(), bounds.max.into()]));
            }
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("liquidity"))));
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("FLOW"))));
            assert_eq!(regions[0].disc, disc);
        }
    }
}

#[test]
fn a_replay_tick_caption_clears_the_highest_sphere_without_a_depth_key() {
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 240.0));
    let disc = FlowDisc {
        center: [120.0, 44.0],
        radius: 22.0,
    };
    let regions = [FlowRegionVisual {
        dot_index: 0,
        source_disc: disc,
        disc,
        role: FlowRegionRole::Peak,
        unresolved_overlap: false,
    }];
    let ctx = egui::Context::default();
    let mut bounds = None;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        bounds = flow_caption(
            &ctx.layer_painter(egui::LayerId::background()),
            area,
            LEGEND_HEADER_CLEARANCE_PX,
            None,
            quantick_orderflow::projection::flow_tape::FlowProgress::default(),
            true,
            Some(&regions),
        );
    });
    let bounds = bounds.unwrap();
    assert!(bounds.top() > disc.center[1] + disc.radius);
    assert!(area.contains_rect(bounds));
}
