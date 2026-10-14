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
            assert_eq!(
                key.top(),
                inset + LEGEND_OUTER_MARGIN_PX - LEGEND_BORDER_WIDTH_PX / 2.0
            );
            assert_eq!(caption.top(), key.bottom() + 4.0);
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
fn a_replay_tick_caption_yields_when_its_header_is_occupied() {
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
    let output = ctx.run(egui::RawInput::default(), |ctx| {
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
    assert!(bounds.is_none());
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("FLOW"))));
}

#[test]
fn quiet_narrow_tick_captions_stay_in_the_header_clear_of_candles_and_current_price() {
    // Logical coordinates reproduce the v3 quiet narrow captures at 1.5x scale:
    // price content occupies the right of the history pane while the left header is free.
    for (candle, highest) in [
        (
            egui::Rect::from_min_max(egui::pos2(442.0, 52.0), egui::pos2(450.0, 332.0)),
            36.0,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(426.0, 60.0), egui::pos2(434.0, 188.0)),
            38.0,
        ),
    ] {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 700.0));
        let regions: Vec<_> = (0..12)
            .map(|index| {
                let disc = FlowDisc {
                    center: [430.0, highest + index as f32 * 50.0],
                    radius: 24.0,
                };
                FlowRegionVisual {
                    dot_index: index,
                    source_disc: disc,
                    disc,
                    role: FlowRegionRole::Peak,
                    unresolved_overlap: false,
                }
            })
            .collect();
        let price = egui::Rect::from_min_max(egui::pos2(0.0, 612.0), egui::pos2(480.0, 613.0));
        let top = 28.0;
        let mut caption = None;
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            painter.rect_filled(candle, egui::Rounding::ZERO, egui::Color32::RED);
            painter.line_segment(
                [price.left_top(), price.right_top()],
                egui::Stroke::new(1.0_f32, egui::Color32::LIGHT_BLUE),
            );
            caption = flow_caption(
                &painter,
                area,
                top,
                None,
                quantick_orderflow::projection::flow_tape::FlowProgress::default(),
                true,
                Some(&regions),
            );
        });
        let caption = caption.expect("the quiet capture's upper-left header has room");
        assert_eq!(
            caption.top(),
            top,
            "chrome must never migrate through price content"
        );
        assert!(caption.width() <= quantick_chart::flow_execution::FLOW_CHROME_MAX_WIDTH_PX);
        assert!(caption.height() <= quantick_chart::flow_execution::FLOW_CHROME_MAX_HEIGHT_PX);
        assert!(area.contains_rect(caption));
        assert!(!caption.intersects(candle) && !caption.intersects(price));
        assert!(regions.iter().all(|region| {
            !region
                .disc
                .visible([caption.min.into(), caption.max.into()])
        }));
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.contains("FLOW") => Some(text),
                _ => None,
            })
            .expect("the complete caption is painted");
        assert!(
            text.galley
                .job
                .text
                .contains("quantity in BTC; sphere area follows gross volume")
        );
        assert_eq!(
            egui::Rect::from_min_size(text.pos, text.galley.size()),
            caption
        );
        assert!(
            output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect == candle)
            )
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment { points, .. } if points == &[price.left_top(), price.right_top()])));
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
                true,
                Some(&[])
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

/// Review #12: the tick caption's Buy and Sell keys wear the pane's theme.
#[test]
fn the_tick_caption_key_follows_the_pane_theme() {
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 400.0));
    let theme = quantick_orderflow::HeatmapTheme::ColorBlind;
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        flow_caption(
            &ctx.layer_painter(egui::LayerId::background()),
            area,
            LEGEND_HEADER_CLEARANCE_PX,
            None,
            quantick_orderflow::projection::flow_tape::FlowProgress::default(),
            true,
            Some(&[]),
            theme,
        );
    });
    let palette = crate::orderflow_render::palette_for_theme(theme);
    let job = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("FLOW") => {
                Some(text.galley.job.clone())
            }
            _ => None,
        })
        .expect("the caption is painted");
    let colour_of = |word: &str| {
        job.sections
            .iter()
            .find(|section| &job.text[section.byte_range.clone()] == word)
            .map(|section| section.format.color)
    };
    assert_eq!(colour_of("Buy"), Some(palette.buy));
    assert_eq!(colour_of("Sell"), Some(palette.sell));
}
