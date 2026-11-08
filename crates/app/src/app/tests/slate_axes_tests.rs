//! The rendered frame keeps plot colors off the axes, including before data arrives.
use super::*;

fn has_fill(output: &egui::FullOutput, area: egui::Rect, color: egui::Color32) -> bool {
    output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape,
        egui::Shape::Rect(rect) if rect.rect == area && rect.fill == color)
    })
}

fn assert_boundaries(app: &QuantickApp, ctx: &egui::Context, output: &egui::FullOutput) {
    let frame = &app.active_tab().flow_pane.frame;
    let (area, gutter, strip) = (
        frame.area.unwrap(),
        frame.price_gutter.unwrap(),
        frame.time_strip.unwrap(),
    );
    let ui = crate::theme::axes(ctx);
    let plot = area.with_max_x(gutter.left()).with_max_y(strip.top());
    assert!(
        has_fill(output, area, ui.canvas()),
        "the frame uses the shared Light canvas"
    );
    assert!(
        has_fill(output, plot, crate::theme::bookmap(ctx).canvas()),
        "only data plots use Slate"
    );
    assert_eq!(ui.canvas(), egui::Color32::from_rgb(255, 252, 247));
    let strip_fill = output.shapes.iter().rev().find_map(|shape| {
        if let egui::Shape::Rect(rect) = &shape.shape
            && rect
                .rect
                .contains(strip.left_center() + egui::vec2(1.0, 0.0))
            && rect.fill.is_opaque()
        {
            Some(rect.fill)
        } else {
            None
        }
    });
    assert_eq!(strip_fill, Some(ui.canvas()), "time axes share one canvas");
    assert!(
        !output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.fill == crate::theme::bookmap(ctx).canvas()
            && rect.rect.contains(gutter.center()))),
        "Slate must not cover the price gutter"
    );
    assert_eq!(
        crate::theme::get(ctx).canvas(),
        ui.canvas(),
        "plot scope ends after each pane"
    );
}

#[test]
fn slate_plot_boundaries_survive_empty_narrow_and_tape_only_layouts() {
    for count in [0, 200] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(count);
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        native_split(&mut app);
        app.runtime
            .appearance
            .apply_candidate(quantick_appearance::LIGHT, false)
            .unwrap();
        for size in [TEST_WINDOW, MIN_WINDOW] {
            for tape_only in [false, true] {
                app.active_tab_mut().flow_pane.set_layer_visible(
                    ChartLayer::TapeOnly,
                    tape_only,
                    &mut Default::default(),
                );
                run_frame_sized(&mut app, &ctx, size, Vec::new(), egui::Modifiers::NONE);
                let output =
                    run_frame_sized(&mut app, &ctx, size, Vec::new(), egui::Modifiers::NONE);
                assert_boundaries(&app, &ctx, &output);
            }
        }
    }
}

#[test]
fn slate_chart_axis_text_uses_root_ink_while_plot_text_uses_surface_ink() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    native_split(&mut app);
    app.runtime
        .appearance
        .apply_candidate(quantick_appearance::LIGHT, false)
        .unwrap();
    run_frame(&mut app, &ctx);
    let output = settled_flow_frame(&mut app, &ctx);
    assert_boundaries(&app, &ctx, &output);
    let frame = &app.active_tab().flow_pane.frame;
    let ui = crate::theme::axes(&ctx);
    let surface = crate::theme::bookmap(&ctx);
    let mut ticks = [0, 0];
    for clipped in &output.shapes {
        let egui::Shape::Text(text) = &clipped.shape else {
            continue;
        };
        let centre = text.visual_bounding_rect().center();
        let axis = if frame.price_gutter.unwrap().contains(centre) {
            Some(0)
        } else if frame.time_strip.unwrap().contains(centre)
            || frame
                .lane_time_strip
                .is_some_and(|strip| strip.contains(centre))
        {
            Some(1)
        } else {
            None
        };
        let Some(axis) = axis else { continue };
        for section in &text.galley.job.sections {
            assert_ne!(
                section.format.color,
                surface.text_muted(),
                "axis must not inherit Slate text ink"
            );
            assert_ne!(
                section.format.color,
                surface.text_primary(),
                "axis must not inherit Slate text ink"
            );
            if section.format.color == ui.text_muted() {
                ticks[axis] += 1;
            }
        }
    }
    assert!(
        ticks.iter().all(|count| *count > 1),
        "price and time labels must both be present: {ticks:?}"
    );
}

#[test]
fn slate_crosshair_keeps_plot_lines_and_light_axis_tags_separate() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    app.runtime
        .appearance
        .apply_candidate(quantick_appearance::LIGHT, false)
        .unwrap();
    app.toolrail.arm(crate::toolrail::Tool::Crosshair);
    run_frame(&mut app, &ctx);
    let output = settled_flow_frame(&mut app, &ctx);
    assert_boundaries(&app, &ctx, &output);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.unwrap();
    let point = egui::pos2(
        (chart.left() + pane.frame.candles_right.unwrap()) / 2.0,
        chart.center().y,
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(point)]);
    let output = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(point)]);
    let frame = &app.active_tab().flow_pane.frame;
    let ui = crate::theme::axes(&ctx);
    let surface = crate::theme::bookmap(&ctx);
    let mut tags = [false, false];
    let mut plot_line = false;
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::Shape::Rect(rect) if rect.fill == ui.tag_bg() => {
                for (index, area) in [frame.price_gutter.unwrap(), frame.time_strip.unwrap()]
                    .iter()
                    .enumerate()
                {
                    if area.contains(rect.rect.center()) {
                        tags[index] = true;
                    }
                }
            }
            egui::Shape::LineSegment { points, stroke }
                if points[0].x == point.x
                    && points[1].x == point.x
                    && stroke.color == egui::epaint::ColorMode::Solid(surface.text_faint()) =>
            {
                plot_line = true;
            }
            _ => {}
        }
    }
    assert!(
        tags[0] && tags[1],
        "both pointer tags use the Light plate: {tags:?}"
    );
    assert!(plot_line, "the crosshair remains visible on Slate");
}

#[test]
fn authored_dark_axes_keep_their_surface_ink_and_timestamp_backing() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    native_split(&mut app);
    app.runtime.appearance.apply_candidate(
        "version=1\nname='Authored Dark axes'\ndark=true\n[bookmap.colors]\ncanvas='#FFFFFF'\ntext_muted='#202020'\nflow_surface='#E1D8C8'\ntag_bg='#B5C6D7'\ncrosshair_ink='#101010'",
        false,
    ).unwrap();
    add_pane_indicator(
        &mut app,
        "cvd",
        (0..200).map(|row| f64::from(row) - 200.0).collect(),
    );
    run_frame(&mut app, &ctx);
    let output = settled_flow_frame(&mut app, &ctx);
    let frame = &app.active_tab().flow_pane.frame;
    let area = frame.area.unwrap();
    let strip = frame.time_strip.unwrap();
    assert!(has_fill(&output, area, egui::Color32::WHITE));
    assert!(has_fill(
        &output,
        area.with_min_y(strip.top()),
        egui::Color32::from_rgb(225, 216, 200),
    ));
    let regions = [frame.price_gutter.unwrap(), pane_gutter(&app, 0), strip];
    let mut ticks = [0; 3];
    for clipped in &output.shapes {
        let egui::Shape::Text(text) = &clipped.shape else {
            continue;
        };
        for (index, region) in regions.iter().enumerate() {
            if region.contains(text.visual_bounding_rect().center()) {
                for section in &text.galley.job.sections {
                    assert_ne!(section.format.color, crate::theme::TEXT_MUTED);
                    if section.format.color == egui::Color32::from_rgb(32, 32, 32) {
                        ticks[index] += 1;
                    }
                }
            }
        }
    }
    assert!(
        ticks.iter().all(|count| *count >= 2),
        "authored axis ticks: {ticks:?}"
    );

    app.toolrail.arm(crate::toolrail::Tool::Crosshair);
    let point = app
        .active_tab()
        .flow_pane
        .frame
        .chart_rect
        .unwrap()
        .center();
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(point)]);
    let output = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(point)]);
    let frame = &app.active_tab().flow_pane.frame;
    for region in [frame.price_gutter.unwrap(), frame.time_strip.unwrap()] {
        assert!(
            output.shapes.iter().any(|clipped| matches!(&clipped.shape,
            egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(181, 198, 215)
                && region.contains(rect.rect.center()))),
            "authored crosshair tag in {region:?}"
        );
    }
}

#[test]
fn empty_loading_words_stand_bare_on_the_chart() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for layout in [CanvasLayout::Single, CanvasLayout::TimeAndFlow] {
            for reconnect in [false, true] {
                let ctx = egui::Context::default();
                let (mut app, _notices, _channels) = test_app_with_notices();
                app.active_tab_mut().set_layout(layout);
                native_split(&mut app);
                app.runtime
                    .appearance
                    .apply_candidate(source, false)
                    .unwrap();
                run_frame(&mut app, &ctx);
                run_frame(&mut app, &ctx);
                if reconnect {
                    app.active_tab_mut().notice =
                        FeedNotice::attention("Connection unavailable", "Reconnect.");
                } else {
                    app.active_tab_mut().feed_connection = FeedConnectionState::Connecting;
                }
                let config = app.config.clone();
                let past = metrics::wall_clock_ms()
                    - quantick_chart::connection_state::OVERLAY_DELAY_MS
                    - 1;
                app.active_tab_mut().connection = Default::default();
                app.active_tab_mut().observe_connection(&config, past, None);
                run_frame(&mut app, &ctx);
                let output = run_frame(&mut app, &ctx);
                let tab = app.active_tab();
                let words = tab
                    .connection
                    .overlay(tab.feed_display_name(&app.config), metrics::wall_clock_ms())
                    .unwrap();
                assert_eq!(words.reconnect, reconnect);
                let title = format!("{}{}", words.verb, tab.symbol);
                let text: Vec<_> = output
                    .shapes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, clipped)| {
                        let egui::Shape::Text(text) = &clipped.shape else {
                            return None;
                        };
                        let value = &text.galley.job.text;
                        (value == &title
                            || value.starts_with(words.meta.trim_end_matches('.'))
                            || value == crate::loading::RECONNECT_LABEL)
                            .then_some((index, text))
                    })
                    .collect();
                assert_eq!(
                    text.len(),
                    2 + usize::from(reconnect),
                    "one title, metadata and optional reconnect"
                );
                // Nothing charted: no plate stands behind the words in either
                // theme, and they speak in the palette of the chart they stand in for.
                let empty = tab.empty_panes_rect().unwrap();
                let plates = output.shapes.iter().filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Rect(rect)
                    if rect.fill.a() > 0 && rect.rect.width() < empty.width()
                        && text.iter().any(|(_, text)| {
                            rect.rect.contains_rect(text.visual_bounding_rect())
                        }))
                });
                assert_eq!(plates.count(), 0, "no loading backing");
                let palette = tab.lead_pane().palette(&ctx);
                for (_, text) in text {
                    for section in &text.galley.job.sections {
                        let color = section.format.color;
                        if color.a() > 0 {
                            assert!(
                                color == palette.loading_title() || color == palette.loading_meta()
                            );
                        }
                    }
                }
            }
        }
    }
}
