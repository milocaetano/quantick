use super::*;
use crate::app::*;

/// A drawing tool takes the *primary button*, never the chart.
///
/// Arming one used to return early from the whole navigation pass, so the
/// trader could not zoom, pan or resize anything while annotating (audit
/// S2) — and carrying that shape into the panes would have multiplied it
/// by every pane on screen.
#[test]
fn an_armed_tool_leaves_the_chart_navigable() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));

    // The wheel over the pane's own axis still zooms that axis.
    let (lo, hi) = pane_range(&app, flow);
    let over = pane_gutter(&app, 0).center();
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(over),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let (zoomed_lo, zoomed_hi) = pane_range(&app, flow);
    assert!(
        zoomed_hi - zoomed_lo < hi - lo,
        "an armed tool must not deafen the pane's axis: {lo}..{hi} -> \
             {zoomed_lo}..{zoomed_hi}"
    );

    // And the wheel over the candles still zooms time.
    let width = app.active_tab().flow_pane.viewport.candle_width();
    let over_candles = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("a frame has been drawn")
        .center();
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(over_candles),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(
        app.active_tab().flow_pane.viewport.candle_width() > width,
        "the candles still zoom while a tool is armed"
    );
}

/// A single press-drag-release with a multi-anchor tool armed drops the
/// first anchor at the press and the second at the release — the drag
/// placement every two-point tool advertises ("click two points or
/// drag"). One gesture, one finished object.
#[test]
fn an_armed_two_point_tool_completes_on_a_single_drag() {
    for tool_id in ["trend-line", "fixed-range-profile", "measure"] {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail.arm(Tool::Drawing(drawing_tool(tool_id)));
        run_frame(&mut app, &ctx);

        let chart = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("a frame has been drawn");
        let start = chart.center() - egui::vec2(120.0, 40.0);
        let end = chart.center() + egui::vec2(120.0, 40.0);
        drag_sized(&mut app, &ctx, TEST_WINDOW, start, end);

        let drawings = app.active_tab().flow_pane.drawings.items();
        assert_eq!(
            drawings.len(),
            1,
            "{tool_id}: one drag places one finished object"
        );
        assert_eq!(drawings[0].points.len(), 2, "{tool_id}: both anchors down");
        let bars: Vec<f32> = drawings[0].points.iter().map(|point| point.bar).collect();
        assert!(
            (bars[0] - bars[1]).abs() >= 1.0,
            "{tool_id}: the anchors span the dragged distance, got {bars:?}"
        );
    }
}

/// The chevron that collapses a pane still beats an armed tool. egui hands
/// an overlap to the last registrant and the chevron registers last — but
/// the drawing path reads the raw pointer, so it has to honour that order
/// itself or arming a tool silently kills the control.
#[test]
fn the_pane_chevron_still_wins_its_pixels_with_a_tool_armed() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));

    let body = pane_body(&app, 0);
    let chevron = crate::indicator_render::pane_disclosure_rect(body, false).center();
    click_chart(&mut app, &ctx, chevron);

    let sizing = app
        .active_tab()
        .flow_pane
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == flow)
        .expect("the pane is still there")
        .sizing;
    assert_eq!(sizing, crate::indicators::PaneSizing::Collapsed);
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "the chevron is chrome, not canvas"
    );
}

/// A print the market made while nobody was listening is history, not a
/// live arrival. Run through the live path it would fill a resting order
/// at a price the trader could never have been filled at, and report the
/// length of the outage as this feed's delay.
#[test]
fn a_recovered_print_seeds_the_mark_and_fills_nothing() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    let floor = app.active_tab().latest_trade_ms.expect("a print landed");
    let live_before = app.active_tab().live_trades;
    let lag_before = app.active_tab().latest_trade_latency_ms;

    app.active_tab_mut().resume_floor_ms = Some(floor);
    let recovered = quantick_engine::Trade {
        agg_id: 2,
        timestamp_ms: floor + 4 * 60_000,
        ..trade(2)
    };
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1), recovered.clone()]))
        .unwrap();
    app.active_tab_mut().drain_feed();

    let tab = app.active_tab();
    assert_eq!(
        tab.live_trades, live_before,
        "a recovered print is history; it is not a live arrival"
    );
    assert_eq!(
        tab.latest_trade_latency_ms, lag_before,
        "the age of the outage is not this feed's delay"
    );
    assert_eq!(
        tab.latest_trade_ms,
        Some(recovered.timestamp_ms),
        "the chart still moved forward to it"
    );
}

/// Paper trading through the app's own event path: backfill only seeds
/// (never fills), the toolbar buy queues, the next live print fills, and
/// the status-bar cell reports the simulated position.
#[test]
fn a_simulated_buy_fills_from_the_next_live_print_only() {
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert!(app.active_tab().paper.ready(), "backfill seeds the mark");
    assert!(
        app.active_tab().paper.status_cell().is_none(),
        "an untouched simulator owes no status line"
    );

    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    assert!(
        app.active_tab().paper.status_cell().is_some(),
        "a queued market order is visible state"
    );
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    let (text, _) = app
        .active_tab()
        .paper
        .status_cell()
        .expect("the fill opened a position");
    assert!(
        text.starts_with("SIM"),
        "the cell is labeled simulated: {text}"
    );
    assert!(
        app.status_model().sim_pnl.is_some(),
        "the status bar model carries the cell"
    );
}

/// The toolbar's exit control end to end: with a position open the model
/// grows the ✕ button, the close action queues, the next print fills it,
/// and the status cell switches from naming the position to `flat`.
#[test]
fn the_toolbar_close_action_exits_the_open_position() {
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    // The close journals; without this the test writes a real
    // `paper-trades/` folder into the crate's source tree.
    let dir = std::env::temp_dir().join(format!("quantick-paper-app-close-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.active_tab_mut().paper.redirect_history_dir(dir.clone());
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    let (text, _) = app.active_tab().paper.status_cell().expect("open");
    assert!(text.contains("LONG"), "the cell names the side: {text}");
    assert!(
        app.active_tab().paper.close_button_label().is_some(),
        "an open position grows the toolbar exit"
    );

    app.apply_toolbar_action(ToolbarAction::PaperClose);
    evt_tx.try_send(FeedEvent::Live(trade(6))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert!(
        app.active_tab().paper.position_summary().is_none(),
        "the close filled at the next print"
    );
    let (text, _) = app.active_tab().paper.status_cell().expect("history");
    assert!(text.contains("flat"), "the cell says flat: {text}");
    assert!(
        app.active_tab().paper.close_button_label().is_none(),
        "flat removes the toolbar exit"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A source reset (replay seek, feed switch) flattens the simulated
/// position at the last mark and journals the round trip — the same
/// honesty contract the drawings' clear follows.
#[test]
fn a_source_reset_flattens_the_simulated_position_and_journals_it() {
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    let dir = std::env::temp_dir().join(format!("quantick-paper-app-reset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.active_tab_mut().paper.redirect_history_dir(dir.clone());
    // No `set_symbol` here: the tab's own drain syncs the journal to its
    // symbol before reading a single event, which is what makes the
    // folder assertion below a proof of that wiring too.
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);

    evt_tx.try_send(FeedEvent::Reset).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert!(
        app.active_tab().paper.status_cell().is_some(),
        "the realized history keeps the cell alive"
    );
    let files: Vec<_> = std::fs::read_dir(dir.join("TESTUSDT"))
        .expect("the flatten was journaled under the symbol's folder")
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "one session, one history file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_ui_drain_uses_one_observation_for_single_and_batched_trades() {
    use std::cell::Cell;

    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    let last_trade = trade(3);
    let received_at_ms = last_trade.timestamp_ms + 75;
    evt_tx.try_send(FeedEvent::Live(trade(1))).unwrap();
    evt_tx
        .try_send(FeedEvent::LiveBatch(vec![trade(2), last_trade]))
        .unwrap();
    let clock_calls = Cell::new(0_u32);

    app.active_tab_mut().drain_feed_with_clock(|| {
        clock_calls.set(clock_calls.get() + 1);
        received_at_ms
    });

    assert_eq!(clock_calls.get(), 1, "one wall-clock read per UI drain");
    // Per market. The window's own summary counter adds these up across
    // every tab, in `drain_tabs`.
    assert_eq!(app.active_tab().live_trades, 3);
    assert_eq!(app.active_tab().trade_arrival_ms(), Some(75));
    assert_eq!(app.active_tab().flow_pane.state.timeline_revision(), 3);
    assert_eq!(
        app.active_tab()
            .flow_pane
            .state
            .partial()
            .map(|bar| bar.trade_count),
        Some(3)
    );
}

/// A band nothing can fill never takes width from the candles.
///
/// The strip draws resting depth and the aggressions landing into it. A
/// source with neither fills none of it, and the shipped default is what
/// made that reachable — the layer opened off until now, so the missing
/// capability gate never showed. Permanently narrowing the candles for a
/// blank rect is the one way this branch could make a chart worse.
#[test]
fn a_source_that_fills_neither_half_gets_no_live_strip() {
    let (app, _commands) = app_without_depth();
    let pane = &app.active_tab().flow_pane;
    assert!(
        pane.live_strip_visible,
        "the shipped config switched it on, which is what makes the gate matter"
    );
    assert_eq!(
        pane.live_strip_width(crate::config::FeedCapabilities::none()),
        0.0,
        "no book and no traded volume: the band would draw nothing"
    );
    assert!(
        pane.layer_blocked(
            ChartLayer::LiveStrip,
            crate::config::FeedCapabilities::none()
        )
        .is_some(),
        "and the menu says why instead of offering a switch that does nothing"
    );
}

/// The whole semi-automatic loop in one place: a rectangle drawn on the
/// chart, a force-bar strategy armed on it through the same call the
/// dialog makes, and the tape walking the operation from trigger to
/// take profit. The human drew the fence; the machine pulled the
/// trigger; the simulator answered with fills the tape proves.
#[test]
fn an_armed_rectangle_fires_on_the_force_bar_inside_it() {
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
    /// One Tick(50) bar: 49 prints at `open`, the fiftieth at `close`.
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
            .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;

    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.window = 3;
    // The fixture's bodies are 4 points; the elephant floor is off so
    // the test exercises the band, not the floor (which has its own).
    form.min_range = "0".to_owned();
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "test BF".to_owned())
        .expect("the form compiles and the drawing exists");

    let mut id = 0u64;
    // Three body-1 warmup bars inside the region: quiet, nothing fires.
    bar(&mut app, &mut id, "100", "101");
    bar(&mut app, &mut id, "101", "102");
    bar(&mut app, &mut id, "102", "103");
    assert!(
        app.active_tab().paper.is_flat(),
        "warmup bars must not fire"
    );

    // The force bar: body 4 against an average of (1+1+4)/3 = 2, closing
    // at 107 inside the region. The command queues on the close — the
    // account is no longer clean (the queued entry counts, which is
    // exactly what stops a second instance stacking on the same bar) —
    // but nothing has filled yet.
    bar(&mut app, &mut id, "103", "107");
    assert!(
        !app.active_tab().paper.is_flat(),
        "the queued entry occupies the account before it fills"
    );
    assert!(
        matches!(
            app.active_tab()
                .flow_pane
                .strategies
                .for_drawing(drawing)
                .expect("instance")
                .armed
                .state(),
            quantick_strategy::ArmedState::Fired { .. }
        ),
        "a market order fills on the *next* print, exactly like a hand"
    );
    // …and the next print fills it.
    print(&mut app, &mut id, "107.5");
    assert_eq!(
        app.active_tab()
            .flow_pane
            .strategies
            .for_drawing(drawing)
            .expect("instance")
            .armed
            .state(),
        &quantick_strategy::ArmedState::InPosition,
        "the entry met the tape at the print after the trigger"
    );

    // Take profit = close 107 + 1× range 4 = 111: a print at the level
    // closes the operation.
    print(&mut app, &mut id, "111");
    assert!(
        app.active_tab().paper.is_flat(),
        "the projected take profit closed the operation"
    );

    // The completion bar: the one-shot instance walks to done and holds
    // fire forever after.
    for _ in 0..48 {
        print(&mut app, &mut id, "108");
    }
    let pane = &app.active_tab().flow_pane;
    let instance = pane
        .strategies
        .for_drawing(drawing)
        .expect("the instance still rides the drawing");
    assert_eq!(
        instance.armed.state(),
        &quantick_strategy::ArmedState::Done,
        "one shot per arming: after the round trip the instance is done"
    );
}

/// One print, one bar, one sound: the whole alarm chain from the tape to
/// the platform's sink, with a recorder standing in for the speaker.
///
/// The two halves the trader asked for are both here. The **preview**
/// fires part-way through a bar that has not closed — before any order
/// could exist, which is the head start the alarm is for — and the
/// **alarm-only** instance places nothing while it does, because this
/// trader is executing on another platform.
#[test]
fn the_signal_alarm_sounds_mid_bar_and_places_nothing() {
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
    /// One Tick(50) bar: 49 prints at `open`, the fiftieth at `close`.
    fn bar(app: &mut QuantickApp, id: &mut u64, open: &str, close: &str) {
        for _ in 0..49 {
            print(app, id, open);
        }
        print(app, id, close);
    }

    let (mut app, _events, _commands, _book) = test_app();
    let recorder = crate::audio::RecordingAlerts::default();
    app.alerts = Box::new(recorder.clone());

    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;

    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.window = 3;
    form.min_range = "0".to_owned();
    form.alarm = true;
    form.alarm_when = "share".to_owned();
    form.alarm_share_percent = 70;
    // One sound per bar keeps the assertion about *which* bar spoke
    // rather than about the wall clock a cooldown would consult.
    form.alarm_repeat = "once_per_bar".to_owned();
    // A library clip with a cut, so the recorder can show the length
    // travelled with the sound all the way from the form to the sink
    // (a platform beep would arrive whole, cut or no cut).
    let clip = crate::audio::AlertSound::in_category(crate::audio::SoundCategory::Standard)
        .next()
        .expect("a standard clip is shipped");
    form.alarm_sound = clip.token().to_owned();
    form.alarm_play_secs = Some(3);
    form.alarm_only = true;
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "alarm".to_owned())
        .expect("the alarm form compiles and the drawing exists");
    assert_eq!(
        recorder.warmed_up(),
        vec![crate::audio::Cue::cut_after(clip, 3)],
        "arming warms the sink up for the cue it will be asked for"
    );

    let mut id = 0u64;
    // Three body-1 warmup bars inside the region: quiet, and silent.
    bar(&mut app, &mut id, "100", "101");
    bar(&mut app, &mut id, "101", "102");
    bar(&mut app, &mut id, "102", "103");
    app.play_pending_alarms();
    assert!(
        recorder.sounds().is_empty(),
        "a warming ruler has nothing to announce: {:?}",
        recorder.sounds()
    );

    // The signal bar, in two halves. Its first print sets the open at
    // 103; the rest move the close to 107, a body of 4 against an
    // average of 2 — force, from print two onward. A Tick(50) bar's
    // 70% gate opens on print 35, so prints 2 to 34 are the control:
    // the bar already qualifies and the alarm is still holding.
    print(&mut app, &mut id, "103");
    for _ in 0..33 {
        print(&mut app, &mut id, "107");
    }
    app.play_pending_alarms();
    assert!(
        recorder.sounds().is_empty(),
        "before 70% of the bar the alarm holds its tongue: {:?}",
        recorder.sounds()
    );

    // Print 35 crosses the share. The bar has not closed.
    print(&mut app, &mut id, "107");
    app.play_pending_alarms();
    assert_eq!(
        recorder.cues(),
        vec![crate::audio::Cue::cut_after(clip, 3)],
        "past the share the forming bar alarms, in the preset's own sound, cut \
             where the preset said"
    );
    let instance = app
        .active_tab()
        .flow_pane
        .strategies
        .for_drawing(drawing)
        .expect("instance");
    assert_eq!(
        instance.mark,
        crate::strategy_anchors::AlarmMark::Preview,
        "a bar that has not closed is announced as provisional"
    );
    assert!(
        app.active_tab().paper.is_flat(),
        "the alarm sounded and nothing was ordered — the bar is still open"
    );

    // The rest of the bar, then its close. Once per bar means the
    // trader hears this signal exactly once, however many prints agree.
    for _ in 0..15 {
        print(&mut app, &mut id, "107");
    }
    app.play_pending_alarms();
    assert_eq!(
        recorder.sounds(),
        vec![clip],
        "one sound per bar, whatever the tape does inside it"
    );
    let instance = app
        .active_tab()
        .flow_pane
        .strategies
        .for_drawing(drawing)
        .expect("instance");
    assert_eq!(
        instance.mark,
        crate::strategy_anchors::AlarmMark::Confirmed,
        "the bar closed still qualifying: the preview held"
    );
    assert_eq!(
        instance.armed.state(),
        &quantick_strategy::ArmedState::Armed,
        "an alarm-only instance is never spent — it keeps watching"
    );
    assert!(
        app.active_tab().paper.is_flat(),
        "alarm only: the whole bar passed and no order was ever placed"
    );
}

/// Arming a region whose drawn span already ended is refused with the
/// fix in hand — "armed" over a structurally dead region is the silent
/// halt the named disarms exist to prevent.
#[test]
fn arming_a_region_that_already_ended_is_refused() {
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

    let (mut app, _events, _commands, _book) = test_app();
    // Sixteen closed Tick(50) bars, so "the newest bar" is slot 15.
    let mut id = 0u64;
    for _ in 0..16 {
        for _ in 0..50 {
            print(&mut app, &mut id, "100");
        }
    }
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 99.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(4.0, 101.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;
    let form = crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    let refused = app
        .arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "dead".to_owned())
        .expect_err("a region ending before the next bar cannot arm");
    assert!(
        refused.contains("extend right"),
        "the refusal hands over the fix: {refused}"
    );

    // The boundary the guard once let through: a right anchor exactly
    // on the newest *closed* bar (slot 15 of 16) still cannot cover
    // the next bar to close (slot 16) — refused, not armed dead.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 99.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(15.0, 101.0));
    }
    let boundary = app.active_tab().flow_pane.drawings.items()[1].id;
    app.arm_strategy_instance(pane::PaneSide::Flow, boundary, &form, "edge".to_owned())
        .expect_err("a span ending on the newest closed bar covers no future bar");
    // One anchored past the next slot arms fine without extend right.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 99.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(16.0, 101.0));
    }
    let alive = app.active_tab().flow_pane.drawings.items()[2].id;
    app.arm_strategy_instance(pane::PaneSide::Flow, alive, &form, "ok".to_owned())
        .expect("a span covering the next slot arms");

    // The same dead drawing with extend right on arms fine.
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
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "live".to_owned())
        .expect("extend right keeps the region alive, so arming is honest");
}

/// Delete-all (and by the same sweep, undo/redo) must not leave a
/// resting bot order behind when the drawings vanish outside the
/// per-drawing removal funnel.
#[test]
fn delete_all_sweeps_the_armed_instances_pending_entries() {
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
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "BF".to_owned())
        .expect("the form compiles");
    let mut id = 0u64;
    bar(&mut app, &mut id, "110", "109");
    bar(&mut app, &mut id, "109", "108");
    bar(&mut app, &mut id, "108", "104");
    assert_eq!(
        app.active_tab().paper.working_orders().len(),
        1,
        "the retest limit rests"
    );

    // Delete every drawing the way the manager's button does, then run
    // the same-frame sweep + drain the tab applies.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings.delete_all();
        pane.sweep_strategy_orphans();
    }
    app.active_tab_mut().apply_strategy_cleanup();
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "no resting bot order outlives its badge"
    );
    assert!(app.active_tab().flow_pane.strategies.is_empty());
}

/// The bug the trader reported, walked through the real chart path: a
/// sell region drawn above the market, and a force bar whose *shadow*
/// reaches into the band while its body stays entirely below it. The
/// body never crossed the edge, so nothing may rest on it.
///
/// The numbers make the test bite: range 10 puts the projected SL at
/// 106 and the TP at 86, both clear of the 105 edge, so the old
/// close-only rule really did rest a sell limit at 105 — an order
/// inside a band the bar's body never entered.
#[test]
fn a_force_bar_that_never_crossed_the_region_leaves_no_order_in_it() {
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
    /// Fifty prints again, but one of them reaches `wick` — so the bar
    /// carries a shadow its body never covers.
    fn bar_with_wick(app: &mut QuantickApp, id: &mut u64, open: &str, wick: &str, close: &str) {
        for _ in 0..48 {
            print(app, id, open);
        }
        print(app, id, wick);
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
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "BF no cut".to_owned())
        .expect("the retest form compiles");

    let mut id = 0u64;
    bar(&mut app, &mut id, "102", "101");
    bar(&mut app, &mut id, "101", "100");
    // Body 4 over average (1+1+4)/3 = 2: a genuine force bar. It opens
    // at 100, already below the region's 105 edge, prints once at 106
    // inside the band, and closes at 96. The shadow visited the region;
    // the body never did.
    bar_with_wick(&mut app, &mut id, "100", "106", "96");

    let tab = app.active_tab();
    // The premise, pinned: if the bar spec or the print counts ever
    // drift, the 106 print lands in a neighbouring bar and this test
    // silently stops testing wick-versus-body — the only thing it is
    // here for.
    assert_eq!(
        tab.flow_pane.closed_slots(),
        3,
        "the three fixture bars closed as three bars"
    );
    let trigger = tab.flow_pane.closed_bar(2).expect("the trigger bar closed");
    assert_eq!(
        (trigger.open, trigger.high, trigger.close),
        (
            rust_decimal::Decimal::from(100),
            rust_decimal::Decimal::from(106),
            rust_decimal::Decimal::from(96)
        ),
        "the shadow reaches into the 105-115 band while the body stays below it"
    );
    assert!(
        tab.paper.working_orders().is_empty(),
        "a bar whose body never cut the region rests no order in it: {:?}",
        tab.paper.working_orders()
    );
    assert!(tab.paper.is_flat(), "and takes no position either");
    let instance = tab
        .flow_pane
        .strategies
        .for_drawing(drawing)
        .expect("instance");
    assert_eq!(
        instance.armed.state(),
        &quantick_strategy::ArmedState::Armed
    );
    assert_eq!(
        instance.armed.status_line(),
        "armed · trigger held: the body never cut the region",
        "the instance names the gate it held on rather than reporting a bare armed"
    );
}

/// The resting retest limit never trades against a hand: if the trader
/// opens a manual position while the order rests, the fill moment
/// stands the order down — the trader's position and bracket survive
/// untouched, and the badge says why the bot declined.
#[test]
fn a_resting_retest_limit_stands_down_over_a_manual_position() {
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
    bar(&mut app, &mut id, "108", "104");
    assert_eq!(app.active_tab().paper.working_orders().len(), 1);

    // The trader buys at market while the bot's limit rests.
    app.active_tab_mut()
        .paper
        .apply_sim_command_for_tests(quantick_sim::Command::PlaceMarket {
            side: quantick_engine::Side::Buy,
            quantity: Decimal::ONE,
            bracket: quantick_sim::Bracket::none(),
        });
    print(&mut app, &mut id, "104");
    assert!(
        app.active_tab().paper.position_summary().is_some(),
        "the manual long filled"
    );

    // The tape returns to the edge: the bot's fill moment finds the
    // account occupied and stands down instead of netting the trader's
    // long closed.
    print(&mut app, &mut id, "105");
    let tab = app.active_tab();
    assert!(
        tab.paper.working_orders().is_empty(),
        "the bot's order stood down"
    );
    assert!(
        tab.paper.position_summary().is_some(),
        "the trader's long survives untouched"
    );
    let instance = tab
        .flow_pane
        .strategies
        .for_drawing(drawing)
        .expect("instance");
    assert_eq!(
        instance.armed.state(),
        &quantick_strategy::ArmedState::Disarmed {
            reason: quantick_strategy::DisarmReason::AccountOccupied
        }
    );
    assert_eq!(instance.armed.status_line(), "stood down — account busy");
}

/// Two instances co-triggered by one closed bar must not stack: the
/// first one's *queued* entry already occupies the account, so the
/// second holds fire — "at most one live operation per chart" is a
/// property of the gate, not of luck.
#[test]
fn co_triggered_instances_do_not_stack_orders() {
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
        .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        .expect("the rectangle tool is registered");
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 95.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(30.0, 115.0));
    }
    let first = app.active_tab().flow_pane.drawings.items()[0].id;
    let second = app.active_tab().flow_pane.drawings.items()[1].id;
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.window = 3;
    form.min_range = "0".to_owned();
    app.arm_strategy_instance(pane::PaneSide::Flow, first, &form, "a".to_owned())
        .expect("arms");
    app.arm_strategy_instance(pane::PaneSide::Flow, second, &form, "b".to_owned())
        .expect("arms");

    let mut id = 0u64;
    bar(&mut app, &mut id, "100", "101");
    bar(&mut app, &mut id, "101", "102");
    bar(&mut app, &mut id, "102", "103");
    // One force bar inside both regions: both triggers say fire, the
    // gate lets exactly one through.
    bar(&mut app, &mut id, "103", "107");
    let fired = app
        .active_tab()
        .flow_pane
        .strategies
        .instances
        .iter()
        .filter(|instance| {
            matches!(
                instance.armed.state(),
                quantick_strategy::ArmedState::Fired { .. }
            )
        })
        .count();
    assert_eq!(
        fired, 1,
        "the queued entry blocks the second instance: the simulator models one netted position carrying one bracket, so a second fill would overwrite the first's stop with no event"
    );
}

/// The safety sweeps are wired, not just written: a rebuilt timeline
/// disarms every instance with the reset's own reason on the badge.
#[test]
fn a_timeline_reset_disarms_the_armed_instances_by_name() {
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
            .place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
    }
    let drawing = app.active_tab().flow_pane.drawings.items()[0].id;
    let form = crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell);
    app.arm_strategy_instance(pane::PaneSide::Flow, drawing, &form, "test".to_owned())
        .expect("arms");

    app.active_tab_mut().reset_market_state();

    let pane = &app.active_tab().flow_pane;
    let instance = pane
        .strategies
        .for_drawing(drawing)
        .expect("still attached");
    assert_eq!(
        instance.armed.state(),
        &quantick_strategy::ArmedState::Disarmed {
            reason: quantick_strategy::DisarmReason::TimelineReset
        },
        "a judgement armed on the old timeline must not carry into the new one"
    );
}

/// Through the real frame pipeline — real pointer events, the pane's own
/// scale — a click on the ✕ of a working order's chart tag cancels the
/// order. It must never read as a drag on the order's line: the hit-test
/// is geometric at press time, because a cached pixel rect goes stale
/// the moment a live chart autoscales between paint and press.
#[test]
fn clicking_the_chart_tag_close_cancels_the_order() {
    let ctx = egui::Context::default();
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![
            trade(2),
            trade(6),
            trade(10),
            trade(14),
            trade(18),
        ]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    // A resting buy limit in the middle of the backfilled price range,
    // so its line and tag are on screen.
    let price = Decimal::new(1005, 1);
    app.active_tab_mut()
        .paper
        .apply_sim_command_for_tests(quantick_sim::Command::PlaceLimit {
            side: quantick_engine::Side::Buy,
            quantity: Decimal::ONE,
            price,
            bracket: quantick_sim::Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
    assert_eq!(app.active_tab().paper.working_orders().len(), 1);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let chart = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("the pane laid out");
    let tag_right = app
        .active_tab()
        .flow_pane
        .last_lane_divider_x
        .unwrap_or(chart.right());
    let y = price_y(&app, PaneSide::Flow, 100.5);
    let close = crate::paper_trading::close_button_rect(
        tag_right,
        crate::paper_trading::clamp_tag_center(y, chart.top(), chart.bottom()),
    );
    drag_chart(&mut app, &ctx, close.center(), close.center());
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "the click cancelled the order instead of dragging it"
    );
}

/// The same ✕, with a drawing tool armed: the button is the tool's, and
/// it can only be handed out once.
///
/// The armed tool used to return early from the whole navigation pass,
/// which is what kept the paper layer from also seeing the press. Fixing
/// that (audit S2) without gating the paper gesture would mean one click
/// dropping an anchor *and* cancelling a working order.
#[test]
fn an_armed_tool_does_not_also_cancel_the_order_under_the_pointer() {
    let ctx = egui::Context::default();
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![
            trade(2),
            trade(6),
            trade(10),
            trade(14),
            trade(18),
        ]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    let price = Decimal::new(1005, 1);
    app.active_tab_mut()
        .paper
        .apply_sim_command_for_tests(quantick_sim::Command::PlaceLimit {
            side: quantick_engine::Side::Buy,
            quantity: Decimal::ONE,
            price,
            bracket: quantick_sim::Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));

    let chart = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("the pane laid out");
    let tag_right = app
        .active_tab()
        .flow_pane
        .last_lane_divider_x
        .unwrap_or(chart.right());
    let y = price_y(&app, PaneSide::Flow, 100.5);
    let close = crate::paper_trading::close_button_rect(
        tag_right,
        crate::paper_trading::clamp_tag_center(y, chart.top(), chart.bottom()),
    );
    drag_chart(&mut app, &ctx, close.center(), close.center());

    assert_eq!(
        app.active_tab().paper.working_orders().len(),
        1,
        "an armed tool must not hand the same click to the order tag"
    );
}

#[test]
fn a_moved_inspector_keeps_its_position_across_selection_changes() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    for price_y in [250.0, 400.0] {
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, price_y));
    }
    click_chart(&mut app, &ctx, egui::pos2(400.0, 250.0));
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    let inspector = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the inspector is open");
    // Drag by the title bar (left of the trailing icons).
    let bar = egui::pos2(inspector.left() + 60.0, inspector.top() + 14.0);
    drag_chart(&mut app, &ctx, bar, bar + egui::vec2(150.0, 120.0));
    assert!(
        app.surfaces.drawing_chrome.inspector_moved(),
        "a title-bar drag records the manual move"
    );
    let held = app
        .surfaces
        .drawing_chrome
        .inspector_pos()
        .expect("the manual position is recorded");

    // Selecting the other line must not snap the window back. The panel
    // closes with the selection now — the context bar is what the next
    // object raises — so this re-opens it and proves the *position* is
    // what survives, which is what the rule was ever about.
    click_chart(&mut app, &ctx, egui::pos2(400.0, 400.0));
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);
    assert_eq!(
        app.surfaces.drawing_chrome.inspector_pos(),
        Some(held),
        "the manual position survives a selection change"
    );
    assert!(
        app.surfaces.drawing_chrome.inspector_moved(),
        "the manual flag is never auto-cleared"
    );
}

/// Unless the trader switched autosave off, which the Workspace menu reads
/// as a promise: "Off, only Save workspace changes what quantick opens on."
/// The popup still moves — that is live state — but the curated startup
/// layout is not rewritten behind their back.
#[test]
fn autosave_off_means_the_popup_position_is_not_written_either() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    with_a_saved_workspace(&mut app, &ctx, "popup-no-autosave");
    app.save_on_exit = false;
    draw_horizontal_line(&mut app, &ctx, 300.0);

    let parked = park_the_popup(&mut app, &ctx, egui::vec2(150.0, 90.0));

    assert_eq!(
        app.surfaces.drawing_chrome.inspector_pos(),
        Some(parked),
        "the window still went where it was dragged"
    );
    assert_eq!(
        ui_state::load(&app.ui_state_path)
            .chrome
            .expect("the chrome is still there")
            .inspector_position,
        None,
        "but autosave off leaves the saved workspace untouched"
    );
    let _ = std::fs::remove_file(&app.ui_state_path);
}

/// One window, every tool: a position remembered from a *previous session*
/// greets whichever drawing is clicked next, not only the one that happened
/// to be selected when the hand let go.
#[test]
fn a_remembered_position_greets_the_next_drawing_too() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.ui_state_path = scratch_ui_state("popup-next-drawing");
    run_frame(&mut app, &ctx);
    draw_horizontal_line(&mut app, &ctx, 250.0);
    draw_horizontal_line(&mut app, &ctx, 400.0);

    // What a restored workspace does, through the same one door.
    let remembered = egui::pos2(420.0, 200.0);
    app.surfaces
        .drawing_chrome
        .place_inspector_by_hand(remembered);

    // A different drawing than the one selected when it was parked.
    click_chart(&mut app, &ctx, egui::pos2(400.0, 250.0));
    run_frame(&mut app, &ctx);
    open_inspector(&mut app, &ctx);

    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open");
    assert_eq!(
        popup.min, remembered,
        "the popup opens where it was left, not beside the object it configures"
    );
}

/// The everyday layout is the split canvas, and its flow pane is narrower
/// than the auto-pin threshold — so without this the panel would re-dock on
/// every selection and the remembered position would never be reached at
/// all. Parking the window is the trader saying they want it floating
/// *there*, which is the same statement the pin button makes.
#[test]
fn a_remembered_position_outranks_the_narrow_chart_auto_pin() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.ui_state_path = scratch_ui_state("popup-auto-pin");
    // A chart under INSPECTOR_AUTO_PIN_CHART_WIDTH_PX, which is what a
    // split canvas gives the pane a drawing lives on.
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_sized(&mut app, &ctx, MIN_WINDOW, egui::pos2(500.0, 300.0));
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());

    // What a restored workspace does.
    let remembered = egui::pos2(300.0, 200.0);
    app.surfaces
        .drawing_chrome
        .restore_inspector_position(Some([remembered.x, remembered.y]));
    app.surfaces.drawing_chrome.set_inspector_open(true);
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());

    assert!(
        !app.surfaces.drawing_chrome.inspector_pinned(),
        "the parked position wins over the narrow-chart auto-pin"
    );
    assert!(
        ctx.memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .is_some(),
        "and the floating window is the one on screen"
    );
}

/// The opposite case, so the rule above does not quietly disable the
/// auto-pin for everyone: a workspace that remembers no position leaves the
/// narrow-chart rule exactly as it was.
#[test]
fn a_workspace_with_no_remembered_position_leaves_the_auto_pin_alone() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.ui_state_path = scratch_ui_state("popup-auto-pin-default");
    // A previous cockpit that *did* park the popup, so this proves the
    // silence is adopted rather than merely never contradicted.
    app.surfaces
        .drawing_chrome
        .place_inspector_by_hand(egui::pos2(300.0, 200.0));
    app.surfaces.drawing_chrome.restore_inspector_position(None);
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_sized(&mut app, &ctx, MIN_WINDOW, egui::pos2(500.0, 300.0));
    app.surfaces.drawing_chrome.set_inspector_open(true);
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());

    assert!(
        !app.surfaces.drawing_chrome.inspector_pin_touched(),
        "silence in the file is not a preference about the pin"
    );
    assert!(
        app.surfaces.drawing_chrome.inspector_pinned(),
        "so a chart too narrow for a floating window still docks the panel"
    );
}

/// A remembered position is what the trader did, not a promise the screen
/// can still keep. Restored onto a window too small for it — the laptop
/// after the desk monitor — the popup is repaired into the chart rather
/// than obeyed off the side of it.
///
/// Through the file, because that is the path the claim is about, and
/// because the second half of it can only be checked there: the repair is
/// for *drawing*. The point the trader parked survives it, so plugging the
/// big monitor back in brings the popup back to where they left it instead
/// of to wherever the small screen could fit it.
#[test]
fn a_position_that_no_longer_fits_is_repaired_for_drawing_and_kept_in_the_file() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.ui_state_path = scratch_ui_state("popup-clamp");
    // The auto-pin owns a chart this narrow until the trader touches the
    // pin; this test is about the floating window, so say they have.
    app.surfaces.drawing_chrome.set_inspector_pin_touched(true);
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_sized(&mut app, &ctx, MIN_WINDOW, egui::pos2(500.0, 300.0));
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());

    // A workspace written on a much larger monitor.
    let parked = [2_400.0, 1_500.0];
    app.restore_workspace(ui_state::Workspace::new(
        true,
        None,
        0,
        Vec::new(),
        Some(chrome_with_popup_at(Some(parked))),
    ));
    app.surfaces.drawing_chrome.set_inspector_open(true);
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());

    let chart = app
        .drawing_pane()
        .last_chart_area
        .expect("the pane has been laid out");
    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open");
    assert!(
        chart.contains(popup.min),
        "the popup draws inside the chart, not at yesterday's monitor: \
             {popup:?} against {chart:?}"
    );
    assert_eq!(
        app.surfaces.drawing_chrome.remembered_inspector_position(),
        Some(parked),
        "and the point the trader parked is not eaten by the repair — the \
             desk monitor gets it back"
    );
}

/// The same rule the other way round, and the reason the repair is not
/// written back: selecting a taller panel while parked low used to ratchet
/// the position upward, permanently, a little at a time.
#[test]
fn a_temporary_squeeze_never_edits_the_parked_position() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.ui_state_path = scratch_ui_state("popup-ratchet");
    app.surfaces.drawing_chrome.set_inspector_pin_touched(true);
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_sized(&mut app, &ctx, MIN_WINDOW, egui::pos2(500.0, 300.0));
    run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());

    // Parked against the bottom-right of a big screen, then squeezed by a
    // small one for a while.
    let parked = egui::pos2(1_500.0, 900.0);
    app.surfaces.drawing_chrome.place_inspector_by_hand(parked);
    app.surfaces.drawing_chrome.set_inspector_open(true);
    for _ in 0..6 {
        run_sized_frame(&mut app, &ctx, MIN_WINDOW, Vec::new());
    }

    assert_eq!(
        app.surfaces.drawing_chrome.inspector_pos(),
        Some(parked),
        "six frames of repair leave the parked point exactly as it was"
    );
}

/// A hand-edited file — the harness doc tells agents to write one — can say
/// `nan`, and NaN is the one value the repair cannot walk back: it survives
/// `clamp`, puts the window where no pointer can reach its title bar, and
/// would be written straight back at the next save.
#[test]
fn a_position_that_is_not_a_number_is_read_as_no_position_at_all() {
    let (mut app, _commands) = app_with_history(50);

    for pair in [[f32::NAN, 200.0], [200.0, f32::NAN], [f32::INFINITY, 200.0]] {
        app.surfaces
            .drawing_chrome
            .place_inspector_by_hand(egui::pos2(10.0, 10.0));
        app.surfaces
            .drawing_chrome
            .restore_inspector_position(Some(pair));
        assert_eq!(
            app.surfaces.drawing_chrome.inspector_pos(),
            None,
            "{pair:?} is not a position, and must not survive as one"
        );
        assert!(
            !app.surfaces.drawing_chrome.inspector_moved(),
            "{pair:?} hands the popup back to automatic placement"
        );
    }
}

#[test]
fn the_object_manager_toggles_eye_lock_and_z_order_per_row() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    for price_y in [250.0, 350.0] {
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, price_y));
    }
    let objects = app
        .toolrail
        .objects_button_rect()
        .expect("the toolbox shows the Objects entry");
    click_chart(&mut app, &ctx, objects.center());
    assert!(
        app.surfaces.drawing_chrome.manager_open(),
        "the Objects button opens the manager"
    );
    run_frame(&mut app, &ctx);

    let rect_of = |app: &QuantickApp, index: usize, action: &str| {
        app.surfaces
            .drawing_chrome
            .manager_action_rects()
            .iter()
            .find(|(row, name, _)| *row == index && *name == action)
            .map(|(_, _, rect)| *rect)
            .expect("manager action rendered")
    };

    let eye = rect_of(&app, 0, "Eye");
    click_chart(&mut app, &ctx, eye.center());
    assert!(
        app.active_tab().flow_pane.drawings.items()[0].hidden,
        "the row's eye hides it"
    );

    run_frame(&mut app, &ctx);
    let lock = rect_of(&app, 1, "Lock");
    click_chart(&mut app, &ctx, lock.center());
    assert!(
        app.active_tab().flow_pane.drawings.items()[1].locked,
        "the row's lock locks it"
    );

    run_frame(&mut app, &ctx);
    let front = rect_of(&app, 0, "Front");
    let hidden_line_price = app.active_tab().flow_pane.drawings.items()[0].points[0].price;
    click_chart(&mut app, &ctx, front.center());
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[1].points[0].price,
        hidden_line_price,
        "Front moves the object to the top of the z-order"
    );
}

/// What is typed lands in the object, and Escape closes the editor
/// without eating the words — nor the selection the context bar hangs
/// off, which is the row that styles the note that was just written.
#[test]
fn typing_into_the_on_chart_editor_fills_the_note_and_escape_keeps_it() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "text");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    let index = app.inline_text_editing().expect("the editor is open");

    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::Text("daily high".to_owned())],
    );
    run_frame(&mut app, &ctx);
    let note = |app: &QuantickApp| -> String {
        let drawing = &app.active_tab().flow_pane.drawings.items()[index];
        drawing
            .tool
            .inline_text(drawing.payload.as_ref())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(note(&app), "daily high", "the words reach the object");

    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    run_frame(&mut app, &ctx);
    assert_eq!(app.inline_text_editing(), None, "Escape closes the editor");
    assert_eq!(note(&app), "daily high", "and keeps what was typed");
    assert_eq!(
        app.active_tab().flow_pane.drawings.selected(),
        Some(index),
        "the note stays selected, so its context bar is still there"
    );
}

/// No drawing tool may share a chord with an order.
///
/// The rail arms tools with `key_pressed`, which does not consume the
/// key, and it runs before the trading shortcuts — so a shared chord
/// fires *both*. `Shift+B` armed the sell mark and bought at market;
/// `Shift+F` armed a Fib extension and flattened the position. Neither
/// is a thing a trader can be asked to remember around.
#[test]
fn no_drawing_shortcut_shares_a_chord_with_an_order() {
    let orders = [
        ("buy at market", PAPER_BUY_SHORTCUT),
        ("sell at market", PAPER_SELL_SHORTCUT),
        ("reverse", PAPER_REVERSE_SHORTCUT),
        ("flatten", PAPER_FLATTEN_SHORTCUT),
        ("cancel working orders", PAPER_CANCEL_SHORTCUT),
    ];
    for tool in drawings::DRAWING_TOOLS {
        let Some(shortcut) = tool.shortcut() else {
            continue;
        };
        for (name, order) in orders {
            // The rail matches key *and* shift exactly, so `B` and
            // `Shift+B` are genuinely different chords — which is what
            // lets the marks keep the letter of the side they mean.
            let same_chord = order.logical_key == shortcut.key
                && order.modifiers.shift == shortcut.shift
                && !order.modifiers.command
                && !order.modifiers.alt;
            assert!(
                !same_chord,
                "{} shares its chord with {name}: one keystroke would draw and trade",
                tool.id()
            );
        }
    }
}

/// The trader's third veto, end to end: a mark that lands at the price
/// under the cursor floats inside the candle body, disappears at a
/// glance and vanishes entirely with the footprint open. It has to grab
/// the bar's extreme, and it has to do so with the magnet *off* —
/// snapping is this tool's own rule, not a setting it borrows.
#[test]
fn a_buy_mark_grabs_the_bars_low_with_the_magnet_off() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    assert!(!app.toolrail.magnet(), "this proof needs the magnet off");

    arm_drawing_from_toolbox(&mut app, &ctx, "arrow-mark-up");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let anchor = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let candle = app
        .active_tab()
        .flow_pane
        .closed_bar(slot_of(anchor.bar))
        .expect("the click landed on a bar")
        .clone();
    let low = rust_decimal::prelude::ToPrimitive::to_f64(&candle.low).unwrap();
    assert!(
        (anchor.price - low).abs() < 1e-9,
        "the buy mark hangs from the low ({low}), not from the pointer ({})",
        anchor.price
    );

    // And the pointer's height is genuinely ignored, not merely equal to
    // the low by luck of this fixture: a second mark 90 px up the same
    // bar lands on the same price.
    arm_drawing_from_toolbox(&mut app, &ctx, "arrow-mark-up");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 210.0));
    let second = app.active_tab().flow_pane.drawings.items()[1].points[0];
    assert!(
        (second.price - anchor.price).abs() < 1e-9,
        "two clicks on one bar, 90 px apart, must give one price"
    );
    assert_eq!(slot_of(second.bar), slot_of(anchor.bar));
}

#[test]
fn a_sell_mark_grabs_the_bars_high() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "arrow-mark-down");
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let anchor = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let candle = app
        .active_tab()
        .flow_pane
        .closed_bar(slot_of(anchor.bar))
        .expect("the click landed on a bar")
        .clone();
    let high = rust_decimal::prelude::ToPrimitive::to_f64(&candle.high).unwrap();
    assert!((anchor.price - high).abs() < 1e-9);
}

/// The trader's first veto, end to end: an armed tool means they are
/// drawing, and an opaque strip left over the canvas would eat the click
/// that places the next object.
#[test]
fn an_armed_tool_takes_the_context_bar_off_the_canvas() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces.drawing_chrome.context_bar_rect().is_some(),
        "the finished object has it"
    );

    app.surfaces.drawing_chrome.forget_context_bar_rect();
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces.drawing_chrome.context_bar_rect().is_none(),
        "arming a tool clears the way for the next drawing"
    );
}

#[test]
fn one_drag_creates_one_undo_entry() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        1,
        "creating the drawing is the first undo entry"
    );

    let before_drag = app.active_tab().flow_pane.drawings.items()[0].points[0];
    // Clear of the inspector the selection opened.
    let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    for step in [
        start + egui::vec2(10.0, 12.0),
        start + egui::vec2(25.0, 26.0),
    ] {
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(step)]);
    }
    let end = start + egui::vec2(40.0, 40.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    );
    assert_ne!(
        app.active_tab().flow_pane.drawings.items()[0].points[0],
        before_drag,
        "the drag really moved the line"
    );

    assert_eq!(
        app.active_tab().flow_pane.drawings.undo_depth(),
        2,
        "a multi-frame drag coalesces into exactly one undo entry"
    );
    assert!(
        app.active_tab_mut().flow_pane.drawings.undo(),
        "undo the drag"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points[0],
        before_drag,
        "one undo rewinds the whole drag"
    );
    assert!(
        app.active_tab_mut().flow_pane.drawings.undo(),
        "undo the creation"
    );
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
}

/// Copying the band copies the bot. A trader who duplicates a region
/// wants it watched the same way; re-typing the form on every copy is a
/// step that only existed because the copy forgot.
///
/// State does not travel with it. The copy starts armed and idle: a
/// cloned `Fired` would hang a second badge on one order, and a cloned
/// spent one-shot would arrive already finished.
#[test]
fn duplicating_a_band_carries_its_armed_strategy_but_not_its_state() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail.arm(Tool::Drawing(drawing_tool("rectangle")));
    click_chart(&mut app, &ctx, egui::pos2(600.0, 260.0));
    click_chart(&mut app, &ctx, egui::pos2(760.0, 340.0));
    let source = app.active_tab().flow_pane.drawings.items()[0].id;
    // Drawn over history, so it needs the band that runs to the chart's
    // edge before anything can be armed on it — the same refusal a
    // trader meets, and the way they answer it.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let index = pane.drawings.index_of(source).expect("drawing lives");
        pane.drawings.items_mut()[index]
            .payload
            .as_any_mut()
            .downcast_mut::<drawings::RectanglePayload>()
            .expect("a rectangle carries a rectangle payload")
            .extend_right = true;
    }

    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell);
    form.window = 7;
    form.min_range = "0".to_owned();
    form.alarm = true;
    app.arm_strategy_instance(pane::PaneSide::Flow, source, &form, "carry me".to_owned())
        .expect("the region arms");
    // Select the band again — arming leaves the dialog, not a selection.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let index = pane.drawings.index_of(source).expect("drawing lives");
        pane.drawings.select(Some(index));
    }

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::D, egui::Modifiers::COMMAND)],
        egui::Modifiers::COMMAND,
    );

    let pane = &app.active_tab().flow_pane;
    assert_eq!(pane.drawings.items().len(), 2, "the band was duplicated");
    let copy = pane.drawings.items()[1].id;
    assert_ne!(copy, source);
    let carried = pane
        .strategies
        .for_drawing(copy)
        .expect("the copy carries its own armed instance");
    assert_eq!(carried.preset, "carry me", "same preset, by name");
    assert_eq!(carried.spec, form, "and by every field of the form");
    assert!(
        carried.alarm.is_some(),
        "an alarm the trader switched on travels with it"
    );
    assert_eq!(
        carried.armed.state(),
        &quantick_strategy::ArmedState::Armed,
        "the copy starts watching, carrying none of the original's state"
    );
    assert!(
        pane.strategies.for_drawing(source).is_some(),
        "and the original keeps its own"
    );
}

#[test]
fn the_repeat_pin_keeps_the_drawing_tool_armed() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    // Default: one-shot back to Pointer.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
    assert_eq!(app.toolrail.tool(), Tool::Pointer);

    // Pinned: the tool stays armed for the next object.
    app.toolrail.set_repeat(true);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 320.0));
    assert_eq!(
        app.toolrail.tool().drawing_tool().map(|tool| tool.id()),
        Some("horizontal-line"),
        "the repeat pin keeps the tool armed"
    );
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 2);
}

/// The paper panel used to keep a toast of its own: same lane along the
/// chart's bottom edge, 96px up instead of 44, on a 4-second clock
/// against the surface's 8. Two acknowledgements could therefore sit on
/// top of each other and disagree about how long one lasts. There is one
/// lane now, and a simulator message travels down it.
#[test]
fn a_paper_acknowledgement_reaches_the_windows_one_toast() {
    let (mut app, _commands) = app_with_history(50);
    assert!(app.surfaces.toast.message().is_none());

    app.tabs[0]
        .paper
        .show_toast("SIM: dropped at the fill - no bid".to_owned());
    app.settle_paper_panels(Instant::now());

    assert_eq!(
        app.surfaces.toast.message(),
        Some("SIM: dropped at the fill - no bid"),
        "the panel posts, the window's one toast shows it"
    );
    assert!(
        !app.surfaces.toast.offers_undo(),
        "a fill cannot be taken back, so no button pretends it can"
    );
}

/// And the panel keeps nothing back: draining takes the message, so one
/// acknowledgement is shown once however many frames pass before the
/// next.
#[test]
fn a_paper_acknowledgement_is_handed_over_once() {
    let (mut app, _commands) = app_with_history(50);
    app.tabs[0].paper.show_toast("SIM: flat".to_owned());
    app.settle_paper_panels(Instant::now());
    app.surfaces.toast.clear();
    app.settle_paper_panels(Instant::now());
    assert!(
        app.surfaces.toast.message().is_none(),
        "the outbox was emptied by the first drain"
    );
}

/// A reset that could not be written leaves the entry live.
///
/// The old arrangement is still on disk and will still be restored, so
/// telling the trader "nothing saved yet" — and disabling the only control
/// that would let them try again — states the opposite of what is true.
#[test]
fn a_reset_that_failed_leaves_the_entry_live() {
    let (mut app, _commands) = app_with_history(50);
    // A path inside a directory that does not exist: the write fails,
    // which is the case a read-only home or a full disk produces.
    app.ui_state_path = scratch_ui_state("reset-fails").join("nope.toml");
    app.toolrail.set_favorites(&["measure".to_owned()]);
    app.workspace_saved = true;

    app.forget_workspace();

    assert!(
        app.workspace_saved,
        "a reset that did not happen must stay retryable"
    );
}

/// Slot ids are per pane, so the same number means different indicators
/// on the two of them. A removal is mirrored by *layout position*, never
/// by number: with two indicators on every pane, removing the second on
/// the time pane removes the second on the flow pane — whatever numbers
/// either pane gave them.
#[test]
fn removing_a_slot_on_one_pane_removes_its_layout_position_everywhere() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
    settle_indicators(&mut app);
    assert_eq!(
        app.slot_kinds.len(),
        4,
        "two indicators, mirrored onto two panes"
    );

    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let time_cvd = app
        .active_tab()
        .time_pane()
        .expect("time pane")
        .indicators
        .all()[1]
        .slot;

    // Focused on the time pane: remove its second slot.
    app.apply_toolbar_action(ToolbarAction::RemoveIndicator(time_cvd.0));

    assert_eq!(
        app.slot_kinds.len(),
        2,
        "one registration per pane went with it"
    );
    for side in [PaneSide::Flow, PaneSide::Time(0)] {
        let pane = app.active_tab().pane(side);
        assert_eq!(
            pane.indicators.all().len(),
            1,
            "one indicator left on {side:?}"
        );
        assert!(
            pane.indicators.all()[0].label().contains("EMA"),
            "and it is the first one, not the removed one, on {side:?}"
        );
    }
}

/// "Load older" moves the first engine bar backwards in time, and the
/// prefix was trimmed against where that bar used to be.
///
/// Three clicks reach this on Binance: split the canvas, let the venue
/// history land, pull older trades. The venue candles covering the newly
/// re-cut minutes then sat in front of engine bars covering the *same*
/// minutes — the window drawn twice, `open_time` going backwards across
/// the seam, and the precondition `slot_at_time` documents quietly false.
#[test]
fn pulling_older_trades_re_trims_the_venue_prefix() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 120);

    // Something anchored to a bar index, and a view off the live edge, so
    // the shift has something to preserve.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let slots = app.active_tab().pane(PaneSide::Time(0)).slots();
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .viewport
        .pan_pixels(40.0, slots);
    let edge_before = app.active_tab().pane(PaneSide::Time(0)).right_edge_time();
    let mark_before = app.active_tab().pane(PaneSide::Time(0)).drawings.items()[0].points[0];
    let mark_time_before = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .slot_open_time(mark_before.bar as usize);
    assert!(edge_before.is_some(), "the view is off the live edge");

    // Five minutes of older trades, inside the window the prefix covers.
    let older: Vec<_> = (-5_i64..0).map(minute_trade_at).collect();
    events.try_send(FeedEvent::HistoryPrepended(older)).unwrap();
    app.drain_tabs();

    let pane = app.active_tab().pane(PaneSide::Time(0));
    let first_engine = pane
        .state
        .bars()
        .first()
        .or_else(|| pane.state.partial())
        .expect("the pane holds bars")
        .open_time;
    assert!(
        pane.history_prefix.iter().all(|bar| bar.open_time
            < crate::resample::bucket_start(first_engine, crate::feed::OHLCV_BASE_INTERVAL_MS)),
        "no venue candle may cover a minute the engine has now re-cut"
    );
    assert_eq!(
        pane.seam_slot(),
        115,
        "the five overlapping buckets left the prefix"
    );
    let opens: Vec<i64> = (0..pane.closed_slots())
        .filter_map(|slot| pane.slot_open_time(slot))
        .collect();
    assert!(
        opens.windows(2).all(|pair| pair[0] <= pair[1]),
        "and open_time still never decreases across the seam"
    );

    // The user was reading a market moment; they still are, and their mark
    // is still on the bar they put it on.
    assert_eq!(
        pane.right_edge_time(),
        edge_before,
        "the view kept the market time it was showing"
    );
    assert_eq!(
        pane.slot_open_time(pane.drawings.items()[0].points[0].bar as usize),
        mark_time_before,
        "and the mark kept the bar it was drawn against"
    );
}

/// A position carried into a replay switch belongs to the session
/// that is ending: its forced flatten must journal under that
/// session's source, not the one the switch is about to install.
#[test]
fn a_replay_switch_flattens_under_the_session_that_owned_the_position() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = history_app(&ctx);
    let dir = std::env::temp_dir().join(format!(
        "quantick-switch-source-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let print = |agg_id: u64, price: i64| quantick_engine::Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small ids") * 1000,
        price: rust_decimal::Decimal::from(price),
        quantity: rust_decimal::Decimal::ONE,
        side: quantick_engine::Side::Buy,
    };
    {
        let paper = &mut app.active_tab_mut().paper;
        paper.redirect_history_dir(dir.clone());
        paper.set_symbol("SWITCHSRC");
        paper.seed(&print(0, 100));
        paper.market(quantick_engine::Side::Buy);
        paper.on_trade(&print(1, 100));
        assert!(
            paper.position_summary().is_some(),
            "a live position is open going into the switch"
        );
    }
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
            crate::feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: crate::feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    let folder = dir.join("SWITCHSRC");
    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the forced flatten journaled")
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "one session file for the flattened trade");
    let parsed =
        quantick_sim::history::parse(&std::fs::read_to_string(files[0].path()).expect("readable"))
            .expect("valid history");
    assert_eq!(
        parsed.source,
        Some(quantick_sim::history::SessionSource::Live),
        "the live session's trade files as live, not under the replay"
    );
    assert_eq!(parsed.trades.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A chart cut by trades carries no venue candle until the trader asks —
/// and then carries the venue's own minutes, unfolded.
///
/// The default half is the important one: a tick chart has always opened
/// on the prints this session saw, and a candle appearing in front of them
/// unasked would be this change taking a decision that is the trader's.
#[test]
fn a_chart_cut_by_trades_takes_the_venue_lead_in_only_when_asked() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    let tab = app.active_tab();
    assert_eq!(
        tab.pane(PaneSide::Time(0)).seam_slot(),
        120,
        "the time pane wears the prefix, as it always has"
    );
    assert_eq!(
        tab.flow_pane.seam_slot(),
        0,
        "and the tick chart beside it is untouched by default"
    );

    app.venue_lead_in = true;
    app.drain_tabs();
    assert_eq!(
        app.active_tab().flow_pane.seam_slot(),
        120,
        "switched on, the tick chart carries the venue's own minutes"
    );
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        120,
        "and the time pane is unaffected by a switch that is not about it"
    );

    app.venue_lead_in = false;
    app.drain_tabs();
    assert_eq!(
        app.active_tab().flow_pane.seam_slot(),
        0,
        "and switching it back off takes them away again"
    );
}

/// (e) A sidecar naming a feed the config no longer has says so and is
/// ignored — a renamed feed costs its additions, not the launch.
#[test]
fn a_sidecar_entry_for_a_dead_feed_is_ignored() {
    let mut added = crate::symbols_file::AddedSymbols::default();
    added.add("a-feed-that-was-renamed", "WINQ26");
    let mut config = test_config();
    let before = config.feeds.clone();

    config.merge_added_symbols(&added);

    assert_eq!(config.feeds, before, "nothing was invented for a dead id");
    assert!(config.validate().is_ok());
}

/// Closing a tab ends that market's paper-trading session, and the
/// simulator's honesty contract says a session ends in a labeled,
/// journaled flatten — never by vanishing with its window. Everything
/// else a tab owns can simply be dropped; an open position is state the
/// user created, so it is the one thing `Tab::close` has to settle.
#[test]
fn closing_a_tab_flattens_and_journals_its_simulated_position() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let dir = std::env::temp_dir().join(format!(
        "quantick-paper-tab-close-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    app.active_tab_mut().paper.redirect_history_dir(dir.clone());

    // A filled position on the second tab: backfill seeds the mark, the
    // toolbar queues the order, the next live print fills it.
    ends.events
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    ends.events.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert!(
        app.active_tab().paper.status_cell().is_some(),
        "this proof needs an open simulated position to lose"
    );

    app.apply_tab_action(TabAction::Close(1));

    assert_eq!(app.tabs.len(), 1, "the tab is gone");
    let files: Vec<_> = std::fs::read_dir(dir.join("ETHUSDT"))
        .expect("the flatten was journaled under the closed tab's symbol")
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "one session, one history file");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The second operator places an order, brackets it and reads it back —
/// the whole chart gesture, without a chart.
///
/// `CLAUDE.md`'s *operable without a hand*: a capability a trader does
/// exists as a named call, not only inside a click handler. This is that
/// call for the one class of capability that had no registry entry at
/// all until now.
#[test]
fn the_trade_actions_place_bracket_and_read_back_an_order() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);

    let mark = app
        .active_tab()
        .paper
        .mark_price()
        .expect("the history seeded a price");
    // A buy limit a whole point below the market: below is where a buy
    // limit can rest, whatever the fixture's absolute prices are.
    let price = mark - rust_decimal::Decimal::ONE;

    let placed = app
        .control_action(
            crate::control::trade::PLACE_CAPABILITY_ID,
            crate::control::trade::CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "side": "buy",
                "kind": "limit",
                "quantity": "2",
                "price": price.to_string(),
            }),
        )
        .expect("the action dispatches");
    assert_eq!(placed["accepted"], true, "{placed}");
    assert_eq!(placed["simulated"], true, "every result says so");
    let order_id = placed["order_id"].as_u64().expect("the venue named it");
    assert_eq!(
        placed["working_orders"][0]["kind"], "limit",
        "the kind was stated, not inferred from where a pointer was"
    );

    // Brackets on the working order, through the same door the chart's
    // drag uses.
    let stop = price - rust_decimal::Decimal::ONE;
    let target = price + rust_decimal::Decimal::from(2);
    let bracketed = app
        .control_action(
            crate::control::trade::BRACKET_CAPABILITY_ID,
            crate::control::trade::CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "order_id": order_id,
                "stop_loss": stop.to_string(),
                "take_profit": target.to_string(),
            }),
        )
        .expect("the action dispatches");
    assert_eq!(bracketed["accepted"], true, "{bracketed}");
    assert_eq!(
        bracketed["working_orders"][0]["stop_loss"],
        stop.to_string(),
        "and the read-back carries the leg that will arm on the fill"
    );
    assert_eq!(
        bracketed["working_orders"][0]["take_profit"],
        target.to_string()
    );

    // A refusal comes back as an answer, in the venue's own words —
    // never as a bare status a caller can forget to render. A stop on
    // the profit side of the entry would exit the instant it filled.
    let refused = app
        .control_action(
            crate::control::trade::BRACKET_CAPABILITY_ID,
            crate::control::trade::CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "order_id": order_id,
                "stop_loss": (price + rust_decimal::Decimal::ONE).to_string(),
            }),
        )
        .expect("the action dispatches");
    assert_eq!(refused["accepted"], false);
    assert!(
        refused["rejected_because"]
            .as_str()
            .is_some_and(|reason| reason.contains("stop loss")),
        "the reason teaches: {refused}"
    );
    assert_eq!(
        refused["working_orders"][0]["stop_loss"],
        stop.to_string(),
        "and a refusal changed nothing"
    );

    // And it can be taken away again.
    let cancelled = app
        .control_action(
            crate::control::trade::CANCEL_CAPABILITY_ID,
            crate::control::trade::CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "order_id": order_id }),
        )
        .expect("the action dispatches");
    assert_eq!(cancelled["accepted"], true, "{cancelled}");
    assert!(
        cancelled["working_orders"]
            .as_array()
            .is_some_and(|orders| orders.is_empty()),
        "nothing is working now"
    );
}

/// Whatever acted is recorded. An order placed through the registry
/// carries its actor into the journal, so one an operator asked for is
/// never indistinguishable from one the trader placed by hand — the
/// authorship half of the data-honesty rule that labels an inferred
/// aggressor side, applied to who asked.
///
/// A refusal records too: "it tried to buy here and was told no" is
/// exactly the line a trader reviewing a session wants to find.
#[test]
fn an_order_placed_through_the_registry_names_who_asked_for_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);
    let mark = app
        .active_tab()
        .paper
        .mark_price()
        .expect("the history seeded a price");

    let accepted = app
        .control_action(
            crate::control::trade::PLACE_CAPABILITY_ID,
            crate::control::trade::CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "side": "buy",
                "kind": "limit",
                "quantity": "1",
                "price": (mark - rust_decimal::Decimal::ONE).to_string(),
            }),
        )
        .expect("the action dispatches");
    assert_eq!(accepted["accepted"], true, "{accepted}");

    // And one the venue refuses: a buy limit above the market.
    app.control_action(
        crate::control::trade::PLACE_CAPABILITY_ID,
        crate::control::trade::CAPABILITY_VERSION,
        crate::control::ActionOrigin::Human,
        serde_json::json!({
            "side": "buy",
            "kind": "limit",
            "quantity": "1",
            "price": (mark + rust_decimal::Decimal::ONE).to_string(),
        }),
    )
    .expect("the action dispatches");

    let events = app
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 64, 1 << 20)
        .events;
    let placed: Vec<_> = events
        .iter()
        .filter(|event| event.kind.as_str() == "trade.order.placed")
        .collect();
    assert_eq!(placed.len(), 2, "both the acceptance and the refusal");

    assert_eq!(placed[0].payload["accepted"], true);
    assert!(
        placed[0].payload["order_id"].is_u64(),
        "the accepted one names the order it made"
    );
    assert_eq!(
        placed[0].payload["simulated"], true,
        "and says the fills are simulated, in the record as on every surface"
    );
    assert_eq!(
        placed[0].actor.as_ref().expect("an actor is recorded").kind,
        quantick_control::wire::ActorKind::HumanUi,
        "the actor rides in the event, not beside it"
    );

    assert_eq!(placed[1].payload["accepted"], false);
    assert!(
        placed[1].payload["rejected_because"]
            .as_str()
            .is_some_and(|reason| reason.contains("fill immediately")),
        "the refusal keeps the venue's own words: {}",
        placed[1].payload
    );
    assert_eq!(
        placed[1].payload["asked"]["side"], "buy",
        "and what was asked for, so the attempt is legible"
    );
}
