//! Native binding exercises over the actual workspace, feeds and frame adapters.
use super::*;
use crate::app::control_host::ScriptsPort;
use quantick_workspace::lifetime::{LifetimeError, ResourceError, WorkspaceId};

fn native_frame(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    id: egui::ViewportId,
) -> egui::FullOutput {
    let mut input = egui::RawInput {
        viewport_id: id,
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        ..Default::default()
    };
    input.viewports.entry(id).or_default();
    let now = frame_instant(ctx);
    ctx.run(input, |ctx| app.draw_frame(ctx, now))
}

#[test]
fn workspace_lifetime_native_rebind_retains_the_full_runtime_and_drains_without_paint() {
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    let (mut app, first, _commands, _book) = test_app();
    let second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    for events in [&first, &second.events] {
        events
            .try_send(FeedEvent::Backfilled((0..200).map(trade).collect()))
            .unwrap();
    }
    settled_frame(&mut app, &ctx);
    let scratch = crate::scratch::ScratchDir::new("workspace-rebind");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(scratch.join("paper"));
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    second.events.try_send(FeedEvent::Live(trade(201))).unwrap();
    app.drain_tabs();
    assert!(app.active_tab().paper.status_cell().is_some());
    app.attach_script(
        "owned".into(),
        "indicator(\"owned\")\nplot(close)".into(),
        true,
    );
    settle_indicators(&mut app);
    app.active_tab_mut().flow_pane.drawings.place(
        drawing_tool("horizontal-line"),
        ChartPoint::at_time(1.0, 100.0, Some(trade(50).timestamp_ms)),
    );
    app.active_tab_mut().replay = Some(quantick_feed::replay::test_support::detached_link(
        recording_at(&scratch.join("replay")),
    ));
    let target = *app
        .runtime
        .indicators
        .registry
        .operator_slots()
        .iter()
        .next()
        .unwrap();
    app.runtime.indicators.registry.defer_hidden(target);
    let owner = app.runtime.id();
    let view = app.presentation.binding.id();
    let commands: Vec<_> = app
        .runtime
        .tabs
        .iter()
        .map(|tab| tab.commands.clone())
        .collect();
    let sessions: Vec<_> = app
        .runtime
        .tabs
        .iter()
        .map(|tab| tab.session.snapshot())
        .collect();
    let identities = |app: &QuantickApp| {
        app.runtime
            .tabs
            .iter_with_ids()
            .flat_map(|(id, tab)| {
                tab.panes().map(move |(pane, _)| {
                    (
                        id,
                        pane.id,
                        pane.state.trades().iter().next().map(std::ptr::from_ref),
                        pane.indicator_worker.progress().instance,
                        pane.orderflow
                            .as_ref()
                            .map(|tape| tape.worker_progress().instance),
                    )
                })
            })
            .collect::<Vec<_>>()
    };
    let before = identities(&app);
    let saved = toml::to_string(&app.workspace_state().capture_workspace()).unwrap();
    let kinds = format!("{:?}", app.runtime.indicators.registry.slot_kinds());
    let replay = app.active_tab().replay.clone().unwrap();
    let paper = app.active_tab().paper.position_summary();
    let drawings = app.active_tab().flow_pane.drawings.items().len();
    app.open_indicator_picker_on(app.active_tab().focused_side());
    app.surfaces.indicator_picker.state.query = "owned".into();
    let picker = app.surfaces.indicator_picker.state.clone();
    let destination = egui::ViewportId::from_hash_of("rebound-workspace");
    app.presentation.rebind(&app.runtime, destination).unwrap();
    assert_eq!(app.runtime.id(), owner);
    assert_eq!(app.presentation.binding.id(), view);
    assert_eq!(before, identities(&app));
    assert_eq!(
        sessions,
        app.runtime
            .tabs
            .iter()
            .map(|tab| tab.session.snapshot())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        saved,
        toml::to_string(&app.workspace_state().capture_workspace()).unwrap()
    );
    assert_eq!(
        kinds,
        format!("{:?}", app.runtime.indicators.registry.slot_kinds())
    );
    assert_eq!(
        app.runtime.indicators.registry.pending_hidden(),
        vec![target]
    );
    assert!(
        app.runtime
            .indicators
            .registry
            .operator_slots()
            .contains(&target)
    );
    assert_eq!(paper, app.active_tab().paper.position_summary());
    assert_eq!(drawings, app.active_tab().flow_pane.drawings.items().len());
    let retained = app.active_tab().replay.as_ref().unwrap();
    assert!(std::sync::Arc::ptr_eq(&replay.session, &retained.session));
    assert!(std::sync::Arc::ptr_eq(&replay.status, &retained.status));
    for (tab, channel) in app.runtime.tabs.iter().zip(commands) {
        assert!(channel.same_channel(&tab.commands));
    }
    assert!(!scratch.join("paper/ETHUSDT").exists());
    let live: Vec<_> = app.runtime.tabs.iter().map(|tab| tab.live_trades).collect();
    first.try_send(FeedEvent::Live(trade(202))).unwrap();
    second.events.try_send(FeedEvent::Live(trade(202))).unwrap();
    assert!(
        native_frame(&mut app, &ctx, egui::ViewportId::ROOT)
            .shapes
            .is_empty()
    );
    assert_eq!(
        app.runtime
            .tabs
            .iter()
            .map(|tab| tab.live_trades)
            .collect::<Vec<_>>(),
        live.iter().map(|n| n + 1).collect::<Vec<_>>()
    );
    assert_eq!(app.surfaces.indicator_picker.state, picker);
    assert!(!native_frame(&mut app, &ctx, destination).shapes.is_empty());
    assert_eq!(app.surfaces.indicator_picker.state.target, picker.target);
    assert_eq!(app.surfaces.indicator_picker.state.query, picker.query);
    assert_eq!(
        app.runtime
            .tabs
            .iter()
            .map(|tab| tab.live_trades)
            .collect::<Vec<_>>(),
        live.iter().map(|n| n + 1).collect::<Vec<_>>()
    );
    assert_eq!(before, identities(&app));
}

#[test]
fn workspace_lifetime_rebind_keeps_report_and_popup_while_unpresented_tail_settles() {
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    let (mut app, _commands) = app_with_history(50);
    let scratch = crate::scratch::ScratchDir::new("workspace-tail-rebind");
    let paper = &mut app.active_tab_mut().paper;
    paper.redirect_history_dir(scratch.path().to_path_buf());
    paper.seed(&frame_tail_tests::print(0, 100));
    paper.market(quantick_engine::Side::Buy);
    paper.on_trade(&frame_tail_tests::print(1, 100));
    paper.on_trade(&frame_tail_tests::print(2, 105));
    let (report, env) = paper.report_parts();
    report.open(&env);
    app.active_tab_mut().notice =
        FeedNotice::attention("Fixture transport is paused", "Choose how to recover.");
    let tab = app.runtime.tabs.active_id();
    app.chrome.feed_popup_tab = Some(tab);
    for _ in 0..3 {
        native_frame(&mut app, &ctx, egui::ViewportId::ROOT);
    }
    settle_workers(&mut app);
    let before = native_frame(&mut app, &ctx, egui::ViewportId::ROOT);
    assert!(painted_text_center(&before, "Simulated performance").is_some());
    assert!(painted_text_center(&before, "Reload").is_some());
    let chip = app.chrome.feed_chip_rect.expect("visible feed chip");
    app.active_tab_mut().paper.on_timeline_reset();
    assert!(
        app.active_tab()
            .paper
            .report_state()
            .snapshot()
            .unwrap()
            .rows
            .is_empty(),
        "the pending journal hint has not settled into the open report"
    );
    let destination = egui::ViewportId::from_hash_of("tail-destination");
    app.presentation.rebind(&app.runtime, destination).unwrap();
    for _ in 0..2 {
        let old = native_frame(&mut app, &ctx, egui::ViewportId::ROOT);
        assert!(old.shapes.is_empty(), "the old native view must not paint");
        assert_eq!(app.chrome.feed_popup_tab, Some(tab));
        assert_eq!(app.chrome.feed_chip_rect, Some(chip));
        let report = app.active_tab().paper.report_state();
        assert!(report.is_open());
        assert!(
            report.snapshot().is_none(),
            "settlement reloads history and invalidates the previous painted view"
        );
        assert!(
            !app.active_tab_mut()
                .paper
                .account_mut()
                .take_journal_changed()
        );
    }
    // A destination can need its own sizing pass before the window paints text.
    native_frame(&mut app, &ctx, destination);
    let rebound = native_frame(&mut app, &ctx, destination);
    assert!(painted_text_center(&rebound, "Simulated performance").is_some());
    assert!(painted_text_center(&rebound, "Reload").is_some());
    assert_eq!(app.chrome.feed_popup_tab, Some(tab));
    assert_eq!(
        app.active_tab()
            .paper
            .report_state()
            .snapshot()
            .unwrap()
            .rows
            .len(),
        1
    );
    assert_eq!(
        quantick_paper::report::load_history(scratch.path(), Some("TESTUSDT"), &[])
            .rows
            .len(),
        1,
        "presentation changes never journal another close"
    );
}

#[test]
fn workspace_lifetime_refuses_foreign_missing_and_replaced_targets_before_detach() {
    let (mut app, _events, _commands, _book) = test_app();
    app.attach_script(
        "owned".into(),
        "indicator(\"owned\")\nplot(close)".into(),
        true,
    );
    let target = *app
        .runtime
        .indicators
        .registry
        .operator_slots()
        .iter()
        .next()
        .unwrap();
    for (invalid, reason) in [
        (
            TabSlot {
                workspace: WorkspaceId(999),
                ..target
            },
            ResourceError::ForeignWorkspace,
        ),
        (
            TabSlot {
                tab: u64::MAX,
                ..target
            },
            ResourceError::MissingTab,
        ),
        (
            TabSlot {
                pane: u64::MAX,
                ..target
            },
            ResourceError::MissingPane,
        ),
        (
            TabSlot {
                slot: SlotId(u64::MAX),
                ..target
            },
            ResourceError::MissingSlot,
        ),
    ] {
        assert_eq!(app.runtime.validate_target(invalid), Err(reason));
        if reason == ResourceError::MissingSlot {
            assert_eq!(app.detach_operator_script(invalid.slot.0), Ok(false));
            continue;
        }
        // Construct another ordinary owner to exercise stale/foreign dispatch;
        // production registry collections are never exposed for mutation.
        struct CapturedHost(TabSlot);
        impl quantick_chart::indicator_registry::IndicatorHost for CapturedHost {
            fn pane_id(&self) -> u64 {
                self.0.pane
            }
            fn add(&mut self, _: crate::indicator_worker::IndicatorSource) -> SlotId {
                self.0.slot
            }
            fn remove(&mut self, _: SlotId) {
                panic!("invalid target reached worker removal");
            }
        }
        let mut registry =
            quantick_chart::indicator_registry::IndicatorRegistry::new(invalid.workspace);
        registry.attach_script(
            &mut CapturedHost(invalid),
            (invalid.tab, invalid.side),
            "owned".into(),
            "indicator(\"owned\")\nplot(close)".into(),
            true,
        );
        let original = std::mem::replace(&mut app.runtime.indicators.registry, registry);
        assert_eq!(app.detach_operator_script(invalid.slot.0), Err(()));
        assert!(
            app.runtime
                .indicators
                .registry
                .operator_slots()
                .contains(&invalid)
        );
        app.runtime.indicators.registry = original;
    }
    let ctx = egui::Context::default();
    let _other = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_ne!(target.tab, app.runtime.tabs.active_id());
    assert_eq!(app.runtime.validate_target(target), Ok(()));
    assert_eq!(app.detach_operator_script(target.slot.0), Ok(true));
    assert_eq!(app.detach_operator_script(target.slot.0), Ok(false));
    assert!(
        !app.runtime
            .indicators
            .registry
            .slot_kinds()
            .iter()
            .any(|(owner, _)| *owner == target)
    );
}

#[test]
fn workspace_lifetime_explicit_close_journals_once_and_exit_preserves_open_paper() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let scratch = crate::scratch::ScratchDir::new("workspace-close-exit");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(scratch.join("paper"));
    ends.events
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.drain_tabs();
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    ends.events.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.drain_tabs();
    assert!(app.active_tab().paper.status_cell().is_some());
    app.presentation
        .rebind(&app.runtime, egui::ViewportId::from_hash_of("close-owner"))
        .unwrap();
    let plan = app.runtime.tabs.plan_close(1).unwrap();
    let retry = app.runtime.tabs.plan_close(1).unwrap();
    app.arrangement_adapter().close_planned(plan).unwrap();
    assert!(app.arrangement_adapter().close_planned(retry).is_err());
    assert!(ends.events.is_closed());
    let journal = std::fs::read_dir(scratch.join("paper/ETHUSDT"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal_before = std::fs::read(&journal).unwrap();
    app.active_tab_mut()
        .paper
        .redirect_history_dir(scratch.join("exit-paper"));
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    // The existing paper adapter accepts a seeded live tape on its first tab.
    let tab = app.active_tab_mut();
    tab.paper.on_trade(&trade(51));
    let position = app.active_tab().paper.position_summary();
    assert!(position.is_some());
    app.runtime
        .workspace
        .set_layouts_path(scratch.join("layouts.toml"));
    app.runtime
        .workspace
        .layouts_mut()
        .mark_changed(Instant::now());
    eframe::App::on_exit(&mut app, None);
    assert!(scratch.join("layouts.toml").exists());
    assert!(!app.runtime.workspace.layouts().is_dirty());
    let written = std::fs::read(scratch.join("layouts.toml")).unwrap();
    app.runtime
        .workspace
        .layouts_mut()
        .mark_changed(Instant::now());
    eframe::App::on_exit(&mut app, None);
    assert!(
        app.runtime.workspace.layouts().is_dirty(),
        "second exit never consumes another flush"
    );
    assert_eq!(
        written,
        std::fs::read(scratch.join("layouts.toml")).unwrap()
    );
    assert_eq!(position, app.active_tab().paper.position_summary());
    assert!(!scratch.join("exit-paper/TESTUSDT").exists());
    assert_eq!(journal_before, std::fs::read(journal).unwrap());
    assert_eq!(
        app.presentation
            .rebind(&app.runtime, egui::ViewportId::ROOT),
        Err(LifetimeError::ShuttingDown)
    );
}

#[test]
fn workspace_lifetime_reordered_pane_never_retargets_a_captured_slot() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(100);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    settle_indicators(&mut app);
    let target = *app
        .runtime
        .indicators
        .registry
        .slot_kinds()
        .iter()
        .map(|(target, _)| target)
        .find(|target| target.side == PaneSide::Time(0))
        .unwrap();
    app.apply_indicator_legend_action(
        target.tab,
        target.side,
        crate::indicator_legend::LegendAction::OpenSettings(target.slot),
    );
    assert!(app.layout_adapter().move_context_pane_at(target.tab, 1, 2));
    assert_eq!(
        app.runtime.validate_target(target),
        Err(ResourceError::MissingPane)
    );
    let moved = app.runtime.indicators.indicator_settings_target;
    assert_eq!(moved.pane, target.pane);
    assert_eq!(moved.side, PaneSide::Time(1));
    assert_eq!(app.runtime.validate_target(moved), Ok(()));
    let before = app.runtime.indicators.registry.slot_kinds().len();
    app.apply_indicator_edit(super::super::indicator_manager::IndicatorEdit::Remove(
        target,
    ));
    assert_eq!(app.runtime.indicators.registry.slot_kinds().len(), before);
    app.apply_indicator_edit(super::super::indicator_manager::IndicatorEdit::Remove(
        moved,
    ));
    assert!(app.runtime.validate_target(moved).is_err());
}

#[test]
fn workspace_lifetime_deferred_state_waits_for_its_stable_pane_not_a_replacement() {
    let (mut app, _events, _commands, _book) = test_app();
    app.attach_script(
        "pending".into(),
        "indicator(\"pending\")\nplot(close)".into(),
        true,
    );
    settle_indicators(&mut app);
    let target = *app
        .runtime
        .indicators
        .registry
        .operator_slots()
        .iter()
        .next()
        .unwrap();
    let mut style = crate::indicator_style::StyleOverride::default();
    style.set(
        0,
        crate::indicator_style::PlotOverride {
            width: Some(0.75),
            ..Default::default()
        },
    );
    app.runtime.indicators.registry.defer_hidden(target);
    app.runtime
        .indicators
        .registry
        .defer_style(target, style.clone());
    app.runtime
        .indicators
        .registry
        .defer_mouse_vertical_line(target);
    app.runtime
        .tabs
        .by_id_mut(target.tab)
        .unwrap()
        .pane_mut(target.side)
        .id = target.pane + 1000;
    app.layout_adapter().apply_pending_indicator_state();
    let view = app
        .runtime
        .tabs
        .by_id(target.tab)
        .unwrap()
        .pane(target.side)
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == target.slot)
        .unwrap();
    assert!(!view.hidden);
    assert!(view.style.is_default());
    assert!(!view.mouse_vertical_line);
    assert_eq!(app.runtime.indicators.registry.pending_hidden(), [target]);
    // Foreign deferred work must not reach an otherwise matching view either.
    let foreign = TabSlot {
        workspace: WorkspaceId(999),
        pane: target.pane + 1000,
        ..target
    };
    app.runtime.indicators.registry.defer_hidden(foreign);
    app.layout_adapter().apply_pending_indicator_state();
    assert_eq!(app.runtime.indicators.registry.pending_hidden(), [target]);
    app.runtime
        .tabs
        .by_id_mut(target.tab)
        .unwrap()
        .pane_mut(target.side)
        .id = target.pane;
    app.layout_adapter().apply_pending_indicator_state();
    let view = app
        .runtime
        .tabs
        .by_id(target.tab)
        .unwrap()
        .pane(target.side)
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == target.slot)
        .unwrap();
    assert!(view.hidden);
    assert_eq!(view.style, style);
    assert!(view.mouse_vertical_line);
    assert!(app.runtime.indicators.registry.pending_hidden().is_empty());
}
