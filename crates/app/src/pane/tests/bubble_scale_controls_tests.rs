use super::*;
use quantick_engine::{BarSpec, Side, Trade};

fn pane() -> ChartPane {
    let mut pane = ChartPane::flow(73, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.orderflow.as_mut().unwrap().edit_config(|config| {
        config.show_aggressions = true;
        config.volume_dots.enabled = true;
        config.live_lane.native_tape = true;
        config.live_lane.enabled = true;
        config.live_lane.show_aggressions = true;
    });
    pane
}

#[test]
fn enabled_empty_tick_and_tape_have_independent_default_controls() {
    let mut pane = pane();
    let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 500.0));
    assert!(pane.state.trades().is_empty());
    let [candle, tape] = layout(&pane, chart);
    assert_eq!(candle.unwrap().value, 1.0);
    assert_eq!(tape.unwrap().value, 1.0);
    assert!(candle.unwrap().rect.right() < tape.unwrap().rect.left());
    pane.orderflow.as_mut().unwrap().edit_config(|config| {
        config.live_lane.native_tape = false;
        config.volume_dots.enabled = false;
        config.volume_dots.tick_bubbles = None;
    });
    assert!(
        layout(&pane, chart).iter().all(Option::is_some),
        "legacy tick and Tape bubbles keep their controls"
    );
    let owner = pane.orderflow.as_mut().unwrap();
    assert!(
        owner.cached_config().volume_dots.tick_bubbles.is_none(),
        "legacy WIN-style regions are eligible too"
    );
    owner.set_bubble_display_scale(BubbleScaleTarget::Candle, 2.0);
    let [candle, tape] = layout(&pane, chart);
    assert_eq!(candle.unwrap().value, 2.0);
    assert_eq!(tape.unwrap().value, 1.0);
    pane.orderflow.as_mut().unwrap().set_bubbles_enabled(false);
    let [candle, tape] = layout(&pane, chart);
    assert!(candle.is_none());
    assert!(tape.is_some());
    pane.orderflow
        .as_mut()
        .unwrap()
        .set_lane_bubbles_enabled(false);
    assert!(layout(&pane, chart).iter().all(Option::is_none));
}

#[test]
fn controls_stay_inside_their_surface_and_below_the_header() {
    for width in [60.0, 100.0, 185.0, 500.0] {
        let region = egui::Rect::from_min_size(egui::pos2(300.0, 50.0), egui::vec2(width, 400.0));
        let rect = control_rect(region).unwrap();
        assert!(region.contains_rect(rect));
        assert!(rect.top() > super::super::tape_switch_rect(region, false).bottom());
    }
    assert!(
        control_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(30.0, 400.0)
        ))
        .is_none()
    );
}

fn drive(
    pane: &mut ChartPane,
    ctx: &egui::Context,
    area: egui::Rect,
    events: Vec<egui::Event>,
) -> [Option<egui::Rect>; 2] {
    drive_with(pane, ctx, area, events, egui::Modifiers::NONE, 1.0).2
}

fn drive_with(
    pane: &mut ChartPane,
    ctx: &egui::Context,
    area: egui::Rect,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    density: f32,
) -> (bool, Vec<egui::Rect>, [Option<egui::Rect>; 2]) {
    let mut toolrail = crate::toolrail::ToolRail::new();
    let presets = crate::drawings::presets::PresetStore::load_from(
        crate::scratch::thread_dir("bubble-scale-presets").join("not-present.toml"),
    );
    let mut begin_text_edit = false;
    let style = crate::style::ChartStyle::default();
    let mut paper = crate::paper_trading::PaperTrading::with_trades_dir(
        crate::scratch::thread_dir("bubble-scale-paper"),
    );
    paper.set_symbol("TESTUSDT");
    paper.set_cmd_trading(crate::paper_trading::CmdTradingSettings {
        enabled: true,
        ..Default::default()
    });
    paper.on_trade(&Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: 100.into(),
        quantity: 1.into(),
        side: Side::Buy,
    });
    let footprint = crate::footprint_config::FootprintConfig::default();
    let mut layers = quantick_layers::LayerActions::default();
    let mut drawing_chrome = crate::surfaces::DrawingChromeSurface::default();
    pane.frame.auto_range.get_or_insert((90.0, 110.0));
    let mut input = egui::RawInput {
        screen_rect: Some(area),
        events,
        modifiers,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .native_pixels_per_point = Some(density);
    let mut controls = [None, None];
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut chrome = PaneChrome {
                tab: 1,
                side: super::super::PaneSide::Flow,
                toolrail: &mut toolrail,
                presets: &presets,
                drawing_chrome: &mut drawing_chrome,
                begin_text_edit: &mut begin_text_edit,
                style: &style,
                tz: crate::timezone::TzOffset::default(),
                feed_gaps: &[],
                symbol: "TESTUSDT",
                paper: &mut paper,
                paper_takes_input: true,
                paper_hud_here: false,
                shared_pick: None,
                shared: Default::default(),
                capabilities: crate::config::FeedCapabilities::none(),
                side_inferred: false,
                footprint: &footprint,
                layers: &mut layers,
            };
            controls = show(pane, ui, area, &chrome);
            // read_response must run inside this pass; after Context::run it can
            // read the buffer for the previous pass instead of these widgets.
            let chart = pane.plot_areas(area, chrome.capabilities).chart;
            for control in layout(pane, chart).into_iter().flatten() {
                let id = pane.interaction_id(match control.target {
                    BubbleScaleTarget::Candle => "candle_bubble_scale",
                    BubbleScaleTarget::Tape => "tape_bubble_scale",
                });
                let registered = ctx.read_response(id.with("move")).unwrap();
                assert!(
                    control
                        .rect
                        .expand(1.0)
                        .contains_rect(registered.interact_rect),
                    "planned {:?}, registered {:?}",
                    control.rect,
                    registered.interact_rect
                );
            }
            pane.handle_navigation(ui, area, &mut chrome);
        });
    });
    assert!(
        paper.working_orders().is_empty(),
        "the input tests never submit an order"
    );
    let panels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_black_alpha(165) => {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect();
    (paper.consumed_scroll(), panels, controls)
}

#[test]
fn fractional_controls_hover_and_resize_without_id_diagnostics() {
    fn has_diagnostic(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text.contains("widget ID"),
            egui::Shape::Rect(rect) => rect.stroke.color == egui::Color32::RED,
            egui::Shape::Vec(shapes) => shapes.iter().any(has_diagnostic),
            _ => false,
        }
    }
    for target in [BubbleScaleTarget::Candle, BubbleScaleTarget::Tape] {
        for density in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            ctx.options_mut(|options| options.warn_on_id_clash = true);
            ctx.style_mut(|style| style.animation_time = 0.0);
            let id = egui::Id::new("fractional_bubble_control");
            for width in [156.0, 156.0, 100.0, 100.0, 156.0] {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(100.4, 50.3), egui::vec2(width, HEIGHT));
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    events: vec![egui::Event::PointerMoved(rect.center())],
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(density);
                let output = ctx.run(input, |ctx| {
                    let (changed, _) = draw_control(
                        ctx,
                        id,
                        ScaleControl {
                            target,
                            rect,
                            value: 0.85,
                        },
                    );
                    assert_eq!(changed, None, "hover and resize do not edit the scale");
                    let registered = ctx.read_response(id.with("move")).unwrap();
                    assert!(rect.expand(1.0).contains_rect(registered.interact_rect));
                });
                assert!(ctx.options(|options| options.warn_on_id_clash));
                assert!(
                    !output
                        .shapes
                        .iter()
                        .any(|shape| has_diagnostic(&shape.shape)),
                    "ID diagnostic at density {density}, width {width}"
                );
            }
        }
    }
}

#[test]
fn initial_controls_do_not_capture_the_chart_below_their_visible_bounds() {
    for density in [1.0, 2.0] {
        for target in 0..2 {
            let mut pane = pane();
            pane.state.ingest_live(&Trade {
                agg_id: 1,
                timestamp_ms: 1_000,
                price: 100.into(),
                quantity: 1.into(),
                side: Side::Buy,
            });
            let ctx = egui::Context::default();
            let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
            drive_with(
                &mut pane,
                &ctx,
                area,
                vec![],
                egui::Modifiers::NONE,
                density,
            );
            let chart = pane.frame.chart_area.unwrap();
            let control = layout(&pane, chart)[target].unwrap();
            let pointer = egui::pos2(control.rect.center().x, chart.center().y);
            assert!(!control.rect.contains(pointer));
            drive_with(
                &mut pane,
                &ctx,
                area,
                vec![egui::Event::PointerMoved(pointer)],
                egui::Modifiers::NONE,
                density,
            );
            assert_eq!(
                pane.hover_pos,
                Some(pointer),
                "density {density}, control {target}"
            );
        }
    }
}

#[test]
fn shrinking_controls_update_the_registered_hit_bounds_immediately() {
    for target in 0..2 {
        let mut pane = pane();
        pane.state.ingest_live(&Trade {
            agg_id: 1,
            timestamp_ms: 1_000,
            price: 100.into(),
            quantity: 1.into(),
            side: Side::Buy,
        });
        let ctx = egui::Context::default();
        let wide = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let narrow = wide.with_max_x(300.0);
        drive(&mut pane, &ctx, wide, vec![]);
        let previous = layout(&pane, pane.frame.chart_area.unwrap())[target].unwrap();
        let chart = pane
            .plot_areas(narrow, crate::config::FeedCapabilities::none())
            .chart;
        let control = layout(&pane, chart)[target].unwrap();
        assert!(control.rect.width() < previous.rect.width());
        let pointer = control.rect.right_center() + egui::vec2(4.0, 0.0);
        assert!(chart.contains(pointer));
        for frame in 0..2 {
            drive(
                &mut pane,
                &ctx,
                narrow,
                vec![egui::Event::PointerMoved(pointer)],
            );
            assert_eq!(
                pane.hover_pos,
                Some(pointer),
                "control {target}, resized frame {frame}"
            );
        }
    }
}

#[test]
fn dragging_a_size_slider_cannot_pan_or_zoom_the_chart_beneath_it() {
    let mut pane = pane();
    pane.state.ingest_live(&Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: 100.into(),
        quantity: 1.into(),
        side: Side::Buy,
    });
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.animation_time = 0.0);
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
    drive(&mut pane, &ctx, area, vec![]);
    let control = drive(&mut pane, &ctx, area, vec![])[0].unwrap();
    let start = control.left_center() + egui::vec2(control.width() * 0.25, 0.0);
    let end = control.left_center() + egui::vec2(control.width() * 0.85, 0.0);
    drive(
        &mut pane,
        &ctx,
        area,
        vec![egui::Event::PointerMoved(start)],
    );
    let before = (
        pane.model.viewport.px_per_bar(),
        pane.model.viewport.right_edge_bar(1),
    );
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    drive(
        &mut pane,
        &ctx,
        area,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    drive(&mut pane, &ctx, area, vec![egui::Event::PointerMoved(end)]);
    drive(&mut pane, &ctx, area, vec![button(end, false)]);
    let owner = pane.orderflow.as_ref().unwrap();
    assert_ne!(
        owner.bubble_display_scale(BubbleScaleTarget::Candle),
        Some(1.0)
    );
    assert_eq!(
        owner.bubble_display_scale(BubbleScaleTarget::Tape),
        Some(1.0)
    );
    assert_eq!(
        (
            pane.model.viewport.px_per_bar(),
            pane.model.viewport.right_edge_bar(1)
        ),
        before
    );
}

#[test]
fn wheel_over_controls_is_private_but_the_bare_chart_still_zooms() {
    let mut pane = pane();
    pane.state.ingest_live(&Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: 100.into(),
        quantity: 1.into(),
        side: Side::Buy,
    });
    let ctx = egui::Context::default();
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
    drive(&mut pane, &ctx, area, vec![]);
    let chart = pane.frame.chart_area.unwrap();
    let control = layout(&pane, chart)[0].unwrap().rect.center();
    let outside = chart.min + egui::vec2(100.0, chart.height() * 0.65);
    let wheel = |at, modifiers| {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 80.0),
                modifiers,
            },
        ]
    };
    let before = pane.model.viewport.px_per_bar();
    let (paper, _, _) = drive_with(
        &mut pane,
        &ctx,
        area,
        wheel(outside, egui::Modifiers::NONE),
        egui::Modifiers::NONE,
        1.0,
    );
    assert!(!paper);
    assert!(
        pane.model.viewport.px_per_bar() > before,
        "outside the slider the wheel still zooms candles"
    );
    let zoom = pane.model.viewport.px_per_bar();
    for modifiers in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
        let (paper, _, _) = drive_with(
            &mut pane,
            &ctx,
            area,
            wheel(control, modifiers),
            modifiers,
            1.0,
        );
        assert!(
            !paper,
            "floating controls cannot step the paper ruler underneath"
        );
        assert_eq!(pane.model.viewport.px_per_bar(), zoom);
    }
    let (paper, _, _) = drive_with(
        &mut pane,
        &ctx,
        area,
        wheel(outside, egui::Modifiers::SHIFT),
        egui::Modifiers::SHIFT,
        1.0,
    );
    assert!(
        paper,
        "the ruler still owns its wheel on the uncovered chart"
    );
    assert_eq!(pane.model.viewport.px_per_bar(), zoom);
}

#[test]
fn actual_control_panels_fit_narrow_surfaces_at_each_display_density() {
    for density in [1.0, 2.0] {
        let mut pane = pane();
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.animation_time = 0.0);
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 500.0));
        drive_with(
            &mut pane,
            &ctx,
            area,
            vec![],
            egui::Modifiers::NONE,
            density,
        );
        let (_, panels, controls) = drive_with(
            &mut pane,
            &ctx,
            area,
            vec![],
            egui::Modifiers::NONE,
            density,
        );
        assert_eq!(panels.len(), 2);
        let chart = pane.frame.chart_area.unwrap();
        let divider = chart.right()
            - pane
                .orderflow
                .as_ref()
                .unwrap()
                .lane_width_px(chart.width());
        for (panel, region) in panels
            .iter()
            .zip([chart.with_max_x(divider), chart.with_min_x(divider)])
        {
            assert!(
                region.contains_rect(*panel),
                "density {density}: {panel:?} escaped {region:?}"
            );
            assert!(panel.top() > super::super::tape_switch_rect(chart, false).bottom());
        }
        assert!(!panels[0].intersects(panels[1]));
        for (panel, control) in panels.iter().zip(controls) {
            assert!(panel.contains_rect(control.unwrap()));
        }
    }
}
