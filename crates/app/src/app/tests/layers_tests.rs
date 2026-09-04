use super::*;

/// The layer menu writes through to whoever owns the layer, and touches
/// nothing else. A menu holding its own copy of "is the heatmap on" would
/// disagree with the toolbar the moment either one was used.
#[test]
fn each_layer_switch_moves_exactly_one_owner() {
    let (mut app, _events, _commands, _book) = test_app();
    // What the chart opens with, and where that is decided: with no file
    // of the trader's own, `config/chart-layers.toml` is what reaches the
    // panes. The flow layers are on there — they are what the chart is
    // for — and only the backfill divider is held back.
    for (layer, expected) in [
        (ChartLayer::Heatmap, true),
        (ChartLayer::Bubbles, true),
        (ChartLayer::Footprint, true),
        (ChartLayer::LiveStrip, true),
        (ChartLayer::LaneMarks, true),
        (ChartLayer::DepthGaps, true),
        (ChartLayer::Grid, true),
        (ChartLayer::LastPrice, true),
        // A full-height rule across the candles for a boundary read once:
        // opt-in, like the market layers above it.
        (ChartLayer::BackfillDivider, false),
        (ChartLayer::SeamDivider, true),
        (ChartLayer::Crosshair, true),
        (ChartLayer::PaperTrading, true),
        (ChartLayer::Drawings, true),
    ] {
        assert_eq!(
            layer_on(&app, layer),
            expected,
            "{} opens in the wrong state",
            layer.id()
        );
    }

    for layer in ChartLayer::ALL {
        let before: Vec<bool> = ChartLayer::ALL
            .into_iter()
            .map(|other| layer_on(&app, other))
            .collect();
        let flipped = !layer_on(&app, layer);
        switch_layer(&mut app, layer, flipped);
        for (other, was) in ChartLayer::ALL.into_iter().zip(before) {
            let expected = if other == layer { flipped } else { was };
            assert_eq!(
                layer_on(&app, other),
                expected,
                "switching {} moved {} too",
                layer.id(),
                other.id()
            );
        }
        switch_layer(&mut app, layer, !flipped);
    }
}

/// Hiding is a view state, never a kill switch: the recorder keeps running
/// behind a hidden heatmap, so unhiding repaints the retained past instead
/// of opening a hole in it.
#[test]
fn hiding_a_layer_never_stops_the_data_behind_it() {
    let (mut app, _events, _commands, _book) = test_app();
    let config = app.config.clone();
    app.active_tab_mut().ensure_book_capture(&config);
    assert!(
        app.active_tab().tape().enabled(),
        "capture is on before the test"
    );

    switch_layer(&mut app, ChartLayer::Heatmap, true);
    assert!(layer_on(&app, ChartLayer::Heatmap));
    switch_layer(&mut app, ChartLayer::Heatmap, false);
    assert!(!layer_on(&app, ChartLayer::Heatmap));
    assert!(
        app.active_tab().tape().enabled(),
        "the map went off screen; the recording must not stop with it"
    );

    // Same for the drawings: the layer switch hides them, it never removes
    // them, and the objects come back with their anchors intact.
    let before = app.active_tab().flow_pane.drawings.items().len();
    switch_layer(&mut app, ChartLayer::Drawings, false);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        before,
        "hiding deletes nothing"
    );
    switch_layer(&mut app, ChartLayer::Drawings, true);
    assert!(layer_on(&app, ChartLayer::Drawings));
}

/// Close the app with layers hidden, open it again, and the canvas comes
/// back the way it was left — through the same restore the constructor runs.
#[test]
fn layer_visibility_survives_a_restart() {
    let dir = std::env::temp_dir().join(format!("quantick-app-layers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("chart-layers.toml");
    let _ = std::fs::remove_file(&path);

    let (mut app, _events, _commands, _book) = test_app();
    app.workspace.set_chart_layers_path(path.clone());
    let mask = app.layer_mask();
    app.workspace.layers_mut().record(mask);
    switch_layer(&mut app, ChartLayer::Crosshair, false);
    switch_layer(&mut app, ChartLayer::PaperTrading, false);
    switch_layer(&mut app, ChartLayer::Grid, false);
    // Switched through the toolbar's own action rather than the menu: the
    // save must follow the state, not the widget that moved it.
    app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
    // A market layer switched *on* has to come back on, which is why the
    // file records each layer's state instead of a list of hidden ones.
    switch_layer(&mut app, ChartLayer::Bubbles, true);
    // The tape's three: this file is their only home (the order-flow preset
    // refuses to carry a switch), so without them the tape would open on
    // its defaults every single launch.
    switch_layer(&mut app, ChartLayer::TapeHeatmap, false);
    switch_layer(&mut app, ChartLayer::TapeChart, false);
    app.maintain_chart_layers();
    assert_eq!(
        app.workspace.layers().mask(),
        app.layer_mask(),
        "a settled canvas writes nothing further"
    );

    let (mut restored, _events, _commands, _book) = test_app();
    restored.workspace.set_chart_layers_path(path.clone());
    restored.restore_chart_layers();
    for (layer, expected) in [
        (ChartLayer::Crosshair, false),
        (ChartLayer::PaperTrading, false),
        (ChartLayer::Grid, false),
        (ChartLayer::Heatmap, true),
        (ChartLayer::Bubbles, true),
        (ChartLayer::LastPrice, true),
        (ChartLayer::Drawings, true),
        // Off, and its layer switch off under it — a tape put back on the
        // canvas has to be the tape that was taken off it, across a
        // restart as much as across a click.
        (ChartLayer::TapeChart, false),
        (ChartLayer::TapeHeatmap, false),
        (ChartLayer::TapeBubbles, true),
    ] {
        assert_eq!(
            layer_on(&restored, layer),
            expected,
            "{} did not survive the restart",
            layer.id()
        );
    }
    // The lane marks belong to the order-flow preset; this file must not
    // have taken a second opinion on them.
    let text = std::fs::read_to_string(&path).expect("state file");
    assert!(!text.contains("lane_marks"), "{text}");
    std::fs::remove_file(&path).ok();
}

/// A tab opened later shows the same canvas as the one beside it: opening a
/// second market is not a request to bring back hidden chrome.
#[test]
fn a_new_tab_opens_on_the_layers_the_user_left_showing() {
    let dir = std::env::temp_dir().join(format!("quantick-app-newtab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("chart-layers.toml");
    let _ = std::fs::remove_file(&path);

    let (mut app, _events, _commands, _book) = test_app();
    app.workspace.set_chart_layers_path(path.clone());
    let mask = app.layer_mask();
    app.workspace.layers_mut().record(mask);
    switch_layer(&mut app, ChartLayer::Crosshair, false);
    app.maintain_chart_layers();
    // A fresh app reads the file, then opens a second market.
    let (mut restored, _events, _commands, _book) = test_app();
    restored.workspace.set_chart_layers_path(path.clone());
    restored.restore_chart_layers();
    let (_evt_tx, evt_rx) = mpsc::channel(4);
    let (_book_tx, book_rx) = mpsc::channel(4);
    let (cmd_tx, _cmd_rx) = mpsc::channel(4);
    restored.adopt_tab(
        "binance".to_owned(),
        "OTHERUSDT".to_owned(),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
        None,
    );
    assert_eq!(restored.tabs.len(), 2, "the second market opened");
    assert!(
        !layer_on(&restored, ChartLayer::Crosshair),
        "the new tab brought back a layer the user had switched off"
    );
    std::fs::remove_file(&path).ok();
}

/// The split's second pane opens wearing the same layers as the first.
///
/// `apply_pending_layout` used to copy `hidden_layers` and stop, which left
/// out every per-pane layer that does not live in that set — the footprint
/// being the one that has one. So the shipped `footprint = true` reached
/// the flow pane and never the time pane, and the toolbar's footprint lamp
/// went dark the moment the trader clicked into the left chart. The
/// deferred finding from PR #229, closed here.
#[test]
fn the_time_pane_opens_on_the_same_layers_as_the_flow_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(120);
    // The state a shipped default leaves behind: the ladder on, before the
    // second pane exists at all.
    app.active_tab_mut().flow_pane.footprint_visible = true;
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // One frame builds the time pane; that is the frame under test.
    run_frame(&mut app, &ctx);
    let time = app
        .active_tab()
        .time_pane()
        .expect("the split is what this proof is about");
    assert!(
        time.footprint_visible,
        "the time pane opened without the ladder the flow pane beside it is drawing"
    );
}

/// A tab opened mid-session inherits what is on screen *now*, not what the
/// file said at startup.
///
/// `open_tab` used to apply the map read off disk during boot. That was
/// harmless only while the map was whatever partial thing the trader's file
/// held; now that a file's silence resolves to the shipped answer, it
/// speaks for every layer, and applying it here would undo the session's
/// own switches on the way past.
#[test]
fn a_new_tab_inherits_the_live_layers_not_the_startup_file() {
    let dir = std::env::temp_dir().join(format!("quantick-app-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("chart-layers.toml");
    let _ = std::fs::remove_file(&path);

    let (mut app, _events, _commands, _book) = test_app();
    app.workspace.set_chart_layers_path(path.clone());
    // Boot on a file that says the crosshair is off...
    std::fs::write(
        &path,
        "version = 1
[layers]
crosshair = false
",
    )
    .unwrap();
    app.restore_chart_layers();
    assert!(!layer_on(&app, ChartLayer::Crosshair), "the file was read");
    // ...then the trader switches it back on, and opens a second market.
    switch_layer(&mut app, ChartLayer::Crosshair, true);
    let (_evt_tx, evt_rx) = mpsc::channel(4);
    let (_book_tx, book_rx) = mpsc::channel(4);
    let (cmd_tx, _cmd_rx) = mpsc::channel(4);
    app.adopt_tab(
        "binance".to_owned(),
        "OTHERUSDT".to_owned(),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
        None,
    );
    assert!(
        layer_on(&app, ChartLayer::Crosshair),
        "the new tab reached past the session and brought back the startup file's answer"
    );
    std::fs::remove_file(&path).ok();
}

/// The menu itself: every layer gets a switch, a layer the feed cannot
/// produce is offered disabled rather than as a lie, and clicking a real
/// checkbox hides the layer behind it.
#[test]
fn the_layer_menu_offers_every_layer_and_its_switches_work() {
    let dir = std::env::temp_dir().join(format!("quantick-app-menu-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("chart-layers.toml");
    let _ = std::fs::remove_file(&path);

    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 700.0));
    let (mut app, _events, _commands, _book) = test_app();
    app.workspace.set_chart_layers_path(path.clone());

    let menu_frame = |app: &mut QuantickApp, events: Vec<egui::Event>| {
        with_flow_pane(app, |pane, chrome| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| pane.draw_layer_menu(ui, chrome));
                },
            );
        });
    };

    menu_frame(&mut app, Vec::new());
    assert_eq!(
        app.active_tab().flow_pane.layer_menu_rects.len(),
        chart_menu_entries(),
        "every layer needs a switch, or it cannot be turned off at all"
    );
    assert!(
        app.active_tab()
            .flow_pane
            .layer_menu_rects
            .iter()
            .all(|(layer, _)| !layer.on_tape()),
        "and the candles' menu never offers a switch for the canvas beside it"
    );

    let crosshair = app
        .active_tab()
        .flow_pane
        .layer_menu_rects
        .iter()
        .find(|(layer, _)| *layer == ChartLayer::Crosshair)
        .expect("the crosshair has a switch")
        .1
        .center();
    assert!(layer_on(&app, ChartLayer::Crosshair));
    menu_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(crosshair),
            egui::Event::PointerButton {
                pos: crosshair,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    menu_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: crosshair,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
    assert!(
        !layer_on(&app, ChartLayer::Crosshair),
        "clicking the switch has to switch the layer"
    );

    // A capability the source lacks is disabled, not silently absent: a
    // recording has no book, and the entry says so instead of offering a
    // switch that would do nothing.
    let full = app.active_tab().capabilities(&app.config);
    let pane = &app.active_tab().flow_pane;
    assert!(pane.layer_blocked(ChartLayer::Heatmap, full).is_none());
    let (mut quote_only, _commands) = app_without_depth();
    let quotes = quote_only.active_tab().capabilities(&quote_only.config);
    let pane = &quote_only.active_tab().flow_pane;
    assert!(
        pane.layer_blocked(ChartLayer::Heatmap, quotes).is_some(),
        "a source with no book cannot promise a heatmap"
    );
    assert!(pane.layer_blocked(ChartLayer::Bubbles, quotes).is_some());
    assert!(pane.layer_blocked(ChartLayer::Grid, quotes).is_none());
    // The tape's twins refuse for the same reasons, and say so in the same
    // words: a source with no book has no map to put on the tape either.
    assert!(
        pane.layer_blocked(ChartLayer::TapeHeatmap, quotes)
            .is_some(),
        "a source with no book cannot promise a heatmap on the tape either"
    );
    assert!(
        pane.layer_blocked(ChartLayer::TapeBubbles, quotes)
            .is_some(),
        "nor bubbles where nothing prints a traded quantity"
    );
    assert!(
        pane.layer_blocked(ChartLayer::TapeChart, quotes).is_none(),
        "the band itself is still the trader's to show: it carries the marks \
             and the time axis whatever the source can produce"
    );
    menu_frame(&mut quote_only, Vec::new());
    assert_eq!(
        quote_only.active_tab().flow_pane.layer_menu_rects.len(),
        chart_menu_entries(),
        "an unavailable layer is still listed, just not switchable"
    );
    std::fs::remove_file(&path).ok();
}

/// §11 keeps the tape on the flow pane, so the time pane's menu says the
/// flow layers are drawn elsewhere instead of offering dead switches.
#[test]
fn the_time_pane_offers_the_flow_layers_as_drawn_elsewhere() {
    let (app, _events, _commands, _book) = test_app();
    let capabilities = app.active_tab().capabilities(&app.config);
    let time = ChartPane::time(99, 60_000);
    for layer in [
        ChartLayer::Heatmap,
        ChartLayer::Bubbles,
        ChartLayer::LiveStrip,
        ChartLayer::LaneMarks,
        ChartLayer::DepthGaps,
    ] {
        assert_eq!(
            time.layer_blocked(layer, capabilities)
                .map(|block| block.explanation),
            Some("the order-flow layers are drawn on the flow pane"),
            "{} has no machinery on a time pane",
            layer.id()
        );
        assert!(!time.layer_visible(layer, &app.style));
    }
    for layer in [
        ChartLayer::Grid,
        ChartLayer::LastPrice,
        ChartLayer::Crosshair,
        ChartLayer::Drawings,
    ] {
        assert!(time.layer_blocked(layer, capabilities).is_none());
    }
}

/// A switched-off layer really stops painting.
///
/// The switches above only prove the *state* moved; this one draws a real
/// chart and counts the shapes, so a gate someone forgets to place
/// (or later deletes) shows up as a menu entry that changes nothing.
#[test]
fn a_hidden_layer_paints_nothing() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let (mut app, _commands) = app_with_history(120);
    let under_test = [
        ChartLayer::LastPrice,
        ChartLayer::BackfillDivider,
        ChartLayer::Crosshair,
    ];

    // Shape totals may only move because one of the three synchronous
    // gates under test moved. A flow pane normally asks the order-flow
    // worker for a projection; that frame can legitimately arrive between
    // any two draws and add unrelated shapes. Hide every other layer so
    // this fixture never starts that asynchronous projection pipeline.
    for layer in ChartLayer::ALL {
        switch_layer(&mut app, layer, under_test.contains(&layer));
    }
    // The crosshair is a mode: it paints under a pointer, with its own tool
    // armed. Both are set here so the layer has something to switch off.
    app.toolrail.arm(Tool::Crosshair);

    let shapes = |app: &mut QuantickApp| -> usize {
        with_flow_pane(app, |pane, chrome| {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let area = ui.available_rect_before_wrap();
                        pane.hover_pos = Some(area.center());
                        pane.draw_chart(ui.painter(), area, chrome);
                    });
                },
            );
            output.shapes.len()
        })
    };

    // One frame to settle: plot geometry and the price range are computed
    // by a draw and read by the next one, so the first frame is
    // not yet the chart this test is counting. **Every** measurement gets
    // that frame, not just the baseline — an exact shape count taken one
    // frame after a switch is counting a chart still converging on its
    // price range, and the asymmetry made this test fail on a loaded CI
    // runner and pass on the next attempt.
    let settled = |app: &mut QuantickApp| {
        let _ = shapes(app);
        shapes(app)
    };
    let all_on = settled(&mut app);
    for layer in under_test {
        switch_layer(&mut app, layer, false);
        let off = settled(&mut app);
        assert!(
            off < all_on,
            "{} kept painting after it was switched off ({off} shapes vs {all_on})",
            layer.id()
        );
        switch_layer(&mut app, layer, true);
        assert_eq!(
            settled(&mut app),
            all_on,
            "{} did not come back exactly as it was",
            layer.id()
        );
    }
}

/// The closed-trade marks obey their own switch: a closed round trip
/// paints marks and a connector, `closed trade marks` off erases them,
/// and the live paper layer is untouched either way — hiding history
/// must never hide the position machinery.
#[test]
fn the_trade_paint_layer_switch_stops_the_marks() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    let dir =
        std::env::temp_dir().join(format!("quantick-trade-paint-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.active_tab_mut().paper.redirect_history_dir(dir.clone());
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperClose);
    evt_tx.try_send(FeedEvent::Live(trade(6))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert_eq!(
        app.active_tab().paper.session_trades().len(),
        1,
        "one closed round trip to paint"
    );

    let shapes = |app: &mut QuantickApp| -> usize {
        with_flow_pane(app, |pane, chrome| {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let area = ui.available_rect_before_wrap();
                        pane.draw_chart(ui.painter(), area, chrome);
                    });
                },
            );
            output.shapes.len()
        })
    };
    // One frame to settle the ranges a draw computes for the next one.
    let _ = shapes(&mut app);
    let marks_on = shapes(&mut app);
    switch_layer(&mut app, ChartLayer::TradePaint, false);
    let marks_off = shapes(&mut app);
    assert!(
        marks_off < marks_on,
        "the marks kept painting with their layer off ({marks_off} vs {marks_on})"
    );
    assert!(
        app.active_tab()
            .flow_pane
            .layer_visible(ChartLayer::PaperTrading, &app.style),
        "hiding closed-trade history leaves the live paper layer alone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The menu's contents are covered above; what this proves is the one thing
/// between the user and all of it, the button it is bound to.
#[test]
fn only_the_secondary_button_opens_the_layer_menu() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let (mut app, _events, _commands, _book) = test_app();

    let click = |app: &mut QuantickApp, button: egui::PointerButton| -> usize {
        with_flow_pane(app, |pane, chrome| {
            let target = screen.center();
            for pressed in [true, false] {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events: vec![
                            egui::Event::PointerMoved(target),
                            egui::Event::PointerButton {
                                pos: target,
                                button,
                                pressed,
                                modifiers: egui::Modifiers::default(),
                            },
                        ],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let area = ui.available_rect_before_wrap();
                            pane.handle_navigation(ui, area, chrome);
                        });
                    },
                );
            }
            pane.layer_menu_rects.len()
        })
    };

    assert_eq!(
        click(&mut app, egui::PointerButton::Primary),
        0,
        "a left click is a pan or a placement; it must not open the menu"
    );
    assert_eq!(
        click(&mut app, egui::PointerButton::Secondary),
        chart_menu_entries(),
        "a right click on the canvas has to open the layer menu"
    );
}

/// Reaching for a tool brings its own layer back — a crosshair that draws
/// no cross, or a line tool that places invisible objects, reads as a
/// broken tool rather than as a hidden layer.
#[test]
fn arming_a_tool_unhides_the_layer_it_draws_on() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let (mut app, _events, _commands, _book) = test_app();

    let navigate = |app: &mut QuantickApp| {
        with_flow_pane(app, |pane, chrome| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let area = ui.available_rect_before_wrap();
                        pane.handle_navigation(ui, area, chrome);
                    });
                },
            );
        });
    };

    for (tool, layer) in [
        (
            Tool::Drawing(drawings::DRAWING_TOOLS[0]),
            ChartLayer::Drawings,
        ),
        (Tool::Crosshair, ChartLayer::Crosshair),
    ] {
        app.toolrail.arm(Tool::Pointer);
        switch_layer(&mut app, layer, false);
        navigate(&mut app);
        assert!(
            !layer_on(&app, layer),
            "{} must stay hidden while nothing needs it",
            layer.id()
        );
        app.toolrail.arm(tool);
        navigate(&mut app);
        assert!(
            layer_on(&app, layer),
            "arming its tool has to bring {} back",
            layer.id()
        );
    }
}

#[test]
fn hidden_drawing_neither_paints_nor_hit_tests() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let stroke_position = egui::pos2(700.0, 300.0);
    click_chart(&mut app, &ctx, stroke_position);
    let marker = egui::Color32::from_rgb(1, 2, 3);
    app.active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .expect("placement selects the line")
        .style
        .color = marker;
    assert!(
        painted_line_with_color(&run_frame(&mut app, &ctx), marker),
        "the visible line paints its stroke"
    );

    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_selected_hidden(true);
    assert!(
        !painted_line_with_color(&run_frame(&mut app, &ctx), marker),
        "a hidden drawing must not paint"
    );

    app.active_tab_mut().flow_pane.drawings.select(None);
    click_chart(&mut app, &ctx, stroke_position);
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        None,
        "a hidden drawing must not hit-test"
    );

    let viewport_before = app
        .active_tab()
        .flow_pane
        .viewport
        .right_edge_bar(app.active_tab().flow_pane.slots());
    drag_chart(&mut app, &ctx, stroke_position, egui::pos2(640.0, 260.0));
    assert_ne!(
        app.active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        viewport_before,
        "over a hidden drawing the gesture belongs to the chart again"
    );
}

/// The user's complaint made executable: hiding the map is pixels only.
/// The recorder keeps running, so reopening it finds the history whole
/// instead of a hole where the map was closed.
#[test]
fn hiding_the_heatmap_never_stops_the_recorder() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    take_capture_start(&mut cmd_rx);
    let gaps_before = app.active_tab_mut().tape_mut().health().gaps;

    app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
    assert!(app.active_tab().tape().depth_visible());

    app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
    assert!(
        !app.active_tab().tape().depth_visible(),
        "the map is hidden"
    );
    assert!(
        app.active_tab().tape().enabled(),
        "the recorder is untouched"
    );
    assert!(
        cmd_rx.try_recv().is_err(),
        "showing or hiding the map sends no feed command"
    );

    app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
    app.active_tab_mut().tape_mut().flush_for_test();
    assert!(app.active_tab().tape().depth_visible());
    assert_eq!(
        app.active_tab_mut().tape_mut().health().gaps,
        gaps_before,
        "the toggle must not punch a coverage gap into the recording"
    );
}

/// §11: flow layers stay on the flow pane. A time pane must not run a book
/// worker, draw a lane, or claim strip pixels.
#[test]
fn the_time_pane_has_no_tape_and_no_flow_layers() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    app.apply_toolbar_action(ToolbarAction::SetLiveStrip(true));
    app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
    app.apply_toolbar_action(ToolbarAction::SetBubbles(true));
    run_frame(&mut app, &ctx);

    let time = app.active_tab().time_pane().expect("time pane");
    assert!(
        time.orderflow.is_none(),
        "no tape means no book worker behind it"
    );
    assert_eq!(
        time.live_strip_width(app.active_tab().capabilities(&app.config)),
        0.0,
        "the strip is a flow layer and claims no pixels here"
    );
    assert!(
        time.last_lane_divider_x.is_none(),
        "and there is no live lane to divide"
    );
    // The toggles still reached the flow pane, which is what owns them.
    assert!(app.active_tab().tape().depth_visible());
    assert!(app.active_tab().tape().bubbles_enabled());
    assert!(app.active_tab().flow_pane.live_strip_visible);
}

/// The instrument's price grid is a fact about the market, so both panes
/// of a split read it the same way — including while the footprint layer
/// is hidden on one of them.
///
/// This is the bug a trader saw as the same volume profile painting as a
/// wash on the time chart and as a slab on the flow chart. The two are one
/// object drawn by one function; what differed was the row height under
/// it, because the tab propagated the flow pane's capture bucket only
/// while the time pane's *footprint layer* was visible. A range profile
/// folds those same ladders with the layer hidden, so on WDO one chart
/// grouped at 0.01 and the other at 1 — a hundredfold difference in the
/// height of every row.
#[test]
fn both_panes_group_the_ladders_at_the_market_bucket_even_with_the_layer_hidden() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = history_app(&ctx);

    // The bucket the flow pane's tape publishes for this market. That is
    // the one answer; the question is whether the other pane reaches it.
    let market_bucket = app
        .active_tab()
        .pane(PaneSide::Flow)
        .state
        .footprint_group();

    // The state the defect lived in: the layer off on both panes, and a
    // range profile on the time pane wanting the ladders anyway.
    for side in [PaneSide::Time(0), PaneSide::Flow] {
        app.active_tab_mut().pane_mut(side).set_layer_visible(
            ChartLayer::Footprint,
            false,
            &mut chart_layers::LayerActions::default(),
        );
    }
    let frvp = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .expect("frvp is registered");
    {
        let time = app.active_tab_mut().pane_mut(PaneSide::Time(0));
        assert!(
            !time
                .drawings
                .place(frvp, crate::drawings::ChartPoint::at(1.0, 100.0))
        );
        assert!(
            time.drawings
                .place(frvp, crate::drawings::ChartPoint::at(20.0, 105.0))
        );
        // And put its ladders deliberately out of step, which is exactly
        // what a pane with no tape of its own drifts to when nothing hands
        // it the market's grid.
        time.state
            .set_footprint_group(market_bucket * Decimal::from(100));
    }
    assert_ne!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .state
            .footprint_group(),
        market_bucket,
        "the panes really start out disagreeing, or this proves nothing"
    );

    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let time_group = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .state
        .footprint_group();
    let flow_group = app
        .active_tab()
        .pane(PaneSide::Flow)
        .state
        .footprint_group();
    assert_eq!(
        time_group, flow_group,
        "one market, one price grid: the two panes must not disagree"
    );
    assert_eq!(
        time_group, market_bucket,
        "and it is the market's bucket, not either pane's default"
    );
}

#[test]
fn a_layer_the_source_cannot_draw_says_why_in_a_code_not_a_sentence() {
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, _cmd_rx) = mpsc::channel(16);
    let app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            // A broker-quoted instrument: prices, no traded size, no book.
            capabilities: feed::fixed_capabilities(FeedCapabilities {
                book_capture: false,
                history_paging: false,
                traded_volume: false,
                deal_counter: false,
                ohlcv_history: false,
                ohlcv_generation: 0,
            }),
            commands: cmd_tx,
            replay: None,
            latency: feed::unsplit_latency(),
        },
    );
    let _ends = (evt_tx, book_tx);

    let scene = observer_scene(&app);
    let bubbles = scene_control(&scene, "toolbar.layers.bubbles");
    assert_eq!(bubbles["availability"]["available"], false);
    assert_eq!(
        bubbles["availability"]["reason"], "source_prints_no_traded_volume",
        "the reason is the code the gate declares"
    );
    let heatmap = scene_control(&scene, "toolbar.layers.heatmap");
    assert_eq!(heatmap["availability"]["available"], false);
    assert_eq!(
        heatmap["availability"]["reason"],
        "source_captures_no_order_book"
    );

    // The strip takes either a book or traded volume and this source has
    // neither, so it is blocked too — with its own code, not one of the
    // two above. A gate declared beside the button instead of read from
    // `ChartPane::layer_blocked` reported this one available, and the
    // trader saw a lit switch that reserved no width.
    let strip = scene_control(&scene, "toolbar.layers.live_strip");
    assert_eq!(strip["availability"]["available"], false);
    assert_eq!(
        strip["availability"]["reason"],
        "source_publishes_neither_book_nor_traded_volume"
    );

    // The contrast: a control nothing gates is available with no reason at
    // all, rather than a reason saying "fine".
    let tab_id = scene_control_ids(&scene)
        .into_iter()
        .find(|id| id.starts_with("tab_strip.tab."))
        .expect("the open chart has a chip");
    let tab = scene_control(&scene, &tab_id);
    assert_eq!(tab["availability"]["available"], true);
    assert!(tab["availability"]["reason"].is_null());

    // Every reason is a code a client can branch on, never the sentence
    // the disabled button shows a human — which is the thing a client
    // would otherwise have to parse, and which translation would break.
    for control in scene["controls"].as_array().unwrap() {
        let Some(reason) = control["availability"]["reason"].as_str() else {
            continue;
        };
        assert!(
            !reason.contains(' '),
            "{} answered with prose: {reason}",
            control["control_id"]
        );
    }
    assert_ne!(
        bubbles["availability"]["reason"], "this source quotes prices but prints no traded volume",
        "the sentence on the button is not the answer on the wire"
    );
}

#[test]
fn the_layer_toggles_are_listed_the_way_the_trader_reads_them() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    let listed: Vec<String> = scene_control_ids(&observer_scene(&app))
        .into_iter()
        .filter(|id| id.starts_with("toolbar.layers."))
        .collect();
    // The group draws right-to-left from `LayerToggle::ALL`, so the eye
    // meets them in the reverse of call order. An assistant asked about
    // "the second button from the left" has to count the way the eye does.
    assert_eq!(
        listed,
        vec![
            "toolbar.layers.live_strip",
            "toolbar.layers.footprint",
            "toolbar.layers.heatmap",
            "toolbar.layers.bubbles",
        ]
    );
}

/// A rail that has been hidden reports nothing, on the very frame it is
/// hidden.
///
/// Control captures are served before the rail draws, so a rail that
/// remembered the stage it last painted would answer the first capture
/// after a hide with the buttons of a rail nobody can see.
#[test]
fn a_rail_hidden_this_frame_names_no_buttons_before_the_next_draw() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    assert!(
        scene_control_ids(&observer_scene(&app))
            .iter()
            .any(|id| id.starts_with("tool_rail.")),
        "the rail opens visible"
    );

    // Hidden, and captured before the next frame paints anything.
    app.toolrail.toggle_visible();
    let folded = scene_control_ids(&observer_scene(&app));
    assert!(
        !folded.iter().any(|id| id.starts_with("tool_rail.")),
        "a rail nobody can see contributes no controls: {folded:?}"
    );
}
