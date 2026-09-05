use super::*;

/// The unit chips write one field and nothing else: a unit-only change
/// is a whole spec change, and the same two settle frames every selector
/// gets must re-cut the series.
#[test]
fn switching_only_the_imbalance_unit_recuts_the_series() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().flow_pane.kind = crate::state::BarKind::Imbalance;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Imbalance(crate::state::ImbalanceUnit::Trades, 100),
        "the kind switch lands on the default trades unit first"
    );

    app.active_tab_mut().flow_pane.imbalance_unit = crate::state::ImbalanceUnit::Volume;
    app.active_tab_mut().apply_spec_changes();
    assert!(
        app.active_tab().loading.is_active(LoadingTask::BarRebuild),
        "the arming frame shows the rebuild overlay"
    );
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Imbalance(crate::state::ImbalanceUnit::Volume, 100),
        "a unit-only change rebuilds the series"
    );
}

/// The gap the removed `Stalled` state used to cover: a transport that
/// stays open and stops delivering. No error, no disconnect, and the
/// stored arrival figure never ages — only the tape's own age does.
#[test]
fn a_quiet_tape_reads_as_stale_while_arrival_stays_frozen() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().feed_connection = FeedConnectionState::Connected;
    let trade = trade(1);
    app.active_tab_mut()
        .ingest_live_trade_at(&trade, trade.timestamp_ms + 42);

    // A moment later: fresh.
    let age = app
        .active_tab()
        .tape_age_at(trade.timestamp_ms + 500)
        .expect("a live tape has an age");
    assert!(age < metrics::STALE_TAPE_MS);
    assert_eq!(
        statusbar::tape_text(None, app.active_tab().trade_arrival_ms(), Some(age), None),
        "arrival 42 ms"
    );

    // A minute of silence on the same open socket.
    let age = app
        .active_tab()
        .tape_age_at(trade.timestamp_ms + 60_000)
        .expect("still a tape, just an old one");
    assert!(age > metrics::STALE_TAPE_MS, "{age} ms");
    assert_eq!(
        app.active_tab().trade_arrival_ms(),
        Some(42),
        "the arrival observation is frozen, which is why it cannot report this"
    );
    assert_eq!(
        statusbar::tape_text(None, app.active_tab().trade_arrival_ms(), Some(age), None),
        "stale 60 s"
    );
}

#[test]
fn one_book_drain_uses_one_clock_observation() {
    use std::cell::Cell;

    let (mut app, _evt_tx, _cmd_rx, book_tx) = test_app();
    for _ in 0..2 {
        book_tx
            .try_send(DepthEvent::Status {
                symbol: "TESTUSDT".to_owned(),
                generation: 1,
                status: quantick_orderbook::DepthStatus::Connecting,
            })
            .unwrap();
    }
    let clock_calls = Cell::new(0_u32);

    app.active_tab_mut().drain_book_feed_with_clock(|| {
        clock_calls.set(clock_calls.get() + 1);
        10_000
    });

    assert_eq!(clock_calls.get(), 1, "one wall-clock read per UI drain");
}

/// A source with no book must never write the trader's answer for them.
///
/// The saved file outranks the shipped default from the next launch on, so
/// a `heatmap = false` banked during a session on a book-less source — a
/// recording, a CFD bridge — would follow the trader onto every market
/// they open afterwards, including the ones that do have a book. Nobody
/// chose that: the source did. And the write is not hypothetical, because
/// `maintain_chart_layers` persists the whole map on any mask change, so
/// one unrelated click anywhere in the layer menu is enough to bank it.
#[test]
fn a_source_with_no_book_never_banks_the_heatmap_as_switched_off() {
    let (app, _commands) = app_without_depth();
    let pane = &app.active_tab().flow_pane;
    assert!(
        !pane.layer_visible(ChartLayer::Heatmap, &app.style),
        "with no book there is nothing to draw, which is the renderer's answer"
    );
    assert_eq!(
        pane.layer_states(&app.style).get(&ChartLayer::Heatmap),
        Some(&true),
        "but the file records the switch, which the shipped config left on"
    );
    assert_eq!(
        pane.layer_states(&app.style).get(&ChartLayer::TapeHeatmap),
        Some(&true),
        "the tape's depth layer answers to the same rule"
    );
}

/// The trader's report, walked through the real chart path: a region
/// armed while its drawn span still covered the future, and a tape that
/// then walked past the span's right edge. Every later bar is judged
/// against a band that can no longer cover it, so nothing fires and
/// nothing alarms — while the badge read a bare "armed" over a bot the
/// gate was refusing on every bar.
///
/// The fix is a badge that says so, not a disarm: the trader moves the
/// region all session, the alarm must keep listening, and a band
/// dragged back over the future has to resume with no button pressed.
#[test]
fn a_region_the_tape_walked_past_says_so_on_the_badge_and_keeps_listening() {
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
    // A band drawn over the bars in front of the trader, ending at slot
    // 4 — past the (empty) tape at arm time, so arming accepts it.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 105.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(4.0, 115.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell);
    form.window = 3;
    form.min_range = "0".to_owned();
    form.alarm = true;
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "BF sell".to_owned())
        .expect("the form compiles and the span still covers the future");

    let mut id = 0u64;
    // Six quiet bars: the tape walks past the band's right anchor at
    // slot 4 without ever presenting a setup.
    for _ in 0..6 {
        bar(&mut app, &mut id, "101", "102");
        bar(&mut app, &mut id, "102", "101");
    }
    // Now the setup the trader was waiting for: a sell force bar that
    // closes inside the band. Body 8 over an average of (1 + 1 + 8) / 3
    // = 3.33 is 2.4x — inside the shipped 1.5x..2.5x band.
    bar(&mut app, &mut id, "118", "110");

    let badge = {
        let tab = app.active_tab();
        let instance = tab
            .flow_pane
            .strategies
            .for_drawing(drawing)
            .expect("instance");
        assert!(
            tab.paper.is_flat() && tab.paper.working_orders().is_empty(),
            "the region cannot cover this bar, so no order is right",
        );
        assert!(
            instance.alarm.is_some()
                && instance.armed.state() == &quantick_strategy::ArmedState::Armed,
            "it is still armed and still listening: the alarm is the half a trader executing elsewhere depends on",
        );
        assert_eq!(
            instance.armed.hold_reason(),
            Some(quantick_strategy::HoldReason {
                reason: "region not active on this bar",
                fresh: true,
            }),
            "the reason is readable as a value, not only as a sentence",
        );
        tab.flow_pane.strategy_badge_text(drawing)
    };
    assert!(
        badge.contains("region ended — stretch it right"),
        "the chart says it where the trader is looking, with the way out: {badge}",
    );
}

/// The tape's window hook reads what a human would type, and refuses
/// everything else rather than guessing — a capture of the wrong state is
/// worse than a capture of the default one.
#[test]
fn the_tape_window_hook_reads_durations_and_refuses_nonsense() {
    assert_eq!(parse_tape_window("auto"), Some(LaneWindow::default()));
    assert_eq!(parse_tape_window("  AUTO "), Some(LaneWindow::default()));
    for (typed, ms) in [
        ("15s", 15_000),
        ("90s", 90_000),
        ("1min", 60_000),
        ("2min", 120_000),
        ("2m", 120_000),
        ("1.5min", 90_000),
        ("120000ms", 120_000),
        ("120000", 120_000),
    ] {
        assert_eq!(
            parse_tape_window(typed),
            Some(LaneWindow::Fixed { ms }),
            "{typed}"
        );
    }
    for refused in ["", "soon", "2 hours", "-5s", "0", "0s", "NaN", "infs"] {
        assert_eq!(parse_tape_window(refused), None, "{refused}");
    }
}

/// A right-click on the tape configures the tape. The candles' layers do
/// not vanish with it — they move one submenu away — because the tape is
/// also where a trader right-clicks to trade, and a menu that answered
/// only for the tape would take order entry and the drawing tools with it.
#[test]
fn a_right_click_on_the_tape_configures_the_tape_without_losing_the_chart() {
    let dir = crate::scratch::ScratchDir::new("tape-menu");
    let path = dir.join("chart-layers.toml");
    let _ = std::fs::remove_file(&path);

    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 700.0));
    let (mut app, _events, _commands, _book) = test_app();
    app.workspace.set_chart_layers_path(path.clone());

    let menu_frame = |app: &mut QuantickApp, on_tape: bool| {
        with_flow_pane(app, |pane, chrome| {
            pane.aim_context_menu_at_tape(on_tape);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| pane.draw_layer_menu(ui, chrome));
                },
            );
        });
    };

    // The candles' own menu is exactly the menu it always was.
    menu_frame(&mut app, false);
    assert_eq!(
        app.active_tab().flow_pane.layer_menu_rects.len(),
        chart_menu_entries(),
        "a click on the candles still lists every chart layer up front"
    );

    // The tape's menu answers for the tape: its own three switches at the
    // top level, and the chart's behind the submenu button rather than
    // laid out beside them.
    menu_frame(&mut app, true);
    let tape_menu = app.active_tab().flow_pane.layer_menu_rects.clone();
    assert_eq!(
        tape_menu.len(),
        tape_menu_entries(),
        "the tape's own switches are what a click on the tape lists"
    );
    assert!(
        tape_menu.iter().all(|(layer, _)| layer.on_tape()),
        "and none of the candles' are laid out beside them"
    );
    for layer in [
        ChartLayer::TapeChart,
        ChartLayer::TapeHeatmap,
        ChartLayer::TapeBubbles,
    ] {
        assert!(
            tape_menu.iter().any(|(entry, _)| *entry == layer),
            "{} is reachable from the tape's own menu",
            layer.id()
        );
    }

    // And back: aiming at the candles restores the full list, so the two
    // menus cannot leak into each other across frames.
    menu_frame(&mut app, false);
    assert_eq!(
        app.active_tab().flow_pane.layer_menu_rects.len(),
        chart_menu_entries()
    );
    std::fs::remove_file(&path).ok();
}

/// The gesture itself: a right-click on the canvas opens the menu, and the
/// primary button — which pans, zooms and places drawings — never does.
///
/// The tape switch in the canvas's top-right corner: one click takes the
/// band away, another brings it back.
///
/// It is the only way back, too — with no band there is nothing to
/// right-click — so this drives the real pointer through
/// `handle_navigation` rather than calling the setter, and checks the
/// corner it lands in.
#[test]
fn the_canvas_switch_takes_the_tape_off_and_puts_it_back() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let (mut app, _events, _commands, _book) = test_app();

    // A frame that clicks wherever `at` says, then draws — the draw is what
    // publishes the lane divider the next assertion reads.
    let frame = |app: &mut QuantickApp, at: Option<egui::Pos2>| {
        with_flow_pane(app, |pane, chrome| {
            for pressed in [true, false] {
                let events = at.map_or_else(Vec::new, |target| {
                    vec![
                        egui::Event::PointerMoved(target),
                        egui::Event::PointerButton {
                            pos: target,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        },
                    ]
                });
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let area = ui.available_rect_before_wrap();
                            pane.handle_navigation(ui, area, chrome);
                            pane.draw_chart(ui.painter(), area, chrome);
                        });
                    },
                );
            }
        });
    };

    // Where the chip is: the canvas's top-right corner, inside it.
    let chart = {
        frame(&mut app, None);
        app.active_tab()
            .flow_pane
            .frame
            .chart_area
            .expect("the canvas laid out")
    };
    let chip = crate::pane::tape_switch_rect(chart);
    assert!(
        chart.contains_rect(chip),
        "the switch is on the canvas, not off its edge"
    );
    assert!(
        chip.right() > chart.center().x && chip.top() < chart.center().y,
        "and in its top-right corner: {chip:?} of {chart:?}"
    );

    assert!(layer_on(&app, ChartLayer::TapeChart), "the tape opens on");

    frame(&mut app, Some(chip.center()));
    assert!(!layer_on(&app, ChartLayer::TapeChart), "one click takes it");
    assert_eq!(
        app.active_tab().tape().lane_width_px(chart.width()),
        0.0,
        "and no band is reserved: the candles have the whole canvas"
    );

    frame(&mut app, Some(chip.center()));
    assert!(
        layer_on(&app, ChartLayer::TapeChart),
        "and one puts it back"
    );
    assert!(app.active_tab().tape().lane_width_px(chart.width()) > 0.0);
}

/// Every point carries its own (bar, price), which is what lets a
/// circled cluster stay on top of what was circled when the view moves.
/// The alternative — anchoring the first point and keeping the shape in
/// pixels — looks better and starts pointing at the wrong bars.
#[test]
fn every_pencil_point_is_anchored_to_the_tape() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "brush");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(620.0, 300.0),
        egui::pos2(760.0, 380.0),
    );
    let stroke = &app.active_tab().flow_pane.drawings.items()[0];
    assert!(
        stroke.points.iter().all(|point| point.time_ms.is_some()),
        "a point with no instant behind it could never be re-anchored"
    );
    let bars: Vec<f32> = stroke.points.iter().map(|point| point.bar).collect();
    assert!(
        bars.windows(2).any(|pair| pair[0] != pair[1]),
        "the path spans bars, it is not one column: {bars:?}"
    );
}

/// "Zoom in for numbers" with no number is why the footprint read as slow
/// to arrive — a trader could not tell a nudge from a different chart
/// entirely.
#[test]
fn the_footprint_says_how_much_further_to_zoom() {
    let (mut app, _cmd_rx) = app_with_history(4_000);
    let ctx = egui::Context::default();
    app.active_tab_mut().flow_pane.footprint.visible = true;

    let opening = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        opening
            .iter()
            .any(|text| text.contains("numbers at") && text.contains("this zoom")),
        "the default zoom says how far off the numbers are: {opening:?}"
    );
}

#[test]
fn a_feed_declaring_a_bubble_preset_opens_wearing_it() {
    let mut config = test_config();
    config.feeds[0].bubble_preset = Some("live lane pie".to_string());
    let app = app_on(config, "binance", "TESTUSDT");
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "live lane pie"
    );
    assert!(
        app.active_tab()
            .tape()
            .config_for_test()
            .bubble_candle_summary,
        "the pie preset folds closed bars into per-price summaries"
    );
}

#[test]
fn a_feed_declaring_an_unknown_bubble_preset_changes_nothing() {
    let mut config = test_config();
    config.feeds[0].bubble_preset = Some("no such preset".to_string());
    let with_unknown = app_on(config, "binance", "TESTUSDT");
    let untouched = app_on(test_config(), "binance", "TESTUSDT");
    assert_eq!(
        with_unknown.active_tab().tape().active_preset_for_test(),
        untouched.active_tab().tape().active_preset_for_test(),
        "a typo in the config must not restyle the chart"
    );
    assert_eq!(
        with_unknown.active_tab().tape().config_for_test(),
        untouched.active_tab().tape().config_for_test()
    );
}

/// The guard that keeps an always-on recorder honest: a source with no
/// depth pipeline — a replay, or a feed missing from the config — gets no
/// recorder and no command, however often the app asks.
#[test]
fn a_source_without_depth_starts_no_recorder() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    take_capture_start(&mut cmd_rx);

    let generation = app.active_tab_mut().next_book_generation();
    app.active_tab_mut()
        .tape_mut()
        .set_enabled(false, generation);
    app.active_tab_mut().feed_id = "not-in-the-config".to_owned();
    assert!(!app.active_tab().capabilities(&app.config).book_capture);

    with_config(&mut app, |tab, config| tab.ensure_book_capture(config));
    assert!(!app.active_tab().tape().enabled());
    assert!(
        cmd_rx.try_recv().is_err(),
        "a source with no book is never asked to record"
    );
}

#[test]
fn depth_channel_updates_heatmap_without_mutating_candles() {
    use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

    let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
    let generation = take_capture_start(&mut cmd_rx);
    let bars_before = app.active_tab().flow_pane.state.bars().len();
    book_tx
        .try_send(DepthEvent::Snapshot {
            symbol: "TESTUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            price_step: None,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        })
        .unwrap();

    app.active_tab_mut().drain_book_feed();
    app.active_tab_mut().tape_mut().flush_for_test();
    let book = app.active_tab_mut().tape_mut().health();
    assert_eq!(book.bid_levels, 1);
    assert_eq!(book.ask_levels, 1);
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), bars_before);
}

#[test]
fn closed_depth_channel_is_reported_once_per_feed_handle() {
    let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
    take_capture_start(&mut cmd_rx);
    drop(book_tx);

    app.active_tab_mut().drain_book_feed();
    assert!(app.active_tab().book_channel_closed_reported);
    app.active_tab_mut().drain_book_feed();
    assert!(
        app.active_tab().book_channel_closed_reported,
        "subsequent frames keep the one-shot diagnostic latched"
    );
}

/// (b) One tape, two panes: the same trades reach both `ChartState`s, and
/// each cuts them by its own spec — which is the whole point of the split.
#[test]
fn one_tape_feeds_both_panes_and_each_cuts_it_its_own_way() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    let time = app.active_tab().time_pane().expect("time pane");
    assert_eq!(
        time.state.trades().len(),
        app.active_tab().flow_pane.state.trades().len(),
        "both panes hold the same tape"
    );
    assert_ne!(
        time.state.bars().len(),
        app.active_tab().flow_pane.state.bars().len(),
        "tick(1) and M1 cannot agree on a bar count over the same trades"
    );

    // And a live trade after the split reaches both of them.
    let flow_before = app.active_tab().flow_pane.state.trades().len();
    let time_before = app
        .active_tab()
        .time_pane()
        .expect("time pane")
        .state
        .trades()
        .len();
    let trade = trade(500);
    app.active_tab_mut()
        .ingest_live_trade_at(&trade, trade.timestamp_ms);
    assert_eq!(
        app.active_tab().flow_pane.state.trades().len(),
        flow_before + 1
    );
    assert_eq!(
        app.active_tab()
            .time_pane()
            .expect("time pane")
            .state
            .trades()
            .len(),
        time_before + 1,
        "a pane off the drain path would silently fall behind the market"
    );
}

/// The regression pin, on the market that already looked right. WIN trades
/// near 140k points on a five-point tick, and five points is the row a
/// trader expects to see; the price-derived width here is *two* points,
/// finer than the tick, and the instrument's own grid has to win.
#[test]
fn an_index_magnitude_tape_keeps_the_five_point_grouping_it_already_had() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = tape_app(&ctx, Decimal::from(140_000), Decimal::from(5));
    place_range_profile_with_the_layer_off(&mut app);
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Flow)
            .state
            .footprint_group(),
        Decimal::from(5),
        "sizing from the tape's magnitude moved a grouping the tick had settled",
    );
    assert_eq!(folded_profile_group(&app), Decimal::from(5));
}
