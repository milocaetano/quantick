//! Characterization captured before ArrangementLifecycle extraction.
use super::*;

#[test]
fn arrangement_baseline_only_the_matching_boot_tab_is_adopted() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let saved = app.capture_workspace();
    assert_eq!(app.tabs.id_at(0), FIRST_TAB_ID);
    app.arrangement_adapter().restore_workspace(saved);
    assert_eq!(app.tabs.id_at(0), FIRST_TAB_ID);

    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "TESTUSDT".to_owned(), None);
    app.arrangement_adapter().close_tab(0);
    let previous = app.tabs.id_at(0);
    assert_ne!(previous, FIRST_TAB_ID);
    let saved = app.capture_workspace();
    app.arrangement_adapter().restore_workspace(saved);
    assert_eq!(app.tabs.len(), 1);
    assert_ne!(
        app.tabs.id_at(0),
        previous,
        "same market after boot is replaced"
    );
    assert_eq!(app.tabs[0].symbol, "TESTUSDT");
}

#[test]
fn arrangement_baseline_close_keeps_position_then_clamps_and_cycle_wraps() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "SECOND".to_owned(), None);
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "THIRD".to_owned(), None);
    app.tabs.select(1);
    app.arrangement_adapter().close_tab(0);
    assert_eq!(app.tabs.active_index(), 1);
    assert_eq!(app.active_tab().symbol, "THIRD");
    app.arrangement_adapter().cycle_tab(1);
    assert_eq!(app.tabs.active_index(), 0);
    app.arrangement_adapter().cycle_tab(-1);
    assert_eq!(app.tabs.active_index(), 1);
    app.arrangement_adapter().close_tab(1);
    assert_eq!(app.tabs.active_index(), 0);
    assert_eq!(app.active_tab().symbol, "SECOND");
    let remaining = app.tabs.id_at(0);
    app.arrangement_adapter().close_tab(0);
    app.arrangement_adapter().close_tab(usize::MAX);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs.id_at(0), remaining);
}

#[test]
fn arrangement_baseline_empty_restore_applies_autosave_but_silent_favorites_keep_stars() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.toolrail.set_favorites(&["measure".to_owned()]);
    let before = app.tabs.id_at(0);
    let mut saved = app.capture_workspace();
    saved.tabs.clear();
    saved.save_on_exit = false;
    saved.favorite_tools.clear();
    saved.active_tab = usize::MAX;
    app.arrangement_adapter().restore_workspace(saved);
    assert!(!app.workspace.session().save_on_exit());
    assert_eq!(app.starred_tool_ids(), vec!["measure".to_owned()]);
    assert_eq!(app.tabs.id_at(0), before);
    assert_eq!(app.tabs.active_index(), 0);
}

#[test]
fn arrangement_baseline_duplicate_markets_keep_distinct_ids_and_clamp_saved_active() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let mut saved = app.capture_workspace();
    saved.tabs.push(saved.tabs[0].clone());
    saved.active_tab = usize::MAX;
    app.arrangement_adapter().restore_workspace(saved);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tabs.id_at(0), FIRST_TAB_ID);
    assert_ne!(app.tabs.id_at(0), app.tabs.id_at(1));
    assert_eq!(app.tabs[0].symbol, app.tabs[1].symbol);
    assert_eq!(app.tabs.active_index(), 1);
}

#[test]
fn arrangement_baseline_unknown_provider_keeps_the_last_runtime_and_applies_saved_canvas() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "SURVIVOR".to_owned(), None);
    let survivor = app.tabs.id_at(1);
    let mut saved = app.capture_workspace();
    saved.tabs.truncate(1);
    saved.tabs[0].feed = "missing-provider".to_owned();
    saved.tabs[0].symbol = "NOT-OPENED".to_owned();
    saved.tabs[0].layout = crate::config::DeclaredLayout::Flow;
    app.arrangement_adapter().restore_workspace(saved);
    // Inherited behavior: refusal does not insert, restoration still targets
    // the last runtime, and closing the last remaining tab is refused.
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs.id_at(0), survivor);
    assert_eq!(app.tabs[0].symbol, "SURVIVOR");
    assert!(matches!(
        app.capture_workspace().tabs[0].layout,
        crate::config::DeclaredLayout::Flow
    ));
}

#[test]
fn arrangement_baseline_restore_journals_stale_position_and_drops_its_feed() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let saved = app.capture_workspace();
    let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let old_ids: Vec<_> = app.tabs.iter_with_ids().map(|(tab_id, _)| tab_id).collect();
    let dir = crate::scratch::ScratchDir::new("arrangement-paper-restore");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(dir.path().to_path_buf());
    ends.events
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    ends.events.try_send(FeedEvent::Live(trade(4))).unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    assert!(app.active_tab().paper.status_cell().is_some());
    app.arrangement_adapter().restore_workspace(saved);
    assert_eq!(app.tabs.len(), 1);
    assert!(!old_ids.contains(&app.tabs.id_at(0)));
    assert_eq!(app.tabs[0].symbol, "TESTUSDT");
    assert!(
        ends.events.is_closed(),
        "stale runtime receiver was dropped"
    );
    let files: Vec<_> = std::fs::read_dir(dir.join("ETHUSDT"))
        .expect("restore journals the stale tab before dropping its runtime")
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn arrangement_runtime_swaps_cannot_replace_entry_identity_or_order() {
    let ctx = egui::Context::default();
    let (mut app, _evt, _cmd, _book) = test_app();
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tabs[0].symbol, "TESTUSDT");
    assert_eq!(app.tabs[1].symbol, "ETHUSDT");
    let ids: Vec<_> = app.tabs.iter_with_ids().map(|(id, _)| id).collect();
    let selected = app.tabs.active_id();
    {
        let mut runtimes = app.tabs.iter_mut();
        let first = runtimes.next().unwrap();
        let second = runtimes.next().unwrap();
        std::mem::swap(first, second);
    }
    assert_eq!(
        app.tabs
            .iter_with_ids()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(app.tabs.active_id(), selected);
    assert_eq!(app.tabs[0].symbol, "ETHUSDT");
    assert_eq!(app.tabs[1].symbol, "TESTUSDT");
}

#[test]
fn arrangement_stale_close_does_not_flatten_journal_remove_or_drop() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let tab_id = app.tabs.active_id();
    let dir = crate::scratch::ScratchDir::new("arrangement-stale-close");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(dir.path().to_path_buf());
    ends.events
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    ends.events.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    assert!(app.tabs[1].paper.status_cell().is_some());
    let membership = app.tabs[1].flow_pane.layout_view.clone();
    let layout = membership.layout();
    let plan = app.tabs.plan_close(1).unwrap();
    app.tabs.select(0);
    let result = app.tabs.close_planned(
        plan,
        &mut app.indicators,
        app.workspace.layouts_mut().session_mut(),
    );
    assert!(matches!(
        result,
        Err(quantick_workspace::arrangement::ArrangementError::StaleTransition)
    ));
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tabs.id_at(1), tab_id);
    assert!(app.tabs[1].paper.status_cell().is_some());
    assert!(!ends.events.is_closed());
    assert_eq!(membership.layout(), layout);
    assert!(
        !dir.join("ETHUSDT").exists(),
        "no journal was written for a refused stale close"
    );
}

#[test]
fn arrangement_idle_host_selection_and_runtime_views_allocate_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _evt, _cmd, _book) = test_app();
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let before = crate::work_meter::tally();
    for _ in 0..1000 {
        std::hint::black_box(app.tabs.active_id());
        std::hint::black_box(app.tabs.get(app.tabs.active_index()));
        for (id, runtime) in app.tabs.iter_with_ids() {
            std::hint::black_box((id, runtime.symbol.as_str()));
        }
        for (id, runtime) in app.tabs.iter_with_ids_mut() {
            std::hint::black_box((id, &mut runtime.symbol));
        }
    }
    let work = crate::work_meter::tally().since(before);
    assert_eq!(work.allocs, 0);
    assert_eq!(work.alloc_bytes, 0);
    assert_eq!(work.reallocs, 0);
    assert_eq!(work.realloc_copy_bytes, 0);
}
