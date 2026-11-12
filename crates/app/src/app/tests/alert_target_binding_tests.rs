use super::*;
use quantick_control_handlers::alerts::AlertDefinitionsPort;
use quantick_control_schema::alerts::{DrawingSelector, TargetResolveError};

fn legacy(app: &mut QuantickApp) -> DrawingSelector {
    app.layout_adapter().seed_new_panes();
    let tab_id = app.runtime.tabs.active_id();
    let pane = app
        .runtime
        .tabs
        .by_id_mut(tab_id)
        .unwrap()
        .focused_pane_mut();
    let tool = quantick_drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    assert!(pane.drawings.place(
        tool,
        quantick_drawings::ChartPoint::at_time(0.0, 10.0, Some(1000))
    ));
    DrawingSelector {
        tab_id,
        pane_id: pane.id,
        drawing_id: pane.drawings.items()[0].id.0,
    }
}

#[test]
fn alert_target_binding_bridge() {
    let (mut app, ..) = test_app();
    let selector = legacy(&mut app);
    let ctx = egui::Context::default();
    let _other_channels = open_second_tab(&mut app, &ctx, "OTHERUSDT");
    let other_selector = legacy(&mut app);
    assert_eq!(selector.drawing_id, other_selector.drawing_id);
    let other_target = app.alert_resolve_target(other_selector).unwrap();
    let target = app.layout_adapter().resolve_alert_target(selector).unwrap();
    assert_ne!(target, other_target);
    assert_eq!(app.alert_resolve_target(selector), Ok(target.clone()));
    let path = app.layout_adapter().store.market().path().to_owned();
    let contents = std::fs::read_to_string(path).unwrap();
    assert!(contents.contains(&target.incarnation));
    let other = DrawingSelector {
        pane_id: u64::MAX,
        ..selector
    };
    assert_eq!(
        app.alert_resolve_target(other),
        Err(TargetResolveError::Missing)
    );
    assert_eq!(app.alert_resolve_target(selector), Ok(target));
}

#[test]
fn alert_target_binding_failure() {
    let (mut app, ..) = test_app();
    let selector = legacy(&mut app);
    app.layout_adapter().store.market_mut().set_blocked(true);
    assert!(matches!(
        app.alert_resolve_target(selector),
        Err(TargetResolveError::WriteFailed(_))
    ));
    app.layout_adapter().store.market_mut().set_blocked(false);
    let target = app.alert_resolve_target(selector).unwrap();
    let pane = app
        .runtime
        .tabs
        .by_id_mut(selector.tab_id)
        .unwrap()
        .focused_pane_mut();
    assert!(
        pane.drawings
            .remove_by_id(quantick_drawings::DrawingId(selector.drawing_id))
    );
    assert_eq!(
        app.alert_resolve_target(selector),
        Err(TargetResolveError::Missing)
    );
    let pane = app
        .runtime
        .tabs
        .by_id_mut(selector.tab_id)
        .unwrap()
        .focused_pane_mut();
    assert!(pane.drawings.undo());
    assert_eq!(app.alert_resolve_target(selector), Ok(target));
}

#[test]
fn alert_target_binding_failure_ambiguous_selector() {
    let (mut app, ..) = test_app();
    let selector = legacy(&mut app);
    let pane = app
        .runtime
        .tabs
        .by_id_mut(selector.tab_id)
        .unwrap()
        .focused_pane_mut();
    let copy = pane.drawings.items()[0].clone();
    let _ = pane.drawings.paste(&copy, 1.0);
    pane.drawings.items_mut()[1].id = copy.id;
    assert_eq!(
        app.alert_resolve_target(selector),
        Err(TargetResolveError::Ambiguous)
    );
}

#[test]
fn alert_target_binding_bottom_edge_button_commits_selected_rectangle() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let index = place_drawing(
        &mut app,
        &ctx,
        "rectangle",
        &[egui::pos2(500.0, 300.0), egui::pos2(650.0, 420.0)],
    );
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(index));
    app.drawings
        .chrome
        .context_bar_mut()
        .set_manual(egui::pos2(500.0, 10000.0));
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let button = painted_text_center(&output, "Create alert").expect("visible alert action");
    let chart = app.active_tab().drawing_pane().frame.chart_area.unwrap();
    assert!(
        chart.contains(button),
        "bottom-edge action remains inside its chart: {button:?}"
    );
    assert!(
        button.y < chart.bottom() - ctx.style().spacing.interact_size.y,
        "bottom-edge action leaves the chart annotation line clear: {button:?}"
    );
    click_chart(&mut app, &ctx, button);
    let target = app.active_tab().drawing_pane().drawings.items()[index]
        .durable_identity
        .clone()
        .expect("click promoted selected target");
    let contents = std::fs::read_to_string(app.layout_adapter().store.market().path()).unwrap();
    assert!(contents.contains(&target.incarnation));
}

#[test]
fn alert_target_binding_offers_every_straight_line() {
    for (tool, anchors) in [
        ("horizontal-line", &[egui::pos2(560.0, 320.0)][..]),
        ("horizontal-ray", &[egui::pos2(560.0, 320.0)][..]),
        (
            "ray",
            &[egui::pos2(500.0, 300.0), egui::pos2(650.0, 360.0)][..],
        ),
        (
            "extended-line",
            &[egui::pos2(500.0, 300.0), egui::pos2(650.0, 360.0)][..],
        ),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        run_frame(&mut app, &ctx);
        let index = place_drawing(&mut app, &ctx, tool, anchors);
        app.toolrail.arm(Tool::Pointer);
        app.drawing_pane_mut().drawings.select(Some(index));
        run_frame(&mut app, &ctx);
        let output = run_frame(&mut app, &ctx);
        let button = painted_text_center(&output, "Create alert")
            .unwrap_or_else(|| panic!("{tool}: selected line offers an alert"));
        click_chart(&mut app, &ctx, button);
        run_frame(&mut app, &ctx);
        let pane = app.active_tab().drawing_pane();
        let target = pane.drawings.items()[index]
            .durable_identity
            .clone()
            .unwrap_or_else(|| panic!("{tool}: click promoted the target"));
        let choices = super::super::alerts_runtime::context(
            app.active_tab(),
            pane,
            app.runtime.tabs.active_id(),
            0,
        )
        .drawings;
        let choice = choices
            .iter()
            .find(|choice| choice.target.as_ref() == Some(&target))
            .unwrap_or_else(|| panic!("{tool}: editor lists the line"));
        assert_eq!(choice.kind, quantick_alerts::TargetKind::Line, "{tool}");
    }
}
