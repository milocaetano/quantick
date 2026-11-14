use super::*;

#[test]
fn text_properties_t_commits_and_rearms_without_copying_content() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Text("first note".into())]);
    let drawing = app
        .active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .unwrap();
    let payload = drawing
        .payload
        .as_any_mut()
        .downcast_mut::<quantick_drawings::TextPayload>()
        .unwrap();
    payload.size_px = 28.0;
    payload.appearance.bold = true;
    payload.appearance.italic = true;
    run_frame(&mut app, &ctx);
    let repeat = app
        .surfaces
        .drawing_chrome
        .context_bar()
        .draw_again_rect()
        .expect("T is offered");
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(repeat.center())],
    );
    click_chart(&mut app, &ctx, repeat.center());
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.surfaces.toolrail.tool(),
        Tool::Drawing(drawing_tool("text"))
    );
    assert_eq!(app.surfaces.drawing_chrome.inline_text_editing(), None);
    click_chart(&mut app, &ctx, egui::pos2(850.0, 420.0));
    run_frame(&mut app, &ctx);
    let items = app.active_tab().flow_pane.drawings.items();
    assert_eq!(items.len(), 2);
    assert_ne!(items[0].id, items[1].id);
    assert_eq!(
        items[0].tool.inline_text(items[0].payload.as_ref()),
        Some("first note")
    );
    let second = items[1]
        .payload
        .as_any()
        .downcast_ref::<quantick_drawings::TextPayload>()
        .unwrap();
    assert!(second.text.is_empty());
    assert_eq!(second.size_px, 28.0);
    assert!(second.appearance.bold && second.appearance.italic);
    assert_eq!(app.surfaces.drawing_chrome.inline_text_editing(), Some(1));
}

#[test]
fn inline_note_moves_out_of_the_chart_with_its_anchor_during_pan_and_zoom() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Text("anchored".into())]);
    let anchor = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let editor = |ctx: &egui::Context| {
        ctx.memory(|memory| memory.area_rect(egui::Id::new("inline-text-editor")))
            .unwrap()
    };
    let first = editor(&ctx);
    let total = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .model
        .viewport
        .pan_pixels(-1200.0, total);
    run_frame(&mut app, &ctx);
    let moved = editor(&ctx);
    let chart = app.active_tab().flow_pane.frame.chart_area.unwrap();
    assert!(
        moved.left() < chart.left(),
        "the editor must not pin to the chart edge: {moved:?}"
    );
    assert!(moved.left() < first.left());
    app.active_tab_mut().flow_pane.model.viewport.zoom(1.4);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points[0],
        anchor
    );
    assert_eq!(app.surfaces.drawing_chrome.inline_text_editing(), Some(0));
}
