//! Exercise the painted title-strip controls with actual pointer input.
use super::*;
use quantick_control_handlers::native_window::NativeWindowHost as _;

fn sized(app: &mut QuantickApp, ctx: &egui::Context, width: f32) -> egui::FullOutput {
    run_frame_sized(
        app,
        ctx,
        egui::vec2(width, 760.0),
        Vec::new(),
        egui::Modifiers::NONE,
    )
}

#[test]
fn title_bar_narrow_overflow_keeps_selection_plus_and_window_buttons_reachable() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let identities: Vec<_> = app.runtime.tabs.iter_with_ids().map(|(id, _)| id).collect();
    for width in [1200.0, 300.0] {
        for _ in 0..3 {
            sized(&mut app, &ctx, width);
        }
        let title = &app.chrome.title_bar;
        assert!(
            title
                .strip
                .tabs
                .iter()
                .any(|(index, _)| *index == app.runtime.tabs.active_index())
        );
        let plus = title.strip.add.unwrap();
        assert!(
            plus.right() <= width && plus.top() < 32.0,
            "plus remains on the title row: {plus:?}"
        );
        for (_, rect) in &title.controls {
            assert!(
                rect.right() <= width && rect.top() < 32.0,
                "native control is reachable: {rect:?}"
            );
            assert!(!rect.intersects(plus));
        }
    }
    let overflow = app.chrome.title_bar.strip.overflow.unwrap().center();
    click_sized(&mut app, &ctx, egui::vec2(300.0, 760.0), overflow);
    let output = sized(&mut app, &ctx, 300.0);
    let first_label = app.runtime.tabs[0].chip_label().to_owned();
    let target = painted_text_center(&output, &first_label).expect("overflow names hidden market");
    click_sized(&mut app, &ctx, egui::vec2(300.0, 760.0), target);
    assert_eq!(app.runtime.tabs.active_index(), 0);
    sized(&mut app, &ctx, 300.0);
    assert!(
        app.chrome
            .title_bar
            .strip
            .tabs
            .iter()
            .any(|(index, _)| *index == 0)
    );
    assert_eq!(
        app.runtime
            .tabs
            .iter_with_ids()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        identities
    );
    let plus = app.chrome.title_bar.strip.add.unwrap().center();
    click_sized(&mut app, &ctx, egui::vec2(300.0, 760.0), plus);
    assert!(app.surfaces.source_picker.is_open());
}

#[test]
fn title_bar_window_requests_emit_native_commands_without_mutating_markets() {
    use quantick_control_schema::native_window::WindowState;
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let identity = app.runtime.tabs.active_id();
    for state in [
        WindowState::Maximized,
        WindowState::Normal,
        WindowState::Minimized,
    ] {
        app.request_native_window(state).unwrap();
        let output = sized(&mut app, &ctx, 1200.0);
        let commands: Vec<_> = output
            .viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
            .collect();
        assert!(commands.iter().any(|command| match state {
            WindowState::Maximized => matches!(command, egui::ViewportCommand::Maximized(true)),
            WindowState::Normal => matches!(command, egui::ViewportCommand::Maximized(false)),
            WindowState::Minimized => matches!(command, egui::ViewportCommand::Minimized(true)),
        }));
        assert_eq!(app.runtime.tabs.active_id(), identity);
        assert_eq!(app.runtime.tabs.len(), 1);
    }
}

#[test]
fn title_bar_drag_space_never_covers_market_widgets_and_close_keeps_last_market() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    sized(&mut app, &ctx, 1200.0);
    let drag = app
        .chrome
        .title_bar
        .controls
        .iter()
        .find(|(id, _)| *id == "drag")
        .unwrap()
        .1;
    for (_, tab) in &app.chrome.title_bar.strip.tabs {
        assert!(!drag.intersects(*tab));
    }
    assert!(!drag.intersects(app.chrome.title_bar.strip.add.unwrap()));
    let at = app.chrome.title_bar.strip.tabs[0].1.center();
    for events in [
        vec![egui::Event::PointerMoved(at), pointer_button(at, true)],
        vec![egui::Event::PointerMoved(at + egui::vec2(8.0, 0.0))],
        vec![pointer_button(at + egui::vec2(8.0, 0.0), false)],
    ] {
        let output = run_frame_sized(
            &mut app,
            &ctx,
            egui::vec2(1200.0, 760.0),
            events,
            egui::Modifiers::NONE,
        );
        assert!(
            !output
                .viewport_output
                .values()
                .flat_map(|viewport| &viewport.commands)
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag))
        );
    }
    app.apply_tab_action(TabAction::Close(0));
    assert_eq!(app.runtime.tabs.len(), 1);
    let output = run_frame_sized(
        &mut app,
        &ctx,
        egui::vec2(1200.0, 760.0),
        vec![
            egui::Event::PointerMoved(drag.center()),
            pointer_button(drag.center(), true),
        ],
        egui::Modifiers::NONE,
    );
    assert!(
        !output
            .viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
            .any(|command| matches!(command, egui::ViewportCommand::Close))
    );
    let output = run_frame_sized(
        &mut app,
        &ctx,
        egui::vec2(1200.0, 760.0),
        vec![egui::Event::PointerMoved(
            drag.center() + egui::vec2(8.0, 0.0),
        )],
        egui::Modifiers::NONE,
    );
    assert!(
        output
            .viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
            .any(|command| matches!(command, egui::ViewportCommand::StartDrag))
    );
}

#[test]
fn title_bar_idle_frames_preserve_text_focus_and_native_close_uses_normal_shutdown_command() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    sized(&mut app, &ctx, 1200.0);
    let focus = egui::Id::new("title_bar_focus_probe");
    let mut draft = String::from("original");
    for frame in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let _ =
                super::super::title_bar::draw(ctx, &app.runtime.tabs, &mut app.chrome.title_bar);
            egui::CentralPanel::default().show(ctx, |ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut draft).id(focus));
                if frame == 0 {
                    field.request_focus();
                }
            });
        });
        assert!(ctx.memory(|memory| memory.has_focus(focus)));
    }
    let close = app
        .chrome
        .title_bar
        .controls
        .iter()
        .find(|(id, _)| *id == "close")
        .unwrap()
        .1
        .center();
    run_frame_sized(
        &mut app,
        &ctx,
        egui::vec2(1200.0, 760.0),
        vec![
            egui::Event::PointerMoved(close),
            pointer_button(close, true),
        ],
        egui::Modifiers::NONE,
    );
    let output = run_frame_sized(
        &mut app,
        &ctx,
        egui::vec2(1200.0, 760.0),
        vec![pointer_button(close, false)],
        egui::Modifiers::NONE,
    );
    assert!(
        output
            .viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
            .any(|command| matches!(command, egui::ViewportCommand::Close))
    );
    assert_eq!(app.runtime.tabs.len(), 1);
}

#[test]
fn title_bar_inactive_close_target_keeps_hover_and_never_closes_the_last_market() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let selected = app.runtime.tabs.active_id();
    sized(&mut app, &ctx, 1200.0);
    let first = app
        .chrome
        .title_bar
        .strip
        .tabs
        .iter()
        .find(|(index, _)| *index == 0)
        .unwrap()
        .1;
    let close = egui::pos2(first.right() + 12.0, first.center().y);
    run_frame_sized(
        &mut app,
        &ctx,
        egui::vec2(1200.0, 760.0),
        vec![egui::Event::PointerMoved(first.center())],
        egui::Modifiers::NONE,
    );
    click_sized(&mut app, &ctx, egui::vec2(1200.0, 760.0), close);
    assert_eq!(app.runtime.tabs.len(), 1);
    assert_eq!(app.runtime.tabs.active_id(), selected);
    sized(&mut app, &ctx, 1200.0);
    let last = app.chrome.title_bar.strip.tabs[0].1;
    click_sized(
        &mut app,
        &ctx,
        egui::vec2(1200.0, 760.0),
        egui::pos2(last.right() + 12.0, last.center().y),
    );
    assert_eq!(app.runtime.tabs.len(), 1);
}

#[test]
fn title_bar_accepts_degenerate_client_widths_without_negative_widget_sizes() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    for width in [1.0, 50.0, 200.0, 300.0] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 100.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let _ =
                super::super::title_bar::draw(ctx, &app.runtime.tabs, &mut app.chrome.title_bar);
        });
        assert!(
            app.chrome
                .title_bar
                .controls
                .iter()
                .all(|(_, rect)| rect.is_finite() && rect.right() <= width)
        );
        assert_eq!(app.runtime.tabs.len(), 1);
    }
}
