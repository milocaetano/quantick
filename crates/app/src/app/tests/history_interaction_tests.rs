use super::*;

#[test]
fn input_and_an_unfinished_drawing_survive_publication_on_every_pane() {
    let (mut app, events, _commands, _book) = test_app();
    let ctx = egui::Context::default();
    let tab_id = app.tabs.active_id();
    events
        .try_send(FeedEvent::Backfilled(
            (350_000..400_000).map(trade).collect(),
        ))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    for (first, side) in [
        (250_000, crate::pane::PaneSide::Flow),
        (150_000, crate::pane::PaneSide::Time(0)),
        (50_000, crate::pane::PaneSide::Time(1)),
    ] {
        for pane in app.active_tab_mut().panes_mut() {
            pane.hold_history_publication(true);
        }
        app.active_tab_mut().loading.begin(LoadingTask::History);
        events
            .try_send(FeedEvent::HistoryPrepended(
                (first..first + 100_000).map(trade).collect(),
            ))
            .unwrap();
        app.active_tab_mut().drain_feed(tab_id);
        assert!(app.active_tab().pane(side).history_pending());
        let chart = app.active_tab().pane(side).frame.chart_rect.unwrap();
        let at = chart.min + chart.size() * egui::vec2(0.4, 0.4);
        let old_zoom = app.active_tab().pane(side).model.viewport.px_per_bar();
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 80.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let zoom = app.active_tab().pane(side).model.viewport.px_per_bar();
        assert_ne!(zoom, old_zoom);
        let gutter = app.active_tab().pane(side).frame.price_gutter.unwrap();
        drag_chart(
            &mut app,
            &ctx,
            gutter.center(),
            gutter.center() - egui::vec2(0.0, 32.0),
        );
        let price = app.active_tab().pane(side).price_view.resolve((0.0, 0.0));
        assert!(!app.active_tab().pane(side).price_view.is_auto());
        app.toolrail.arm(Tool::Drawing(drawing_tool("trend-line")));
        click_chart(&mut app, &ctx, at);
        let anchor = app
            .active_tab()
            .pane(side)
            .drawings
            .draft()
            .expect("first click starts the draft")
            .points[0];
        let focus = ctx.memory(|memory| memory.focused());
        assert!(
            app.active_tab().pane(side).history_pending(),
            "the inputs preceded publication"
        );
        for pane in app.active_tab_mut().panes_mut() {
            pane.hold_history_publication(false);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.active_tab().pane(side).history_pending() {
            run_frame(&mut app, &ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let pane = app.active_tab().pane(side);
        assert_eq!(pane.state.trades()[0].agg_id, first);
        assert_eq!(pane.model.viewport.px_per_bar(), zoom);
        assert_eq!(pane.price_view.resolve((0.0, 0.0)), price);
        let published = pane
            .drawings
            .draft()
            .expect("publication keeps the draft")
            .points[0];
        assert_eq!(published.time_ms, anchor.time_ms);
        assert_eq!(published.price, anchor.price);
        assert_eq!(ctx.memory(|memory| memory.focused()), focus);
        click_chart(&mut app, &ctx, at + egui::vec2(35.0, 20.0));
        assert!(app.active_tab().pane(side).drawings.draft().is_none());
        assert_eq!(app.active_tab().pane(side).drawings.items().len(), 1);
    }
}

#[test]
fn a_held_chart_drag_survives_repeated_history_publications() {
    let (mut app, events, _commands, _book) = test_app();
    let ctx = egui::Context::default();
    let tab_id = app.tabs.active_id();
    events
        .try_send(FeedEvent::Backfilled(
            (350_000..400_000).map(trade).collect(),
        ))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    app.active_tab_mut().history_step = 100_000;
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    for (first, side) in [
        (250_000, crate::pane::PaneSide::Flow),
        (150_000, crate::pane::PaneSide::Time(0)),
        (50_000, crate::pane::PaneSide::Time(1)),
    ] {
        // An hour of tape is met by the page below, so each run ends on it
        // and publishes.
        with_config(&mut app, |tab, config| {
            tab.load_history(config, quantick_feed::history_reach::HistoryReach::Hours(1))
        });
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let chart = app.active_tab().pane(side).frame.chart_rect.unwrap();
        let start = chart.min + chart.size() * egui::vec2(0.2, 0.2);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        let held = start + egui::vec2(16.0, 0.0);
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(held)]);
        let pane = app.active_tab().pane(side);
        assert!(
            !pane.model.viewport.follows_live(),
            "the pending request must accept the drag"
        );
        let distance = pane.slots() as f32 - pane.model.viewport.right_edge_bar(pane.slots());
        events
            .try_send(FeedEvent::HistoryPrepended(
                (first..first + 100_000).map(trade).collect(),
            ))
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(held)]);
            if !app.active_tab().flow_pane.history_pending() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let pane = app.active_tab().pane(side);
        assert_eq!(pane.state.trades()[0].agg_id, first);
        assert!(
            (pane.slots() as f32 - pane.model.viewport.right_edge_bar(pane.slots()) - distance)
                .abs()
                < 0.001,
            "publication preserves the held gesture's current view"
        );
        let end = held + egui::vec2(16.0, 0.0);
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
        run_frame_with_events(&mut app, &ctx, vec![pointer_button(end, false)]);
        let pane = app.active_tab().pane(side);
        assert!(
            pane.slots() as f32 - pane.model.viewport.right_edge_bar(pane.slots()) > distance,
            "the same gesture still moves after publication"
        );
    }
}

#[test]
fn history_wait_keeps_chart_drag_zoom_price_and_drawing_input_available() {
    let (mut app, _commands) = app_with_history(400);
    let ctx = egui::Context::default();
    app.active_tab_mut().loading.begin(LoadingTask::History);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
    for fraction in [egui::vec2(0.2, 0.15), egui::vec2(0.7, 0.7)] {
        let start = chart.min + chart.size() * fraction;
        let before = right_edge(&app);
        drag_chart(&mut app, &ctx, start, start + egui::vec2(32.0, 0.0));
        assert!(
            right_edge(&app) < before,
            "history must leave drag input available at {start:?}"
        );
        let before = app.active_tab().flow_pane.model.viewport.px_per_bar();
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 80.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_ne!(
            app.active_tab().flow_pane.model.viewport.px_per_bar(),
            before,
            "history must leave zoom input available"
        );
    }
    let gutter = app.active_tab().flow_pane.frame.price_gutter.unwrap();
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() - egui::vec2(0.0, 40.0),
    );
    assert!(!app.active_tab().flow_pane.price_view.is_auto());
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(
        &mut app,
        &ctx,
        chart.min + chart.size() * egui::vec2(0.25, 0.2),
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a real click places the drawing during history wait"
    );
    assert!(app.active_tab().loading.is_active(LoadingTask::History));
}
