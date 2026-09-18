use super::*;

fn profile_app() -> (QuantickApp, mpsc::Receiver<FeedCommand>, egui::Context) {
    let (mut app, events, commands, _book) = test_app();
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.set_layer_visible(
        crate::chart_layers::ChartLayer::LiveStrip,
        false,
        &mut Default::default(),
    );
    pane.spec.retain(crate::state::BarSpec::Tick(1));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut()
        .flow_pane
        .state
        .set_footprint_enabled(true);
    let trades = (1..=200)
        .map(|id| {
            let mut print = trade(id);
            print.price = Decimal::from([100, 100, 100, 100, 102, 110][id as usize % 6]);
            print
        })
        .collect();
    events.try_send(FeedEvent::Backfilled(trades)).unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), 200);
    let tool = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .unwrap();
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.drawings.place(tool, ChartPoint::at(80.0, 110.0));
    pane.drawings.place(tool, ChartPoint::at(160.0, 100.0));
    let payload = pane.drawings.items_mut()[0]
        .payload
        .as_any_mut()
        .downcast_mut::<crate::drawings::FrvpPayload>()
        .unwrap();
    payload.width_frac = 0.8;
    payload.show_poc = false;
    payload.show_value_area = false;
    payload.show_labels = false;
    pane.drawings.select(None);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        run_frame_at(&mut app, &ctx, TEST_WINDOW);
    }
    (app, commands, ctx)
}

fn target(app: &QuantickApp, bar: f32, price: f64) -> egui::Pos2 {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.unwrap();
    let right = pane.frame.lane_divider_x.unwrap_or(chart.right());
    let (low, high) = pane.frame.auto_range.unwrap();
    let scale = pane.price_view.scale(
        (low, high),
        pane.frame.chart_top,
        pane.frame.chart_top + pane.frame.chart_height,
    );
    egui::pos2(
        pane.viewport.x_at_bar_position(bar, right, pane.slots()),
        scale.y(price),
    )
}

fn profile_handle(app: &QuantickApp, index: usize) -> egui::Pos2 {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.unwrap();
    let scale = pane
        .price_view
        .scale(pane.frame.auto_range.unwrap(), chart.top(), chart.bottom());
    let drawing = &pane.drawings.items()[0];
    let points: Vec<_> = drawing
        .points
        .iter()
        .map(|point| target(app, point.bar, point.price))
        .collect();
    let drawing_context = crate::drawings::DrawContext {
        payload: drawing.payload.as_ref(),
        anchors: &drawing.points,
        scale: &scale,
        px_per_bar: pane.viewport.px_per_bar(),
        unit: crate::drawings::ValueUnit::Price,
        primary_band: true,
        style: drawing.style,
        selected: true,
        halo: false,
        content_editing: false,
    };
    drawing.tool.handles(chart, &points, &drawing_context)[index]
}

#[test]
fn precise_profile_empty_space_pans_and_deselects_without_moving_the_drawing() {
    let (mut app, _commands, ctx) = profile_app();
    let before = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    let start = target(&app, 100.0, 106.0);
    let hover = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(start)]);
    assert_ne!(hover.platform_output.cursor_icon, egui::CursorIcon::Move);
    let reference = target(&app, 120.0, 106.0);
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        start,
        start - egui::vec2(60.0, 0.0),
    );
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), None);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        before
    );
    assert!(
        target(&app, 120.0, 106.0).x < reference.x - 20.0,
        "the chart pans through the missing row"
    );
    app.active_tab_mut().flow_pane.drawings.select(Some(0));
    run_frame(&mut app, &ctx);
    let empty = target(&app, 100.0, 106.0);
    click_chart(&mut app, &ctx, empty);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), None);
}

#[test]
fn precise_profile_painted_row_moves_and_visible_handle_resizes() {
    let (mut app, _commands, ctx) = profile_app();
    let payload = app.active_tab().flow_pane.drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<crate::drawings::FrvpPayload>()
        .unwrap();
    let group = crate::chart::to_f64(
        payload
            .cache
            .as_ref()
            .unwrap()
            .output()
            .profile
            .as_ref()
            .unwrap()
            .0
            .group(),
    );
    let start = target(&app, 100.0, 100.0 + group / 2.0);
    let hover = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(start)]);
    assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::Move);
    let before = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        start,
        start - egui::vec2(45.0, 0.0),
    );
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    let moved = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    assert_ne!(moved[0].bar, before[0].bar);
    assert!(((moved[1].bar - moved[0].bar) - (before[1].bar - before[0].bar)).abs() < 0.1);
    // Drive the visible affordance after the move has refreshed its range.
    let handle = profile_handle(&app, 1);
    let hover = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(handle)]);
    assert_eq!(
        hover.platform_output.cursor_icon,
        egui::CursorIcon::ResizeNwSe
    );
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        handle,
        handle - egui::vec2(30.0, 0.0),
    );
    let resized = &app.active_tab().flow_pane.drawings.items()[0].points;
    assert_eq!(resized[0].bar, moved[0].bar);
    assert_eq!(resized[0].time_ms, moved[0].time_ms);
    assert!((resized[0].price - moved[0].price).abs() < 1e-5);
    assert_ne!(resized[1].bar, moved[1].bar);
    assert!((resized[1].price - moved[1].price).abs() < 1e-5);
}

#[test]
fn precise_profile_locked_selection_leaves_hidden_handle_space_to_the_chart() {
    let (mut app, _commands, ctx) = profile_app();
    app.active_tab_mut().flow_pane.drawings.select(Some(0));
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_locked_at(0, true);
    run_frame(&mut app, &ctx);
    // Six pixels misses the border but used to hit its hidden 7.125 px handle.
    let start = profile_handle(&app, 0) + egui::vec2(6.0, 0.0);
    let hover = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(start)]);
    assert_ne!(
        hover.platform_output.cursor_icon,
        egui::CursorIcon::NotAllowed
    );
    assert_ne!(hover.platform_output.cursor_icon, egui::CursorIcon::Move);
    let before = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    let reference = target(&app, 120.0, 106.0);
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        start,
        start - egui::vec2(60.0, 0.0),
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        before
    );
    assert!(target(&app, 120.0, 106.0).x < reference.x - 20.0);
    // The actual painted body still owns the blocked gesture on a locked object.
    let body = target(&app, 100.0, 100.5);
    let reference = target(&app, 120.0, 106.0);
    let hover = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(body)]);
    assert_eq!(
        hover.platform_output.cursor_icon,
        egui::CursorIcon::NotAllowed
    );
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        body,
        body - egui::vec2(40.0, 0.0),
    );
    assert_eq!(target(&app, 120.0, 106.0), reference);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        before
    );
}

/// Campaign sync X4: the right-drag repairs (D28) meet #461's precise picking.
/// Space a precise profile does not paint belongs to the chart, so a secondary
/// drag started there measures a quick range: the chart does not pan and the
/// profile neither moves nor becomes selected.
#[test]
fn a_secondary_drag_through_a_profile_gap_measures_without_panning_or_moving_it() {
    let (mut app, _commands, ctx) = profile_app();
    let before = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    let start = target(&app, 100.0, 106.0);
    let end = target(&app, 140.0, 104.0);
    let reference = target(&app, 120.0, 106.0);
    let secondary = |position: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), secondary(start, true)],
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), secondary(end, false)],
    );

    assert!(
        crate::app::control_quick_range(&app).is_some(),
        "the drag through the profile's gap raised a quick range"
    );
    let after = target(&app, 120.0, 106.0);
    assert!(
        (after.x - reference.x).abs() < 0.5,
        "a secondary drag does not pan: bar 120 moved from {} to {}",
        reference.x,
        after.x
    );
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), None);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        before
    );
}
