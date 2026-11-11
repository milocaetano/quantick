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
fn tick_chrome_omits_a_blocked_l2_key_and_an_incomplete_caption() {
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
        assert!(
            flow_caption(
                &painter,
                area,
                70.0,
                None,
                quantick_orderflow::projection::flow_tape::FlowProgress::default(),
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

/// Review #8: a tick caption still says what its spheres leave out, whatever
/// else goes: partial volume and excluded records never vanish.
#[test]
fn a_tick_caption_keeps_its_honesty_notes() {
    use quantick_orderflow::projection::{
        PriceWindow,
        flow_tape::{FlowExecution, FlowReference, FlowTapeView, project_flow_tape},
    };
    let trade = quantick_engine::Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: 100.into(),
        quantity: 3.into(),
        side: quantick_engine::Side::Buy,
    };
    let mut frame = project_flow_tape(
        std::iter::once(FlowExecution {
            ordinal: 0,
            slot: 0,
            accepted_ordinal: 0,
            ticks_per_bar: 50.into(),
            trade: &trade,
            opening: false,
        }),
        7,
        2,
        2,
        FlowTapeView {
            first_slot: 0,
            end_slot: 1,
            clip_left: 0.into(),
            clip_right: 1.into(),
            width_px: 240.0,
            height_px: 240.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::Typed(3.into()),
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            region: Default::default(),
            exclude_opening: false,
        },
    );
    frame.cache_limit_reached = true;
    frame.ineligible_executions = 3;
    let progress = quantick_orderflow::projection::flow_tape::FlowProgress {
        pending: true,
        ..Default::default()
    };
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 240.0));
    let ctx = egui::Context::default();
    let mut bounds = None;
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        bounds = flow_caption(
            &ctx.layer_painter(egui::LayerId::background()),
            area,
            LEGEND_HEADER_CLEARANCE_PX,
            Some(&frame),
            progress,
        );
    });
    let painted: Vec<String> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    let text = painted.join("\n");
    assert!(text.contains("Partial regional volume"), "{text}");
    assert!(text.contains("3 records excluded"), "{text}");
    let bounds = bounds.expect("the notes report where they landed");
    assert!(area.contains_rect(bounds));
}
