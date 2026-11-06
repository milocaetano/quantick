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
    let ui = crate::theme::ui(ctx);
    let plot = area
        .shrink(crate::plot_area::PLOT_PADDING_PX)
        .with_max_x(gutter.left())
        .with_max_y(strip.top());
    assert!(
        has_fill(output, area, ui.canvas()),
        "the frame uses the shared Light canvas"
    );
    assert!(
        has_fill(output, plot, crate::theme::bookmap(ctx).canvas()),
        "only data plots use Slate"
    );
    assert!(
        has_fill(output, area.with_min_y(strip.top()), ui.flow_surface()),
        "the timestamp strip stays Light"
    );
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
    let ui = crate::theme::ui(&ctx);
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
    let ui = crate::theme::ui(&ctx);
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
