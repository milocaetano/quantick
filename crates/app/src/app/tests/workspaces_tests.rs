use super::*;

/// The whole point of the feature, end to end inside a running app:
/// export a cockpit, change it, open the file back, and the cockpit the
/// trader saved is the cockpit on screen.
///
/// Under test every store resolves to a per-process scratch home
/// (`store_home::test_path`), so this touches no real documents folder.
#[test]
fn a_cockpit_exported_from_the_app_comes_back_when_it_is_opened() {
    let (mut app, _evt, _cmd, _book) = test_app();
    // A cockpit worth keeping: a layer off, a symbol added, a rail
    // favourite — three different stores.
    app.added_symbols.add("binance", "WINQ26");
    symbols_file::save(app.workspace.symbols_path(), &app.added_symbols).expect("symbols written");
    app.toolrail.set_favorites(&["measure".to_owned()]);
    app.save_workspace("test");

    let file = crate::scratch::ScratchFile::new("app-bundle", "workspace.qws.toml");
    app.export_workspace_to(&file);
    assert!(file.is_file(), "the export reached the disk");
    assert_eq!(
        app.workspace.session().recent().len(),
        1,
        "and the file joined the Open-recent menu"
    );

    // Now undo all of it, the way a trader rearranging their screen would.
    app.added_symbols.remove("binance", "WINQ26");
    symbols_file::save(app.workspace.symbols_path(), &app.added_symbols).expect("symbols written");
    app.toolrail.set_favorites(&[]);
    app.save_workspace("test");
    assert!(!app.added_symbols.contains("binance", "WINQ26"));

    app.import_workspace_from(&file);

    assert!(
        app.added_symbols.contains("binance", "WINQ26"),
        "the added symbol came back"
    );
    assert_eq!(
        app.starred_tool_ids(),
        vec!["measure".to_owned()],
        "and so did the toolbar favourite"
    );
    let _ = std::fs::remove_file(&file);
}

/// Opening a workspace *replaces* the tab strip rather than growing it.
///
/// `restore_workspace` was written for startup, where tab zero is already
/// the file's first market. Mid-session it is whatever the trader was
/// looking at, so adopting it would leave one market carrying another's
/// name — and every other saved tab would be opened on top of the ones
/// already there, multiplying the strip and the live feeds with it.
#[test]
fn opening_a_workspace_replaces_the_tab_strip_instead_of_growing_it() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    assert_eq!(app.tabs.len(), 2, "the trader has two markets open");

    // A saved workspace naming one market, and not the one on screen.
    app.restore_workspace(
        ui_state::Workspace::new(
            true,
            None,
            0,
            vec![ui_state::SavedTab {
                feed: "binance".to_owned(),
                symbol: "TESTUSDT".to_owned(),
                layout: crate::config::DeclaredLayout::Flow,
                split_fraction: None,
                context_collapsed: false,
                focus: None,
                focus_slot: 0,
                context_bars: vec![],
                flow_layout: None,
                context_layouts: vec![],
                flow_bars: "tick:50".to_owned(),
                time_bars: None,
                flow_legend_collapsed: false,
                time_legend_collapsed: false,
            }],
            None,
        )
        .restore(&app.config.clone()),
    );

    assert_eq!(
        app.tabs.len(),
        1,
        "the strip is the workspace's, not the workspace on top of what was there"
    );
    assert_eq!(app.tabs[0].symbol, "TESTUSDT");
    assert!(
        app.active_tab < app.tabs.len(),
        "and the active index points at a tab that exists"
    );
}

/// The all-or-nothing rule where the trader actually meets it: a bad file
/// leaves the screen exactly as it was, and says so.
#[test]
fn opening_a_file_that_is_not_a_workspace_changes_nothing_on_screen() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.toolrail.set_favorites(&["measure".to_owned()]);
    let before = app.starred_tool_ids();

    let file = crate::scratch::ScratchFile::new("app-bad-bundle", "workspace.qws.toml");
    std::fs::write(&file, "version = 99\nname = \"from tomorrow\"\n").unwrap();
    app.import_workspace_from(&file);

    assert_eq!(app.starred_tool_ids(), before, "the cockpit is untouched");
    let _ = std::fs::remove_file(&file);
}

/// A workspace save must carry the trader's *pick* and nothing else.
///
/// The failure this guards is quiet and expensive: a validation run under
/// `QUANTICK_REPLAY_DIR`, or any run that merely accepted the default
/// home, writing that path into `ui-state.toml` on exit and replacing the
/// folder a trader spends every morning in.
#[test]
fn a_workspace_save_never_invents_a_replay_folder() {
    let (app, _evt, _cmd, _book) = test_app();
    assert_eq!(
        app.capture_workspace().replay_folder,
        None,
        "nothing was chosen, so nothing is stored"
    );
}

/// And when there *is* a pick, every save carries it — a save that dropped
/// it would send the browser back to nowhere on the next launch.
#[test]
fn a_workspace_save_carries_the_pick_that_was_made() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.replay_view = ReplayView::new(Some("D:/tape"), None);
    assert_eq!(
        app.capture_workspace().replay_folder.as_deref(),
        Some("D:/tape")
    );
}

/// The user's retest flow end to end in the app: a sell preset with the
/// retest option armed on a region, a force bar cutting below it, the
/// limit resting at the cut edge — then the tape reaching the target
/// first, the order removing itself, and the badge saying so.
#[test]
fn a_cut_with_the_retest_preset_rests_a_limit_and_cancels_at_the_target() {
    fn print(app: &mut QuantickApp, id: &mut u64, price: &str) {
        *id += 1;
        let trade = quantick_engine::Trade {
            agg_id: *id,
            timestamp_ms: 1_700_000_000_000 + *id as i64 * 100,
            price: rust_decimal::Decimal::from_str_exact(price).unwrap(),
            quantity: rust_decimal::Decimal::ONE,
            side: quantick_engine::Side::Buy,
        };
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms);
    }
    fn bar(app: &mut QuantickApp, id: &mut u64, open: &str, close: &str) {
        for _ in 0..49 {
            print(app, id, open);
        }
        print(app, id, close);
    }

    let (mut app, _events, _commands, _book) = test_app();
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 105.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(30.0, 115.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell);
    form.window = 3;
    form.min_range = "0".to_owned();
    form.on_break = "retest_limit".to_owned();
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "BF retest".to_owned())
        .expect("the retest form compiles");

    let mut id = 0u64;
    bar(&mut app, &mut id, "110", "109");
    bar(&mut app, &mut id, "109", "108");
    // Body 4 over average (1+1+4)/3 = 2: force, closing below the 105
    // edge. The bar's range is 4 (prints at 108 and 104): TP 100.
    bar(&mut app, &mut id, "108", "104");
    {
        let tab = app.active_tab();
        assert_eq!(
            tab.paper.working_orders().len(),
            1,
            "the retest limit rests at the cut edge"
        );
        let instance = tab
            .flow_pane
            .strategies
            .for_drawing(drawing)
            .expect("instance");
        assert!(
            matches!(
                instance.armed.state(),
                quantick_strategy::ArmedState::Fired { retest: true, .. }
            ),
            "the instance narrates a resting retest, got {:?}",
            instance.armed.state()
        );
    }

    // The tape walks straight down through the projected target (100):
    // the order removes itself — no fill, no trade — and the one-shot
    // instance stops with the reason on the badge.
    print(&mut app, &mut id, "100");
    let tab = app.active_tab();
    assert!(
        tab.paper.working_orders().is_empty(),
        "the target print removed the resting limit"
    );
    assert!(tab.paper.is_flat(), "no trade happened");
    let instance = tab
        .flow_pane
        .strategies
        .for_drawing(drawing)
        .expect("instance");
    assert_eq!(
        instance.armed.state(),
        &quantick_strategy::ArmedState::Disarmed {
            reason: quantick_strategy::DisarmReason::TargetBeforeRetest
        }
    );
    assert_eq!(instance.armed.status_line(), "target hit before retest");
}

/// The two panes open different menus, so the scripted right-click has to
/// name one — and refuse anything it does not recognise, since opening the
/// wrong pane's menu photographs a defect that is not there.
#[test]
fn the_context_menu_hook_names_a_pane_or_opens_nothing() {
    assert_eq!(
        ContextMenuPane::from_env_value("tape"),
        Some(ContextMenuPane::Tape)
    );
    assert_eq!(
        ContextMenuPane::from_env_value(" LANE "),
        Some(ContextMenuPane::Tape)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("chart"),
        Some(ContextMenuPane::Chart)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("candles"),
        Some(ContextMenuPane::Chart)
    );
    for refused in ["", "1", "true", "both", "pane"] {
        assert_eq!(ContextMenuPane::from_env_value(refused), None, "{refused}");
    }
}

/// The gesture the trader asked for: drag the properties popup somewhere
/// useful, take the hand off it, and the cockpit has kept it — with no trip
/// through the Workspace menu, because nobody arranging a chart mid-session
/// stops to save a layout.
///
/// And *only* that: the second half of this test is the important one. A
/// drag must not quietly adopt whatever the tab strip happens to hold as
/// the startup screen — a trader who opens a tab to check something, then
/// nudges the popup, must not find that tab waiting for them tomorrow.
#[test]
fn parking_the_properties_popup_autosaves_it_to_the_workspace() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    with_a_saved_workspace(&mut app, &ctx, "popup-parked");
    let tabs_when_saved = ui_state::load(app.workspace.ui_state_path()).tabs;
    // Drift away from the saved cockpit, the way a session does — a bar
    // rule, which is recorded per tab, so a full capture would show here.
    app.active_tab_mut().flow_pane.set_spec(BarSpec::Tick(500));
    run_frame(&mut app, &ctx);
    draw_horizontal_line(&mut app, &ctx, 300.0);

    let parked = park_the_popup(&mut app, &ctx, egui::vec2(150.0, 90.0));

    let file = ui_state::load(app.workspace.ui_state_path());
    assert_eq!(
        file.chrome
            .expect("the chrome is still there")
            .inspector_position,
        Some([parked.x, parked.y]),
        "the file holds the position the hand came off at"
    );
    assert_eq!(
        file.tabs, tabs_when_saved,
        "and nothing else — the drift is not adopted as the startup screen"
    );
    assert!(
        !app.surfaces
            .toast
            .message()
            .is_some_and(|message| message.contains("Workspace saved")),
        "an autosave nobody asked for by name does not talk over the trader"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// A workspace with no chrome section is left alone rather than grown one.
/// That is what a `Reset startup layout` leaves behind, and recreating the
/// file from a popup drag would undo the reset the trader just asked for.
#[test]
fn parking_the_popup_never_recreates_a_workspace_that_was_reset() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.workspace
        .set_ui_state_path(scratch_ui_state("popup-after-reset"));
    run_frame(&mut app, &ctx);
    draw_horizontal_line(&mut app, &ctx, 300.0);

    park_the_popup(&mut app, &ctx, egui::vec2(150.0, 90.0));

    assert!(
        !app.workspace.ui_state_path().exists(),
        "no startup workspace is conjured out of a window drag"
    );
}

#[test]
fn a_default_preset_shapes_new_fibs_and_leaves_existing_ones_alone() {
    use crate::drawings::DrawingPayload as _;
    use crate::drawings::fib::FibPayload;

    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    // First fib: the built-in standard start.
    arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(600.0, 250.0),
        egui::pos2(900.0, 400.0),
    );
    let standard_levels = app.active_tab().flow_pane.drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<FibPayload>()
        .expect("fib payload")
        .levels
        .len();
    assert_eq!(standard_levels, 7);

    // Save a compact custom preset and make it the default for new fibs.
    let presets = crate::scratch::ScratchFile::new("default-preset-test", "presets.toml");
    let mut store = drawings::presets::PresetStore::load_from(presets.path().to_path_buf());
    let mut compact = FibPayload::new(drawings::fib::FibKind::Retracement);
    compact.apply_preset(&drawings::fib::RETRACEMENT_PRESETS[1]);
    let exported = compact.export_preset().expect("fib exports presets");
    assert!(store.save_custom_preset("fib-retracement", "mine", exported, false));
    store.set_default_preset("fib-retracement", Some("mine".into()));
    let preset_path = store.path().to_path_buf();
    app.drawing_presets = store;

    // Second fib starts from the default preset. Drawn clear of the
    // inspector the first fib opened (x >= 410): the panel is opaque to
    // the pointer, so a press behind it drops no anchor.
    arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(500.0, 250.0),
        egui::pos2(650.0, 400.0),
    );
    let new_levels = app.active_tab().flow_pane.drawings.items()[1]
        .payload
        .as_any()
        .downcast_ref::<FibPayload>()
        .expect("fib payload")
        .levels
        .len();
    assert_eq!(new_levels, 5, "a new fib starts from the default preset");

    // ...and the first one is untouched.
    let old_levels = app.active_tab().flow_pane.drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<FibPayload>()
        .expect("fib payload")
        .levels
        .len();
    assert_eq!(old_levels, 7, "the default never rewrites existing objects");
    let _ = std::fs::remove_file(preset_path);
}

#[test]
fn switching_to_a_feed_with_a_declared_preset_applies_it_then() {
    // Feed "binance" declares nothing; a second feed declares the pie
    // look. Opening on the first must not apply it — moving to the
    // second must.
    let mut config = test_config();
    config.feeds.push(FeedConfig {
        id: "mt".to_string(),
        name: "MetaTrader 5".to_string(),
        provider: ProviderKind::MetaTrader,
        symbols: vec!["WINQ26".to_string()],
        bubble_preset: Some("live lane pie".to_string()),
        symbol_bubble_presets: Default::default(),
        default_layout: None,
        default_bars: None,
    });
    let mut app = app_on(config, "binance", "TESTUSDT");
    let opened_with = app.active_tab().tape().active_preset_for_test().to_string();
    assert_ne!(
        opened_with, "live lane pie",
        "nothing declared, nothing applied"
    );

    // The switch path runs this after installing the new feed handle.
    app.active_tab_mut().feed_id = "mt".to_string();
    with_config(&mut app, |tab, config| {
        tab.apply_feed_bubble_preset_after_switch(config, "binance", "TESTUSDT")
    });
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "live lane pie"
    );
}

#[test]
fn a_symbol_hop_onto_a_symbol_declared_preset_applies_it() {
    // The feed declares the pie look; one of its symbols reads
    // differently and says so. Hopping onto that symbol applies its
    // look; hopping back applies the feed-wide one again, because the
    // resolved declarations differ in both directions.
    let mut config = test_config();
    config.feeds[0].bubble_preset = Some("live lane pie".to_string());
    config.feeds[0]
        .symbol_bubble_presets
        .insert("ETHUSDT".to_string(), "dense tape".to_string());
    let mut app = app_on(config, "binance", "TESTUSDT");
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "live lane pie"
    );

    app.active_tab_mut().symbol = "ETHUSDT".to_string();
    with_config(&mut app, |tab, config| {
        tab.apply_feed_bubble_preset_after_switch(config, "binance", "TESTUSDT")
    });
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "dense tape"
    );

    app.active_tab_mut().symbol = "TESTUSDT".to_string();
    with_config(&mut app, |tab, config| {
        tab.apply_feed_bubble_preset_after_switch(config, "binance", "ETHUSDT")
    });
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "live lane pie"
    );
}

#[test]
fn grouping_restart_commits_only_after_command_is_queued() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
    let grouping = Decimal::new(5, 2);

    assert!(
        app.active_tab_mut()
            .tape_mut()
            .stage_capture_grouping_for_test(grouping)
    );
    assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
    app.active_tab_mut().restart_book_capture();

    assert!(matches!(
        cmd_rx.try_recv(),
        Ok(FeedCommand::RestartBookCapture { .. })
    ));
    assert_eq!(
        app.active_tab_mut()
            .tape_mut()
            .base_capture_grouping_for_test(),
        grouping
    );
    assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 0);
    assert_eq!(
        app.active_tab_mut().tape_mut().health().status,
        "connecting"
    );
}

/// And restoring it puts the window back. The pair is the whole feature:
/// a capture nothing can reopen is a file, not a workspace.
#[test]
fn a_restored_workspace_puts_the_window_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.restore_workspace(ui_state::Workspace::new(
        true,
        None,
        0,
        vec![ui_state::SavedTab {
            feed: "binance".to_owned(),
            symbol: "TESTUSDT".to_owned(),
            layout: crate::config::DeclaredLayout::TimeAndFlow,
            split_fraction: Some(0.4),
            context_collapsed: false,
            focus: Some(ui_state::SavedFocus::Flow),
            focus_slot: 0,
            context_bars: vec![],
            flow_layout: None,
            context_layouts: vec![],
            flow_bars: "dollar:250000".to_owned(),
            time_bars: Some("time:5m".to_owned()),
            flow_legend_collapsed: false,
            time_legend_collapsed: false,
        }],
        Some(ui_state::SavedChrome {
            timezone_minutes: 330,
            dock_visible: false,
            dock_tab: Some(ui_state::SavedDockTab::Trades),
            rail_visible: false,
            rail_dock: ui_state::SavedRailDock::Bottom,
            perf_readings: false,
            legacy_favorite_tools: Vec::new(),
            progressive_history: false,
            history_reach: None,
            history_reach_span_minutes: None,
            venue_lead_in: false,
            inspector_position: Some([260.0, 480.0]),
        }),
    ));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    assert_eq!(tab.layout, CanvasLayout::TimeAndFlow);
    assert!((tab.split_fraction - 0.4).abs() < f32::EPSILON);
    assert_eq!(
        tab.focused_side(),
        PaneSide::Flow,
        "the saved focus wins over the pane the layout switch revealed"
    );
    assert_eq!(
        tab.flow_pane.state.spec(),
        &BarSpec::Dollar(rust_decimal::Decimal::from(250_000)),
        "the flow pane opens on the rule the workspace recorded"
    );
    assert_eq!(
        tab.time_pane().map(|pane| pane.state.spec().clone()),
        Some(BarSpec::Time(300_000)),
        "and the time pane on its saved interval, not the header default"
    );
    assert_eq!(app.tz.minutes(), 330);
    assert!(
        !app.dock.visible(),
        "a dock the trader hid stays hidden, tab remembered underneath"
    );
    assert_eq!(app.dock.tab(), Some(DockTab::Trades));
    assert!(!app.toolrail.visible());
    assert_eq!(app.toolrail.dock(), ToolboxDock::Bottom);
    assert!(!app.show_perf);
    assert!(
        !app.progressive_history,
        "a trader who chose the single-request fetch reopens on it"
    );
    assert!(
        app.tabs.iter().all(|tab| !tab.progressive_history),
        "and every tab phrases its request that way"
    );
    assert_eq!(
        app.surfaces.drawing_chrome.inspector_pos(),
        Some(egui::pos2(260.0, 480.0)),
        "the properties popup reopens where the trader parked it"
    );
    assert!(
        app.surfaces.drawing_chrome.inspector_moved(),
        "and counts as hand-placed, so automatic placement does not undo it"
    );
}

/// A stop filling on a chart the trader is not looking at is exactly the
/// news they most need, and the old per-tab toast dropped it silently —
/// it was drawn for the active tab only, on a clock that started when the
/// message was raised, so by the time the tab was looked at the toast had
/// already expired. It travels now, and it says which market it is about:
/// an unlabelled "SIM: dropped at the fill" would read as being about the
/// chart on screen.
#[test]
fn a_background_tabs_acknowledgement_travels_and_names_its_market() {
    let (mut app, _commands) = app_with_history(50);
    app.open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    assert!(app.tabs.len() >= 2, "a second market is open");
    let watched = app.active_tab;
    let background = app
        .tabs
        .iter()
        .position(|tab| tab.symbol != app.tabs[watched].symbol)
        .expect("the two tabs are on different markets");
    let symbol = app.tabs[background].symbol.clone();

    app.tabs[background]
        .paper
        .show_toast("SIM: stop filled".to_owned());
    app.settle_paper_panels(Instant::now());

    let toast = app
        .surfaces
        .toast
        .message()
        .expect("a background tab is still heard");
    assert!(
        toast.starts_with(&format!("{symbol} · ")),
        "it names the market it is about, in the window's own separator, got '{toast}'"
    );
    assert!(
        toast.contains("stop filled"),
        "and still says what happened"
    );
}

/// Saving says so. A trader who arranges a cockpit and clicks Save has no
/// other way to tell it worked than restarting — and it says so through
/// the acknowledgement channel the window already has, rather than by
/// pushing a cell onto the status line and sliding the readings sideways
/// for eight seconds.
#[test]
fn saving_the_workspace_acknowledges_itself() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace.set_ui_state_path(scratch_ui_state("notice"));
    assert!(app.surfaces.toast.message().is_none());

    app.save_workspace("test");

    let toast = app
        .surfaces
        .toast
        .message()
        .expect("the save reports itself");
    assert!(
        toast.contains("saved"),
        "the answer has to say what happened, got '{toast}'"
    );
    assert!(
        !app.surfaces.toast.offers_undo(),
        "the file it replaced is gone; an Undo button here would lie"
    );
    assert!(
        app.workspace.ui_state_path().exists(),
        "and the file it claims to have written is on disk"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// Naming an arrangement keeps it without touching what the app opens on.
/// The two are separate settings, and a trader saving a way back must not
/// discover they also redefined their opening screen.
#[test]
fn naming_an_arrangement_does_not_change_what_opens() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("named-startup"));
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    run_frame(&mut app, &ctx);
    app.save_workspace("test");
    let startup_before = ui_state::load(app.workspace.ui_state_path()).tabs;

    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.save_named_workspace("scalp");

    let file = ui_state::load(app.workspace.ui_state_path());
    assert_eq!(
        file.tabs, startup_before,
        "the startup arrangement is untouched by a bookmark"
    );
    let saved = file.named("scalp").expect("the bookmark is in the file");
    assert_eq!(
        saved.tabs.first().map(|tab| tab.layout),
        Some(crate::config::DeclaredLayout::TimeAndFlow),
        "and the bookmark holds the arrangement that was on screen"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// Saving the startup screen must not throw the bookmarks away: every
/// write rewrites the whole file.
#[test]
fn saving_the_startup_screen_keeps_the_bookmarks() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("bookmarks-survive"));
    app.save_named_workspace("scalp");

    app.save_workspace("test");

    assert!(
        ui_state::load(app.workspace.ui_state_path())
            .named("scalp")
            .is_some(),
        "a bookmark cannot be collateral damage of saving the startup screen"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// The same name twice replaces, so the menu never grows five entries
/// called "scalp".
#[test]
fn saving_over_a_name_replaces_that_bookmark() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace.set_ui_state_path(scratch_ui_state("replace"));
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    run_frame(&mut app, &ctx);
    app.save_named_workspace("scalp");

    app.active_tab_mut().set_layout(CanvasLayout::Time);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.save_named_workspace("  scalp  ");

    let file = ui_state::load(app.workspace.ui_state_path());
    assert_eq!(file.saved.len(), 1, "one name, one bookmark");
    assert_eq!(
        file.named("scalp")
            .and_then(|e| e.tabs.first())
            .map(|t| t.layout),
        Some(crate::config::DeclaredLayout::Time),
        "and it holds the newer arrangement"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// Opening a bookmark replaces the whole tab strip — which is only
/// possible by growing before shrinking, since the last tab cannot close.
#[test]
fn opening_a_bookmark_replaces_what_is_on_screen() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace.set_ui_state_path(scratch_ui_state("open"));
    app.active_tab_mut().set_layout(CanvasLayout::Time);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.tz = TzOffset::new(0);
    // The properties popup is part of an arrangement like the dock and the
    // rail are, so a bookmark carries where it was parked.
    app.surfaces
        .drawing_chrome
        .place_inspector_by_hand(egui::pos2(510.0, 240.0));
    app.save_named_workspace("context");

    // Drift away from it, then come back.
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    app.tz = TzOffset::new(-180);
    app.surfaces
        .drawing_chrome
        .place_inspector_by_hand(egui::pos2(120.0, 640.0));
    run_frame(&mut app, &ctx);

    app.open_named_workspace("context");
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(app.tabs.len(), 1, "the strip is replaced, not appended to");
    assert_eq!(app.active_tab().layout, CanvasLayout::Time);
    assert_eq!(app.tz.minutes(), 0, "the chrome comes back with it");
    assert_eq!(
        app.surfaces.drawing_chrome.inspector_pos(),
        Some(egui::pos2(510.0, 240.0)),
        "including where the popup was parked when the bookmark was named"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// Deleting a bookmark throws away a way back, not the place you are.
#[test]
fn deleting_a_bookmark_leaves_the_window_alone() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace.set_ui_state_path(scratch_ui_state("delete"));
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.save_named_workspace("scalp");

    app.delete_named_workspace("scalp");

    assert!(
        ui_state::load(app.workspace.ui_state_path())
            .named("scalp")
            .is_none()
    );
    assert_eq!(
        app.active_tab().layout,
        CanvasLayout::TimeAndFlow,
        "the charts on screen are not what was deleted"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// The reason the user asked for named workspaces: a way back after a
/// reset. Reset deleting the bookmarks would break the feature at exactly
/// the moment it exists for.
#[test]
fn resetting_the_startup_layout_keeps_the_bookmarks() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("reset-keeps"));
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.save_named_workspace("before the mess");
    app.save_workspace("test");

    app.forget_workspace();

    let file = ui_state::load(app.workspace.ui_state_path());
    assert!(
        file.tabs.is_empty(),
        "the startup arrangement is what Reset clears"
    );
    assert!(
        file.named("before the mess").is_some(),
        "the way back survives the reset it exists for"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// With nothing named, Reset still removes the file outright.
#[test]
fn resetting_with_no_bookmarks_removes_the_file() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("reset-removes"));
    app.save_workspace("test");
    assert!(app.workspace.ui_state_path().exists());

    app.forget_workspace();

    assert!(!app.workspace.ui_state_path().exists());
}

/// A name that is only whitespace is not a name.
#[test]
fn a_blank_name_saves_nothing_and_says_so() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace.set_ui_state_path(scratch_ui_state("blank"));

    app.save_named_workspace("   ");

    assert!(app.workspace.session().bookmarks().is_empty());
    assert!(
        !app.workspace.ui_state_path().exists(),
        "a refused save must not write the file either"
    );
    assert!(
        app.surfaces
            .toast
            .message()
            .is_some_and(|message| message.contains("needs a name")),
        "and the trader is told why nothing happened"
    );
}

/// The automatic tier: a trader who never opens the Workspace menu still
/// reopens where they left off. Without this the feature is only the
/// explicit half, and the half most people would never find.
#[test]
fn closing_the_window_keeps_the_arrangement_when_autosave_is_on() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("exit-save"));
    *app.workspace.session_mut().save_on_exit_mut() = true;
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);

    close_requested_frame(&mut app, &ctx);

    let saved = ui_state::load(app.workspace.ui_state_path());
    assert_eq!(
        saved.tabs.first().map(|tab| tab.layout),
        Some(crate::config::DeclaredLayout::TimeAndFlow),
        "the window that closed is the window that reopens"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// And switching it off means exactly that: the trader who curates their
/// startup layout by hand must not have it overwritten by whatever their
/// last session drifted into.
#[test]
fn closing_the_window_writes_nothing_when_autosave_is_off() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("exit-no-save"));
    *app.workspace.session_mut().save_on_exit_mut() = false;
    run_frame(&mut app, &ctx);

    close_requested_frame(&mut app, &ctx);

    assert!(
        !app.workspace.ui_state_path().exists(),
        "autosave off must leave the saved workspace untouched"
    );
}

/// Autosave is a property of the file it governs, so switching it has to
/// reach the disk on the spot — waiting for the exit would mean waiting
/// for the exit it may have just switched off.
#[test]
fn switching_autosave_off_is_itself_saved() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("autosave"));
    *app.workspace.session_mut().save_on_exit_mut() = false;
    app.save_workspace("save_on_exit_toggled");

    assert!(
        !ui_state::load(app.workspace.ui_state_path()).save_on_exit,
        "a trader who switched autosave off must not find it back on"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// A file this build cannot read is not this build's to rewrite.
///
/// Writing a standing choice reads the file, swaps one field and writes it
/// back. Reading through the startup loader would hand back the *defaults*
/// for a workspace from a newer build or one a bad shutdown truncated — so
/// a single star click would replace the trader's tabs, bookmarks and
/// replay folder with an empty file, and would do it with autosave off.
#[test]
fn a_workspace_this_build_cannot_read_survives_a_star() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("star-unreadable"));
    // A workspace from a version this build does not know.
    let from_tomorrow = "version = 99\nsaved = []\nkeep_me = true\n";
    std::fs::write(app.workspace.ui_state_path(), from_tomorrow).unwrap();

    app.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    assert_eq!(
        std::fs::read_to_string(app.workspace.ui_state_path()).expect("still there"),
        from_tomorrow,
        "a file this build cannot parse is left byte for byte alone"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// Picking a replay folder must not switch autosave back on.
///
/// Every standing choice goes through one read-swap-write now, which is
/// what makes this impossible: the folder pick used to skip carrying
/// `save_on_exit`, so on an installation with no file yet it wrote the
/// default — `true` — over a switch the trader had turned off.
#[test]
fn picking_a_replay_folder_does_not_switch_autosave_back_on() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("folder-autosave"));
    *app.workspace.session_mut().save_on_exit_mut() = false;

    app.write_replay_folder(Some("D:/tape"));

    let file = ui_state::load(app.workspace.ui_state_path());
    assert_eq!(file.replay_folder.as_deref(), Some("D:/tape"));
    assert!(
        !file.save_on_exit,
        "a folder pick is not a request to switch autosave on"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// The layouts are one shared library, whatever each pane is showing:
/// renaming one renames it on every pane that shows it — and on the strip
/// every pane reads — while a pane on another layout keeps its own name.
///
/// The trader's way of putting it: rename `Layout 1` and it is renamed in
/// every window. The name lives in the book; a pane carries a copy only
/// for its own header, and this is what keeps that copy from becoming a
/// second source of truth.
#[test]
fn renaming_a_layout_renames_it_on_every_pane_that_shows_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let first = app.layouts().active_id();
    assert_eq!(
        app.pane_layout(app.active_tab().id, PaneSide::Time(0)),
        first,
        "both panes open on the one layout there is"
    );

    assert_eq!(app.rename_layout(first, "opening"), Ok(true));
    for side in [PaneSide::Flow, PaneSide::Time(0)] {
        assert_eq!(
            app.active_tab().pane(side).layout_label,
            "opening",
            "the rename reached {side:?}"
        );
    }

    // Put the context pane on a second layout, and rename that one.
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let second = app.create_layout(Some("levels")).expect("second");
    assert_eq!(app.rename_layout(second, "levels revisited"), Ok(true));
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).layout_label,
        "levels revisited"
    );
    assert_eq!(
        app.active_tab().flow_pane.layout_label,
        "opening",
        "the pane on the other layout kept its own name"
    );

    // The strip both panes read lists both layouts, under the new names.
    let names: Vec<&str> = app
        .layouts()
        .layouts()
        .iter()
        .map(|layout| layout.name.as_str())
        .collect();
    assert_eq!(names, vec!["opening", "levels revisited"]);
}
