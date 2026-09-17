use super::*;

/// The chip is the popup's only door, in both directions — the rule the
/// trader asked for after a card that opened itself every morning.
#[test]
fn the_chip_is_the_popups_only_door() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.active_tab_mut().forced_stall = Some(quantick_feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    assert!(
        !app.control_feed_popup_open(),
        "a stall alone must not open anything"
    );

    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    assert!(app.control_feed_popup_open(), "the chip opens it");

    let chip = app
        .control_feed_chip_rect()
        .expect("the corner is still up");
    click_chart(&mut app, &ctx, chip.center());
    assert!(!app.control_feed_popup_open(), "and the chip closes it");
}

/// The reason is one hover away, not one click: a trader mid-session who
/// wants to know *why* the tape stopped should not have to open something
/// and then put it away again.
#[test]
fn the_corner_answers_a_hover_without_being_opened() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    let ctx = egui::Context::default();
    // Bars, so the empty pane's own line cannot be what the hover finds.
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1), trade(2)]))
        .unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    app.active_tab_mut().forced_stall = Some(quantick_feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    let chip = app.control_feed_chip_rect().expect("the corner is up");
    let headline = app
        .active_tab()
        .stall_at(&app.config, metrics::wall_clock_ms())
        .expect("the stall is forced")
        .headline;

    let at = chip.center();
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    let output = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    assert!(
        painted_text(&output).iter().any(|text| text == &headline),
        "the hover has to carry the reason: {headline}"
    );
    assert!(
        !app.control_feed_popup_open(),
        "and it must not open anything"
    );
}

/// The window has no minimum, so a trader can drag it down to nothing —
/// and drag it back. Nothing is promised about the layout in between:
/// this asserts only that the frames at a degenerate size are survivable
/// and that the chart is whole again on the other side, which is the
/// difference between a window that reads badly and a chart that is gone.
#[test]
fn the_window_drags_down_to_nothing_and_comes_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);

    for size in [
        A_DEGENERATE_WINDOW,
        egui::vec2(0.0, 0.0),
        egui::vec2(1.0, 900.0),
        egui::vec2(1400.0, 0.0),
    ] {
        // Twice each: the first frame is where a cached layout from the
        // previous size is still around, and the second is the steady
        // state at this one.
        run_sized_frame(&mut app, &ctx, size, Vec::new());
        run_sized_frame(&mut app, &ctx, size, Vec::new());
    }

    let output = run_sized_frame(&mut app, &ctx, TEST_WINDOW, Vec::new());
    assert!(
        has_price_axis(&painted_text(&output)),
        "the chart paints again once the window has room"
    );
}

/// The corridor grows on the side the hand went, both ways. A channel
/// that always opened the same way would be a coin toss the trader has to
/// correct by hand every other time.
#[test]
fn the_corridor_opens_on_the_side_the_pointer_moved() {
    for (label, width_at, opens_below) in [
        ("down", egui::pos2(800.0, 460.0), true),
        ("up", egui::pos2(800.0, 220.0), false),
    ] {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 400.0),
            egui::pos2(800.0, 340.0),
        );
        click_chart_with(&mut app, &ctx, width_at, egui::Modifiers::NONE);

        let channel = app
            .active_tab()
            .flow_pane
            .drawings
            .items()
            .last()
            .expect("the click placed the channel")
            .clone();
        let line = trend_price_at(&channel.points, channel.points[2].bar);
        assert_eq!(
            channel.points[2].price < line,
            opens_below,
            "moving {label} opens the corridor {label}: {:?}",
            channel.points
        );
    }
}

/// A press and release a few pixels apart — the wander an ordinary click
/// carries — must stay a click.
///
/// Reported from the running build against the fixed-range profile: a
/// click placed **both** anchors, so the object was born less than one
/// bar wide, and completing it disarmed the tool. The pointer moving
/// afterwards then did nothing, which reads exactly like a frozen chart:
/// the trader is waiting for a range to follow their hand and there is no
/// longer a draft to follow it.
///
/// The gesture must instead become the click-move-click the hand was
/// already doing — draft alive, tool still armed, preview following.
#[test]
fn a_click_that_wanders_a_few_pixels_is_still_a_click() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "fixed-range-profile");

    let press = egui::pos2(600.0, 400.0);
    // Five pixels: inside the tremor of a real click, and it used to be
    // enough to finish the object.
    let release = egui::pos2(605.0, 400.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(press),
            pointer_button(press, true),
        ],
    );
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(release),
            pointer_button(release, false),
        ],
    );

    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "a click is not an object"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        1,
        "it anchored one end and is waiting for the other"
    );
    assert_eq!(
        app.toolrail
            .tool()
            .drawing_tool()
            .map(drawings::DrawingTool::id),
        Some("fixed-range-profile"),
        "and the tool is still armed, so the range can still follow the hand"
    );

    // The half of the gesture the trader was waiting for: the preview
    // follows, and the next click finishes a range worth having.
    let far = egui::pos2(900.0, 400.0);
    let output = move_chart_with(&mut app, &ctx, far, egui::Modifiers::NONE);
    assert!(
        drawing_strokes(&output) >= 2,
        "both edges of the range follow the pointer, painted {}",
        drawing_strokes(&output)
    );
    click_chart_with(&mut app, &ctx, far, egui::Modifiers::NONE);

    let profile = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the second click placed the profile")
        .clone();
    assert!(
        (profile.points[1].bar - profile.points[0].bar).abs() > 1.0,
        "and it spans real bars, not a fraction of one: {:?}",
        profile.points
    );
}

/// The same guarantee for the other tool whose third anchor gives a shape
/// its thickness: a triangle of three collinear corners is a line.
#[test]
fn a_dragged_triangle_is_born_with_an_area() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "triangle");
    let release = egui::pos2(800.0, 340.0);
    drag_chart(&mut app, &ctx, egui::pos2(600.0, 400.0), release);
    click_chart(&mut app, &ctx, release);

    let triangle = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the click committed the triangle")
        .clone();
    assert_eq!(triangle.tool.id(), "triangle");
    assert!(
        anchor_cross(&triangle.points).abs() > 0.0,
        "the third corner is off the first side; anchors {:?}",
        triangle.points
    );
}

/// Reported from the running app: "a pop Fib Retracement settings, quando
/// eu mexo ela e movo ela para algum lugar ela treme."
///
/// A drag is many frames, not one. Each frame the popup has to end up
/// exactly where the pointer put it — if the position it is drawn at and
/// the position the next frame's delta is added to disagree, the window
/// oscillates around the pointer instead of following it.
#[test]
fn dragging_the_popup_follows_the_pointer_without_shaking() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    draw_horizontal_line(&mut app, &ctx, 320.0);
    open_inspector(&mut app, &ctx);

    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open");
    let grip = egui::pos2(popup.left() + 60.0, popup.top() + 14.0);
    let start = popup.min;

    // Press, then walk the pointer one step at a time, reading where the
    // window actually landed on every frame of the gesture.
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(grip), pointer_button(grip, true)],
    );
    const STEP: f32 = 8.0;
    const STEPS: usize = 6;
    let mut drawn = Vec::with_capacity(STEPS);
    for step in 1..=STEPS {
        let at = grip + egui::vec2(STEP * step as f32, STEP * step as f32);
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        drawn.push(
            ctx.memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
                .expect("still open")
                .min,
        );
    }
    let travel = egui::vec2(STEP * STEPS as f32, STEP * STEPS as f32);
    let end = grip + travel;
    run_frame_with_events(&mut app, &ctx, vec![pointer_button(end, false)]);
    // One more frame: the window is positioned from the remembered point
    // at the top of a frame, so the last step of a gesture lands on the
    // frame after it. That single frame of lag is how an immediate-mode
    // window works; losing *steps* is the bug.
    run_frame(&mut app, &ctx);

    // Monotonic: every frame of a one-way drag moves the window the same
    // way the pointer went, never back.
    for pair in drawn.windows(2) {
        assert!(
            pair[1].x >= pair[0].x - 0.5 && pair[1].y >= pair[0].y - 0.5,
            "the popup went backwards mid-drag: {drawn:?}"
        );
    }
    // Every step arrives: the hand moved `travel`, so the window did too.
    // It used to arrive at half of it — the delta was being added to the
    // rect the window was *drawn* at, which is one frame behind the point
    // the next delta is added to, so each frame discarded the last one.
    let settled = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("still open")
        .min;
    assert!(
        (settled.x - (start.x + travel.x)).abs() <= 1.0
            && (settled.y - (start.y + travel.y)).abs() <= 1.0,
        "the popup should have travelled {travel:?} from {start:?}, settled at {settled:?}"
    );
}

/// And it is still there at the next launch, because that is the only
/// version of "remembers" worth having: a position that died with the
/// process would have the trader re-dragging the window every morning.
#[test]
fn a_parked_popup_comes_back_after_a_restart() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    with_a_saved_workspace(&mut app, &ctx, "popup-restart");
    draw_horizontal_line(&mut app, &ctx, 300.0);
    let parked = park_the_popup(&mut app, &ctx, egui::vec2(120.0, 80.0));

    // A second launch reading the same file, through the gate startup puts
    // in front of it — a workspace is filtered against the live config
    // before the app adopts it.
    let (mut next, _commands) = app_with_history(200);
    let config = next.config.clone();
    next.arrangement_adapter()
        .restore_workspace(ui_state::load(app.workspace.ui_state_path()).restore(&config));

    assert_eq!(
        next.drawings.chrome.inspector_pos(),
        Some(parked),
        "the popup opens where the last session left it"
    );
    assert!(
        next.drawings.chrome.inspector_moved(),
        "and counts as hand-placed, so nothing places it again"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

#[test]
fn button_manager_and_keyboard_send_the_same_delete_command() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    app.drawings.chrome.set_manager_open(true);
    // Let the manager window settle its size before reading button rects.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let delete = app
        .drawings
        .chrome
        .manager_action_rects()
        .iter()
        .find(|(index, action, _)| *index == 0 && *action == "Delete")
        .map(|(_, _, rect)| *rect)
        .expect("the manager lists the drawing with a Delete action");
    click_chart(&mut app, &ctx, delete.center());
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "the manager's Delete lands the same command"
    );
    assert!(
        app.surfaces.toast.message().is_some(),
        "the manager delete raises the same Undo toast as the keyboard"
    );
    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "one undo rewinds the manager delete, exactly like the keyboard one"
    );
}

/// The frame the note is placed on is already the frame the field is on,
/// so the placeholder never appears under it — not even for one frame.
///
/// The placement happens *inside* the canvas pass, so a host that waited
/// until the next frame to suppress the object would paint "Note" under
/// the field exactly once, on the click that made it.
#[test]
fn the_placeholder_never_flashes_under_the_field_it_is_replaced_by() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");

    let position = egui::pos2(700.0, 300.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    // The release is the frame the object is born on.
    let born = run_frame_with_events(&mut app, &ctx, vec![pointer_button(position, false)]);
    assert!(
        !painted_text(&born).iter().any(|text| text == "Note"),
        "the object must stand down on the very frame it is placed"
    );
}

#[test]
fn delete_and_backspace_never_leak_out_of_focused_inputs() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("test-text-input")));
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            key_press(egui::Key::Delete),
            key_press(egui::Key::Backspace),
        ],
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "while an input owns the keyboard the delete keys stay in it"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "with focus released the same key deletes the selection"
    );
}

#[test]
fn keyboard_delete_offers_an_undo_toast_and_ctrl_z_restores() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
    let texts = painted_text(&run_frame(&mut app, &ctx));
    // The toast names the tool: on a crowded chart "Drawing deleted"
    // leaves the trader unable to tell whether they want the undo.
    for label in ["Horizontal line deleted.", "Undo"] {
        assert!(
            texts.iter().any(|text| text.contains(label)),
            "the toast must offer {label:?}; painted: {texts:?}"
        );
    }

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "Ctrl+Z drives the same history as the toast's Undo"
    );

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Y, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "Ctrl+Y redoes the undone delete"
    );
}

#[test]
fn alt_l_and_alt_h_protect_the_selection_from_the_keyboard() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::L, egui::Modifiers::ALT)],
        egui::Modifiers::ALT,
    );
    assert!(
        app.active_tab().flow_pane.drawings.items()[0].locked,
        "Alt+L locks the selection"
    );

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::H, egui::Modifiers::ALT)],
        egui::Modifiers::ALT,
    );
    assert!(
        app.active_tab().flow_pane.drawings.items()[0].hidden,
        "Alt+H hides the selection"
    );
    assert_eq!(
        app.toolrail.tool(),
        Tool::Pointer,
        "Alt+H must not arm the horizontal-line tool"
    );
}

#[test]
fn ctrl_d_duplicates_the_selection_offset_and_selected() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let original_bar = app.active_tab().flow_pane.drawings.items()[0].points[0].bar;

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::D, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 2);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(1));
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[1].points[0].bar,
        original_bar + DUPLICATE_OFFSET_BARS
    );
}

#[test]
fn the_paste_chord_pastes_a_drawing_when_the_platform_sends_no_paste_event() {
    // What the trader hit: with an image (or nothing) on the Windows
    // clipboard, `egui-winit` sends neither `Event::Paste` nor the pressed
    // `Key::V` — only the release survives, so the release is the signal.
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let original = app.active_tab().flow_pane.drawings.items()[0].clone();

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::C, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    // The press frame of Ctrl+V, exactly as the platform delivers it with a
    // non-text clipboard: the key is swallowed and no paste event arrives.
    run_frame_with_modifiers(&mut app, &ctx, Vec::new(), egui::Modifiers::COMMAND);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "nothing to paste from yet"
    );

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_release_with(egui::Key::V, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    let drawings = &app.active_tab().flow_pane.drawings;
    assert_eq!(drawings.items().len(), 2, "the release pasted the copy");
    assert_eq!(drawings.selected(), Some(1));
    assert_eq!(drawings.items()[1].tool, original.tool);
    assert_eq!(
        drawings.items()[1].points[0].bar,
        original.points[0].bar + DUPLICATE_OFFSET_BARS
    );
}

#[test]
fn the_paste_chord_pastes_once_when_the_platform_also_sends_a_paste_event() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::C, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    // A text clipboard: the press frame carries `Event::Paste` instead of
    // the key, and the release frame follows. One gesture, one drawing.
    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![egui::Event::Paste("clipboard text".to_owned())],
        egui::Modifiers::COMMAND,
    );
    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_release_with(egui::Key::V, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        2,
        "two signals for one chord must paste exactly one drawing"
    );
}

#[test]
fn native_clipboard_events_paste_the_copied_drawing_not_the_current_selection() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let original = app.active_tab().flow_pane.drawings.items()[0].clone();
    let undo_before_copy = app.active_tab().flow_pane.drawings.undo_depth();

    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Copy]);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items(),
        std::slice::from_ref(&original)
    );
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        undo_before_copy,
        "copy is not a chart edit"
    );

    app.toolrail
        .arm(Tool::Drawing(drawing_tool("vertical-line")));
    click_chart(&mut app, &ctx, egui::pos2(760.0, 360.0));
    let other = app.active_tab().flow_pane.drawings.items()[1].clone();
    assert_ne!(other.tool, original.tool, "the current selection differs");
    let undo_before_paste = app.active_tab().flow_pane.drawings.undo_depth();

    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Paste(String::new())]);
    let drawings = &app.active_tab().flow_pane.drawings;
    assert_eq!(drawings.items().len(), 3);
    assert_eq!(drawings.selected(), Some(2), "the pasted copy is selected");
    let pasted = &drawings.items()[2];
    assert_ne!(pasted.id, original.id, "paste allocates a fresh identity");
    assert_eq!(
        pasted.tool, original.tool,
        "paste uses the clipboard snapshot"
    );
    assert_eq!(pasted.style, original.style);
    assert_eq!(
        pasted.points[0].bar,
        original.points[0].bar + DUPLICATE_OFFSET_BARS
    );
    assert!(!pasted.locked, "a pasted drawing is editable");
    assert_eq!(
        pasted.name, None,
        "a pasted drawing has no ambiguous copied name"
    );
    assert_eq!(
        drawings.undo_depth(),
        undo_before_paste + 1,
        "paste is exactly one undo entry"
    );
}

#[test]
fn drawing_clipboard_targets_the_focused_pane_and_repeated_pastes_do_not_overlap() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Time(0), 101.0);
    app.active_tab_mut().focus = PaneSide::Time(0);
    let source = app.active_tab().pane(PaneSide::Time(0)).drawings.items()[0].clone();

    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Copy]);
    let flow = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, flow);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
    for _ in 0..2 {
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::Paste(String::new())]);
    }

    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).drawings.items(),
        std::slice::from_ref(&source),
        "paste leaves the source pane untouched"
    );
    let pasted = app.active_tab().pane(PaneSide::Flow).drawings.items();
    assert_eq!(pasted.len(), 2);
    assert_eq!(pasted[0].tool, source.tool);
    assert_eq!(pasted[1].tool, source.tool);
    assert_eq!(
        pasted[0].points[0].bar,
        source.points[0].bar + DUPLICATE_OFFSET_BARS
    );
    assert_eq!(
        pasted[1].points[0].bar,
        source.points[0].bar + DUPLICATE_OFFSET_BARS * 2.0,
        "each paste advances from the clipboard source"
    );
    assert_eq!(
        app.active_tab().pane(PaneSide::Flow).drawings.selected(),
        Some(1),
        "each paste selects the new drawing"
    );

    for expected_len in [1, 0] {
        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(
            app.active_tab().pane(PaneSide::Flow).drawings.items().len(),
            expected_len,
            "each undo removes exactly one pasted drawing"
        );
    }
}

#[test]
fn ctrl_v_before_copy_is_a_no_op() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Paste(String::new())]);

    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
    assert_eq!(app.active_tab().flow_pane.drawings.undo_depth(), 0);
}

#[test]
fn native_clipboard_events_remain_available_to_a_focused_text_field() {
    let (mut app, _commands) = app_with_history(200);
    let drawing_ctx = egui::Context::default();
    run_frame(&mut app, &drawing_ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &drawing_ctx, egui::pos2(700.0, 300.0));
    let text_ctx = egui::Context::default();
    let text_id = egui::Id::new("native-clipboard-text-field");
    let mut text = "chart title".to_owned();

    let _ = text_ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(text_id))
                .request_focus();
        });
    });
    let _ = text_ctx.run(
        egui::RawInput {
            events: vec![key_press_with(egui::Key::A, egui::Modifiers::COMMAND)],
            modifiers: egui::Modifiers::COMMAND,
            ..Default::default()
        },
        |ctx| {
            app.drawings.handle_drawing_keys(
                &mut crate::app::drawing_controller::DrawingAccess::new(&mut app.tabs),
                &mut app.toolrail,
                &mut *app.audio.alerts,
                ctx,
                Instant::now(),
            );
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(text_id));
            });
        },
    );
    let copied = text_ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Copy],
            ..Default::default()
        },
        |ctx| {
            app.drawings.handle_drawing_keys(
                &mut crate::app::drawing_controller::DrawingAccess::new(&mut app.tabs),
                &mut app.toolrail,
                &mut *app.audio.alerts,
                ctx,
                Instant::now(),
            );
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(text_id));
            });
        },
    );
    assert_eq!(copied.platform_output.copied_text, "chart title");

    let _ = text_ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Paste("replacement".to_owned())],
            ..Default::default()
        },
        |ctx| {
            app.drawings.handle_drawing_keys(
                &mut crate::app::drawing_controller::DrawingAccess::new(&mut app.tabs),
                &mut app.toolrail,
                &mut *app.audio.alerts,
                ctx,
                Instant::now(),
            );
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(text_id));
            });
        },
    );
    assert_eq!(text, "replacement");
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "text clipboard events must not paste or replace a drawing"
    );
}

#[test]
fn arrow_nudges_move_the_selection_and_shift_multiplies_by_ten() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let start = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let depth = app.active_tab().flow_pane.drawings.undo_depth();

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowRight)]);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points[0].bar,
        start.bar + 1.0,
        "one press is one bar"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        depth + 1,
        "one press, one entry"
    );

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(
            egui::Key::ArrowRight,
            egui::Modifiers::SHIFT,
        )],
        egui::Modifiers::SHIFT,
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points[0].bar,
        start.bar + 11.0,
        "Shift multiplies the nudge by ten"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowUp)]);
    assert!(
        app.active_tab().flow_pane.drawings.items()[0].points[0].price > start.price,
        "ArrowUp raises the price"
    );
}

#[test]
fn drawing_controller_characterization_delete_precedes_stale_manager_rows() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    app.resolve_drawing_response(
        crate::surfaces::drawing_chrome::DrawingChromeAsk {
            delete_all: true,
            manager_select: Some(0),
            manager_toggle_hidden: Some(0),
            manager_toggle_locked: Some(0),
            ..Default::default()
        },
        Instant::now(),
    );
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), None);
    app.active_tab_mut().flow_pane.drawings.undo();
    let drawings = &app.active_tab().flow_pane.drawings;
    assert_eq!(drawings.items().len(), 1);
    assert!(!drawings.items()[0].hidden);
    assert!(!drawings.items()[0].locked);
}

#[test]
fn drawing_controller_characterization_cancel_follows_locked_delete_request() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_selected_locked(true);
    app.resolve_drawing_response(
        crate::surfaces::drawing_chrome::DrawingChromeAsk {
            request_delete: true,
            cancel_delete: true,
            ..Default::default()
        },
        Instant::now(),
    );
    assert!(!app.drawings.chrome.delete_confirm());
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
    assert!(app.active_tab().flow_pane.drawings.items()[0].locked);
}

#[test]
fn drawing_controller_preserves_undo_before_deferred_preference_notice() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    app.surfaces.toast.clear();
    app.resolve_drawing_response(
        crate::surfaces::drawing_chrome::DrawingChromeAsk {
            force_delete: true,
            saved_default: Some(crate::surfaces::drawing_chrome::SavedDefault::OneTool),
            ..Default::default()
        },
        Instant::now(),
    );
    assert_eq!(app.surfaces.toast.message(), Some("Drawing deleted."));
    assert!(app.surfaces.toast.offers_undo());
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
}

#[test]
fn drawing_controller_combined_destructive_notices_keep_last_undo() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    for y in [260.0, 320.0, 380.0] {
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, y));
    }
    let drawings = &mut app.active_tab_mut().flow_pane.drawings;
    drawings.select(Some(1));
    drawings.selected_mut().unwrap().author = Some(drawings::DrawingAuthor {
        actor_kind: "agent".into(),
        client_name: "fixture".into(),
    });
    drawings.select(Some(2));
    drawings.set_selected_locked(true);
    app.surfaces.toast.clear();
    app.resolve_drawing_response(
        crate::surfaces::drawing_chrome::DrawingChromeAsk {
            request_delete: true,
            cancel_delete: true,
            force_delete: true,
            duplicate: true,
            saved_default: Some(crate::surfaces::drawing_chrome::SavedDefault::OneTool),
            sweep_authored: true,
            delete_all: true,
            manager_delete: Some(0),
            show_all: true,
            unlock_all: true,
            ..Default::default()
        },
        Instant::now(),
    );
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
    assert_eq!(app.surfaces.toast.message(), Some("All drawings deleted."));
    assert!(app.surfaces.toast.offers_undo());
    app.active_tab_mut().flow_pane.drawings.undo();
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
    assert!(
        app.active_tab().flow_pane.drawings.items()[0]
            .author
            .is_none()
    );
}
