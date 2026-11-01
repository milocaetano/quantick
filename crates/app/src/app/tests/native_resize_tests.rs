//! Real egui pointer routing for the native viewport resize border.
use super::*;
use egui::viewport::ResizeDirection as D;

fn resize_commands(output: &egui::FullOutput) -> Vec<D> {
    output
        .viewport_output
        .values()
        .flat_map(|viewport| &viewport.commands)
        .filter_map(|command| match command {
            egui::ViewportCommand::BeginResize(direction) => Some(*direction),
            _ => None,
        })
        .collect()
}

fn frame(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_frame_sized(app, ctx, size, events, egui::Modifiers::NONE)
}

#[test]
fn native_resize_all_edges_and_both_arms_of_each_corner_emit_the_native_direction() {
    use egui::CursorIcon as C;
    for width in [300.0, 1200.0] {
        let (mut app, _, _, _) = test_app();
        let ctx = egui::Context::default();
        let size = egui::vec2(width, 760.0);
        let market = app.runtime.tabs.active_id();
        let channel = app.active_tab().commands.clone();
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![]);
        }
        for (x, y, direction, cursor) in [
            (2.0, 8.0, D::NorthWest, C::ResizeNwSe),
            (8.0, 2.0, D::NorthWest, C::ResizeNwSe),
            (width / 2.0, 2.0, D::North, C::ResizeVertical),
            (width - 8.0, 2.0, D::NorthEast, C::ResizeNeSw),
            (width - 2.0, 8.0, D::NorthEast, C::ResizeNeSw),
            (2.0, 380.0, D::West, C::ResizeHorizontal),
            (width - 2.0, 380.0, D::East, C::ResizeHorizontal),
            (2.0, 752.0, D::SouthWest, C::ResizeNeSw),
            (8.0, 758.0, D::SouthWest, C::ResizeNeSw),
            (width / 2.0, 758.0, D::South, C::ResizeVertical),
            (width - 8.0, 758.0, D::SouthEast, C::ResizeNwSe),
            (width - 2.0, 752.0, D::SouthEast, C::ResizeNwSe),
        ] {
            let at = egui::pos2(x, y);
            frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(at)]);
            let hover = frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(at)]);
            assert_eq!(
                hover.platform_output.cursor_icon, cursor,
                "cursor at {at:?}"
            );
            assert!(resize_commands(&hover).is_empty());
            let pressed = frame(&mut app, &ctx, size, vec![pointer_button(at, true)]);
            assert_eq!(
                resize_commands(&pressed),
                vec![direction],
                "press at {at:?}"
            );
            assert!(
                !pressed
                    .viewport_output
                    .values()
                    .flat_map(|v| &v.commands)
                    .any(|command| matches!(
                        command,
                        egui::ViewportCommand::StartDrag
                            | egui::ViewportCommand::Close
                            | egui::ViewportCommand::Maximized(_)
                            | egui::ViewportCommand::Minimized(_)
                    ))
            );
            let held = frame(&mut app, &ctx, size, vec![]);
            assert!(resize_commands(&held).is_empty(), "one command per gesture");
            frame(&mut app, &ctx, size, vec![pointer_button(at, false)]);
            assert_eq!(app.runtime.tabs.active_id(), market);
            assert!(app.active_tab().commands.same_channel(&channel));
        }
    }
}

#[test]
fn native_resize_leaves_widget_interiors_and_existing_drags_alone() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let size = egui::vec2(1200.0, 760.0);
    for _ in 0..3 {
        frame(&mut app, &ctx, size, vec![]);
    }
    let mut positions = vec![
        egui::pos2(600.0, 400.0),
        egui::pos2(8.0, 20.0),
        egui::pos2(1192.0, 20.0),
    ];
    positions.extend(
        app.chrome
            .title_bar
            .strip
            .tabs
            .iter()
            .map(|(_, rect)| rect.center()),
    );
    positions.extend(
        app.chrome
            .title_bar
            .controls
            .iter()
            .map(|(_, rect)| rect.center()),
    );
    positions.push(app.chrome.title_bar.strip.add.unwrap().center());
    for at in positions {
        frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(at)]);
        let pressed = frame(&mut app, &ctx, size, vec![pointer_button(at, true)]);
        assert!(
            resize_commands(&pressed).is_empty(),
            "widget interior {at:?}"
        );
        let moved = frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerMoved(egui::pos2(2.0, 380.0))],
        );
        assert!(
            resize_commands(&moved).is_empty(),
            "a widget drag cannot become native resize"
        );
        frame(
            &mut app,
            &ctx,
            size,
            vec![pointer_button(egui::pos2(2.0, 380.0), false)],
        );
    }
}

#[test]
fn native_resize_is_disabled_for_maximized_minimized_and_fullscreen_viewports() {
    for state in ["maximized", "minimized", "fullscreen"] {
        let (mut app, _, _, _) = test_app();
        let ctx = egui::Context::default();
        let size = egui::vec2(1200.0, 760.0);
        for _ in 0..3 {
            frame(&mut app, &ctx, size, vec![]);
        }
        for at in [
            egui::pos2(2.0, 2.0),
            egui::pos2(600.0, 2.0),
            egui::pos2(1198.0, 2.0),
            egui::pos2(2.0, 380.0),
            egui::pos2(1198.0, 380.0),
            egui::pos2(2.0, 758.0),
            egui::pos2(600.0, 758.0),
            egui::pos2(1198.0, 758.0),
        ] {
            for pressed in [true, false] {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events: vec![egui::Event::PointerMoved(at), pointer_button(at, pressed)],
                    ..Default::default()
                };
                let viewport = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
                viewport.maximized = Some(state == "maximized");
                viewport.minimized = Some(state == "minimized");
                viewport.fullscreen = Some(state == "fullscreen");
                let output = ctx.run(input, |ctx| app.draw_frame(ctx, frame_instant(ctx)));
                assert!(resize_commands(&output).is_empty(), "{state} at {at:?}");
            }
        }
    }
}
