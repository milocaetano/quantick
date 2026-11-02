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

#[derive(Default)]
struct NativeViewportOutputReplay {
    windows: egui::ViewportIdMap<ReplayedNativeViewport>,
}

struct ReplayedNativeViewport {
    builder: egui::ViewportBuilder,
    deferred: Vec<egui::ViewportCommand>,
    visible: bool,
    visibility: Vec<bool>,
    placements: usize,
}

impl NativeViewportOutputReplay {
    fn update(&mut self, id: egui::ViewportId, builder: egui::ViewportBuilder) {
        use std::collections::hash_map::Entry;
        match self.windows.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(ReplayedNativeViewport {
                    visible: builder.visible.unwrap_or(true),
                    builder,
                    deferred: Vec::new(),
                    visibility: Vec::new(),
                    placements: 0,
                });
            }
            Entry::Occupied(mut entry) => {
                let window = entry.get_mut();
                let (commands, recreate) = window.builder.patch(builder);
                assert!(!recreate, "tab transfers retain their native windows");
                window.deferred.extend(commands);
            }
        }
    }

    fn output(&mut self, output: egui::ViewportIdMap<egui::ViewportOutput>) {
        // Match eframe 0.29's handle_viewport_output: patch every builder,
        // then apply its deferred patch commands before explicit commands.
        for (id, output) in output {
            self.update(id, output.builder);
            let window = self.windows.get_mut(&id).unwrap();
            window.deferred.extend(output.commands);
            for command in window.deferred.drain(..) {
                match command {
                    egui::ViewportCommand::Visible(visible) => {
                        window.visible = visible;
                        window.visibility.push(visible);
                    }
                    egui::ViewportCommand::OuterPosition(_) => window.placements += 1,
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn market_windows_positioned_children_recover_visibility_after_repeated_backend_output() {
    use std::{cell::RefCell, rc::Rc};
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let identities = |app: &QuantickApp| {
        app.runtime
            .tabs
            .iter_with_ids()
            .map(|(id, tab)| {
                (
                    id,
                    std::ptr::from_ref(tab),
                    tab.flow_pane.indicator_worker.progress().instance,
                )
            })
            .collect::<Vec<_>>()
    };
    let before = identities(&app);
    let first = TabId(app.runtime.tabs.id_at(0));
    let second = TabId(app.runtime.tabs.id_at(1));
    let replay = Rc::new(RefCell::new(NativeViewportOutputReplay::default()));
    let renderer = Rc::clone(&replay);
    ctx.set_embed_viewports(false);
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        renderer
            .borrow_mut()
            .update(viewport.ids.this, viewport.builder);
        let mut input = viewport_input(WindowId::ROOT, 1.5, egui::Pos2::ZERO, vec![]);
        input.viewport_id = viewport.ids.this;
        let info = input.viewports.entry(viewport.ids.this).or_default();
        info.native_pixels_per_point = Some(1.5);
        info.inner_rect = input.screen_rect;
        let output = ctx.run(input, |ctx| (viewport.viewport_ui_cb)(ctx));
        renderer.borrow_mut().output(output.viewport_output);
    });
    let frame = |app: &mut QuantickApp| {
        let now = frame_instant(&ctx);
        let output = ctx.run(
            viewport_input(WindowId::ROOT, 1.5, egui::Pos2::ZERO, vec![]),
            |ctx| super::super::native_windows::draw(app, ctx, now),
        );
        replay.borrow_mut().output(output.viewport_output);
    };
    let detach = |app: &mut QuantickApp, tab| {
        let child = app
            .runtime
            .tabs
            .transfer_tab(tab, TransferTarget::NewWindow, 0)
            .unwrap()
            .destination;
        app.presentation.native.request_position(
            child,
            quantick_workspace::arrangement::tab_drag::Point { x: 420.0, y: 360.0 },
        );
        child
    };
    let assert_visible = |app: &QuantickApp| {
        let replay = replay.borrow();
        for window in app.runtime.tabs.windows().windows() {
            let id = super::super::native_windows::viewport(window.id);
            let native = &replay.windows[&id];
            assert!(native.visible, "live child {:?} remains hidden", window.id);
            assert_eq!(native.placements, 1, "placement must not pin later moves");
        }
    };

    let child = detach(&mut app, second);
    frame(&mut app);
    let child_viewport = super::super::native_windows::viewport(child);
    assert!(replay.borrow().windows[&child_viewport].visible);
    frame(&mut app);
    assert!(replay.borrow().windows[&child_viewport].visible);
    app.runtime.tabs.close_window(WindowId::ROOT).unwrap();
    let sibling = detach(&mut app, first);
    for _ in 0..3 {
        let source = app.runtime.tabs.windows().owner(second).unwrap();
        let destination = detach(&mut app, second);
        assert_ne!(source, destination);
        assert!(app.runtime.tabs.windows().window(source).is_none());
        assert!(app.runtime.tabs.windows().window(WindowId::ROOT).is_none());
        assert!(app.runtime.tabs.windows().window(sibling).is_some());
        frame(&mut app);
        frame(&mut app);
        assert_visible(&app);
        assert_eq!(identities(&app), before);
    }
    assert!(
        replay.borrow().windows.values().any(|window| {
            window
                .visibility
                .windows(2)
                .any(|pair| pair == [true, false])
        }),
        "an earlier sibling reapplies a later child's stale hidden builder before its next paint"
    );
    ctx.set_embed_viewports(true);
    egui::Context::set_immediate_viewport_renderer(|_, _| {
        panic!("unexpected immediate renderer after test cleanup");
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

#[test]
fn market_window_control_placement_reports_acceptance_separately_from_native_observation() {
    use quantick_control::wire::WireU64;
    use quantick_control_handlers::native_window::NativeWindowHost;
    use quantick_control_schema::{
        market_windows::{PhysicalPlacement, PlacementInput},
        native_window::{WindowInput, WindowState},
    };
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let child = app
        .runtime
        .tabs
        .transfer_tab(
            TabId(app.runtime.tabs.id_at(1)),
            TransferTarget::NewWindow,
            0,
        )
        .unwrap()
        .destination;
    let requested = PhysicalPlacement {
        x_px: -1700,
        y_px: 180,
        width_px: 900,
        height_px: 600,
    };
    let accepted = app
        .place_native_window(PlacementInput {
            window_id: Some(WireU64::new(child.0)),
            placement: requested,
        })
        .unwrap();
    assert_eq!(accepted.requested, requested);
    assert!(
        app.market_windows().windows[1].observed.is_none(),
        "request acceptance must not fabricate OS observation"
    );
    app.request_targeted_native_window(WindowInput {
        window_id: Some(WireU64::new(child.0)),
        state: WindowState::Maximized,
    })
    .unwrap();
    let now = frame_instant(&ctx);
    let output = ctx.run(
        viewport_input(child, 1.5, egui::pos2(-1200.0, 104.0), vec![]),
        |ctx| super::super::native_windows::paint(&mut app, ctx, child, now),
    );
    let commands = &output.viewport_output[&super::super::native_windows::viewport(child)].commands;
    assert!(
        commands.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
            requested.x_px as f32 / 1.5,
            requested.y_px as f32 / 1.5
        )))
    );
    assert!(commands.contains(&egui::ViewportCommand::InnerSize(egui::vec2(600.0, 400.0))));
    assert!(commands.contains(&egui::ViewportCommand::Maximized(true)));
    let observed = app.market_windows().windows[1]
        .observed
        .as_ref()
        .unwrap()
        .inner
        .unwrap();
    assert_eq!(
        observed.x_px, -1800,
        "observed desktop position comes from viewport input, not the requested -1700"
    );
    assert_eq!(
        app.runtime
            .tabs
            .windows()
            .window(WindowId::ROOT)
            .unwrap()
            .tabs()
            .len(),
        1
    );
}
