use super::*;
use quantick_workspace::canvas::{CanvasRect, PaneKind, preset};

fn pointer(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(
    ctx: &egui::Context,
    menus: &mut PaneMenus,
    state: &CanvasState,
    slot: u32,
    events: Vec<egui::Event>,
) -> (bool, egui::FullOutput) {
    let mut clicked = false;
    menus.lift = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
        events,
        ..Default::default()
    };
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let strip = Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(500.0, 34.0));
            menus.header(ui, strip, slot, state);
            clicked = ui
                .put(
                    Rect::from_min_size(egui::pos2(20.0, 15.0), vec2(90.0, 24.0)),
                    egui::Button::new("Options"),
                )
                .clicked();
        });
    });
    (clicked, output)
}

#[test]
fn header_drag_preserves_click_threshold_and_child_button_priority() {
    let ctx = egui::Context::default();
    let mut menus = PaneMenus::default();
    let mut state = CanvasState::from_preset(preset("charts-2").unwrap());
    let slot = state
        .add(PaneKind::Time, CanvasRect::new(0.0, 0.0, 600.0, 400.0))
        .unwrap();
    frame(&ctx, &mut menus, &state, slot, vec![]);
    for start in [egui::pos2(220.0, 27.0), egui::pos2(470.0, 27.0)] {
        frame(
            &ctx,
            &mut menus,
            &state,
            slot,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        frame(
            &ctx,
            &mut menus,
            &state,
            slot,
            vec![egui::Event::PointerMoved(start + vec2(2.0, 0.0))],
        );
        assert_eq!(menus.lift, None, "a small movement stays a click");
        let end = start + vec2(20.0, 40.0);
        frame(
            &ctx,
            &mut menus,
            &state,
            slot,
            vec![egui::Event::PointerMoved(end)],
        );
        assert_eq!(menus.lift, Some(slot), "free title space moves the pane");
        frame(&ctx, &mut menus, &state, slot, vec![pointer(end, false)]);
    }
    let button = egui::pos2(60.0, 27.0);
    frame(
        &ctx,
        &mut menus,
        &state,
        slot,
        vec![egui::Event::PointerMoved(button), pointer(button, true)],
    );
    assert!(frame(&ctx, &mut menus, &state, slot, vec![pointer(button, false)]).0);
    frame(&ctx, &mut menus, &state, slot, vec![pointer(button, true)]);
    frame(
        &ctx,
        &mut menus,
        &state,
        slot,
        vec![egui::Event::PointerMoved(button + vec2(60.0, 60.0))],
    );
    assert_eq!(
        menus.lift, None,
        "a drag starting on a button is not a title drag"
    );
}

#[test]
fn a_pop_out_header_requests_native_movement_instead_of_lifting_to_canvas() {
    let ctx = egui::Context::default();
    let mut menus = PaneMenus::default();
    let mut state = CanvasState::from_preset(preset("charts-2").unwrap());
    let rect = CanvasRect::new(0.0, 0.0, 600.0, 400.0);
    let slot = state.add(PaneKind::Time, rect).unwrap();
    state.pop_out(slot, "primary", rect).unwrap();
    frame(&ctx, &mut menus, &state, slot, vec![]);
    let start = egui::pos2(300.0, 27.0);
    frame(
        &ctx,
        &mut menus,
        &state,
        slot,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    let (_, output) = frame(
        &ctx,
        &mut menus,
        &state,
        slot,
        vec![egui::Event::PointerMoved(start + vec2(30.0, 40.0))],
    );
    assert_eq!(menus.lift, None);
    assert!(output.viewport_output.values().any(|viewport| {
        viewport
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::StartDrag))
    }));
}
