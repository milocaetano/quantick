use super::*;

/// An anchor dropped in an indicator pane belongs to that pane.
#[test]
fn a_click_in_an_indicator_pane_draws_on_that_band() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));

    let inside = pane_body(&app, 0).center();
    click_chart(&mut app, &ctx, inside);

    let placed = &app.active_tab().flow_pane.drawings;
    assert_eq!(placed.items().len(), 1, "the click placed one object");
    assert!(
        matches!(placed.items()[0].band, drawings::DrawingBand::Indicator(_)),
        "an anchor dropped in the CVD pane is a CVD level, not a price"
    );
}

/// An import must not cost the session its indicator persistence.
///
/// Opening a workspace replaces the tab strip, which closes the tab the
/// indicator file was written for — and closing that tab is exactly what
/// makes the app stop saving the indicator set, on purpose, for the rest
/// of the session. Right when a trader closes it themselves; wrong here,
/// where the imported set is restored onto the new tab a moment later.
/// Without this the trader would import a cockpit, tune an indicator, and
/// silently lose the tuning at every restart.
#[test]
fn opening_a_workspace_keeps_the_indicator_set_being_saved() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let file = crate::scratch::ScratchFile::new("app-persist", "workspace.qws.toml");
    app.export_workspace_to(&file);
    // A market the live tab is not on, so the import replaces the strip.
    app.open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    app.import_workspace_from(&file);

    assert!(
        app.tabs
            .iter()
            .all(|tab| tab.panes().all(|(pane, _)| pane.layout_seeded)),
        "every pane of the imported strip carries the imported layout"
    );
    assert!(
        !app.workspace.layouts().is_dirty(),
        "what is on screen is the file's; nothing to write back yet"
    );
    let _ = std::fs::remove_file(&file);
}

/// A cockpit from before layouts existed keeps its indicator set: the
/// old file becomes Layout 1, restores through the same commands the
/// menu sends, and the layouts file — not the old one — is what a settled
/// edit writes back.
#[test]
fn the_indicator_set_restores_from_disk_and_saves_back() {
    use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind};

    let (mut app, _events, _commands, _book) = test_app();
    let path = crate::indicators::state_file::default_path();
    let layouts_path = crate::layouts::default_path();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&layouts_path);
    app.workspace.set_layouts_path(layouts_path.clone());

    // A saved set: one native with bound inputs, one hidden native, and
    // a script the library does not have.
    crate::indicators::state_file::save(
        &path,
        &[
            SavedIndicator {
                kind: SavedKind::NativeEma,
                hidden: false,
                inputs: vec![SavedInput::Int(21), SavedInput::Source("close".to_owned())],
                plot_styles: Vec::new(),
            },
            SavedIndicator {
                kind: SavedKind::NativeCvd,
                hidden: true,
                inputs: Vec::new(),
                plot_styles: Vec::new(),
            },
            SavedIndicator {
                kind: SavedKind::Script {
                    name: "not-in-the-library.pine".to_owned(),
                },
                hidden: false,
                inputs: Vec::new(),
                plot_styles: Vec::new(),
            },
        ],
    );

    app.reload_layouts(&[]);
    assert_eq!(
        app.layouts().active().name,
        "Layout 1",
        "the old set migrated into the first layout"
    );
    assert_eq!(
        app.slot_kinds.len(),
        3,
        "a script the library lacks takes an error slot saying so, keeping the layout's positions aligned"
    );
    assert_eq!(app.slot_kinds[0].1, SavedKind::NativeEma);
    assert_eq!(app.slot_kinds[1].1, SavedKind::NativeCvd);
    assert_eq!(app.pending_hidden.len(), 1, "the hidden flag survived");
    assert!(
        !app.workspace.layouts().is_dirty(),
        "restoring is not a user edit and must not rewrite the file"
    );

    // A user edit, settled: the file must match the live set.
    app.mark_indicator_state_dirty();
    // Let the worker answer the adds, so the views the snapshot reads
    // exist — a settled edit reads what is on screen.
    app.active_tab_mut().flow_pane.indicator_worker.flush();
    app.active_tab_mut().flow_pane.apply_indicator_events();
    app.maintain_indicator_state();
    app.flush_layouts();
    let crate::layouts::Loaded::Book(book) = crate::layouts::load(&layouts_path) else {
        panic!("the layouts file was written");
    };
    let written = &book.active().indicators;
    assert_eq!(
        written.len(),
        3,
        "the whole migrated set is written — the script the library lacks stays in the layout, waiting for its file"
    );
    assert_eq!(
        app.slot_kinds.len(),
        3,
        "and every entry has its slot on the chart"
    );
    assert!(
        !app.workspace.layouts().is_dirty(),
        "the debounce fired, so the change is written"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&layouts_path);
}

#[test]
fn an_unchanged_spec_never_arms_the_rebuild_indicator() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().apply_spec_changes();
    assert!(!app.active_tab().loading.is_active(LoadingTask::BarRebuild));
    assert!(app.active_tab().flow_pane.pending_spec.is_none());
}

/// Both folded legends come back folded — the time pane's included.
///
/// The time pane does not exist on the frame the workspace is restored:
/// `set_layout` only arms it and `apply_pending_layout` builds it on the
/// next one. A restore that wrote the fold straight into `time_pane`
/// would write it into `None`, the pane would open expanded, and the
/// following `capture_arrangement` would persist that `false` back over
/// the trader's choice — losing it permanently rather than for one
/// session. Driving two frames is what makes this test able to fail.
#[test]
fn both_panes_reopen_with_the_legend_the_trader_folded() {
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
            split_fraction: Some(0.5),
            context_collapsed: false,
            focus: Some(ui_state::SavedFocus::Flow),
            focus_slot: 0,
            context_bars: vec![],
            flow_layout: None,
            context_layouts: vec![],
            flow_bars: "tick:50".to_owned(),
            time_bars: Some("time:1m".to_owned()),
            flow_legend_collapsed: true,
            time_legend_collapsed: true,
        }],
        None,
    ));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    assert!(
        tab.flow_pane.legend_collapsed,
        "the flow pane reopens folded"
    );
    assert!(
        tab.time_pane()
            .expect("the split was restored")
            .legend_collapsed,
        "and so does the time pane, built a frame after the restore"
    );

    // The round trip closes here: what is captured next must be what was
    // restored, or the choice survives the open and dies on the save.
    let (tabs, _chrome) = app.capture_arrangement();
    assert!(tabs[0].flow_legend_collapsed);
    assert!(tabs[0].time_legend_collapsed);
}

/// (d) `Insert → Indicator` is a layout edit: the slot lands on the pane
/// the user is working in *and* on every other pane, on the same frame —
/// a layout's indicators are shared by every chart that shows it.
#[test]
fn an_indicator_added_on_one_pane_appears_on_every_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let flow_before = app.active_tab().flow_pane.indicators.all().len();

    let point = pane_point(&app, PaneSide::Time(0));

    click_chart(&mut app, &ctx, point);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Time(0),
        "clicking a pane focuses it"
    );

    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);

    for side in [PaneSide::Time(0), PaneSide::Flow] {
        let pane = app.active_tab().pane(side);
        let labels: Vec<String> = pane
            .indicators
            .all()
            .iter()
            .map(|view| view.label().to_owned())
            .collect();
        assert!(
            labels.iter().any(|label| label.contains("EMA")),
            "the EMA is on {side:?}, and it really built: {labels:?}"
        );
    }
    assert_eq!(
        app.active_tab().flow_pane.indicators.all().len(),
        flow_before + 1,
        "the pane beside the focused one gained the same indicator"
    );
    assert_eq!(
        app.slot_kinds.len(),
        2,
        "one registration per pane, each on its own slot"
    );
    assert_eq!(
        app.slot_kinds
            .iter()
            .filter(|(owner, _)| owner.side == PaneSide::Time(0))
            .count(),
        1
    );

    // Settled, the edited pane's set is the layout's.
    app.maintain_indicator_state();
    assert_eq!(app.layouts().active().indicators.len(), 1);
    assert_eq!(
        app.layouts().active().indicators[0].kind,
        crate::indicators::state_file::SavedKind::NativeEma
    );
}

/// The four doors into the dialog have to be one door: whichever gesture a
/// trader used, the dialog opens on the indicator they pointed at, holding
/// that indicator's own values. A pane that asks is drained exactly once —
/// a request left behind would re-open the dialog every frame, over
/// whatever the trader did next.
#[test]
fn a_pane_gesture_opens_the_dialog_once_on_the_indicator_it_named() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    let slot = app.active_tab().flow_pane.indicators.all()[0].slot;

    app.active_tab_mut().flow_pane.request_settings(slot);
    app.open_requested_indicator_settings();
    let dialog = app.indicator_settings.as_ref().expect("the dialog opened");
    assert_eq!(dialog.slot, slot, "on the indicator the gesture named");
    assert_eq!(
        dialog.draft,
        app.active_tab().flow_pane.indicators.all()[0].input_values,
        "holding that indicator's own values"
    );

    app.indicator_settings = None;
    app.open_requested_indicator_settings();
    assert!(
        app.indicator_settings.is_none(),
        "the request was taken, not left to fire again next frame"
    );
}

/// A restyled plot survives a restart, and travels with its own indicator.
///
/// The style layer is written beside the inputs and restored the same way
/// the hidden flag is — deferred until the view the worker builds exists.
/// Without that deferral the layer lands on nothing and the trader's
/// colours are silently dropped on every launch.
#[test]
fn a_restyled_plot_comes_back_after_a_restart() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    let slot = app.active_tab().flow_pane.indicators.all()[0].slot;
    app.active_tab_mut()
        .flow_pane
        .indicators
        .view_mut(slot)
        .expect("the view")
        .style
        .set(
            0,
            crate::indicator_style::PlotOverride {
                color: Some(quantick_indicators::Rgba8::opaque(255, 0, 0)),
                width: Some(3.0),
                ..crate::indicator_style::PlotOverride::default()
            },
        );

    // What the save path would write, and what a fresh launch reads back.
    use crate::indicators::state_file::SavedPlotStyle;
    let saved: Vec<_> = app.active_tab().flow_pane.indicators.all()[0]
        .style
        .plots()
        .iter()
        .copied()
        .map(SavedPlotStyle::from_override)
        .collect();
    assert_eq!(saved.len(), 1, "one plot carries an override");
    let restored = crate::indicator_style::StyleOverride::from_plots(
        saved
            .iter()
            .copied()
            .map(SavedPlotStyle::to_override)
            .collect(),
    );
    assert_eq!(
        restored,
        app.active_tab().flow_pane.indicators.all()[0].style,
        "the layer survives the round trip exactly"
    );
}

/// The harness hook has to find the indicators wherever they are.
///
/// A split tab can open with the *time* pane focused while every indicator
/// — the ones `QUANTICK_INDICATORS_AUTOSTART` adds and the ones the state
/// file restores — lives on the flow pane. Asking only the focused side
/// meant the hook waited for a view that was never coming, and a scripted
/// run captured a chart with no dialog on it and nothing saying why. Caught
/// by launching the real app and finding the dialog absent.
#[test]
fn the_settings_hook_finds_indicators_on_the_flow_pane_while_the_time_pane_has_focus() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    // Indicators on the flow pane...
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    // ...mirrored onto the time pane by the layout; the hook's index names
    // the same indicator on whichever pane has focus.
    let slot = app.active_tab().pane(PaneSide::Time(0)).indicators.all()[0].slot;

    // ...and the focus on the other one, which is how a split tab can open.
    let time_point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, time_point);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Time(0),
        "the fixture has to actually focus the pane without indicators"
    );

    app.harness
        .arm_settings_autostart(0, crate::indicator_panel::SettingsTab::Style);
    app.open_requested_indicator_settings();

    let dialog = app
        .indicator_settings
        .as_ref()
        .expect("the hook found the indicator on the pane that has one");
    assert_eq!(dialog.slot, slot);
    assert_eq!(
        app.indicator_settings_target.side,
        PaneSide::Time(0),
        "and addressed it on the focused pane, which carries the layout too"
    );
    assert_eq!(dialog.tab, crate::indicator_panel::SettingsTab::Style);
    assert!(
        app.harness.settings_autostart().is_none(),
        "spent by the first open, so closing the dialog leaves it closed"
    );
}

/// A legend row acts on the pane it is drawn on, never the focused one —
/// the routing that keeps the audit's "commands target one pane, chrome
/// speaks for another" contradiction from reappearing here.
#[test]
fn legend_actions_land_on_their_own_pane_not_the_focused_one() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    // An EMA on the time pane...
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    let slot = app
        .active_tab()
        .time_pane()
        .expect("time pane")
        .indicators
        .all()[0]
        .slot;
    // ...while focus returns to the flow pane.
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);

    let target = TabSlot {
        tab: app.active_tab().id,
        side: PaneSide::Time(0),
        slot,
    };
    app.toggle_indicator_hidden_at(target);
    assert!(
        app.active_tab()
            .time_pane()
            .expect("time pane")
            .indicators
            .all()[0]
            .hidden,
        "the time pane's own slot was toggled, focus notwithstanding"
    );
    app.open_indicator_settings_at(target);
    assert!(app.indicator_settings.is_some());
    assert_eq!(
        app.indicator_settings_target.side,
        PaneSide::Time(0),
        "the dialog's Apply will land on the legend's pane"
    );
}

/// The three-pane canvas shipped with a dead bottom chart: focus was a
/// two-arm enum, so every reader mapped "time" to the *top* context pane.
/// Clicking the bottom one has to focus it — and the status bar, the
/// BARS group, the indicator command and the saved workspace have to
/// follow that focus, not the top chart's.
#[test]
fn the_second_context_pane_takes_focus_bars_and_indicators() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().time_panes.len(),
        2,
        "the layout built both context panes"
    );
    let flow_before = app.active_tab().flow_pane.indicators.all().len();

    let point = pane_point(&app, PaneSide::Time(1));
    click_chart(&mut app, &ctx, point);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Time(1),
        "clicking the bottom context chart focuses it"
    );
    assert_eq!(
        app.status_model().spec_summary,
        app.active_tab()
            .pane(PaneSide::Time(1))
            .state
            .spec()
            .summary(),
        "the status bar speaks for it"
    );

    // The BARS group borrows the focused pane's selector fields.
    let top_spec = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .state
        .spec()
        .clone();
    let pane = app.active_tab_mut().focused_pane_mut();
    pane.kind = crate::state::BarKind::Time;
    pane.time_interval_ms = 900_000;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(1)).state.spec(),
        &BarSpec::Time(900_000),
        "the bar rule changed on the bottom chart"
    );
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).state.spec(),
        &top_spec,
        "and the top chart kept its own"
    );

    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(1))
            .indicators
            .all()
            .len(),
        1,
        "the EMA landed on the bottom chart"
    );
    // The command targeted the bottom chart; the layout put the same
    // indicator on the other two.
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .indicators
            .all()
            .len(),
        1,
        "and on the top one, through the layout"
    );
    assert_eq!(
        app.active_tab().flow_pane.indicators.all().len(),
        flow_before + 1,
        "and on the flow pane, through the layout"
    );
    assert!(
        app.slot_kinds
            .iter()
            .any(|(owner, _)| owner.side == PaneSide::Time(1)),
        "the bottom chart's own registration is the one the command made"
    );

    // The workspace remembers which of the two it was.
    let (tabs, _chrome) = app.capture_arrangement();
    assert_eq!(tabs[0].focus, Some(ui_state::SavedFocus::Time));
    assert_eq!(tabs[0].focus_slot, 1, "the slot travels with the word");
}

/// (e) The rebuild an indicator sees spans the prefix, so an average over
/// the loaded context is a real average and not a warm-up.
#[test]
fn the_indicator_rebuild_covers_the_venue_prefix() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    settle_indicators(&mut app);

    let pane = app.active_tab().pane(PaneSide::Time(0));
    let view = pane
        .indicators
        .all()
        .first()
        .expect("the EMA is on the time pane");
    assert!(
        view.columns[0].len() >= pane.seam_slot(),
        "the EMA has a row for every venue bar, not just the ones from prints"
    );
    // A value inside the prefix region is a real number, not a warm-up gap.
    let inside = view.columns[0]
        .iter()
        .take(pane.seam_slot())
        .rev()
        .find(|value| value.is_finite());
    assert!(
        inside.is_some(),
        "and the average is finite over the venue history"
    );
}
