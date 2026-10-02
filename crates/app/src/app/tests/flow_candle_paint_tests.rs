//! Actual FLOW frame layering respects generic barcolor and configured candle paint.
use super::*;
use crate::indicator_worker::IndicatorEvent;
use quantick_indicators::{IndicatorDescriptor, PreviewFrame, Rgba8};

fn painted_bodies(output: &egui::FullOutput, colour: egui::Color32) -> Vec<(usize, egui::Rect)> {
    output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(index, clipped)| {
            let egui::Shape::Rect(rect) = &clipped.shape else {
                return None;
            };
            (rect.fill == colour && rect.stroke == egui::Stroke::NONE && rect.rect.is_positive())
                .then_some((index, rect.rect))
        })
        .collect()
}

#[test]
fn flow_keeps_configured_fills_indicator_alpha_and_one_foreground_candle_pass() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(23);
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(5));
    app.active_tab_mut().apply_spec_changes();
    native_split(&mut app);
    app.active_tab_mut().flow_pane.set_layer_visible(
        ChartLayer::Footprint,
        false,
        &mut Default::default(),
    );
    app.style.candles.body_mode = crate::style::CandleBodyMode::Filled;
    app.style.candles.fill_opacity = 0.6;
    app.style.candles.forming_opacity = 0.5;
    settled_frame(&mut app, &ctx);
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.viewport.set_px_per_bar(45.0);
    assert!(pane.price_view.set_manual_range(99.0, 103.0));
    assert_eq!(pane.state.bars().len(), 4);
    assert!(pane.state.partial().is_some());
    let slot = pane.indicators.allocate_slot("paint.fixture");
    pane.indicators.apply(IndicatorEvent::Rebuilt {
        slot,
        descriptor: IndicatorDescriptor {
            title: "Paint fixture".into(),
            short_title: None,
            overlay: true,
            plots: Vec::new(),
            inputs: Vec::new(),
            fills: Vec::new(),
        },
        columns: Vec::new(),
        bar_paint: vec![
            Some(Rgba8::opaque(231, 41, 177)),
            None,
            Some(Rgba8 {
                r: 19,
                g: 73,
                b: 223,
                a: 128,
            }),
            None,
        ],
        inputs: Vec::new(),
        rows: 4,
        stale: None,
    });
    let mut preview = PreviewFrame::new(Vec::new());
    preview.paint = Some(Rgba8 {
        r: 231,
        g: 147,
        b: 31,
        a: 128,
    });
    pane.indicators.apply(IndicatorEvent::Preview {
        slot,
        frame: Some(preview),
    });
    let output = settled_flow_frame(&mut app, &ctx);
    let circles = regional_circles(&app, &output);
    assert!(!circles.is_empty());
    let facts = app
        .active_tab()
        .tape()
        .flow_execution_frame()
        .unwrap()
        .dots
        .clone();
    assert_eq!(
        facts
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<rust_decimal::Decimal>(),
        23.into()
    );
    let last_circle = output
        .shapes
        .iter()
        .rposition(|shape| {
            matches!(&shape.shape,
        egui::Shape::Mesh(mesh) if circles.contains(mesh))
        })
        .unwrap();
    let rim_colours = [
        egui::Color32::from_rgb(112, 185, 244),
        egui::Color32::from_rgb(232, 175, 99),
    ];
    assert!(
        !output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Mesh(mesh) if !mesh.vertices.is_empty()
            && mesh.vertices.iter().all(|vertex| rim_colours.contains(&vertex.color)))),
        "ordinary FLOW regions have no opaque foreground rims"
    );
    let colours = [
        egui::Color32::from_rgba_unmultiplied(231, 41, 177, 153),
        egui::Color32::from_rgba_unmultiplied(19, 73, 223, 76),
        egui::Color32::from_rgba_unmultiplied(231, 147, 31, 38),
    ];
    for colour in colours {
        let bodies = painted_bodies(&output, colour);
        assert_eq!(
            bodies.len(),
            1,
            "one filled body at configured/script/forming alpha: {colour:?}"
        );
        assert!(
            bodies[0].0 > last_circle,
            "regional circles precede the normal filled candle"
        );
        let rect = bodies[0].1;
        let outlines = output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape,
            egui::Shape::Rect(outline) if outline.rect==rect && outline.stroke.width>0.0)
            })
            .count();
        assert_eq!(
            outlines, 1,
            "a second silhouette must not compound candle alpha"
        );
    }
    assert!(
        regional_labels(&output).is_empty(),
        "exact quantities stay on passive hover"
    );
    // None/hidden paint keeps the ordinary direction fill, without altering regions.
    app.active_tab_mut()
        .flow_pane
        .indicators
        .view_mut(slot)
        .unwrap()
        .hidden = true;
    let hidden = settled_flow_frame(&mut app, &ctx);
    for colour in colours {
        assert!(painted_bodies(&hidden, colour).is_empty());
    }
    assert_eq!(regional_circles(&app, &hidden), circles);
    assert_eq!(
        app.active_tab().tape().flow_execution_frame().unwrap().dots,
        facts
    );
    let pane = &app.active_tab().flow_pane;
    let plain = pane
        .state
        .bars()
        .iter()
        .filter(|bar| bar.close >= bar.open)
        .count();
    let normal = app.style.candles.resolved(true, false).fill.unwrap();
    let normal = egui::Color32::from_rgba_unmultiplied(normal[0], normal[1], normal[2], normal[3]);
    let history = pane
        .frame
        .chart_rect
        .unwrap()
        .with_max_x(pane.frame.lane_divider_x.unwrap());
    assert_eq!(
        painted_bodies(&hidden, normal)
            .iter()
            .filter(|(_, rect)| history.contains(rect.center()))
            .count(),
        plain
    );
    // OutlineOnly is an intentional preference; barcolor must never invent a fill.
    app.active_tab_mut()
        .flow_pane
        .indicators
        .view_mut(slot)
        .unwrap()
        .hidden = false;
    app.style.candles.body_mode = crate::style::CandleBodyMode::OutlineOnly;
    let outline = settled_flow_frame(&mut app, &ctx);
    for colour in colours {
        assert!(painted_bodies(&outline, colour).is_empty());
    }
    assert_eq!(regional_circles(&app, &outline), circles);
    app.active_tab_mut().flow_pane.indicators.remove(slot);
    let removed = settled_flow_frame(&mut app, &ctx);
    assert_eq!(regional_circles(&app, &removed), circles);

    assert_footprint_layering(&mut app, &ctx, circles, facts, history, plain);
}

fn assert_footprint_layering(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    circles: Vec<egui::Mesh>,
    facts: Vec<quantick_orderflow::projection::flow_tape::FlowTapeDot>,
    history: egui::Rect,
    plain: usize,
) {
    // Footprint keeps its existing candle dressing, but completes before the
    // neutral backing and sectors. Exactly one normal candle pass stays last.
    app.style.candles.body_mode = crate::style::CandleBodyMode::Filled;
    app.style.canvas.background = [13, 17, 23];
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.footprint.config = Some(crate::footprint_config::FootprintConfig {
        style: crate::footprint_config::FootprintStyle::Split,
        ..Default::default()
    });
    pane.set_layer_visible(ChartLayer::Footprint, true, &mut Default::default());
    let footprint = settled_flow_frame(app, ctx);
    assert_eq!(regional_circles(app, &footprint), circles);
    assert_eq!(
        app.active_tab().tape().flow_execution_frame().unwrap().dots,
        facts
    );
    let region_indices: Vec<_> = footprint
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(i, shape)| {
            matches!(&shape.shape, egui::Shape::Mesh(mesh) if circles.contains(mesh)).then_some(i)
        })
        .collect();
    let first_region = region_indices[0];
    let last_region = *region_indices.last().unwrap();
    for index in region_indices {
        let egui::Shape::Circle(backing) = &footprint.shapes[index - 1].shape else {
            panic!("footprint sectors must have an immediately preceding backing")
        };
        assert_eq!(backing.fill, egui::Color32::from_rgb(13, 17, 23));
        assert_eq!(backing.stroke, egui::Stroke::NONE);
        assert!(facts.iter().any(|dot| dot.radius == backing.radius));
    }
    let profiles: Vec<_> = footprint.shapes.iter().enumerate().filter_map(|(i, shape)| {
        matches!(&shape.shape, egui::Shape::Rect(rect) if history.contains(rect.rect.center()) &&
            [0.60, 0.95].iter().any(|alpha| rect.fill == egui::Color32::from_gray(0xD8).gamma_multiply(*alpha))
        ).then_some(i)
    }).collect();
    assert!(
        !profiles.is_empty(),
        "the fixture must actually paint footprint profiles"
    );
    assert!(profiles.iter().all(|i| *i < first_region));
    let (_, dressed) = crate::footprint_render::candle_dressing(
        true,
        crate::footprint_config::CandleTreatment::Fade,
        app.active_tab().flow_pane.viewport.candle_width(),
        app.style.candles,
    );
    let dressed = dressed.unwrap_or(app.style.candles);
    assert!(dressed.fill_opacity > 0.0 && dressed.fill_opacity < app.style.candles.fill_opacity);
    let [r, g, b, a] = dressed.resolved(true, false).fill.unwrap();
    let bodies: Vec<_> = painted_bodies(
        &footprint,
        egui::Color32::from_rgba_unmultiplied(r, g, b, a),
    )
    .into_iter()
    .filter(|(_, rect)| history.contains(rect.center()))
    .collect();
    assert_eq!(
        bodies.len(),
        plain,
        "the inherited dressed body fill paints once"
    );
    assert!(bodies.iter().all(|(index, _)| *index > last_region));
    let outline_colors = [true, false].map(|up| {
        let [r, g, b, a] = dressed.resolved(up, false).outline;
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    });
    let outlines: Vec<_> = footprint.shapes.iter().enumerate().filter_map(|(index, shape)| {
        matches!(&shape.shape, egui::Shape::Rect(rect) if
                history.contains(rect.rect.center()) && outline_colors.contains(&rect.stroke.color) && rect.stroke.width > 0.0
        ).then_some(index)
    }).collect();
    assert_eq!(outlines.len(), 4, "each closed candle outline paints once");
    assert!(outlines.iter().all(|index| *index > last_region));
}

#[test]
fn flow_legend_keeps_native_swatches_in_tape_and_preserves_depth_and_hide_switches() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    let keys = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                let label = text.galley.job.text.clone();
                ([
                    "Buy",
                    "Sell",
                    "buy aggression",
                    "sell aggression",
                    "liquidity",
                ]
                .contains(&label.as_str()))
                .then_some((label, text.pos))
            })
            .collect::<Vec<_>>()
    };
    let frame = settled_flow_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane.frame;
    let divider = pane.lane_divider_x.unwrap();
    let top = pane.chart_rect.unwrap().top();
    let shown: Vec<_> = keys(&frame)
        .into_iter()
        .filter(|(label, _)| label != "liquidity")
        .collect();
    assert_eq!(
        shown.len(),
        2,
        "only compact native Buy/Sell aggression keys"
    );
    assert!(
        shown
            .iter()
            .all(|(label, pos)| ["Buy", "Sell"].contains(&label.as_str())
                && pos.x > divider
                && pos.y >= top
                && pos.y < top + 22.0)
    );
    app.active_tab_mut().tape_mut().set_enabled(true, 10);
    let depth = settled_flow_frame(&mut app, &ctx);
    assert!(
        keys(&depth)
            .iter()
            .any(|(label, pos)| label == "liquidity" && pos.x < divider)
    );
    app.active_tab_mut().tape_mut().set_legend_visible(false);
    assert!(keys(&settled_flow_frame(&mut app, &ctx)).is_empty());
    app.active_tab_mut().tape_mut().set_legend_visible(true);
    app.active_tab_mut().flow_pane.set_layer_visible(
        ChartLayer::TapeChart,
        false,
        &mut Default::default(),
    );
    assert!(
        keys(&settled_flow_frame(&mut app, &ctx))
            .iter()
            .all(|(label, _)| label != "Buy" && label != "Sell")
    );
}
