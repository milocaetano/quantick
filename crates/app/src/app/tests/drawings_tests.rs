use super::*;
use quantick_feed::history_reach;
use quantick_feed::replay::test_support as replay_test_support;

#[test]
fn a_quote_driven_feed_says_so_where_the_side_note_goes() {
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
            // What a live Tickmill US500 session publishes once its bridge
            // says hello.
            capabilities: feed::fixed_capabilities(FeedCapabilities {
                book_capture: false,
                history_paging: false,
                traded_volume: false,
                ohlcv_history: false,
                ohlcv_generation: 0,
            }),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let _ends = (evt_tx, book_tx);

    let (label, detail) = app
        .active_tab()
        .side_note(&app.config)
        .expect("a quote-driven feed discloses itself");
    assert_eq!(
        label, "prints: quote-derived",
        "a chart of one-unit prints must not read as a real tape"
    );
    // Short label, full story on hover: this row shares its space with the
    // machinery readouts, and a long label paints over them.
    assert!(label.len() < 25, "the label has to fit beside the readouts");
    assert!(
        detail.is_some_and(|text| text.contains("one synthetic print per tick")),
        "the hover has to explain what a quote-derived print is"
    );
    // And the affordances that would need a size are off with it.
    assert!(!app.active_tab().capabilities(&app.config).traded_volume);
    assert!(!app.active_tab().capabilities(&app.config).book_capture);
}

/// A reconnect that keeps the timeline: the bars survive, the window the
/// new session replays is dropped rather than counted twice, and the
/// silence in between is marked instead of stitched over.
#[test]
fn a_resumed_session_keeps_the_timeline_and_marks_the_hole() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    // A first session prints, so there is a timeline to keep.
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1), trade(2), trade(3)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    let held = app.active_tab().flow_pane.state.trades().len();
    let floor = app.active_tab().latest_trade_ms.expect("a print landed");
    assert!(held > 0, "the timeline this test keeps has to exist");

    // What `reconnect_feed` sets before it swaps the handle. Set here
    // rather than by calling it, because respawning the feed would open a
    // real socket and this is a proof about the filter, not the transport.
    app.active_tab_mut().resume_floor_ms = Some(floor);

    // The new session opens by replaying its recent window — the same
    // prints, plus one from after the silence.
    let resumed = quantick_engine::Trade {
        agg_id: 1,
        timestamp_ms: floor + 4 * 60_000,
        ..trade(1)
    };
    events
        .blocking_send(FeedEvent::LiveBatch(vec![
            trade(1),
            trade(2),
            trade(3),
            resumed.clone(),
        ]))
        .unwrap();
    app.active_tab_mut().drain_feed();

    let tab = app.active_tab();
    assert_eq!(
        tab.flow_pane.state.trades().len(),
        held + 1,
        "the replayed window is overlap, not three new prints"
    );
    assert_eq!(
        tab.resume_floor_ms, None,
        "one print past the floor is what retires it"
    );
    assert_eq!(
        tab.feed_gaps,
        vec![quantick_feed::FeedGap {
            from_ms: floor,
            to_ms: resumed.timestamp_ms,
        }],
        "four minutes nobody was listening is marked, not stitched over"
    );
}

/// The short silence a working reconnect costs is not a hole worth
/// drawing: a mark for every one of them is noise that teaches the trader
/// to stop reading marks.
#[test]
fn a_reconnect_that_worked_leaves_no_mark() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    let floor = app.active_tab().latest_trade_ms.expect("a print landed");
    app.active_tab_mut().resume_floor_ms = Some(floor);

    let resumed = quantick_engine::Trade {
        agg_id: 2,
        timestamp_ms: floor + quantick_feed::MIN_MARKED_GAP_MS - 1,
        ..trade(2)
    };
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1), resumed]))
        .unwrap();
    app.active_tab_mut().drain_feed();

    assert!(
        app.active_tab().feed_gaps.is_empty(),
        "a recovery that took under the threshold has nothing to declare"
    );
}

/// A right-click that lands on a drawing owns a section of the menu:
/// the object by name, rename, lock, hide, delete — and the lock keeps
/// guarding the delete there like everywhere else.
#[test]
fn the_drawing_section_of_the_menu_acts_on_the_clicked_object() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 700.0));
    let (mut app, _events, _commands, _book) = test_app();

    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(1.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(5.0, 110.0));
        let id = pane.drawings.items()[0].id;
        // The press half of the gesture, staged: the click resolved the
        // object and seeded the rename buffer, like the canvas path does.
        pane.context_menu.drawing = Some(id);
    }

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
    let labels: Vec<&str> = app
        .active_tab()
        .flow_pane
        .context_menu
        .menu_rects
        .iter()
        .map(|(label, _)| *label)
        .collect();
    assert_eq!(
        labels,
        ["Rename", "Add strategy", "Lock", "Hide", "Delete"],
        "the clicked object owns its section of the menu — rename, the \
             strategy seat, and the guarded actions"
    );

    let click = |rects: &[(&'static str, egui::Rect)], label: &str| {
        let pos = rects
            .iter()
            .find(|(entry, _)| *entry == label)
            .unwrap_or_else(|| panic!("{label} is offered"))
            .1
            .center();
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ]
    };

    // Locked first: the delete is offered disabled and must do nothing.
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_locked_at(0, true);
    menu_frame(&mut app, Vec::new());
    let rects = app.active_tab().flow_pane.context_menu.menu_rects.clone();
    let events = click(&rects, "Delete");
    menu_frame(&mut app, events);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a locked object never deletes from the menu"
    );

    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_locked_at(0, false);
    menu_frame(&mut app, Vec::new());
    let rects = app.active_tab().flow_pane.context_menu.menu_rects.clone();
    let events = click(&rects, "Delete");
    menu_frame(&mut app, events);
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "unlocked, the menu's delete removes the object"
    );
    assert_eq!(
        app.active_tab().flow_pane.context_menu.drawing,
        None,
        "the section lets go of the object it deleted"
    );
}

/// The replay-seek trap: a disarm that reset the ruler (timeline reset,
/// bar-spec change, market switch) used to leave "re-armed" silently
/// meaning "warming up for another N bars", so a force bar right after
/// the seek never fired. The pane's re-arm re-warms the ruler from the
/// bars the chart already shows; a bare kernel re-arm does not.
#[test]
fn rearm_after_a_series_reset_rewarms_the_ruler_from_the_chart() {
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
    fn state_of(app: &QuantickApp, drawing: drawings::DrawingId) -> quantick_strategy::ArmedState {
        app.active_tab()
            .flow_pane
            .strategies
            .anchors
            .for_drawing(drawing)
            .expect("instance")
            .armed
            .state()
            .clone()
    }

    let (mut app, _events, _commands, _book) = test_app();
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(60.0, 110.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.window = 3;
    form.min_range = "0".to_owned();
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "test BF".to_owned())
        .expect("the form compiles and the drawing exists");

    // Two quiet bars on the chart, then the series "changes" under the
    // ruler (the disarm reason a bar-spec change or replay seek names).
    let mut id = 0u64;
    bar(&mut app, &mut id, "100", "101");
    bar(&mut app, &mut id, "101", "102");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let _ = pane
            .strategies
            .anchors
            .for_drawing_mut(drawing)
            .expect("instance")
            .armed
            .disarm(quantick_strategy::DisarmReason::BarSpecChanged);
        // The pane's re-arm: kernel re-arm (which resets the ruler)
        // plus the re-warm from the chart's own closed bars.
        pane.rearm_strategy_for_drawing(drawing);
    }
    assert_eq!(
        state_of(&app, drawing),
        quantick_strategy::ArmedState::Armed
    );

    // The very next force bar fires: bodies 1, 1 re-warmed the window,
    // and this bar's body 4 makes ratio 2 on a full window of 3.
    bar(&mut app, &mut id, "102", "106");
    assert!(
        matches!(
            state_of(&app, drawing),
            quantick_strategy::ArmedState::Fired { .. }
        ),
        "the first eligible force bar after the re-arm fires; got {:?}",
        state_of(&app, drawing)
    );
}

/// An extended rectangle's region never expires off its right anchor;
/// an unextended one holds fire past it and the badge names the gate.
#[test]
fn extend_right_keeps_the_region_active_past_the_drawn_end() {
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
        // The drawn span ends at slot 2; the force bars land past it.
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(2.0, 110.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.window = 3;
    form.min_range = "0".to_owned();
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "test BF".to_owned())
        .expect("arming before any bar closed skips the span guard");

    let mut id = 0u64;
    bar(&mut app, &mut id, "100", "101");
    bar(&mut app, &mut id, "101", "102");
    bar(&mut app, &mut id, "102", "103");
    // Slot 3, past the drawn end: a force bar in price, held in time —
    // and the badge says exactly which gate held it.
    bar(&mut app, &mut id, "103", "107");
    {
        let instance = app
            .active_tab()
            .flow_pane
            .strategies
            .anchors
            .for_drawing(drawing)
            .expect("instance");
        assert_eq!(
            instance.armed.state(),
            &quantick_strategy::ArmedState::Armed,
            "past the right anchor the region is inactive: a hold, not a disarm, so dragging the band forward resumes it"
        );
        assert_eq!(
            instance.armed.status_line(),
            "armed · trigger held: region not active on this bar"
        );
    }

    // Turn on extend right — the drawn band now runs to the chart's
    // edge, and the region with it.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let index = pane.drawings.index_of(drawing).expect("drawing lives");
        pane.drawings.items_mut()[index]
            .payload
            .as_any_mut()
            .downcast_mut::<drawings::RectanglePayload>()
            .expect("a rectangle carries a rectangle payload")
            .extend_right = true;
    }
    // No re-arm: the instance never left `Armed`. A region is a
    // rectangle the trader moves all session, so its span expiring is a
    // per-bar hold that heals itself the moment the band covers the
    // future again — never a latch that needs a button.
    // Three quiet bars drain the held force bar's body out of the
    // window (back to an average of 1), then a fresh buy force bar
    // closes at 110 — inside the price band, far past the drawn end.
    bar(&mut app, &mut id, "107", "108");
    bar(&mut app, &mut id, "108", "107");
    bar(&mut app, &mut id, "107", "106");
    bar(&mut app, &mut id, "106", "110");
    assert!(
        matches!(
            app.active_tab()
                .flow_pane
                .strategies
                .anchors
                .for_drawing(drawing)
                .expect("instance")
                .armed
                .state(),
            quantick_strategy::ArmedState::Fired { .. }
        ),
        "with extend right on, the region stays active past the drawn end"
    );
}

/// The replay seek, end to end: a round trip closes on the tape, the
/// source rebuilds the timeline under it — the trade survives (it
/// happened), the bars do not — and the marks wait for the rebuilt tape
/// to reach the fills instead of stacking on whichever bar sits at the
/// edge and accumulating there as the replay runs on.
#[test]
fn a_rebuilt_timeline_does_not_stack_old_marks_on_its_edge() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    let dir = crate::scratch::ScratchDir::new("trade-paint-rebuild");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(dir.path().to_path_buf());
    // A round trip: bought on print 4, closed on print 6.
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

    // Convex polygons only — the entry triangles and the exit diamonds.
    // Counting the whole frame would ride on the price range still
    // easing after the rebuild, and a gridline more or less between two
    // measurements is not what this test is about.
    let marks = |app: &mut QuantickApp| -> usize {
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
            output
                .shapes
                .iter()
                .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Path(_)))
                .count()
        })
    };
    // Frames to settle the ranges a draw computes for the next one.
    let settled = |app: &mut QuantickApp| {
        for _ in 0..3 {
            let _ = marks(app);
        }
        marks(app)
    };

    // The seek: the bars go, the round trip stays, and the tape refills
    // from a print older than both of its fills.
    app.active_tab_mut().reset_market_state();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(0)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert_eq!(
        app.active_tab().paper.session_trades().len(),
        1,
        "the trade happened; the rebuild does not un-happen it"
    );

    let ahead_of_the_tape = settled(&mut app);
    switch_layer(&mut app, ChartLayer::TradePaint, false);
    let marks_off = settled(&mut app);
    assert_eq!(
        ahead_of_the_tape, marks_off,
        "a fill the rebuilt tape has not reached still painted: {ahead_of_the_tape} vs {marks_off}"
    );
    switch_layer(&mut app, ChartLayer::TradePaint, true);

    // The tape runs on past both fills; now the marks have their bars.
    for id in [2, 4, 6] {
        evt_tx.try_send(FeedEvent::Live(trade(id))).unwrap();
    }
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    let covered = settled(&mut app);
    switch_layer(&mut app, ChartLayer::TradePaint, false);
    let covered_off = settled(&mut app);
    assert_eq!(
        covered - covered_off,
        2,
        "the marks did not come back once the tape covered them: {covered} vs {covered_off}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The repo's capability rule: an unsupported property is *absent* from
/// the inspector, never present and inert. A text note has glyphs and no
/// stroke, so a "line width" slider on its Style tab would be a control
/// that moves nothing — which reads as a broken app, not as a no-op.
/// Found by the visual pass on a real screen.
#[test]
fn the_style_tab_offers_line_width_only_to_tools_that_have_a_stroke() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    let style_tab_labels = |app: &mut QuantickApp, ctx: &egui::Context| -> Vec<String> {
        app.surfaces
            .drawing_chrome
            .set_inspector_tab(InspectorTab::Style);
        open_inspector(app, ctx);
        painted_text(&run_frame(app, ctx))
    };

    arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let with_stroke = style_tab_labels(&mut app, &ctx);
    assert!(
        with_stroke.iter().any(|text| text.contains("line width")),
        "a stroked tool keeps its width slider; painted: {with_stroke:?}"
    );

    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(760.0, 340.0));
    assert_eq!(
        app.active_tab()
            .flow_pane
            .drawings
            .selected()
            .and_then(|index| app
                .active_tab()
                .flow_pane
                .drawings
                .items()
                .get(index)
                .map(|drawing| drawing.tool.id())),
        Some("text"),
        "the note is the selection the inspector is describing"
    );
    let words_only = style_tab_labels(&mut app, &ctx);
    assert!(
        !words_only.iter().any(|text| text.contains("line width")),
        "a note has no stroke to widen; painted: {words_only:?}"
    );
    assert!(
        words_only.iter().any(|text| text.contains("Style")),
        "the tab itself is still there, with the colour control"
    );
}

/// Reported from the running app: "clico no desenho, abre a propriedade
/// e fecha rapidamente — como se eu tivesse clicado fora dele". Selecting
/// an object must survive the release that made it and every frame after.
#[test]
fn clicking_a_drawing_keeps_it_selected_after_the_release() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
    let on_the_line = egui::pos2(700.0, 300.0);
    click_chart(&mut app, &ctx, on_the_line);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

    // Deselect the way the user would, then click the line again — this
    // is the gesture that flickers.
    app.active_tab_mut().flow_pane.drawings.select(None);
    run_frame(&mut app, &ctx);

    click_chart(&mut app, &ctx, egui::pos2(900.0, 300.0));
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(0),
        "the click that selects must not also deselect"
    );
    for frame in 0..4 {
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "the selection vanished {frame} frames after the click"
        );
    }
}

/// The two Fib tools ask different questions, so they open with different
/// spans — this is not one shared default:
///
/// - a retracement measures how much of *one move* was given back, so its
///   lines live between its own anchors and end where the trader stopped
///   dragging ("a linha deve ir até o ponto onde eu tô traçando e não ir
///   para o infinito", reported from the running build);
/// - an extension projects what comes *after* its last anchor, so drawing
///   its targets back over the measured leg is backwards on its face.
#[test]
fn each_fib_kind_opens_with_the_span_its_own_question_asks_for() {
    use crate::drawings::fib::{Extend, FibPayload};
    for (tool, expected) in [
        ("fib-retracement", Extend::Anchors),
        ("fib-extension", Extend::Forward),
    ] {
        let payload = drawing_tool(tool).default_payload();
        let fib = payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("the fib tools carry a fib payload");
        assert_eq!(
            fib.extend, expected,
            "{tool} must open with the span its own question asks for"
        );
    }
}

/// A three-anchor tool that stops following the pointer reads as frozen —
/// reported from the running build after a drag left a channel sitting
/// there. It is waiting for a click, and it now says so beside the
/// cursor, not only in a badge on the far side of the screen.
#[test]
fn a_draft_says_what_the_next_click_will_do() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
    // Drag the trend line, exactly the gesture that was reported.
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(600.0, 400.0),
        egui::pos2(800.0, 340.0),
    );
    let hover = egui::pos2(820.0, 300.0);
    let texts = painted_text(&run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(hover)],
    ));
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        2,
        "the drag placed the trend line and the object waits for its width"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Move to set the width")),
        "the draft must say what it is waiting for; painted: {texts:?}"
    );
}

/// The reported defect, end to end: drag a channel, let go, click to
/// confirm what is on screen — and get a channel.
///
/// The pointer is still standing on the trend line when the drag lets go,
/// and a channel's width is measured across that line and nowhere else.
/// So the width the pointer implied was exactly zero: the preview drew a
/// corridor of no width, which is a straight line, and the click that
/// looked like it confirmed the shape committed one — a three-anchor
/// object that is a line. The tool now refuses to be born degenerate
/// (`DrawingToolImpl::pending_anchor`), and the preview and the commit
/// come through the same door, so what is clicked is what is created.
#[test]
fn a_dragged_channel_is_born_a_corridor_and_not_a_line() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
    let release = egui::pos2(800.0, 340.0);
    drag_chart(&mut app, &ctx, egui::pos2(600.0, 400.0), release);
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        2,
        "the drag laid the trend line and the object waits for its width"
    );
    // Confirming without moving: the worst case, and the one a trader who
    // expects the drag to have finished the object actually performs.
    click_chart(&mut app, &ctx, release);

    let channel = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the click committed the channel")
        .clone();
    assert_eq!(channel.tool.id(), "parallel-channel");
    assert_eq!(channel.points.len(), 3);
    assert!(
        anchor_cross(&channel.points).abs() > 0.0,
        "the width anchor is off the trend line; anchors {:?}",
        channel.points
    );
}

/// Holding Shift while dragging the trend line lays the channel level, so
/// a range comes out flat instead of "nearly flat" — which a free hand
/// cannot produce and which no later gesture could fix by eye.
///
/// End to end: the host reads the modifier, the tool decides what level
/// means for it, and the anchor that gets committed is the levelled one.
#[test]
fn holding_shift_lays_a_channel_dead_level() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
    drag_chart_with(
        &mut app,
        &ctx,
        egui::pos2(600.0, 400.0),
        // Well off the horizontal: without the modifier this is a channel
        // sloping down across sixty pixels of price.
        egui::pos2(800.0, 340.0),
        egui::Modifiers::SHIFT,
    );
    let draft = app
        .active_tab()
        .flow_pane
        .drawings
        .draft()
        .expect("the drag laid the trend line")
        .clone();
    assert_eq!(draft.points.len(), 2);
    assert!(
        (draft.points[0].price - draft.points[1].price).abs() < 1e-9,
        "both ends of the trend line sit on one price: {:?}",
        draft.points
    );
    assert!(
        draft.points[1].bar > draft.points[0].bar,
        "and it still ran along the tape: {:?}",
        draft.points
    );
}

/// The same drag without the modifier keeps the slope the hand gave it —
/// the constraint is opt-in, never a snap the trader did not ask for.
#[test]
fn without_shift_a_channel_keeps_the_slope_it_was_drawn_with() {
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
    let draft = app
        .active_tab()
        .flow_pane
        .drawings
        .draft()
        .expect("the drag laid the trend line")
        .clone();
    assert!(
        (draft.points[0].price - draft.points[1].price).abs() > 1e-6,
        "the trend line kept its slope: {:?}",
        draft.points
    );
}

/// Shift on the click-move-click path, not just on the drag: the same
/// gesture placed a different way must mean the same thing, and a trader
/// who places by clicks is not a trader who wants a sloped range.
#[test]
fn holding_shift_levels_a_channel_placed_by_clicks() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");

    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(600.0, 400.0),
        egui::Modifiers::NONE,
    );
    // The second click lands well below the first, which without the
    // modifier is a channel sloping down across sixty pixels of price.
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(800.0, 460.0),
        egui::Modifiers::SHIFT,
    );

    let draft = app
        .active_tab()
        .flow_pane
        .drawings
        .draft()
        .expect("two clicks laid the trend line")
        .clone();
    assert_eq!(draft.points.len(), 2);
    assert!(
        (draft.points[0].price - draft.points[1].price).abs() < 1e-9,
        "clicking below still lands level: {:?}",
        draft.points
    );
}

/// The pane's half of the cmd aim's yield rule, end to end, and where
/// it draws the line: a held buy modifier over a drawing's **handle**
/// places nothing — the press goes to the drawing, so Shift still
/// levels a corner — while over its **body** and over bare canvas it
/// rests the order.
///
/// The body half is not a detail. A fixed-range profile's hit test
/// claims its whole histogram strip, so yielding bodies left a chart
/// with a profile on it with a region where the aim never appeared at
/// all. The paper-side tests hand `canvas_claimed` in by hand and so
/// prove only what the module does with the answer; this proves what
/// the pane puts in it.
#[test]
fn the_aim_yields_a_drawings_handle_but_not_its_body() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    let start = egui::pos2(600.0, 400.0);
    let end = egui::pos2(800.0, 400.0);
    click_chart_with(&mut app, &ctx, start, egui::Modifiers::NONE);
    click_chart_with(&mut app, &ctx, end, egui::Modifiers::NONE);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "the line is on the canvas"
    );
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "placing it rested nothing"
    );

    // The line's own endpoint handle: Shift there means "level this",
    // and it is the only way to ask for that.
    click_chart_with(&mut app, &ctx, end, egui::Modifiers::SHIFT);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "a held modifier over a handle rests no order"
    );

    // Its body, midway between the anchors: a region, not a target.
    // Moving a body needs no modifier, so the aim wins here — this is
    // the pixel a volume profile's histogram strip used to swallow.
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(700.0, 400.0),
        egui::Modifiers::SHIFT,
    );
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().paper.working_orders().len(),
        1,
        "over a body the aim still places"
    );

    // Bare canvas well clear of it: the aim is the last claimant, and
    // there is nothing left to claim.
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(700.0, 200.0),
        egui::Modifiers::SHIFT,
    );
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().paper.working_orders().len(),
        2,
        "and on empty canvas the click is the order"
    );
}

/// Shift on a **corner of a finished channel**, driven through the real
/// app rather than through the tool alone.
///
/// The tool-level test proves the geometry; this proves the wiring, and
/// the two are not the same claim. The host reads the modifier in a
/// different place for editing than for placing, and a `Constrain::Free`
/// left behind in that second place would pass every unit test while the
/// feature was dead in the trader's hands — which is exactly the shape of
/// the preview-versus-commit split this PR started from.
#[test]
fn shift_on_a_corner_levels_a_channel_through_the_app() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");

    let corner = egui::pos2(800.0, 340.0);
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(600.0, 400.0),
        egui::Modifiers::NONE,
    );
    click_chart_with(&mut app, &ctx, corner, egui::Modifiers::NONE);
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(800.0, 460.0),
        egui::Modifiers::NONE,
    );
    run_frame(&mut app, &ctx);

    let before = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the channel was placed")
        .clone();
    assert!(
        (before.points[0].price - before.points[1].price).abs() > 1e-6,
        "it starts sloped, or there is nothing to straighten: {:?}",
        before.points
    );

    // Grab that same corner and carry it somewhere plainly not level.
    drag_chart_with(
        &mut app,
        &ctx,
        corner,
        egui::pos2(860.0, 290.0),
        egui::Modifiers::SHIFT,
    );

    let after = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the channel is still there")
        .clone();
    assert!(
        (after.points[0].price - after.points[1].price).abs() < 1e-9,
        "the corner drag straightened it: {:?}",
        after.points
    );
}

/// The same modifier on the trend line, which is the tool a trader
/// reaches for when they want a level in the first place. A two-anchor
/// tool finishes on the release, so this proves the constraint survives
/// the path where placement and completion are the same event.
#[test]
fn holding_shift_lays_a_trend_line_dead_level() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    drag_chart_with(
        &mut app,
        &ctx,
        egui::pos2(600.0, 400.0),
        egui::pos2(800.0, 340.0),
        egui::Modifiers::SHIFT,
    );
    let line = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the release finished the trend line")
        .clone();
    assert_eq!(line.tool.id(), "trend-line");
    assert_eq!(line.points.len(), 2);
    assert!(
        (line.points[0].price - line.points[1].price).abs() < 1e-9,
        "both ends sit on one price: {:?}",
        line.points
    );
    assert!(
        line.points[1].bar > line.points[0].bar,
        "and it still ran along the tape: {:?}",
        line.points
    );
}

/// What is under the cursor while the width is being set has to be a
/// *channel*. The complaint that opened this work was that it was a
/// straight line — and it was: the preview completed the geometry with
/// the pointer, the pointer was still on the trend line the drag drew,
/// and a corridor of no width is a line. Two rails means a corridor;
/// one stroke means the tool still looks broken.
#[test]
fn a_channel_being_shaped_previews_as_a_corridor() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
    let release = egui::pos2(800.0, 340.0);
    drag_chart(&mut app, &ctx, egui::pos2(600.0, 400.0), release);
    // The pointer has not moved since the release: the worst case, and
    // the frame the trader is actually looking at.
    let output = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(release)]);
    assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 2);
    let strokes = drawing_strokes(&output);
    assert!(
        strokes >= 2,
        "both rails have to be on screen, painted {strokes} stroke(s)"
    );
}

/// The harness hook's parked pointer stands in for a hand: the preview of
/// a half-placed object is a surface, and a run with nothing touching the
/// mouse must still be able to photograph it. It is read exactly where
/// the real pointer is read, so the preview it produces is the real one.
#[test]
fn a_parked_pointer_previews_the_draft_with_no_hand_on_the_mouse() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
    let release = egui::pos2(800.0, 340.0);
    drag_chart(&mut app, &ctx, egui::pos2(600.0, 400.0), release);

    // The hand leaves the window: with no pointer there is no anchor to
    // complete the shape with, so the draft is back to the bare line the
    // two placed anchors describe.
    let gone = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerGone]);
    assert_eq!(
        drawing_strokes(&gone),
        1,
        "only the trend line the anchors themselves make"
    );

    app.active_tab_mut().flow_pane.gestures.parked_hand = Some(pane::ParkedHand {
        position: release,
        constrain: drawings::Constrain::Free,
    });
    let parked = run_frame(&mut app, &ctx);
    assert!(
        drawing_strokes(&parked) >= 2,
        "the parked pointer brings the corridor back for the camera"
    );
}

/// A tool of two anchors is finished by the release, exactly as it always
/// was: the shaping port must not have turned every drag into a gesture
/// that owes a click.
#[test]
fn a_two_anchor_tool_still_closes_on_the_release() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(600.0, 400.0),
        egui::pos2(800.0, 340.0),
    );
    let pane = &app.active_tab().flow_pane;
    assert_eq!(pane.drawings.draft_len(), 0, "nothing is left in flight");
    assert_eq!(
        pane.drawings.items().last().map(|item| item.tool.id()),
        Some("trend-line"),
        "the release finished the object"
    );
}

/// Escape during the shaping phase drops the whole draft and leaves no
/// half-made object behind — the trader's way out of a gesture they did
/// not mean to start.
#[test]
fn escape_drops_a_half_placed_channel_and_leaves_nothing_behind() {
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
    assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 2);

    run_frame_with_events(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    let pane = &app.active_tab().flow_pane;
    assert_eq!(pane.drawings.draft_len(), 0, "the draft is gone");
    assert!(
        pane.drawings.items().is_empty(),
        "a cancelled draft is not a drawing; items {:?}",
        pane.drawings.items().len()
    );
}

/// Backspace steps back one anchor at a time, so a trader who mis-placed
/// the trend line fixes that one anchor instead of starting over. The
/// last step back ends the draft rather than leaving an anchor stranded.
#[test]
fn backspace_steps_back_one_anchor_of_a_channel_at_a_time() {
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
    assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 2);

    let backspace = || key_press_with(egui::Key::Backspace, egui::Modifiers::NONE);
    run_frame_with_events(&mut app, &ctx, vec![backspace()]);
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        1,
        "one anchor back, not the whole draft"
    );
    run_frame_with_events(&mut app, &ctx, vec![backspace()]);
    let pane = &app.active_tab().flow_pane;
    assert_eq!(pane.drawings.draft_len(), 0);
    assert!(
        pane.drawings.items().is_empty(),
        "stepping back past the first anchor deletes nothing that exists"
    );
}

/// A tool with nothing specific to say still reports progress, because a
/// count beats an object that looks like it stopped responding.
#[test]
fn a_draft_without_a_hint_still_shows_its_progress() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    let fib = drawing_tool("fib-retracement");
    assert_eq!(fib.placement_hint(1), None, "this tool has no words for it");
    arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
    click_chart(&mut app, &ctx, egui::pos2(600.0, 400.0));
    let hover = egui::pos2(700.0, 320.0);
    let texts = painted_text(&run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(hover)],
    ));
    assert!(
        texts.iter().any(|text| text == "1/2"),
        "an unnamed next step still shows the count; painted: {texts:?}"
    );
}

/// The control that shares a drawing across the tab's charts has to be
/// *findable*. It lives with the anchors, on the Coordinates tab, because
/// sharing is a statement about the anchors — and every tool has that tab.
#[test]
fn the_coordinates_tab_offers_sharing_across_charts() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    app.surfaces
        .drawing_chrome
        .set_inspector_tab(InspectorTab::Coordinates);
    open_inspector(&mut app, &ctx);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.contains("Show on all charts")),
        "the sharing control must be on screen, not folded away; painted: {texts:?}"
    );
}

/// The chore this removes: re-picking the same colour on every object.
/// Saving a default must reach the *next* drawing and leave the ones
/// already on the chart exactly as the trader drew them.
#[test]
fn a_saved_default_style_reaches_the_next_drawing_and_only_that() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    let presets = crate::scratch::ScratchFile::new("default-style", "presets.toml");
    app.drawing_presets = drawings::presets::PresetStore::load_from(presets.path().to_path_buf());
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);

    let mine = egui::Color32::from_rgb(0xFF, 0xA0, 0x10);
    {
        let drawing = app
            .active_tab_mut()
            .flow_pane
            .drawings
            .selected_mut()
            .expect("the placed line is selected");
        drawing.style.color = mine;
        drawing.style.width_px = 2.5;
    }
    let edited = app.active_tab().flow_pane.drawings.items()[0].style;
    app.drawing_presets
        .set_default_style(drawings::DRAWING_TOOLS[0].id(), Some(edited));
    // Saving for one tool is saving for one tool.
    app.drawing_presets
        .set_default_style("horizontal-line", Some(edited));

    arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 380.0));
    run_frame(&mut app, &ctx);

    let items = app.active_tab().flow_pane.drawings.items();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[1].style, edited,
        "the next object opens with the saved look"
    );
    assert_eq!(
        items[0].style, edited,
        "the first object is the one that was edited, untouched by the save"
    );

    // A tool with no saved default still opens as it always did.
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    click_chart(&mut app, &ctx, egui::pos2(760.0, 420.0));
    click_chart(&mut app, &ctx, egui::pos2(860.0, 480.0));
    run_frame(&mut app, &ctx);
    let items = app.active_tab().flow_pane.drawings.items();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[2].style,
        drawings::DrawingStyle::default(),
        "a default is per tool, not a global repaint"
    );

    let _ = std::fs::remove_file(app.drawing_presets.path());
}

/// Selecting is not moving. Without a drag threshold on the move gesture,
/// a couple of pixels of hand tremor during a click re-angled a channel
/// or shifted a level — and recorded it as an undo step, so the trader's
/// line was quietly no longer where they put it. Placement already
/// refused to read a twitch as a drag; moving refuses too now.
#[test]
fn a_twitch_while_clicking_does_not_move_the_drawing() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    click_chart(&mut app, &ctx, egui::pos2(600.0, 400.0));
    click_chart(&mut app, &ctx, egui::pos2(900.0, 300.0));
    run_frame(&mut app, &ctx);
    let placed = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();

    // Press on the stroke, wobble inside the threshold, release.
    let grab = egui::pos2(750.0, 350.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
    );
    // Ending *away* from the press, still inside the threshold: a wobble
    // that returned to its origin would net to zero movement and prove
    // nothing about the threshold.
    let wobbled = grab + egui::vec2(3.0, 0.0);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(wobbled)]);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(wobbled),
            pointer_button(wobbled, false),
        ],
    );

    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        placed,
        "a click that wobbled under the threshold must leave the geometry alone"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(0),
        "and it is still a click, so it still selects"
    );

    // The same gesture past the threshold does move it.
    let far = grab + egui::vec2(40.0, 0.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(far)]);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(far), pointer_button(far, false)],
    );
    assert_ne!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        placed,
        "a real drag still moves it"
    );
}

/// The reported flicker, root cause.
///
/// The pinned inspector is a `SidePanel::right` laid out *before* the
/// central panel, so the frame a selection appears is the frame the
/// canvas narrows by the panel's width — and every drawing slides left
/// with it. Press on frame N (wide canvas) selects; release on frame N+1
/// (narrow canvas) hit-tests the same screen pixel, finds the drawing has
/// moved out from under it, and deselects. The panel opens and shuts in
/// two frames, forever, with the mouse standing still.
#[test]
fn a_pinned_inspector_cannot_wipe_the_selection_that_opened_it() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    click_chart(&mut app, &ctx, egui::pos2(600.0, 400.0));
    click_chart(&mut app, &ctx, egui::pos2(900.0, 300.0));
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

    // Pinned, and the user has expressed the preference — the exact
    // state the report was in.
    app.surfaces.drawing_chrome.set_inspector_pinned(true);
    app.surfaces.drawing_chrome.set_inspector_pin_touched(true);
    app.active_tab_mut().flow_pane.drawings.select(None);
    run_frame(&mut app, &ctx);
    let wide = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the canvas drew")
        .width();

    // Click the middle of the line: nowhere near a handle, squarely on
    // the stroke. Nothing about this gesture is marginal.
    click_chart(&mut app, &ctx, egui::pos2(750.0, 350.0));
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(0),
        "the release must not undo the selection the press made"
    );

    let narrow = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the canvas drew")
        .width();
    assert!(
        narrow < wide,
        "this proof needs the pinned panel to actually steal canvas              width: {wide} -> {narrow}"
    );

    for frame in 0..4 {
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "the selection vanished {frame} frames after the click"
        );
    }
}

/// The reported flicker, reduced to its cause.
///
/// The press selects on an anchor grab (12 px); the release used to
/// body-test only (10 px). Just past the end of a trend line those two
/// disagree: the handle is in reach and the stroke is not. So the press
/// opened the panel and the release closed it, over and over, with the
/// mouse standing still.
#[test]
fn grabbing_a_handle_off_the_stroke_selects_and_stays_selected() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    let start = egui::pos2(600.0, 400.0);
    let end = egui::pos2(800.0, 400.0);
    click_chart(&mut app, &ctx, start);
    click_chart(&mut app, &ctx, end);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

    app.active_tab_mut().flow_pane.drawings.select(None);
    run_frame(&mut app, &ctx);

    // 11 px past the far anchor, straight along the line: inside the
    // anchor radius, outside the stroke radius.
    let past_the_end = egui::pos2(end.x + 11.0, end.y);
    click_chart(&mut app, &ctx, past_the_end);
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(0),
        "grabbing the handle is clicking the object"
    );
    for frame in 0..4 {
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "the selection was wiped {frame} frames after the handle grab"
        );
    }
}

/// The same gesture on a crowded chart — the demo hook's seventeen
/// overlapping objects, which is what the report was looking at.
#[test]
fn clicking_a_drawing_on_a_crowded_chart_keeps_it_selected() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
        arm_drawing_from_toolbox(&mut app, &ctx, tool.id());
        for point in 0..tool.required_points() {
            let offset = index as f32;
            let step = point as f32;
            click_chart(
                &mut app,
                &ctx,
                egui::pos2(
                    560.0 + (offset % 4.0) * 50.0 + step * 70.0,
                    250.0 + (offset % 3.0) * 70.0 + step * 50.0,
                ),
            );
        }
    }
    run_frame(&mut app, &ctx);
    app.active_tab_mut().flow_pane.drawings.select(None);
    run_frame(&mut app, &ctx);

    // Press-release on a spot the objects cover.
    let spot = egui::pos2(630.0, 320.0);
    click_chart(&mut app, &ctx, spot);
    let picked = app.active_tab().flow_pane.drawings.selected();
    assert!(picked.is_some(), "the click found something to select");
    for frame in 0..5 {
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            picked,
            "the selection changed {frame} frames after the click"
        );
    }
}

/// Marina's ask (`docs/ux/drawing-tools-2026-08.md` §D7): a level drawn on
/// one chart of the tab shows on the other, at the same moment in market
/// time — one version of the truth instead of two hand-drawn ones.
#[test]
fn a_shared_drawing_is_painted_on_the_other_pane_of_the_tab() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // One frame builds the time pane, the next lets both panes draw and
    // cache the projection a foreign mark is re-expressed through.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().has_time_pane(),
        "the split is what this proof is about"
    );

    // Anchored on a real bar, so the anchor carries a real market time.
    let slot = 100;
    let anchored = {
        let pane = &app.active_tab().flow_pane;
        let time = pane.slot_open_time(slot).expect("a closed bar has a time");
        let price = pane
            .closed_bar(slot)
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .expect("the bar has a close");
        drawings::ChartPoint::at_time(slot as f32 + 0.5, price, Some(time))
    };
    let pane = &mut app.active_tab_mut().flow_pane;
    assert!(pane.drawings.place_with(
        drawing_tool("horizontal-line"),
        &drawings::DrawingBand::Price,
        anchored,
        |tool| {
            drawings::NewDrawing {
                style: drawings::DrawingStyle::default(),
                payload: tool.default_payload(),
            }
        },
    ));

    let own_pane_only = drawing_strokes(&run_frame(&mut app, &ctx));
    assert!(
        own_pane_only > 0,
        "the object paints on the chart it was drawn on"
    );

    let drawing = app
        .active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .expect("placement selects what it completed");
    assert!(drawing.shareable(), "an anchor on a real bar has a time");
    drawing.scope = drawings::DrawingScope::AllCharts;

    let both_panes = drawing_strokes(&run_frame(&mut app, &ctx));
    assert!(
        both_panes > own_pane_only,
        "sharing must add strokes on the other pane: {own_pane_only} -> {both_panes}"
    );
}

#[test]
fn a_shared_mark_is_selected_and_deleted_from_the_other_chart() {
    let ctx = egui::Context::default();
    let (mut app, _commands, on_the_time_pane) = split_with_a_shared_line(&ctx);

    click_chart(&mut app, &ctx, on_the_time_pane);

    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(0),
        "pressing the mirrored copy takes the one object it mirrors"
    );
    assert_eq!(
        app.active_tab().drawing_side(),
        PaneSide::Flow,
        "and the chrome follows the object, not the pane under the pointer"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "Delete on the chart the mark was seen on deletes the mark"
    );
}

#[test]
fn a_shared_mark_is_dragged_from_the_other_chart() {
    let ctx = egui::Context::default();
    let (mut app, _commands, on_the_time_pane) = split_with_a_shared_line(&ctx);
    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let undo_before = app.active_tab().flow_pane.drawings.undo_depth();

    // Straight up: a horizontal line has one anchor and price is the
    // coordinate both panes read the same way.
    drag_chart(
        &mut app,
        &ctx,
        on_the_time_pane,
        on_the_time_pane - egui::vec2(0.0, 60.0),
    );

    let after = app.active_tab().flow_pane.drawings.items()[0].points[0];
    assert!(
        after.price > before.price,
        "dragging the mirror up moves the object up: {} -> {}",
        before.price,
        after.price
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        undo_before + 1,
        "the whole drag is one undo entry on the store that holds it"
    );
}

/// The other direction, and the regression this pass exists to hold:
/// moving a shared mark on the chart it *lives* on has to carry its
/// instants with it, or the twin on the other chart stays where it was.
/// `translate_selected` moved bar indices only, which made the two views
/// disagree the moment either was dragged.
#[test]
fn moving_a_shared_mark_on_its_own_chart_carries_its_instants() {
    let ctx = egui::Context::default();
    let (mut app, _commands, _position) = split_with_a_shared_line(&ctx);
    app.active_tab_mut().flow_pane.drawings.select(Some(0));
    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];

    // Four bars to the right, through the same path a drag takes.
    app.active_tab_mut().flow_pane.drawings.begin_gesture();
    app.active_tab_mut()
        .flow_pane
        .drawings
        .translate_selected(4.0, 0.0);
    app.active_tab_mut().flow_pane.retime_selected();
    app.active_tab_mut().flow_pane.drawings.commit_gesture();

    let after = app.active_tab().flow_pane.drawings.items()[0].points[0];
    assert!(after.bar > before.bar, "the mark moved on its own chart");
    assert_ne!(
        after.time_ms, before.time_ms,
        "and the instant behind it moved too, or the other chart still \
             paints it at the old moment"
    );
    assert_eq!(
        app.active_tab()
            .flow_pane
            .slot_at_time(after.time_ms.expect("a mark on a bar has an instant")),
        Some(slot_of(after.bar)),
        "the bar and the instant say the same thing"
    );
}

#[test]
fn a_drag_on_a_mirrored_mark_does_not_also_pan_the_chart_under_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands, on_the_time_pane) = split_with_a_shared_line(&ctx);
    let time_pane = app
        .active_tab()
        .time_pane()
        .expect("the split built a time pane");
    let before = time_pane.viewport.right_edge_bar(time_pane.slots());

    drag_chart(
        &mut app,
        &ctx,
        on_the_time_pane,
        on_the_time_pane - egui::vec2(80.0, 40.0),
    );

    let time_pane = app
        .active_tab()
        .time_pane()
        .expect("the split built a time pane");
    assert_eq!(
        time_pane.viewport.right_edge_bar(time_pane.slots()),
        before,
        "the gesture belongs to the mark, so the chart behind it holds still"
    );
}

/// Full UI interaction proof: every registered drawing is placed through
/// egui pointer events against the real chart frame. This catches the
/// original regression where multi-point tools silently ignored drags.
#[test]
fn every_toolbox_drawing_can_be_plotted_on_the_chart() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    // Registry-driven, not a hand-written list: a tool added to
    // `DRAWING_TOOLS` without a rail path (family flyout, shortcut,
    // placement) fails here on the day it is registered, instead of
    // shipping unreachable.
    for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
        arm_drawing_from_toolbox(&mut app, &ctx, tool.id());
        let offset = index as f32;
        let origin = egui::pos2(560.0 + (offset % 4.0) * 50.0, 250.0 + (offset % 3.0) * 70.0);
        if tool.freehand() {
            // Held drag, not clicks — the gesture this tool declares.
            drag_chart(&mut app, &ctx, origin, origin + egui::vec2(60.0, 40.0));
            continue;
        }
        for anchor in 0..tool.required_points() {
            let step = anchor as f32;
            click_chart(
                &mut app,
                &ctx,
                origin + egui::vec2(step * 70.0, step * 50.0),
            );
        }
    }

    let tools: Vec<_> = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .iter()
        .map(|drawing| drawing.tool)
        .collect();
    assert_eq!(tools, drawings::DRAWING_TOOLS);
    assert!(
        app.active_tab_mut()
            .flow_pane
            .drawings
            .items()
            .iter()
            .all(|drawing| if drawing.tool.freehand() {
                drawing.points.len() >= 2
            } else {
                drawing.points.len() == drawing.tool.required_points()
            })
    );
    assert_eq!(
        app.toolrail.tool(),
        Tool::Pointer,
        "placing a complete drawing restores navigation"
    );
}

#[test]
fn a_drawing_can_be_selected_from_its_stroke_and_moved_without_panning() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let viewport_before = app
        .active_tab()
        .flow_pane
        .viewport
        .right_edge_bar(app.active_tab().flow_pane.slots());
    // Clear of the inspector: the panel is opaque to presses by
    // contract, so a proof about dragging must not start under it.
    let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
    drag_chart(&mut app, &ctx, start, start + egui::vec2(40.0, 40.0));
    let after = app.active_tab().flow_pane.drawings.items()[0].points[0];

    assert!(
        after.bar > before.bar,
        "dragging right moves the anchor right"
    );
    assert!(
        after.price < before.price,
        "dragging down moves the anchor to a lower price"
    );
    assert_eq!(
        app.active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        viewport_before,
        "moving a drawing must not pan the market underneath it"
    );
    assert_eq!(
        app.active_tab().flow_pane.gestures.drag,
        DrawingDrag::None,
        "release ends the move gesture"
    );
}

#[test]
fn a_press_on_the_inspector_never_grabs_the_stroke_beneath_it() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let anchor = egui::pos2(700.0, 300.0);
    click_chart(&mut app, &ctx, anchor);
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    // Automatic placement now sends the panel to a chart corner (§D3),
    // deliberately clear of the object — so park it over the stroke by
    // hand. What this proves is pointer routing over an opaque panel, not
    // where the panel opens.
    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("placing the selected line opens its inspector");
    app.surfaces.drawing_chrome.set_inspector_moved(true);
    app.surfaces
        .drawing_chrome
        .set_inspector_pos(Some(egui::pos2(
            inspector.left(),
            anchor.y - inspector.height() / 2.0,
        )));
    run_frame(&mut app, &ctx);
    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is still open");
    let line_y = anchor.y;
    let start = egui::pos2(inspector.center().x, line_y);
    assert!(
        inspector.contains(start),
        "this proof needs the inspector to cover a stroke pixel"
    );
    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];

    drag_chart(&mut app, &ctx, start, egui::pos2(start.x, line_y + 100.0));

    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points[0],
        before,
        "a press on the inspector must never fall through to the chart"
    );
    assert_eq!(
        app.active_tab().flow_pane.gestures.drag,
        DrawingDrag::None,
        "no drawing drag may start from a press on the inspector"
    );
}

#[test]
fn a_canvas_drag_keeps_running_while_crossing_the_inspector() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
    assert!(
        !inspector.contains(start),
        "the gesture must begin on the open canvas"
    );
    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];

    // The gate applies at press time only: a drag that began on the
    // canvas keeps moving the object while the pointer crosses the panel.
    drag_chart(&mut app, &ctx, start, inspector.center());

    assert_ne!(
        app.active_tab().flow_pane.drawings.items()[0].points[0].price,
        before.price,
        "continuity: the drag survives crossing the inspector"
    );
}

#[test]
fn drawing_actions_visible_without_scroll_at_360px() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

    open_inspector(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    assert!(
        inspector.width() >= INSPECTOR_MIN_WIDTH_PX - 1.0,
        "the inspector must respect its minimum width; got {}",
        inspector.width()
    );
    let texts = painted_text(&output);
    for label in ["Lock drawing", "Delete drawing"] {
        assert!(
            texts.iter().any(|text| text.contains(label)),
            "the named action {label:?} must be visible without scrolling; painted: {texts:?}"
        );
    }
}

#[test]
fn inspector_opens_beside_the_selection_inside_the_chart() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(300.0, 300.0),
        egui::pos2(400.0, 380.0),
    );
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the chart pane was laid out");
    let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 300.0), egui::pos2(400.0, 380.0))
        .expand(DRAWING_ANCHOR_RADIUS_PX);
    assert!(
        !inspector.intersects(bbox),
        "the inspector must open beside the selection, not on top of it: {inspector:?} vs {bbox:?}"
    );
    assert!(
        chart.contains_rect(inspector),
        "the inspector must stay inside the chart pane (never over the axes): {inspector:?} vs {chart:?}"
    );
}

#[test]
fn pinning_the_inspector_docks_it_and_frees_the_canvas() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(300.0, 300.0),
        egui::pos2(400.0, 380.0),
    );
    // Let the window settle its size and position before reading rects.
    open_inspector(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let chart_before = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("chart laid out");

    let pin = app
        .surfaces
        .drawing_chrome
        .inspector_pin_rect()
        .expect("pin button rendered");
    click_chart(&mut app, &ctx, pin.center());
    assert!(
        app.surfaces.drawing_chrome.inspector_pinned(),
        "clicking Pin docks the inspector"
    );
    run_frame(&mut app, &ctx);
    let chart_after = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("chart laid out");
    assert!(
        chart_after.width() < chart_before.width(),
        "the docked inspector must be paid for by the canvas, not float over it"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.contains("Delete drawing")),
        "the docked panel still shows the named actions"
    );
}

#[test]
fn double_clicking_the_title_bar_returns_to_automatic_placement() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    let bar = egui::pos2(inspector.left() + 60.0, inspector.top() + 14.0);
    drag_chart(&mut app, &ctx, bar, bar + egui::vec2(120.0, 90.0));
    assert!(app.surfaces.drawing_chrome.inspector_moved());

    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("still open");
    let bar = egui::pos2(inspector.left() + 60.0, inspector.top() + 14.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(bar),
            pointer_button(bar, true),
            pointer_button(bar, false),
            pointer_button(bar, true),
            pointer_button(bar, false),
        ],
    );
    assert!(
        !app.surfaces.drawing_chrome.inspector_moved(),
        "double-click on the title bar re-arms automatic placement"
    );
}

/// The bar a *click* raises — the row of icons beside the object, the
/// first surface a trader meets and the one they drag aside when it
/// covers the price action they are reading.
///
/// It used to snap back to the next object, which handed the covered
/// chart straight back, once per click, with no way to answer it once.
#[test]
fn a_parked_context_bar_greets_the_next_drawing_too() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let avwap = place_drawing(&mut app, &ctx, "anchored-vwap", &[egui::pos2(500.0, 300.0)]);
    let profile = place_drawing(
        &mut app,
        &ctx,
        "fixed-range-profile",
        &[egui::pos2(700.0, 300.0), egui::pos2(820.0, 420.0)],
    );

    // A finished object with the tool disarmed is what a trader has in
    // hand when they reach for the bar.
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(avwap));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let bar = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("the selection raised the bar");
    let grip = egui::pos2(bar.left() + 8.0, bar.center().y);
    drag_chart(&mut app, &ctx, grip, grip + egui::vec2(-180.0, 200.0));
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces
            .drawing_chrome
            .context_bar()
            .manual_position()
            .is_some(),
        "the grip drag records a hand-placed position"
    );
    let parked = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("still up")
        .min;

    app.drawing_pane_mut().drawings.select(Some(profile));
    // The mirror is written only when the bar actually draws, and none of
    // the host's early returns clear it — so a stale value would answer
    // for a bar that stopped appearing, which is the regression this test
    // exists to catch. Blank it and let the frame fill it in.
    app.surfaces.drawing_chrome.forget_context_bar_rect();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.surfaces
            .drawing_chrome
            .context_bar_rect()
            .expect("the next object raises it too")
            .min,
        parked,
        "the bar stays where the hand put it, on the next object too"
    );

    // …and the way back is still one gesture on the same grip.
    // Far enough after the last click that egui reads the pair below as a
    // double and not the tail of a triple: it counts a third click inside
    // twice `max_double_click_delay`. Both halves come off egui rather
    // than a literal — the harness clock advances by `predicted_dt` a
    // frame, since `run_frame_sized` sends no `time` — so a change to
    // either default moves this wait with it instead of turning the test
    // into a puzzle about a double-click that stopped registering.
    let quiet_frames = {
        let seconds = ctx.options(|options| 2.0 * options.input_options.max_double_click_delay);
        let dt = f64::from(ctx.input(|input| input.predicted_dt));
        (seconds / dt).ceil() as usize + 2
    };
    for _ in 0..quiet_frames {
        run_frame(&mut app, &ctx);
    }
    let bar = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("still up");
    let grip = egui::pos2(bar.left() + 8.0, bar.center().y);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(grip),
            pointer_button(grip, true),
            pointer_button(grip, false),
            pointer_button(grip, true),
            pointer_button(grip, false),
        ],
    );
    app.surfaces.drawing_chrome.forget_context_bar_rect();
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.surfaces.drawing_chrome.context_bar().manual_position(),
        None,
        "the double-click hands placement back to the rule"
    );

    // And the bar is drawn where that rule wants it, not merely somewhere
    // that is no longer the parked point. This is the only door out of the
    // parked state, and a door that clears the flag while leaving the bar
    // in a third place would be no door at all.
    let placed = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("the bar is still up");
    let chart = app.drawing_pane().frame.chart_area.expect("the pane drew");
    let expected = drawings::context_bar::place(
        chart,
        app.drawing_pane()
            .frame
            .lane_divider_x
            .unwrap_or(chart.right()),
        app.drawing_bbox_on_screen(chart, profile)
            .expect("the object projects"),
        placed.size(),
    );
    assert_ne!(
        expected, parked,
        "the setup has to park it somewhere the rule would not have chosen"
    );
    assert_eq!(
        placed.min, expected,
        "the double-click puts it back beside the object it belongs to"
    );
}

/// The way back is a decision too. Double-clicking the title bar hands the
/// popup back to automatic placement, and that has to reach the file —
/// otherwise the next launch returns the position the trader just gave up.
#[test]
fn giving_the_placement_back_is_saved_as_well() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    with_a_saved_workspace(&mut app, &ctx, "popup-reset");
    draw_horizontal_line(&mut app, &ctx, 300.0);
    park_the_popup(&mut app, &ctx, egui::vec2(120.0, 90.0));
    assert!(
        ui_state::load(app.workspace.ui_state_path())
            .chrome
            .is_some_and(|chrome| chrome.inspector_position.is_some()),
        "the parked position is on disk before the reset"
    );

    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("still open");
    let grip = egui::pos2(popup.left() + 60.0, popup.top() + 14.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(grip),
            pointer_button(grip, true),
            pointer_button(grip, false),
            pointer_button(grip, true),
            pointer_button(grip, false),
        ],
    );
    run_frame(&mut app, &ctx);

    assert_eq!(
        ui_state::load(app.workspace.ui_state_path())
            .chrome
            .expect("the chrome is still recorded")
            .inspector_position,
        None,
        "the file forgets the position the trader discarded"
    );
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// The harness table tells an agent to pair `QUANTICK_DRAWING_INSPECTOR=1`
/// with the drawings demo. That instruction is only true if the demo's own
/// selection does not close the panel the hook asked for — it did, so
/// every capture of the popup was a capture of a chart without one.
#[test]
fn the_drawings_demo_keeps_the_panel_the_hook_asked_for() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(500);
    app.workspace
        .set_ui_state_path(scratch_ui_state("demo-inspector"));
    app.harness.arm_drawings_demo(DrawingsDemo::default());
    app.surfaces.drawing_chrome.set_inspector_open(true);

    for _ in 0..4 {
        run_frame(&mut app, &ctx);
    }

    assert!(!app.harness.drawings_demo_armed(), "the demo has run");
    assert!(
        app.drawing_pane().drawings.selected().is_some(),
        "and left an object selected, which is what closes the panel"
    );
    assert!(
        app.surfaces.drawing_chrome.inspector_open(),
        "the panel the hook asked for survives the demo's own selection"
    );
}

/// The same guarantee reached the way a capture run reaches it: through
/// the environment variable, not by setting the field first.
///
/// The distinction is the whole test. `QUANTICK_DRAWING_INSPECTOR` used to
/// be read in the constructor, and when it moved to the surface's own hook
/// it landed in the pass the registry applies on the first *drawn* frame —
/// which runs after `apply_drawing_demo`, the very code that asks whether
/// the panel is open before it moves the selection. The panel then closed
/// itself and every capture pairing the two hooks photographed a chart
/// with no inspector. Nothing failed: the sibling test above sets the
/// field directly, so it could not see the order slip.
#[test]
fn the_inspector_hook_survives_the_demo_that_runs_before_it() {
    let ctx = egui::Context::default();
    // `set_var` is process-wide and the suite is threaded, and this one
    // is a *real* hook name every `QuantickApp::new` in the suite reads —
    // so unlike `store_home`'s unique-name trick, a lock is the only way
    // a neighbour cannot see it.
    static LAUNCH_HOOK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LAUNCH_HOOK.lock().unwrap_or_else(|held| held.into_inner());
    // SAFETY: single-threaded section held by the lock above, and the
    // variable is removed again before it is released.
    unsafe { std::env::set_var("QUANTICK_DRAWING_INSPECTOR", "1") };
    let (mut app, _commands) = app_with_history(500);
    unsafe { std::env::remove_var("QUANTICK_DRAWING_INSPECTOR") };
    app.workspace
        .set_ui_state_path(scratch_ui_state("hook-inspector"));
    app.harness.arm_drawings_demo(DrawingsDemo::default());

    assert!(
        app.surfaces.drawing_chrome.inspector_open(),
        "the hook is read at launch, before any frame the demo runs in"
    );
    for _ in 0..4 {
        run_frame(&mut app, &ctx);
    }
    assert!(!app.harness.drawings_demo_armed(), "the demo has run");
    assert!(
        app.drawing_pane().drawings.selected().is_some(),
        "and left an object selected, which is what closes the panel"
    );
    assert!(
        app.surfaces.drawing_chrome.inspector_open(),
        "the panel the hook asked for survives the demo's own selection"
    );
}

#[test]
fn a_narrow_chart_opens_the_inspector_pinned_until_the_pin_is_touched() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    let narrow = egui::vec2(1_150.0, 900.0);
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    assert!(
        app.focused_pane()
            .frame
            .chart_area
            .is_some_and(|chart| chart.width() < INSPECTOR_AUTO_PIN_CHART_WIDTH_PX),
        "this proof needs a chart narrower than the auto-pin threshold"
    );
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_sized(&mut app, &ctx, narrow, egui::pos2(600.0, 300.0));
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    // The auto-pin is about the *panel*, and the panel now opens on
    // request: asking for it on a chart this narrow is what trips the
    // rule, because a 320 px floating window has nowhere to go here.
    app.surfaces.drawing_chrome.set_inspector_open(true);
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    assert!(
        app.surfaces.drawing_chrome.inspector_pinned(),
        "opening the panel on a narrow chart opens it pinned"
    );

    // The user unpins: their preference holds from here on.
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    let pin = app
        .surfaces
        .drawing_chrome
        .inspector_pin_rect()
        .expect("the panel renders its pin");
    click_sized(&mut app, &ctx, narrow, pin.center());
    assert!(
        !app.surfaces.drawing_chrome.inspector_pinned(),
        "the pin toggles the panel off"
    );
    assert!(
        app.surfaces.drawing_chrome.inspector_pin_touched(),
        "the preference is recorded"
    );

    // Unpinning with the same selection must recompute placement — not
    // fall back to the fixed default corner (the pinned host claimed the
    // selection each frame, so the floating host has to re-place it).
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    let floating = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("unpinning reopens the floating inspector");
    assert_ne!(
        floating.min, DRAWING_INSPECTOR_DEFAULT_POSITION,
        "the reopened window must be placed, not parked at the default"
    );
    // The selected line's anchor bbox (anchor ± select radius).
    let bbox = egui::Rect::from_center_size(egui::pos2(600.0, 300.0), egui::vec2(24.0, 24.0));
    assert!(
        !floating.intersects(bbox),
        "the reopened window sits beside the selected object: {floating:?}"
    );

    // Deselect, reselect: same width, but the touched pin wins now.
    run_sized_frame(
        &mut app,
        &ctx,
        narrow,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    click_sized(&mut app, &ctx, narrow, egui::pos2(500.0, 300.0));
    run_sized_frame(&mut app, &ctx, narrow, Vec::new());
    assert!(
        !app.surfaces.drawing_chrome.inspector_pinned(),
        "once touched, the auto-pin width rule stops firing"
    );
}

#[test]
fn hovering_the_inspector_sets_no_chart_cursor() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    // Parked over the stroke by hand: automatic placement now clears the
    // object on purpose (§D3), and this proof needs the overlap.
    app.surfaces.drawing_chrome.set_inspector_moved(true);
    app.surfaces
        .drawing_chrome
        .set_inspector_pos(Some(egui::pos2(
            inspector.left(),
            300.0 - inspector.height() / 2.0,
        )));
    run_frame(&mut app, &ctx);
    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is still open");
    // Over the inspector AND over the selected line's stroke: without
    // the chrome gate this hover would show a Move cursor.
    let hover = egui::pos2(inspector.center().x, 300.0);
    assert!(inspector.contains(hover));
    let output = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(hover)]);
    assert!(
        !matches!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::Move | egui::CursorIcon::ResizeNwSe | egui::CursorIcon::NotAllowed
        ),
        "the chart must not read a hover through the inspector; got {:?}",
        output.platform_output.cursor_icon
    );
}

#[test]
fn the_object_manager_opens_beside_the_rail() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let objects = app
        .toolrail
        .objects_button_rect()
        .expect("the rail shows the Objects entry");
    click_chart(&mut app, &ctx, objects.center());
    assert!(app.surfaces.drawing_chrome.manager_open());
    run_frame(&mut app, &ctx);

    let manager = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_manager")))
        .expect("the manager is open");
    let chart = app.focused_pane().frame.chart_area.expect("chart laid out");
    // Default dock is Left: the manager opens one gap inboard of the
    // rail's inner edge, aligned with its leading (top) end.
    assert!(
        (manager.left() - (chart.left() + DRAWING_MANAGER_GAP_PX)).abs() < 1.0,
        "manager left edge: {} vs chart {}",
        manager.left(),
        chart.left()
    );
    assert!(
        (manager.top() - (chart.top() + DRAWING_MANAGER_GAP_PX)).abs() < 1.0,
        "manager top edge: {} vs chart {}",
        manager.top(),
        chart.top()
    );
}

/// The live use of a mark is marking the bar that is *running* — marking
/// a closed one is review. `closed_bar` stops one slot short of the
/// forming bar, so the snap used to fall through to the pointer's own
/// price there: the mark landed inside the candle body, which is the one
/// failure the whole tool exists to avoid, in the only moment it is used
/// under pressure.
#[test]
fn a_mark_on_the_forming_bar_still_grabs_its_extreme() {
    // tick(2) with an odd trade count: the last print leaves a bar open,
    // which `app_with_history`'s tick(1) never does.
    let (mut app, evt_tx, _commands, _book) = test_app();
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(2));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    let trades: Vec<_> = (1..=201).map(trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.active_tab_mut().drain_feed();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let forming = pane.closed_slots();
    assert_eq!(
        pane.slots(),
        forming + 1,
        "this proof needs a bar still forming"
    );

    // Aim at the forming slot: the newest one, at the right edge of the
    // history area.
    let chart = pane.frame.chart_area.expect("chart laid out");
    let width = pane.viewport.candle_width();
    let right = pane.frame.lane_divider_x.unwrap_or(chart.right());
    let x = right - width * 0.5;

    arm_drawing_from_toolbox(&mut app, &ctx, "arrow-mark-up");
    click_chart(&mut app, &ctx, egui::pos2(x, 300.0));
    let placed = app.active_tab().flow_pane.drawings.items().last();
    let Some(placed) = placed else {
        panic!("the mark was not placed on the forming bar");
    };
    let anchor = placed.points[0];
    assert_eq!(
        slot_of(anchor.bar),
        forming,
        "the click has to land on the forming slot for this to prove anything"
    );
    let low = app
        .active_tab()
        .flow_pane
        .state
        .partial()
        .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.low))
        .expect("the forming bar has a low");
    assert!(
        (anchor.price - low).abs() < 1e-9,
        "the mark must hang from the forming bar's low ({low}), not the cursor ({})",
        anchor.price
    );
}

/// The draft belongs to the band its first anchor landed in. A hand that
/// strays into the indicator pane mid-stroke would otherwise write that
/// pane's value into an object living on the price axis — and a stroke
/// has no handles, so it could only be deleted and redrawn.
#[test]
fn a_pencil_stroke_ignores_points_outside_its_own_band() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("chart laid out");

    arm_drawing_from_toolbox(&mut app, &ctx, "brush");
    // Start well inside the price band, then run the pointer far below
    // the pane and back — the shape of a hand overshooting the divider.
    let start = egui::pos2(620.0, chart.center().y);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    for offset in [30.0_f32, 60.0, 90.0] {
        let below = egui::pos2(start.x + offset, chart.bottom() + 200.0);
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(below)]);
    }
    let end = egui::pos2(start.x + 120.0, chart.center().y + 20.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    );

    let items = app.active_tab().flow_pane.drawings.items();
    assert_eq!(items.len(), 1, "one stroke");
    let stroke = &items[0];
    assert_eq!(stroke.band, drawings::DrawingBand::Price);
    // Every anchor has to be a price this chart could actually show.
    let (lo, hi) = app
        .active_tab()
        .flow_pane
        .frame
        .auto_range
        .expect("the pane has a range");
    let span = hi - lo;
    for point in &stroke.points {
        assert!(
            point.price > lo - span && point.price < hi + span,
            "a stray point escaped the band: {} not near {lo}..{hi}",
            point.price
        );
    }
}

/// A text note is placed *empty*, and its words are its content — so the
/// caret goes into the object, on the chart, on the frame it lands. It
/// used to open the settings panel instead, which put the field on the
/// far side of the screen from the note being written.
#[test]
fn placing_a_text_note_opens_the_editor_in_the_note_itself() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    assert_eq!(app.inline_text_editing(), None);

    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    let index = app
        .inline_text_editing()
        .expect("the one tool that arrives empty takes the caret");
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[index].tool.id(),
        "text"
    );
    assert!(
        ctx.memory(|memory| memory.focused().is_some()),
        "the field opens focused: a caret nobody can see is a click nobody was told about"
    );
    assert!(
        !app.surfaces.drawing_chrome.inspector_open(),
        "the panel is no longer how a note is written"
    );

    // …and no other tool takes the caret: the keyboard stays with the
    // chart for everything whose content is geometry.
    arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
    click_chart(&mut app, &ctx, egui::pos2(640.0, 320.0));
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.inline_text_editing(),
        None,
        "a line is complete when it is drawn; it gets the bar, not a caret"
    );
}

/// Typing is one edit, not one per keystroke: undo takes the note back,
/// not the last letter of it.
#[test]
fn undo_after_typing_takes_the_whole_note_back() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    let index = app.inline_text_editing().expect("the editor is open");

    for word in ["swing ", "low"] {
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::Text(word.to_owned())]);
    }
    run_frame(&mut app, &ctx);
    // Close the editor: the typing gesture commits with it.
    app.end_inline_text_edit();
    assert!(app.active_tab_mut().flow_pane.drawings.undo());
    let drawing = &app.active_tab().flow_pane.drawings.items()[index];
    assert_eq!(
        drawing.tool.inline_text(drawing.payload.as_ref()),
        Some(""),
        "one undo, the whole note"
    );
}

/// One note, one placeholder. The object paints "Note" when it is empty
/// and the field offers "Add text" — stacked, they read as two objects,
/// which is what the first capture of this editor showed. The object
/// stands down for as long as the field is holding its words.
#[test]
fn the_note_stops_painting_itself_while_its_editor_is_open() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let painted = painted_text(&run_frame(&mut app, &ctx));
    assert!(app.inline_text_editing().is_some(), "the editor is open");
    assert!(
        !painted.iter().any(|text| text == "Note"),
        "the object's placeholder must not sit over the field: {painted:?}"
    );

    // Closed again, the object is the only thing holding the words — so
    // an empty note goes back to saying it is there.
    app.end_inline_text_edit();
    let painted = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        painted.iter().any(|text| text == "Note"),
        "an empty note must stay findable once nothing is editing it: {painted:?}"
    );
}

/// A note against the top of the chart opens its field *below* the
/// anchor. Pinned upward it lands on the flow legend — chrome over the
/// key it is annotating — and the trader reads neither.
#[test]
fn the_field_opens_below_a_note_that_has_no_room_above_it() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    // As high in the chart as a note can be placed.
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("a drawn chart");
    click_chart(&mut app, &ctx, egui::pos2(700.0, chart.top() + 2.0));
    let output = run_frame(&mut app, &ctx);
    assert!(app.inline_text_editing().is_some(), "the editor is open");

    let hint = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == INLINE_TEXT_HINT => Some(text.pos),
            _ => None,
        })
        .next()
        .expect("the field is on screen with its hint");
    assert!(
        hint.y >= chart.top(),
        "the field must stay inside the chart: {hint:?} against {chart:?}"
    );
    assert!(
        hint.y > chart.top() + 2.0,
        "with no room above the anchor the field opens below it: {hint:?}"
    );
}

/// Fixing a typo has to be possible. Placing a note no longer opens the
/// panel, so the way back into its words is the object itself: pointing
/// at a note and double clicking asks to type in it, the same reading as
/// double clicking a curve to open its settings.
#[test]
fn a_double_click_on_a_note_opens_its_editor_again() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    let position = egui::pos2(700.0, 300.0);
    click_chart(&mut app, &ctx, position);
    run_frame(&mut app, &ctx);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::Text("swing high".to_owned())],
    );
    app.end_inline_text_edit();
    run_frame(&mut app, &ctx);
    assert_eq!(app.inline_text_editing(), None, "the editor is closed");

    // The placement click has to age out of egui's click sequence first:
    // it counts presses closer than `max_double_click_delay` as a double
    // and twice that as a triple, so without the quiet stretch the pair
    // below is reported as a *triple* click and `double_clicked()` never
    // fires. Derived from egui's own default, not a magic frame count.
    let quiet = ctx.options(|options| options.input_options.max_double_click_delay) * 2.0;
    let frames = (quiet / f64::from(ctx.input(|input| input.predicted_dt))).ceil() as usize + 2;
    for _ in 0..frames {
        run_frame(&mut app, &ctx);
    }

    // Two clicks on the note the way a hand makes them.
    click_chart(&mut app, &ctx, position);
    click_chart(&mut app, &ctx, position);
    run_frame(&mut app, &ctx);
    let index = app
        .inline_text_editing()
        .expect("a double click on the note reopens its editor");
    let drawing = &app.active_tab().flow_pane.drawings.items()[index];
    assert_eq!(
        drawing.tool.inline_text(drawing.payload.as_ref()),
        Some("swing high"),
        "and it opens on the words already there"
    );
}

/// The editor belongs to one note on one pane of one tab. Switching tabs
/// with it open closes it and records the edit where the note actually
/// lives — never against whatever object sits at that index on the tab
/// now in front.
#[test]
fn switching_tabs_closes_the_editor_and_leaves_the_note_on_its_own_tab() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Text("mine".to_owned())]);
    assert!(app.inline_text_editing().is_some());
    let home = app.active_tab().id;

    app.open_tab("binance".to_owned(), "TESTUSDT".to_owned(), None);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.inline_text_editing(),
        None,
        "the editor does not follow the trader to another tab"
    );
    // The new tab is on the same market, so the layout puts the same note
    // on it — a drawing belongs to a market on a pane, not to a tab. The
    // words were committed when the editor closed, and the mirror follows
    // a commit on the next frame.
    run_frame(&mut app, &ctx);
    let mirrored = app
        .tabs
        .iter()
        .find(|tab| tab.id != home)
        .expect("the new tab")
        .flow_pane
        .drawings
        .items();
    assert_eq!(mirrored.len(), 1, "the market's note came with the market");
    assert_eq!(
        mirrored[0].tool.inline_text(mirrored[0].payload.as_ref()),
        Some("mine")
    );

    let owner = app
        .tabs
        .iter()
        .find(|tab| tab.id == home)
        .expect("home tab");
    let drawing = &owner.flow_pane.drawings.items()[0];
    assert_eq!(
        drawing.tool.inline_text(drawing.payload.as_ref()),
        Some("mine"),
        "the words stayed with the note"
    );
    assert_eq!(
        owner.flow_pane.gestures.content_editing, None,
        "and the pane holding it stopped suppressing it, so it paints again"
    );
}

/// The `QUANTICK_TEXT_NOTE` hook reaches the editor without a hand: it
/// places a note and opens it through the very calls a click makes, so a
/// screenshot of this state is a screenshot of the real surface.
#[test]
fn the_text_note_hook_opens_the_editor_without_a_click() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.surfaces.drawing_chrome.set_pending_text_note(true);
    run_frame(&mut app, &ctx);
    let index = app
        .inline_text_editing()
        .expect("the hook opened the editor");
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[index].tool.id(),
        "text"
    );
}

/// The pencil is the first tool placed by a held drag. One gesture, one
/// object, one undo entry — and a path, not two anchors.
#[test]
fn the_pencil_draws_a_path_from_one_held_drag() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let undo_before = app.active_tab().flow_pane.drawings.undo_depth();

    arm_drawing_from_toolbox(&mut app, &ctx, "brush");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(620.0, 300.0),
        egui::pos2(760.0, 380.0),
    );

    let items = app.active_tab().flow_pane.drawings.items();
    assert_eq!(items.len(), 1, "one gesture makes one object");
    assert_eq!(items[0].tool.id(), "brush");
    assert!(items[0].points.len() >= 2, "a stroke is a path, not a dot");
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        undo_before + 1,
        "the whole stroke is one undo entry, not one per captured point"
    );
    assert_eq!(
        app.toolrail.tool(),
        Tool::Pointer,
        "finishing the stroke restores navigation, like every other tool"
    );
}

/// A press that never moved is a click that missed, not a drawing. An
/// invisible one-point object the trader can neither see nor select is
/// worse than nothing happening.
#[test]
fn a_pencil_click_that_never_moved_leaves_nothing_behind() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "brush");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "a dot is not a stroke"
    );
    assert!(
        app.active_tab().flow_pane.drawings.draft().is_none(),
        "and it leaves no half-finished draft behind either"
    );
}

/// A buy mark that arrives in the stock blue is one the trader repaints
/// every single time.
#[test]
fn the_marks_are_born_in_the_colour_of_the_side_they_mean() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    for (id, expected) in [
        ("arrow-mark-up", theme::BUY),
        ("arrow-mark-down", theme::SELL),
    ] {
        arm_drawing_from_toolbox(&mut app, &ctx, id);
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        let placed = app
            .active_tab()
            .flow_pane
            .drawings
            .items()
            .last()
            .expect("the mark was placed");
        assert_eq!(placed.tool.id(), id);
        assert_eq!(placed.style.color, expected, "{id} was born wrong");
    }
}

/// The stamp and the vector stay two different tools: one click versus
/// two anchors is the whole difference, and folding them together would
/// cost the stamp its reason to exist.
#[test]
fn a_mark_is_one_click_and_the_arrow_tool_is_still_two() {
    assert_eq!(drawing_tool("arrow-mark-up").required_points(), 1);
    assert_eq!(drawing_tool("arrow-mark-down").required_points(), 1);
    assert_eq!(drawing_tool("arrow").required_points(), 2);
}

/// The headline of this change: clicking a drawing must not throw a
/// 320 px panel over the chart. It raises the strip, and nothing else.
#[test]
fn selecting_a_drawing_raises_the_context_bar_not_the_panel() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let texts = painted_text(&run_frame(&mut app, &ctx));

    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    assert!(
        app.surfaces.drawing_chrome.context_bar_rect().is_some(),
        "the selection raises the context bar"
    );
    for absent in ["Horizontal line settings", "Delete drawing", "Style"] {
        assert!(
            !texts.iter().any(|text| text.contains(absent)),
            "the panel must stay shut until it is asked for; found {absent:?}"
        );
    }
    assert!(
        ctx.memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .is_none(),
        "no inspector window exists before the gear is pressed"
    );
}

/// The thing the whole change is for: recolouring a drawing without a
/// panel. Swatch, click, done — and it lands as one undo entry, not one
/// per frame the popover was open.
#[test]
fn the_bar_recolours_a_drawing_in_two_clicks() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    let before = app.active_tab().flow_pane.drawings.items()[0].style.color;
    let undo_before = app.active_tab().flow_pane.drawings.undo_depth();

    let swatch = app
        .surfaces
        .drawing_chrome
        .context_bar()
        .color_rect()
        .expect("the bar renders its colour slot");
    click_chart(&mut app, &ctx, swatch.center());
    run_frame(&mut app, &ctx);

    // The palette's fourth entry is BUY — "this is the buy zone" is the
    // reason a level gets recoloured at all.
    let buy = app
        .surfaces
        .drawing_chrome
        .context_bar()
        .swatch_rect(3)
        .expect("the palette opened under the slot");
    click_chart(&mut app, &ctx, buy.center());
    run_frame(&mut app, &ctx);

    let after = app.active_tab().flow_pane.drawings.items()[0].style.color;
    assert_ne!(after, before, "the swatch has to reach the object");
    assert_eq!(after, theme::BUY);
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        undo_before + 1,
        "one colour change is one undo entry"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(0),
        "styling an object never costs the selection"
    );
}

/// …and the gear is the one door to the panel, which is what makes the
/// `open_inspector` shortcut the rest of these tests use legitimate.
#[test]
fn the_gear_on_the_context_bar_opens_the_inspector() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    let gear = app
        .surfaces
        .drawing_chrome
        .context_bar()
        .gear_rect()
        .expect("the bar rendered its gear");
    click_chart(&mut app, &ctx, gear.center());
    run_frame(&mut app, &ctx);

    assert!(
        app.surfaces.drawing_chrome.inspector_open(),
        "the gear opens the panel"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Horizontal line settings")),
        "…and the panel that opens is the one that always existed; painted: {texts:?}"
    );
}

/// Condition (c) of the bare-glyph contract: the protected object still
/// asks. Without this the Del key on a locked drawing is a silent no-op
/// now that the panel is not there to raise the question.
#[test]
fn a_locked_drawing_asks_before_it_goes_from_the_bar() {
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
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);

    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a locked object is not deleted by the first ask"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Delete locked drawing?")),
        "the bar speaks in words for the one act that is protected; painted: {texts:?}"
    );
}

#[test]
fn rectangle_anchor_resizes_while_the_settings_window_stays_non_modal() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    let first_anchor = egui::pos2(620.0, 300.0);
    let second_anchor = egui::pos2(800.0, 450.0);
    drag_chart(&mut app, &ctx, first_anchor, second_anchor);
    let before = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    let viewport_before = app
        .active_tab()
        .flow_pane
        .viewport
        .right_edge_bar(app.active_tab().flow_pane.slots());

    open_inspector(&mut app, &ctx);
    let inspector = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        inspector
            .iter()
            .any(|text| text.contains("Rectangle settings")),
        "the settings window must be visible before the resize gesture"
    );

    drag_chart(&mut app, &ctx, first_anchor, egui::pos2(560.0, 240.0));
    let after = &app.active_tab().flow_pane.drawings.items()[0].points;

    assert_ne!(after[0], before[0], "the dragged corner must move");
    assert_eq!(
        after[1], before[1],
        "resizing one corner must leave the opposite corner fixed"
    );
    assert_eq!(
        app.active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        viewport_before,
        "resizing a drawing must not pan the chart"
    );
    assert_eq!(app.active_tab().flow_pane.gestures.drag, DrawingDrag::None);
    assert!(
        painted_text(&run_frame(&mut app, &ctx))
            .iter()
            .any(|text| text.contains("Rectangle settings")),
        "the non-modal settings window remains usable after resizing"
    );
}

#[test]
fn locked_drawing_rejects_geometry_and_keyboard_delete() {
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

    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let viewport_before = app
        .active_tab()
        .flow_pane
        .viewport
        .right_edge_bar(app.active_tab().flow_pane.slots());
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(1_000.0, 300.0),
        egui::pos2(1_040.0, 340.0),
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points[0],
        before,
        "locked geometry must not move"
    );
    assert_eq!(
        app.active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        viewport_before,
        "the blocked gesture still belongs to the drawing - the chart must not pan"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "keyboard delete must not remove a locked drawing"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Delete locked drawing?")),
        "the same confirmation appears next to the trigger; painted: {texts:?}"
    );
}

#[test]
fn a_source_reset_keeps_the_marks_and_re_anchors_them_by_market_time() {
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    let ctx = egui::Context::default();
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(1));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    run_frame(&mut app, &ctx);
    // One bar per trade, so the anchor placed on bar 3 is the trade at
    // that instant — and the rewind below refills with half as many bars
    // per trade, moving where that instant lives.
    let trades: Vec<_> = (1..=8).map(trade).collect();
    let anchor_time = trades[3].timestamp_ms;
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.drain_tabs();
    app.active_tab_mut().flow_pane.drawings.place(
        drawing_tool("horizontal-line"),
        ChartPoint::at_time(3.5, 100.0, Some(anchor_time)),
    );

    // A rewind: the source throws the timeline away and refills it.
    evt_tx.try_send(FeedEvent::Reset).unwrap();
    app.drain_tabs();
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a rebuilt timeline never deletes what the trader drew"
    );
    evt_tx
        .try_send(FeedEvent::Backfilled((1..=8).map(trade).collect()))
        .unwrap();
    app.drain_tabs();

    let point = app.active_tab().flow_pane.drawings.items()[0].points[0];
    assert_eq!(
        point.time_ms,
        Some(anchor_time),
        "the instant it was placed at is what survives"
    );
    assert_eq!(
        app.active_tab().flow_pane.slot_at_time(anchor_time),
        Some(slot_of(point.bar)),
        "and the bar it sits on is that instant, re-asked of the new series"
    );
    assert!(
        !app.active_tab().flow_pane.drawings.items()[0].off_series,
        "the refilled series does reach the anchor, so nothing is faded"
    );

    run_frame(&mut app, &ctx);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        !texts.iter().any(|text| text.contains("Drawings cleared")),
        "there is no loss left to announce; painted: {texts:?}"
    );
}

#[test]
fn escape_walks_confirm_draft_selection_then_pointer() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    // Draft first: Esc cancels it and returns to Pointer.
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 1);
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert!(
        app.active_tab().flow_pane.drawings.draft().is_none(),
        "Esc cancels the draft"
    );
    assert_eq!(app.toolrail.tool(), Tool::Pointer);

    // A locked selection with a pending confirmation: Esc peels one
    // layer per press — confirm, then selection, then nothing new.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_selected_locked(true);
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
    assert!(
        app.surfaces.drawing_chrome.delete_confirm(),
        "the confirmation is pending"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert!(
        !app.surfaces.drawing_chrome.delete_confirm(),
        "first Esc cancels the confirm"
    );
    assert!(
        app.active_tab().flow_pane.drawings.selected().is_some(),
        "the selection survives"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        None,
        "second Esc deselects"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "nothing was deleted"
    );
}

#[test]
fn backspace_steps_back_through_the_draft_anchors() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
    click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
    click_chart(&mut app, &ctx, egui::pos2(750.0, 300.0));
    assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 2);

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Backspace)]);
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        1,
        "Backspace removes the last placed anchor"
    );
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "the draft workflow never deletes finished objects"
    );
}

#[test]
fn the_fib_inspector_mounts_its_level_editor_tab() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(600.0, 250.0),
        egui::pos2(900.0, 400.0),
    );
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

    open_inspector(&mut app, &ctx);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.contains("Levels")),
        "the tool-owned tab is offered by name; painted: {texts:?}"
    );

    app.surfaces
        .drawing_chrome
        .set_inspector_tab(InspectorTab::Extra);
    run_frame(&mut app, &ctx);
    // The level editor is taller than the window. Everything in it must
    // still be *reachable* — which is what the panel's scroll is for, and
    // what a silent cut at the window edge used to deny.
    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    let over = inspector.center();
    let mut texts = painted_text(&run_frame(&mut app, &ctx));
    for _ in 0..12 {
        if texts.iter().any(|text| text.contains("log scale")) {
            break;
        }
        texts = painted_text(&run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ));
    }
    for label in ["band opacity", "log scale"] {
        assert!(
            texts.iter().any(|text| text.contains(label)),
            "the level editor must show {label:?}; painted: {texts:?}"
        );
    }
}

#[test]
fn a_placed_fib_paints_its_levels_and_labels() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(600.0, 250.0),
        egui::pos2(900.0, 400.0),
    );

    let texts = painted_text(&run_frame(&mut app, &ctx));
    for label in ["61.8%", "38.2%", "50.0%"] {
        assert!(
            texts.iter().any(|text| text.contains(label)),
            "the standard retracement labels paint on the chart; painted: {texts:?}"
        );
    }
}

#[test]
fn closed_restart_channel_rolls_back_grouping_without_losing_history() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
    let original = app
        .active_tab_mut()
        .tape_mut()
        .base_capture_grouping_for_test();

    assert!(
        app.active_tab_mut()
            .tape_mut()
            .stage_capture_grouping_for_test(Decimal::new(5, 2))
    );
    drop(cmd_rx);
    app.active_tab_mut().restart_book_capture();

    assert_eq!(
        app.active_tab_mut()
            .tape_mut()
            .base_capture_grouping_for_test(),
        original
    );
    assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
}

#[test]
fn full_restart_channel_rolls_back_grouping_without_losing_history() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
    let original = app
        .active_tab_mut()
        .tape_mut()
        .base_capture_grouping_for_test();
    let (full_tx, mut full_rx) = mpsc::channel(1);
    app.active_tab_mut().commands = full_tx;
    app.active_tab()
        .commands
        .try_send(FeedCommand::LoadOlder { count: 1 })
        .unwrap();

    assert!(
        app.active_tab_mut()
            .tape_mut()
            .stage_capture_grouping_for_test(Decimal::new(5, 2))
    );
    app.active_tab_mut().restart_book_capture();

    assert!(matches!(
        full_rx.try_recv(),
        Ok(FeedCommand::LoadOlder { count: 1 })
    ));
    assert_eq!(
        app.active_tab_mut()
            .tape_mut()
            .base_capture_grouping_for_test(),
        original
    );
    assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
}

/// Apply keeps the dialog open (audit M2): tuning is a nudge-and-look
/// loop, and each Apply must land without re-opening anything. The nudge
/// really lands — the EMA's length changes and the view retitles.
#[test]
fn apply_keeps_the_settings_dialog_open_and_lands_the_draft() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    settle_indicators(&mut app);
    let slot = app.active_tab().flow_pane.indicators.all()[0].slot;
    app.apply_toolbar_action(ToolbarAction::OpenIndicatorSettings(slot.0));
    assert!(
        app.indicators.indicator_settings.is_some(),
        "the dialog opened"
    );

    if let Some(dialog) = app.indicators.indicator_settings.as_mut()
        && let Some(quantick_indicators::InputValue::Int(len)) = dialog.draft.first_mut()
    {
        *len = 21;
    }
    app.apply_indicator_settings_draft();
    assert!(
        app.indicators.indicator_settings.is_some(),
        "Apply keeps the dialog open for the next nudge"
    );
    settle_indicators(&mut app);
    let label = app.active_tab().flow_pane.indicators.all()[0]
        .label()
        .to_owned();
    assert!(
        label.contains("21"),
        "the applied draft rebuilt the indicator: {label}"
    );
}

/// A twin pane is never left behind for holding a selection.
///
/// Two tabs on one market show one set of drawings on the same pane
/// address. A twin the sync skips keeps a copy that is one object short,
/// and its own next edit writes that copy back over the key — taking the
/// level drawn on the other chart with it, off the screen and out of the
/// file, with nothing said. So the twin is refreshed like any other and
/// its selection is carried across by id.
#[test]
fn a_selection_on_a_twin_does_not_cost_the_other_chart_its_drawing() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let home = app.active_tab().id;
    place_level(&mut app, PaneSide::Flow, 100.0);
    run_frame(&mut app, &ctx);

    // A second tab on the same market: one drawing key, two panes.
    app.open_tab("binance".to_owned(), "TESTUSDT".to_owned(), None);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let twin = app.active_tab().id;
    assert_ne!(twin, home, "a second tab opened");
    assert_eq!(
        drawings_on(&app, PaneSide::Flow),
        vec![100.0],
        "the market's level came with the market"
    );

    // The trader selects it here.
    app.active_tab_mut().flow_pane.drawings.select(Some(0));
    let held = app.active_tab().flow_pane.drawings.items()[0].id;

    // And draws a second level on the other tab.
    app.cycle_tab(-1);
    assert_eq!(app.active_tab().id, home);
    place_level(&mut app, PaneSide::Flow, 200.0);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    // The twin took the new level and kept pointing at the same object.
    app.cycle_tab(1);
    assert_eq!(app.active_tab().id, twin);
    assert_eq!(
        drawings_on(&app, PaneSide::Flow),
        vec![100.0, 200.0],
        "the twin was refreshed rather than left one object short"
    );
    let selected = app
        .active_tab()
        .flow_pane
        .drawings
        .selected()
        .expect("the twin kept its selection across the refresh");
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[selected].id,
        held,
        "and it points at the object the trader picked, not at whatever              now sits at that index"
    );

    // The twin's own next edit must not write a stale set over the key.
    // It moves the object it holds — the second tab was opened on a
    // market whose bars it has not been served, so a new mark has nothing
    // to anchor on, and a move is what a trader does to a selection
    // anyway.
    app.active_tab_mut().flow_pane.drawings.begin_gesture();
    app.active_tab_mut()
        .flow_pane
        .drawings
        .translate_selected(0.0, 5.0);
    app.active_tab_mut().flow_pane.drawings.commit_gesture();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        drawings_on(&app, PaneSide::Flow).len(),
        2,
        "the twin still holds both levels after an edit of its own"
    );
    app.cycle_tab(-1);
    assert_eq!(app.active_tab().id, home);
    let here = drawings_on(&app, PaneSide::Flow);
    assert!(
        here.contains(&200.0),
        "the level drawn on this chart was not written away by the twin: {here:?}"
    );
    assert_eq!(
        here.len(),
        2,
        "and both levels of the market are still on it: {here:?}"
    );
}

/// Switching one pane's layout swaps that pane's drawings only.
#[test]
fn switching_one_panes_layout_swaps_only_its_drawings() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Time(0), 100.0);
    place_level(&mut app, PaneSide::Flow, 50.0);
    run_frame(&mut app, &ctx);
    let first = app.layouts().active_id();
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let second = app.create_layout(None).expect("second");
    assert!(
        drawings_on(&app, PaneSide::Time(0)).is_empty(),
        "the time pane's level went with layout 1"
    );
    assert_eq!(
        drawings_on(&app, PaneSide::Flow),
        vec![50.0],
        "the flow pane, still on layout 1, kept its own"
    );
    app.switch_layout(first).expect("back");
    assert_eq!(drawings_on(&app, PaneSide::Time(0)), vec![100.0]);
    assert_eq!(app.layouts().get(second).unwrap().drawing_count(), 0);
}

/// A drawing belongs to the layout, the market and the pane it was drawn
/// on: it is put away when the layout changes and comes back when the
/// layout does; it never appears on the pane beside it.
#[test]
fn drawings_are_kept_per_layout_and_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Time(0), 100.0);
    assert_eq!(drawings_on(&app, PaneSide::Time(0)), vec![100.0]);
    assert!(
        drawings_on(&app, PaneSide::Flow).is_empty(),
        "not on the flow pane"
    );
    run_frame(&mut app, &ctx);

    let first = app.layouts().active_id();
    let second = app.create_layout(None).expect("a second layout");
    assert!(
        drawings_on(&app, PaneSide::Time(0)).is_empty(),
        "layout 2 has no level on this market yet"
    );
    place_level(&mut app, PaneSide::Time(0), 200.0);
    run_frame(&mut app, &ctx);

    app.switch_layout(first).expect("back");
    assert_eq!(
        drawings_on(&app, PaneSide::Time(0)),
        vec![100.0],
        "layout 1's level is back, and layout 2's is not on it"
    );
    app.switch_layout(second).expect("forth");
    assert_eq!(drawings_on(&app, PaneSide::Time(0)), vec![200.0]);
    let key = crate::layouts::DrawingKey {
        feed: "binance".to_owned(),
        symbol: "TESTUSDT".to_owned(),
        pane: 1,
    };
    assert_eq!(
        app.layouts()
            .get(first)
            .expect("kept")
            .drawings(&key)
            .map(<[_]>::len),
        Some(1),
        "the book holds layout 1's level under its market and pane"
    );
}

/// A drawing belongs to the market it was drawn on: when the tab moves to
/// another symbol the level is put away, and it is back — on its bar —
/// when the tab returns. The other market starts clean.
#[test]
fn drawings_follow_the_market_the_tab_shows() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Flow, 100.0);
    run_frame(&mut app, &ctx);

    app.active_tab_mut().symbol = "ETHUSDT".to_owned();
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().active.1,
        "ETHUSDT",
        "the tab switched market"
    );
    assert!(
        drawings_on(&app, PaneSide::Flow).is_empty(),
        "a level on TESTUSDT is not a level on ETHUSDT"
    );
    run_frame(&mut app, &ctx);

    app.active_tab_mut().symbol = "TESTUSDT".to_owned();
    run_frame(&mut app, &ctx);
    assert_eq!(
        drawings_on(&app, PaneSide::Flow),
        vec![100.0],
        "back on TESTUSDT, its own level is back"
    );
    assert!(
        !app.active_tab().flow_pane.drawings.items()[0].foreign_market,
        "and it is at home, not marked as another market's"
    );
}

/// "Show on all charts" is stored once, under the pane it was drawn on,
/// and comes back shared — so the other pane mirrors it again after a
/// layout round trip.
#[test]
fn a_shared_drawing_comes_back_shared() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Time(0), 100.0);
    {
        let pane = app.active_tab_mut().pane_mut(PaneSide::Time(0));
        pane.drawings.select(Some(0));
        pane.drawings.selected_mut().expect("selected").scope =
            crate::drawings::DrawingScope::AllCharts;
    }
    run_frame(&mut app, &ctx);
    let first = app.layouts().active_id();
    let second = app.create_layout(None).expect("second");
    app.switch_layout(second).ok();
    app.switch_layout(first).expect("back");
    let items = app.active_tab().pane(PaneSide::Time(0)).drawings.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].scope, crate::drawings::DrawingScope::AllCharts);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .shared_count(),
        1,
        "the flow pane mirrors it again"
    );
}

/// A drawing keeps its id across a put-away and a bring-out, so whatever
/// named it — a strategy armed on it, an agent's annotation — still does.
#[test]
fn a_drawing_keeps_its_id_across_a_layout_round_trip() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Time(0), 100.0);
    let id = app.active_tab().pane(PaneSide::Time(0)).drawings.items()[0].id;
    run_frame(&mut app, &ctx);
    let first = app.layouts().active_id();
    let second = app.create_layout(None).expect("second");
    assert_eq!(app.layouts().active_id(), second);
    app.switch_layout(first).expect("back");
    let items = app.active_tab().pane(PaneSide::Time(0)).drawings.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, id, "the level came back under its own id");
}

/// Moving a context chart moves its drawings' key and its slot
/// bookkeeping with it, so the charts do not swap sets on the next
/// switch and the layout keeps addressing the right pane.
#[test]
fn moving_a_context_chart_moves_its_drawings_and_slots_with_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.apply_toolbar_action(ToolbarAction::AddNative("native.ema"));
    settle_indicators(&mut app);
    place_level(&mut app, PaneSide::Time(0), 100.0);
    run_frame(&mut app, &ctx);
    let top_id = app.active_tab().pane(PaneSide::Time(0)).id;

    let tab_id = app.active_tab().id;
    assert!(
        app.move_context_pane_at(tab_id, 1, 2),
        "the top chart moved down"
    );
    assert_eq!(app.active_tab().pane(PaneSide::Time(1)).id, top_id);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(1))
            .drawings_key
            .as_ref()
            .map(|key| key.pane),
        Some(2),
        "its drawing key names its new address"
    );
    assert_eq!(drawings_on(&app, PaneSide::Time(1)), vec![100.0]);
    assert!(
        app.indicators
            .slot_kinds
            .iter()
            .filter(|(owner, _)| owner.tab == tab_id && owner.side == PaneSide::Time(1))
            .count()
            == 1,
        "and its indicator registration followed it"
    );

    // A switch away and back finds the level on the same chart.
    let first = app.layouts().active_id();
    let second = app.create_layout(None).expect("second");
    assert_eq!(app.layouts().active_id(), second);
    app.switch_layout(first).expect("back");
    assert_eq!(drawings_on(&app, PaneSide::Time(1)), vec![100.0]);
    assert!(drawings_on(&app, PaneSide::Time(0)).is_empty());
}

/// Drawings are per pane, and the tool rail is one: an object lands on the
/// pane under the cursor and stays out of the other's overlay.
#[test]
fn a_drawing_lands_on_the_pane_under_the_cursor() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);

    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1,
        "the click landed on the time pane"
    );
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "and nowhere else"
    );

    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1,
        "placing on one pane must not add to the other"
    );
}

/// The other half of the same root cause: an edit committed against
/// whatever had focus when the gesture settled, not against the pane it
/// was captured on. Focus moves legitimately — clicking the other chart —
/// so the baseline has to carry its own owner.
#[test]
fn an_inspector_edit_commits_on_the_pane_it_started_on() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    // A mark on each pane, so an index means something on both.
    for side in [PaneSide::Time(0), PaneSide::Flow] {
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, side);
        click_chart(&mut app, &ctx, point);
    }
    let time_depth = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .drawings
        .undo_depth();
    let flow_depth = app.active_tab().flow_pane.drawings.undo_depth();

    // An edit begun on the time pane: the baseline, then a real change to
    // the object (the store records an entry only if something moved).
    let before = app.active_tab().pane(PaneSide::Time(0)).drawings.items()[0].clone();
    let tab_id = app.active_tab().id;
    app.surfaces
        .drawing_chrome
        .open_edit_gesture(tab_id, PaneSide::Time(0), 0, before);
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .drawings
        .select(Some(0));
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .drawings
        .selected_mut()
        .expect("the time pane's mark")
        .style
        .width_px = MAX_DRAWING_WIDTH_PX;
    // ...that settles after focus has moved to the chart beside it.
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
    // No hand on the commit: the chrome notices that pointer and keyboard
    // have let go, hands the baseline back through its response, and the
    // host records it against the pane the edit named.
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .undo_depth(),
        time_depth + 1,
        "the entry lands on the pane the edit started on"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        flow_depth,
        "and never on the one that happened to take focus"
    );
}

/// A note is a passing remark, not a card to dismiss: it leaves on its
/// own, and a fresh press clears whatever the last one left behind.
#[test]
fn a_history_note_leaves_on_its_own_and_a_new_press_clears_it() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    drain_load_older(&mut commands);
    events
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    app.drain_tabs();
    assert!(app.active_tab().history_note().is_some());

    let past_its_welcome = std::time::Instant::now() + crate::tab::HISTORY_NOTE_LINGER;
    app.active_tab_mut().expire_history_note(past_its_welcome);
    assert_eq!(
        app.active_tab().history_note(),
        None,
        "it fades rather than waiting to be dismissed"
    );

    // A second press must not leave the previous sentence hanging over a
    // request whose outcome is not known yet.
    events
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    app.drain_tabs();
    assert!(app.active_tab().history_note().is_some(), "the new one");
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    assert_eq!(
        app.active_tab().history_note(),
        None,
        "a press in flight has no outcome yet, so it reports none"
    );
}

/// Putting the reach back to one page is the trader's way out of a run.
#[test]
fn withdrawing_the_reach_calls_off_a_run_in_flight() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.history.history_reach = history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    assert_eq!(drain_load_older(&mut commands).len(), 1);
    assert!(app.active_tab().history_reach_running());

    // The trader changes their mind and picks "one page" again.
    app.history.history_reach = history_reach::HistoryReach::Page;
    app.drain_tabs();
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-60..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert!(
        drain_load_older(&mut commands).is_empty(),
        "the page in flight is the last one"
    );
    assert!(!app.active_tab().history_reach_running());
    assert!(!app.active_tab().loading.is_active(LoadingTask::History));
}

/// (d) Two dividers, two boundaries: the venue seam and the backfill mark
/// sit at their own slots and neither moves the other.
#[test]
fn the_seam_and_the_backfill_divider_mark_different_slots() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: quantick_feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: quantick_feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    // Live prints after the backfill, so the backfill boundary is real.
    for minute in 200..205 {
        let trade = minute_trade(minute);
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms);
    }
    run_frame(&mut app, &ctx);

    let pane = app.active_tab().pane(PaneSide::Time(0));
    let seam = pane.seam_slot();
    let backfill = pane
        .state
        .backfill_boundary()
        .expect("the pane took a backfill batch");
    assert_eq!(seam, 120);
    assert!(
        backfill + seam > seam,
        "the backfill mark sits inside the trade-derived half, past the seam"
    );
    // Both marks paint. The backfill divider is opt-in (see
    // `ChartPane::new`) and this test is about where it lands, so switch
    // it on first — on both panes, since either one's mark answers the
    // assertion below.
    for side in [PaneSide::Time(0), PaneSide::Flow] {
        app.active_tab_mut().pane_mut(side).set_layer_visible(
            ChartLayer::BackfillDivider,
            true,
            &mut chart_layers::LayerActions::default(),
        );
    }
    // The view follows the live edge, and the venue history is far behind
    // it, so bring the seam on screen the way a user scrolling back
    // would.
    let slots = app.active_tab().pane(PaneSide::Time(0)).slots();
    let width = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .frame
        .chart_area
        .expect("the time pane was laid out")
        .width();
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .viewport
        .center_on_bar(seam as f32, width, slots);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text == "venue"),
        "the seam names itself: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "backfill"),
        "and the backfill divider still draws: {texts:?}"
    );
}

/// §11's amber dot: a background tab says something is wrong with its
/// feed without the user having to open it.
///
/// It marks trouble, not activity — a tab still connecting has nothing to
/// report yet, and a recording has no transport to lose.
#[test]
fn the_attention_dot_marks_lost_connections_and_nothing_else() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let tab = app.active_tab_mut();

    assert_eq!(tab.feed_connection, FeedConnectionState::Connecting);
    assert!(
        !tab.needs_attention(),
        "still connecting is not yet trouble"
    );

    tab.feed_connection = FeedConnectionState::Connected;
    assert!(!tab.needs_attention(), "nor is a healthy feed");

    tab.feed_connection = FeedConnectionState::Reconnecting;
    assert!(
        tab.needs_attention(),
        "a feed that had a connection and lost it is"
    );

    tab.feed_connection = FeedConnectionState::Connected;
    tab.notice = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
    assert!(
        tab.needs_attention(),
        "so is one asking the user to fix something"
    );

    // A recording has no transport to lose, whatever it is holding.
    let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("WINJ26_2026-03-16.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("fixture session parses");
    with_config(&mut app, |tab, config| {
        tab.open_replay(
            config,
            quantick_feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: quantick_feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    let tab = app.active_tab_mut();
    tab.feed_connection = FeedConnectionState::Reconnecting;
    assert!(
        !tab.needs_attention(),
        "a replaying tab has no transport to report on"
    );
    run_frame(&mut app, &ctx);
}

/// Criterion 1: one handler, two operators. The trader's own placement
/// and an authorized agent's remote call put the same kind of object on
/// the same pane through `Drawings::place_with`; only the attribution
/// differs. Two paths to one door is exactly what this tier must not be.
#[test]
fn the_same_handler_places_the_traders_object_and_the_agents() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    // The trader's own: the note hook takes the click path's own door.
    app.surfaces.drawing_chrome.set_pending_text_note(true);
    run_frame(&mut app, &ctx);
    let after_trader = app.active_tab().drawing_pane().drawings.items().len();
    assert_eq!(after_trader, 1, "the trader placed one object");
    assert!(
        app.active_tab().drawing_pane().drawings.items()[0]
            .author
            .is_none(),
        "the trader's own object carries no author"
    );

    let directory = gateway_test_directory("annotate-same-handler");
    grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &annotator_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let anchor = newest_anchor(&app);
    let response = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "annotate.label.create",
        serde_json::json!({ "anchors": [anchor], "text": "supply here" }),
    );
    let result = success_result(&response);
    assert_eq!(result["author"]["actor_kind"], "agent");
    assert_eq!(result["tool_id"], "text");

    let items = app.active_tab().drawing_pane().drawings.items();
    assert_eq!(items.len(), 2, "both operators placed through one door");
    let agent_object = items
        .iter()
        .find(|drawing| drawing.author.is_some())
        .expect("the agent's object is attributed");
    assert_eq!(
        agent_object.tool.id(),
        items[0].tool.id(),
        "the same tool, the same placement path"
    );
    assert_eq!(
        agent_object.author.as_ref().unwrap().client_name,
        "quantick integration test"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn a_mark_during_replay_is_traced_and_replayed_at_the_same_logical_time() {
    let ctx = egui::Context::default();
    let dir = crate::scratch::ScratchDir::new("control-trace-replay");
    let session = recording_at(&dir);
    let trace_path = dir.join("20260316.csv.control-trace.jsonl");

    // First run: a human takes a mark while the session is linked.
    let (mut app, _commands) = app_with_history(12);
    app.active_tab_mut().replay = Some(replay_test_support::detached_link(session));
    hover_bar(&mut app, &ctx, 6);
    app.take_mark(Some("traced".to_owned()));
    assert!(
        trace_path.exists(),
        "the mark was recorded beside the recording"
    );
    let first_events = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 16, 1 << 20)
        .events;
    let original = first_events
        .iter()
        .find(|event| event.kind.as_str() == "attention.mark.created")
        .expect("the human mark is journaled");
    assert_eq!(original.payload["note"], "traced");
    assert_eq!(
        original.actor.as_ref().unwrap().kind,
        quantick_control::wire::ActorKind::HumanUi
    );
    drop(app);

    // Second run: the same recording with its sidecar. The frame loop
    // re-injects the mark at its logical time with no human present, and
    // the replayed mark names the same bar.
    let (mut app, _commands) = app_with_history(12);
    app.active_tab_mut().replay = Some(replay_test_support::detached_link(recording_at(&dir)));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let replayed_events = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 16, 1 << 20)
        .events;
    let replayed = replayed_events
        .iter()
        .find(|event| event.kind.as_str() == "attention.mark.created")
        .expect("the traced mark was re-injected");
    assert_eq!(replayed.payload["note"], "traced");
    assert_eq!(replayed.payload["target_source"], "replayed");
    assert_eq!(
        replayed.payload["target"]["pointer"]["bar"]["slot"],
        original.payload["target"]["pointer"]["bar"]["slot"],
        "the replayed mark points at the same bar"
    );
    assert_eq!(
        replayed.actor.as_ref().unwrap().kind,
        quantick_control::wire::ActorKind::Automation,
        "a replayed mark is attributed to automation, not to a human"
    );
    // Re-injection did not grow the trace: the sidecar still holds one
    // intent and one result.
    let lines = std::fs::read_to_string(&trace_path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(lines, 2, "a replayed action is not recorded again");

    // Switching away and back neither repeats nor skips the injection.
    let replayed_marks = |app: &QuantickApp| {
        app.control
            .control_access
            .as_ref()
            .unwrap()
            .journal()
            .read(1, 64, 1 << 20)
            .events
            .iter()
            .filter(|event| event.kind.as_str() == "attention.mark.created")
            .count()
    };
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    run_frame(&mut app, &ctx);
    app.active_tab = 0;
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(replayed_marks(&app), 1, "a tab switch does not re-inject");

    // The playhead moves on, then restarts: the rerun injects the mark
    // again, at its logical time, and the sidecar still does not grow.
    let status = std::sync::Arc::clone(&app.tabs[0].replay.as_ref().unwrap().status);
    replay_test_support::set_position_ms(&status, status.start_ms() + 5_000);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        replayed_marks(&app),
        1,
        "moving forward injects nothing new"
    );
    replay_test_support::set_position_ms(&status, status.start_ms());
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(replayed_marks(&app), 2, "a restart replays the mark");
    let lines = std::fs::read_to_string(&trace_path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(lines, 2, "re-injection never writes the sidecar");

    // A mark the human takes during this run joins the walk: it is not
    // injected back on the spot, and the next rerun replays it beside
    // the recorded one — exactly what a fresh process would do.
    replay_test_support::set_position_ms(&status, status.start_ms() + 7_000);
    run_frame(&mut app, &ctx);
    hover_bar(&mut app, &ctx, 9);
    app.take_mark(Some("this run".to_owned()));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        replayed_marks(&app),
        3,
        "the human's own mark is journaled once and not replayed back at once"
    );
    // A restart whose rerun has already advanced past the last sampled
    // position is still a rewind — the worker counts it and says where
    // it began — and the rerun replays both marks at their times.
    replay_test_support::note_rewind(&status, status.start_ms());
    replay_test_support::set_position_ms(&status, status.start_ms() + 7_500);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        replayed_marks(&app),
        5,
        "the rerun replays the recorded mark and this run's mark"
    );
    let lines = std::fs::read_to_string(&trace_path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(
        lines, 4,
        "this run's mark was recorded once; re-injection never writes the sidecar"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_replayed_mark_without_its_target_is_refused_and_an_unknown_version_is_named() {
    use quantick_control::error::codes;

    let (mut app, _commands) = app_with_history(4);
    let refused = app
        .control_action(
            crate::control::MARK_CAPABILITY_ID,
            crate::control::MARK_CAPABILITY_VERSION,
            crate::control::ActionOrigin::TraceReplay(Box::new(crate::control::RecordedActor {
                actor_kind: quantick_control::wire::ActorKind::HumanUi,
                client_name: "quantick-ui".to_owned(),
            })),
            serde_json::json!({ "note": "no recorded target" }),
        )
        .unwrap_err();
    assert_eq!(refused.code.as_str(), codes::INVALID_REQUEST);
    let unknown = app
        .control_action(
            crate::control::MARK_CAPABILITY_ID,
            crate::control::MARK_CAPABILITY_VERSION + 1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({}),
        )
        .unwrap_err();
    assert_eq!(unknown.code.as_str(), codes::CAPABILITY_UNKNOWN);
    assert!(
        app.control
            .control_access
            .as_ref()
            .unwrap()
            .journal()
            .read(1, 16, 1 << 20)
            .events
            .is_empty(),
        "a refused mark leaves no event"
    );
    // The recorded author is set for the handler and cleared after it. A
    // refusal never reaches the handler, so it must leave nothing behind:
    // a latched `HumanUi` author would sign the *next* action's object as
    // the trader's own, which is the one claim the annotate tier cannot
    // get wrong.
    assert!(
        app.control
            .control_access
            .as_ref()
            .unwrap()
            .recorded_author()
            .is_none(),
        "a refused replay leaves no author to sign the next action"
    );
}
