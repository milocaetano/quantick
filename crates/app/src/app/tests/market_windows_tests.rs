//! Real frame adapters over one retained runtime, including hidden-root intake.
use super::*;
use quantick_workspace::arrangement::{
    TabId,
    windows::{TransferTarget, WindowId},
};

fn viewport_input(
    id: WindowId,
    scale: f32,
    origin: egui::Pos2,
    events: Vec<egui::Event>,
) -> egui::RawInput {
    let viewport = super::super::native_windows::viewport(id);
    let mut input = egui::RawInput {
        viewport_id: viewport,
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        events,
        ..Default::default()
    };
    let info = input.viewports.entry(viewport).or_default();
    info.native_pixels_per_point = Some(scale);
    info.inner_rect = Some(egui::Rect::from_min_size(origin, TEST_WINDOW));
    input
}

fn paint_window(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    id: WindowId,
    scale: f32,
    origin: egui::Pos2,
) {
    let now = frame_instant(ctx);
    let _ = ctx.run(viewport_input(id, scale, origin, vec![]), |ctx| {
        super::super::native_windows::paint(app, ctx, id, now)
    });
}

#[test]
fn market_windows_native_paint_never_drains_or_reconstructs_runtime_owners() {
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    let (mut app, first, _, _) = test_app();
    let second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let identities = |app: &QuantickApp| {
        app.runtime
            .tabs
            .iter_with_ids()
            .map(|(id, tab)| {
                (
                    id,
                    std::ptr::from_ref(tab),
                    tab.flow_pane.indicator_worker.progress().instance,
                    tab.session.snapshot(),
                )
            })
            .collect::<Vec<_>>()
    };
    let before = identities(&app);
    let second_id = TabId(app.runtime.tabs.id_at(1));
    let child = app
        .runtime
        .tabs
        .transfer_tab(second_id, TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    assert_eq!(identities(&app), before);
    for source in [&first, &second.events] {
        source.try_send(FeedEvent::Live(trade(1))).unwrap();
    }
    let now = frame_instant(&ctx);
    let _ = ctx.run(
        viewport_input(WindowId::ROOT, 1.5, egui::pos2(0.0, 0.0), vec![]),
        |ctx| super::super::frame::process(&mut app, ctx, now),
    );
    assert!(app.runtime.tabs.iter().all(|tab| tab.live_trades == 1));
    for source in [&first, &second.events] {
        source.try_send(FeedEvent::Live(trade(2))).unwrap();
    }
    let intake_frame = app.health.last_frame;
    paint_window(&mut app, &ctx, WindowId::ROOT, 1.5, egui::pos2(0.0, 0.0));
    paint_window(&mut app, &ctx, child, 1.0, egui::pos2(-1920.0, 104.0));
    assert_eq!(
        app.health.last_frame, intake_frame,
        "view painting cannot become a process tick"
    );
    assert!(
        app.runtime.tabs.iter().all(|tab| tab.live_trades == 1),
        "both view paints leave intake to the one process stage"
    );
    let now = frame_instant(&ctx);
    let _ = ctx.run(
        viewport_input(WindowId::ROOT, 1.5, egui::Pos2::ZERO, vec![]),
        |ctx| super::super::frame::process(&mut app, ctx, now),
    );
    assert!(app.runtime.tabs.iter().all(|tab| tab.live_trades == 2));
    assert_eq!(
        app.runtime
            .tabs
            .windows()
            .window(WindowId::ROOT)
            .unwrap()
            .selected(),
        TabId(app.runtime.tabs.id_at(0))
    );
    assert_eq!(
        app.runtime.tabs.windows().window(child).unwrap().selected(),
        second_id
    );
    app.runtime.tabs.close_window(WindowId::ROOT).unwrap();
    assert!(app.runtime.tabs.windows().window(WindowId::ROOT).is_none());
    first.try_send(FeedEvent::Live(trade(3))).unwrap();
    let now = frame_instant(&ctx);
    let _ = ctx.run(
        viewport_input(WindowId::ROOT, 1.5, egui::Pos2::ZERO, vec![]),
        |ctx| super::super::frame::process(&mut app, ctx, now),
    );
    assert_eq!(
        app.runtime.tabs[0].live_trades, 3,
        "a hidden native root still services every retained feed"
    );
    let after = identities(&app);
    assert_eq!(
        before
            .iter()
            .map(|entry| (&entry.0, &entry.1, &entry.2))
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|entry| (&entry.0, &entry.1, &entry.2))
            .collect::<Vec<_>>()
    );
}

#[test]
fn market_windows_capture_all_native_strips_in_visible_order_with_matching_selected_index() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    app.config.feeds[0].symbols.push("BTCUSDT".to_owned());
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let _third = open_second_tab(&mut app, &ctx, "BTCUSDT");
    let ids: Vec<_> = app
        .runtime
        .tabs
        .iter_with_ids()
        .map(|(id, _)| TabId(id))
        .collect();
    app.runtime
        .tabs
        .transfer_tab(ids[2], TransferTarget::Window(WindowId::ROOT), 0)
        .unwrap();
    let child = app
        .runtime
        .tabs
        .transfer_tab(ids[0], TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    app.runtime.tabs.focus_window(child).unwrap();
    app.runtime.tabs.present_window(child).unwrap();
    let saved = app.workspace_state().capture_workspace();
    assert_eq!(
        saved
            .tabs
            .iter()
            .map(|tab| tab.symbol.as_str())
            .collect::<Vec<_>>(),
        ["BTCUSDT", "ETHUSDT", "TESTUSDT"]
    );
    assert_eq!(saved.active_tab, 2);
    let roundtrip: ui_state::Workspace = toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
    assert_eq!(roundtrip.tabs.len(), 3);
    assert_eq!(roundtrip.active_tab, 2);
    let restored = roundtrip.restore(&app.config);
    assert_eq!(restored.tabs, saved.tabs);
    app.workspace_save_adapter()
        .save_named_workspace("all-windows");
    let bookmark = &app.runtime.workspace.session().bookmarks()[0];
    assert_eq!(bookmark.tabs, saved.tabs);
    assert_eq!(bookmark.active_tab, 2);
}

#[test]
fn market_windows_actual_pointer_reorder_is_atomic_and_escape_wins_release() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    app.config.feeds[0].symbols.push("BTCUSDT".to_owned());
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let _third = open_second_tab(&mut app, &ctx, "BTCUSDT");
    let frame = |app: &mut QuantickApp, events| {
        let now = frame_instant(&ctx);
        ctx.run(
            viewport_input(WindowId::ROOT, 1.0, egui::Pos2::ZERO, events),
            |ctx| super::super::native_windows::draw(app, ctx, now),
        )
    };
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let ids: Vec<_> = app
        .runtime
        .tabs
        .iter_with_ids()
        .map(|(id, _)| TabId(id))
        .collect();
    let first = app
        .chrome
        .title_bar
        .strip
        .tabs
        .iter()
        .find(|(index, _)| *index == 0)
        .unwrap()
        .1
        .center();
    let third = app
        .chrome
        .title_bar
        .strip
        .tabs
        .iter()
        .find(|(index, _)| *index == 2)
        .unwrap()
        .1;
    let target = egui::pos2(third.right() - 4.0, third.center().y);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &mut app,
        vec![egui::Event::PointerMoved(first), button(first, true)],
    );
    let moving = frame(&mut app, vec![egui::Event::PointerMoved(target)]);
    assert!(app.presentation.native.gesture.active().is_some());
    assert_eq!(
        app.runtime
            .tabs
            .windows()
            .window(WindowId::ROOT)
            .unwrap()
            .tabs(),
        ids
    );
    assert!(moving.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::LineSegment { stroke,.. } if stroke.color == egui::epaint::ColorMode::Solid(crate::theme::AMBER) && stroke.width == 3.0_f32)),"the actual drag paints its insertion position");
    frame(&mut app, vec![button(target, false)]);
    assert_eq!(
        app.runtime
            .tabs
            .windows()
            .window(WindowId::ROOT)
            .unwrap()
            .tabs(),
        [ids[1], ids[2], ids[0]]
    );
    let before = app.runtime.tabs.windows().clone();
    let chip = app.chrome.title_bar.strip.tabs[0].1.center();
    frame(
        &mut app,
        vec![egui::Event::PointerMoved(chip), button(chip, true)],
    );
    let outside = egui::pos2(chip.x, 150.0);
    frame(&mut app, vec![egui::Event::PointerMoved(outside)]);
    frame(
        &mut app,
        vec![
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            button(outside, false),
        ],
    );
    assert_eq!(app.runtime.tabs.windows(), &before);
    assert!(app.presentation.native.gesture.active().is_none());
}

#[test]
fn market_windows_last_tab_detach_and_native_root_close_preserve_siblings() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let second = TabId(app.runtime.tabs.id_at(1));
    let child = app
        .runtime
        .tabs
        .transfer_tab(second, TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    let mut input = viewport_input(WindowId::ROOT, 1.5, egui::Pos2::ZERO, vec![]);
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .events
        .push(egui::ViewportEvent::Close);
    let now = frame_instant(&ctx);
    let output = ctx.run(input, |ctx| {
        super::super::native_windows::paint(&mut app, ctx, WindowId::ROOT, now)
    });
    let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(commands.contains(&egui::ViewportCommand::Visible(false)));
    assert_eq!(app.runtime.tabs.len(), 2);
    assert_eq!(app.runtime.tabs.windows().windows().len(), 1);
    assert_eq!(app.runtime.tabs.windows().windows()[0].id, child);
    let moved = app
        .runtime
        .tabs
        .transfer_tab(second, TransferTarget::NewWindow, 0)
        .unwrap();
    assert_ne!(moved.destination, child);
    assert_eq!(app.runtime.tabs.len(), 2);
}

#[test]
fn market_windows_final_child_close_exits_its_hidden_root_shell() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let tab = TabId(app.runtime.tabs.active_id());
    let child = app
        .runtime
        .tabs
        .transfer_tab(tab, TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    assert!(
        app.runtime.tabs.windows().window(WindowId::ROOT).is_none(),
        "the last root tab may detach"
    );
    let mut input = viewport_input(child, 1.0, egui::pos2(-1920.0, 104.0), vec![]);
    input
        .viewports
        .get_mut(&super::super::native_windows::viewport(child))
        .unwrap()
        .events
        .push(egui::ViewportEvent::Close);
    let now = frame_instant(&ctx);
    let output = ctx.run(input, |ctx| {
        super::super::native_windows::paint(&mut app, ctx, child, now)
    });
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::Close)
    );
    let mut input = viewport_input(WindowId::ROOT, 1.5, egui::Pos2::ZERO, vec![]);
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .events
        .push(egui::ViewportEvent::Close);
    let now = frame_instant(&ctx);
    let output = ctx.run(input, |ctx| {
        super::super::native_windows::draw(&mut app, ctx, now)
    });
    assert!(
        !output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::CancelClose)
    );
    assert_eq!(
        app.runtime.tabs.len(),
        1,
        "exit keeps the tab available for final workspace save"
    );
}

#[test]
fn market_windows_physical_placement_uses_child_scale_after_a_higher_dpi_source() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    paint_window(&mut app, &ctx, WindowId::ROOT, 1.5, egui::Pos2::ZERO);
    let tab = TabId(app.runtime.tabs.id_at(1));
    let child = app
        .runtime
        .tabs
        .transfer_tab(tab, TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    app.presentation.native.request_position(
        child,
        quantick_workspace::arrangement::tab_drag::Point {
            x: -1800.0,
            y: 180.0,
        },
    );
    let now = frame_instant(&ctx);
    let output = ctx.run(
        viewport_input(child, 1.0, egui::pos2(-1920.0, 104.0), vec![]),
        |ctx| super::super::native_windows::paint(&mut app, ctx, child, now),
    );
    let commands = &output.viewport_output[&super::super::native_windows::viewport(child)].commands;
    assert!(
        commands.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
            -1800.0, 180.0
        )))
    );
    assert!(commands.contains(&egui::ViewportCommand::Visible(true)));
    let now = frame_instant(&ctx);
    let next = ctx.run(
        viewport_input(child, 1.0, egui::pos2(-1800.0, 180.0), vec![]),
        |ctx| super::super::native_windows::paint(&mut app, ctx, child, now),
    );
    assert!(
        !next.viewport_output[&super::super::native_windows::viewport(child)]
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::OuterPosition(_))),
        "one-time placement cannot pin a trader's independently moved window"
    );
}
