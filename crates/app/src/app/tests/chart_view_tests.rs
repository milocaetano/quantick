use super::*;

/// `QUANTICK_POINTER` parks the mouse over the candles, which is the only
/// way a scripted run photographs anything that exists while a pointer is
/// over the chart — the compass, the crosshair, every hover readout.
///
/// Fractions of the *candles'* pane, so `0.5,0.5` frames the same place
/// whatever the window size and whatever share the live lane has taken.
#[test]
fn the_pointer_hook_parks_the_mouse_among_the_candles() {
    // The fractions themselves are parsed and refused in `harness`; this is
    // the trunk's half — where a fraction lands on the pane that drew.
    let (mut app, _cmd_rx) = app_with_history(50);
    let ctx = egui::Context::default();
    app.harness.arm_pointer(egui::vec2(0.5, 0.5));
    assert_eq!(
        app.scripted_pointer_pos(),
        None,
        "no draw yet, so no candle area to be a fraction of"
    );
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let candles = pane.drawing_area(pane.last_chart_rect.expect("the canvas laid out"));
    let position = app.scripted_pointer_pos().expect("one frame published it");
    assert!(candles.contains(position), "{position:?} vs {candles:?}");
    assert!((position.x - candles.center().x).abs() < 0.5);
    assert!((position.y - candles.center().y).abs() < 0.5);

    // And it is delivered as a real pointer event on the app's own input
    // path — never a field the paint reads — so what the chart does with
    // it is what it does with a trader's own mouse. `raw_input_hook` is
    // where eframe hands the frame's input over, so the test calls it
    // exactly where eframe does and feeds the frame what it produced.
    let mut raw = egui::RawInput::default();
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    assert!(
        matches!(
            raw.events.as_slice(),
            [egui::Event::PointerMoved(moved)] if *moved == position
        ),
        "one pointer move, where the hook said: {:?}",
        raw.events
    );
    run_frame_with_events(&mut app, &ctx, raw.events);
    assert_eq!(
        app.active_tab().flow_pane.hover_pos,
        Some(position),
        "and the pane is hovering there, which is what every readout gates on"
    );
}

#[test]
fn the_lane_axis_reads_its_window_in_a_human_unit() {
    use crate::orderflow::format_window_ms;
    // One duration, one wording. The tape's axis and the menu that sets
    // its window sit a hand's width apart, so "1.5 min" under the tape
    // while the menu reads "1 min 30 s" is two languages for one number.
    assert_eq!(format_window_ms(800), "800 ms");
    assert_eq!(format_window_ms(8_000), "8 s");
    assert_eq!(format_window_ms(90_000), "1 min 30 s");
    assert_eq!(format_window_ms(120_000), "2 min");
    assert_eq!(format_window_ms(-1), "0 ms");
}

#[test]
fn bar_spec_change_defers_one_frame_and_shows_the_rebuild() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().flow_pane.tick_n = 100;
    app.active_tab_mut()
        .flow_pane
        .drawings
        .place(drawing_tool("horizontal-line"), ChartPoint::at(1.0, 100.0));

    app.active_tab_mut().apply_spec_changes();
    assert!(app.active_tab().loading.is_active(LoadingTask::BarRebuild));
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Tick(50),
        "the arming frame must paint the overlay before the rebuild runs"
    );

    app.active_tab_mut().apply_spec_changes();
    assert_eq!(app.active_tab().flow_pane.state.spec(), &BarSpec::Tick(100));
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a new bar partition re-anchors the marks, it does not drop them"
    );
    assert!(!app.active_tab().loading.is_active(LoadingTask::BarRebuild));
}

#[test]
fn a_still_moving_selector_keeps_deferring_the_rebuild() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().flow_pane.tick_n = 100;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().flow_pane.tick_n = 200; // the drag continues
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Tick(50),
        "no rebuild mid-gesture"
    );
    assert!(app.active_tab().loading.is_active(LoadingTask::BarRebuild));

    app.active_tab_mut().apply_spec_changes();
    assert_eq!(app.active_tab().flow_pane.state.spec(), &BarSpec::Tick(200));
    assert!(!app.active_tab().loading.is_active(LoadingTask::BarRebuild));
}

/// The corner appears only while the chart is not being fed, and it is a
/// corner: what the trader asked for in place of a card across the chart.
#[test]
fn the_corner_appears_only_while_the_chart_is_not_being_fed() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    assert!(
        app.control_feed_chip_rect().is_none(),
        "a chart with nothing wrong with it says nothing"
    );

    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    let chip = app
        .control_feed_chip_rect()
        .expect("a stalled feed shows the corner");
    assert!(
        chip.width() < 100.0 && chip.height() < 30.0,
        "the corner is a corner, not a card: {chip:?}"
    );
    // And the status line is reading the same report rather than the
    // socket's opinion.
    let stall = app
        .active_tab()
        .stall_at(&app.config, metrics::wall_clock_ms());
    assert!(
        app.feed_offline_accent(stall.as_ref()).is_some(),
        "the line has to know what the corner knows"
    );

    app.active_tab_mut().forced_stall = None;
    run_frame(&mut app, &ctx);
    assert!(
        app.control_feed_chip_rect().is_none(),
        "a feed that came back takes its corner with it"
    );
}

/// One sentence, once. The muted line and the popup carry the same
/// headline, and on an empty chart both of them draw in the same place —
/// which is how a capture caught the same words twice, a hand apart.
#[test]
fn the_empty_chart_never_says_the_same_thing_twice() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    let output = run_frame(&mut app, &ctx);
    let headline = app
        .active_tab()
        .stall_at(&app.config, metrics::wall_clock_ms())
        .expect("the stall is forced")
        .headline;

    let says_it = |output: &egui::FullOutput| {
        painted_text(output)
            .iter()
            .filter(|text| *text == &headline)
            .count()
    };
    assert_eq!(
        says_it(&output),
        1,
        "the empty pane explains itself once: {headline}"
    );

    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    let output = run_frame(&mut app, &ctx);
    assert!(app.control_feed_popup_open(), "the popup is up");
    assert_eq!(
        says_it(&output),
        1,
        "the popup takes the line's job rather than joining it: {headline}"
    );
}

/// A click on the chart puts the popup away. The rule is a table in
/// `feed_notice`; this proves the frame feeds it the right answer, which
/// means measuring the click against the two rectangles that were drawn.
#[test]
fn a_click_on_the_chart_puts_the_popup_away() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    assert!(app.control_feed_popup_open(), "the chip opened it");

    // Far from both rectangles: the popup grows up and left of the chip,
    // and this is the other side of the canvas.
    click_chart(
        &mut app,
        &ctx,
        egui::pos2(chip.left() - 600.0, chip.top() - 500.0),
    );
    assert!(
        !app.control_feed_popup_open(),
        "a click on the chart is a click somewhere else"
    );
    assert!(
        app.control_feed_chip_rect().is_some(),
        "and the corner itself stays, because the feed is still stalled"
    );
}

/// The mission's own rule: seeing no data is worse than not being
/// connected, so nothing the corner does may throw a chart away.
#[test]
fn nothing_the_corner_does_throws_a_chart_away() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    let ctx = egui::Context::default();
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1), trade(2), trade(3)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    let held = app.active_tab().flow_pane.state.trades().len();
    assert!(held > 0, "the chart has something to lose");

    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    let chip = app
        .control_feed_chip_rect()
        .expect("the corner is still up");
    click_chart(&mut app, &ctx, chip.center());
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.active_tab().flow_pane.state.trades().len(),
        held,
        "a stall, a chip and a popup are things to read, not things that reset a chart"
    );
}

/// A view following the live edge is already anchored to the newest bar,
/// whatever the rebuild does to the ones behind it.
#[test]
fn a_rebuild_leaves_a_live_view_at_the_live_edge() {
    let (mut app, _cmd_rx) = app_with_history(400);
    assert!(app.active_tab().flow_pane.viewport.follows_live());
    app.active_tab_mut().flow_pane.tick_n = 40;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert!(app.active_tab().flow_pane.viewport.follows_live());
    assert_eq!(
        app.active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        9.0
    );
}

/// Escape does not spend itself on the parked bar.
///
/// The escape stack unwinds gestures a trader left half-finished — a
/// draft, an armed order, a confirmation. Where the bar sits is not one
/// of those; it is a preference, and Escape is a key pressed many times
/// an hour to drop a selection. Undoing the parking with it would take
/// away, several times a session and unasked, the thing parking was for.
///
/// This is also the behaviour on `main`, reached from the other side:
/// there, Escape dropped the selection and `note_selection` discarded the
/// parked point with it. The point surviving is what is new — the press
/// still does exactly one thing.
#[test]
fn escape_drops_the_selection_and_leaves_the_parked_bar_parked() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let line = place_drawing(
        &mut app,
        &ctx,
        "horizontal-line",
        &[egui::pos2(700.0, 300.0)],
    );
    let other = place_drawing(
        &mut app,
        &ctx,
        "horizontal-line",
        &[egui::pos2(700.0, 460.0)],
    );
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(line));
    run_frame(&mut app, &ctx);
    let parked = egui::pos2(320.0, 240.0);
    app.surfaces
        .drawing_chrome
        .context_bar_mut()
        .set_manual(parked);
    app.surfaces.drawing_chrome.forget_context_bar_rect();
    run_frame(&mut app, &ctx);
    let drawn = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("the bar is up where it was put")
        .min;

    run_frame_with_events(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.drawing_pane().drawings.selected(),
        None,
        "the press does what the trader aimed it at: the selection goes"
    );
    assert_eq!(
        app.surfaces.drawing_chrome.context_bar().manual_position(),
        Some(parked),
        "and the position they chose is still theirs on the next object"
    );

    // Which is the whole claim: select a *different* object and the bar
    // opens where the hand left it, not beside the new one. Compared
    // against where it was actually drawn before the press rather than
    // against the parked point itself, so the test says "unchanged" and
    // not "happens to need no repair at this window size".
    app.drawing_pane_mut().drawings.select(Some(other));
    app.surfaces.drawing_chrome.forget_context_bar_rect();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.surfaces
            .drawing_chrome
            .context_bar_rect()
            .expect("the bar is back")
            .min,
        drawn,
        "the bar comes back parked, on another object, not beside it"
    );
}

/// The grip is the escape hatch when the bar sits over something the
/// trader needs to see, so it has to survive being used. It did not:
/// `on_the_bar` compared the press origin against the *current* rect,
/// which moves with the drag, so past ~20 px the origin fell outside and
/// the bar suppressed the very gesture moving it.
#[test]
fn dragging_the_bar_by_its_grip_does_not_make_it_vanish() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    run_frame(&mut app, &ctx);
    let before = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("the bar is on screen");

    // The grip is the leading cell; grab its middle and pull well past
    // the distance that used to break it.
    let grip = egui::pos2(before.left() + 9.0, before.center().y);
    drag_chart(&mut app, &ctx, grip, grip + egui::vec2(90.0, 60.0));
    run_frame(&mut app, &ctx);

    let after = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("the bar must survive its own drag");
    assert!(
        app.surfaces
            .drawing_chrome
            .context_bar()
            .manual_position()
            .is_some(),
        "the drag has to be recorded as a hand-placed position"
    );
    assert!(
        (after.left() - before.left()).abs() > 40.0,
        "the bar has to have actually travelled: {} -> {}",
        before.left(),
        after.left()
    );
}

/// The arrows speak the screen's language: upside down, ArrowUp still
/// moves the object up — which on an inverted chart means a lower price.
#[test]
fn arrow_nudges_follow_the_screen_when_the_chart_is_inverted() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
    let start = app.active_tab().flow_pane.drawings.items()[0].points[0];

    app.active_tab_mut().flow_pane.price_view.set_inverted(true);
    // One frame so the bands are re-carved with the new orientation —
    // the nudge reads the band the last frame published.
    run_frame(&mut app, &ctx);
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowUp)]);
    assert!(
        app.active_tab().flow_pane.drawings.items()[0].points[0].price < start.price,
        "upside down, up on screen is a lower price"
    );
}

/// The whole point, on a real frame: after a spec change with the view
/// panned into history, the chart is drawn — not a black rectangle.
#[test]
fn a_rebuilt_chart_still_paints_itself() {
    let (mut app, _cmd_rx) = app_with_history(400);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx); // one frame to settle the layout
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .viewport
        .pan_pixels(200.0 * 8.0, slots);

    app.active_tab_mut().flow_pane.tick_n = 40;
    let armed = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        armed.iter().any(|text| text.contains("rebuilding bars")),
        "the arming frame says what it is doing: {armed:?}"
    );

    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        has_price_axis(&texts),
        "the chart must be on screen after the rebuild: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("no bars in view")),
        "and it must be showing bars, not an empty window: {texts:?}"
    );
}

/// The trust law, held at the app level: one bar is one candle at every
/// zoom the squeeze can reach. A trader enters on a single bar of their
/// rule — a candle that could stand for several would poison all of them.
/// The 1 px floor still doubles what the old 2 px one showed, with nothing
/// merged to pay for it.
#[test]
fn squeezing_shows_more_bars_and_never_merges_any() {
    let (mut app, _cmd_rx) = app_with_history(4_000);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let slots = app.active_tab().flow_pane.slots();
    let bars_across = |viewport: &crate::viewport::Viewport| {
        let (start, end) = viewport.visible_range(800.0, slots);
        end - start
    };

    // Where zooming out used to stop.
    app.active_tab_mut().flow_pane.viewport.set_px_per_bar(2.0);
    let shallow = run_frame(&mut app, &ctx);
    let shallow_bars = bars_across(&app.active_tab().flow_pane.viewport);
    let shallow_rects = painted_rects(&shallow);

    // As far out as it now goes: twice the bars, each still its own candle.
    app.active_tab_mut()
        .flow_pane
        .viewport
        .set_px_per_bar(crate::viewport::MIN_PX_PER_BAR);
    let deep = run_frame(&mut app, &ctx);
    let deep_bars = bars_across(&app.active_tab().flow_pane.viewport);
    let deep_rects = painted_rects(&deep);
    let viewport = app.active_tab().flow_pane.viewport;
    // Not an exact 2x: `visible_range` is deliberately generous by a bar
    // at each edge, and the cushion must absorb that at both zooms.
    assert!(
        deep_bars + 4 >= 2 * shallow_bars,
        "history in the window: {deep_bars} vs {shallow_bars}"
    );
    assert!(
        (viewport.candle_width() - viewport.px_per_bar()).abs() < f32::EPSILON,
        "one bar, one candle, at the deepest squeeze"
    );
    // The law held at the *paint* level, where a fold would actually
    // happen. The chrome's rectangles are a constant, so the growth from
    // shallow to deep is the extra bars' candles alone: at least two
    // rectangles each (body fill + outline — a draw-side fold would erase
    // them), and boundedly few (a layer forgetting its zoom gate would
    // blow past any per-bar budget).
    let extra_bars = deep_bars - shallow_bars;
    let extra_rects = deep_rects.saturating_sub(shallow_rects);
    assert!(
        extra_rects >= 2 * extra_bars && extra_rects <= 8 * extra_bars,
        "each of the {extra_bars} extra bars drawn as its own candle: {extra_rects} extra rects"
    );
    assert!(
        !painted_text(&deep)
            .iter()
            .any(|text| text.contains("bars per candle") || text.contains("bars grouped")),
        "and nothing on the canvas claims otherwise"
    );
}

/// Pushing the chart left is how a projected channel or a Fibonacci
/// extension gets somewhere to be drawn, so the margin is a whole window of
/// empty canvas. It is also why that gesture can no longer empty the
/// window: the newest bar stops at the left edge instead of leaving through
/// it. ("no bars in view" stays as the renderer's guard for a window
/// emptied some other way — a rebuild re-cutting the series under it.)
#[test]
fn pushing_the_chart_left_clears_a_window_and_keeps_the_series_on_screen() {
    let (mut app, _cmd_rx) = app_with_history(400);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    app.active_tab_mut().flow_pane.viewport.zoom(8.0); // 64 px candles: only a dozen fit
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .viewport
        .pan_pixels(-10_000.0, slots); // as far into the empty future as it goes
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        !texts.iter().any(|text| text.contains("no bars in view")),
        "the series stays on screen however far left it is pushed: {texts:?}"
    );
    assert!(
        has_price_axis(&texts),
        "and keeps the axis, so the chart never reads as hung: {texts:?}"
    );
    let viewport = app.active_tab().flow_pane.viewport;
    let newest = (slots - 1) as f32;
    assert!(
        viewport.right_edge_bar(slots) > newest + 1.0,
        "with real empty canvas past the newest bar to draw into"
    );
    assert!(
        !viewport.follows_live(),
        "and the view is off the live edge"
    );
}

#[test]
fn candle_appearance_change_is_render_only() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    take_capture_start(&mut cmd_rx);
    let capture_epoch = app.active_tab().book_capture_epoch;
    let bar_spec = app.active_tab().flow_pane.state.spec().clone();

    app.style.candles = CandlePreset::OutlineOnly.style();
    app.style_revision = app.style_revision.saturating_add(1);
    app.emit_style_changed(Some(CandlePreset::OutlineOnly));

    assert_eq!(app.active_tab().flow_pane.state.spec(), &bar_spec);
    assert!(app.active_tab().tape().enabled());
    assert_eq!(app.active_tab().book_capture_epoch, capture_epoch);
    assert!(
        matches!(cmd_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
        "appearance changes must not restart or reconfigure market data"
    );
}

#[test]
fn fmt_time_in_utc() {
    // Epoch: 1970-01-01 00:00:00 UTC, then +1h 2m 3s.
    assert_eq!(fmt_time(0, TzOffset::new(0)), "00:00:00");
    assert_eq!(fmt_time(3_723_000, TzOffset::new(0)), "01:02:03");
}

/// The BARS selectors read the pane's own fields, so restoring the state
/// without them would give the trader a chart whose controls disagree with
/// it — and snap it back to a rule they never chose on first touch.
#[test]
fn a_restored_bar_rule_moves_the_selector_that_edits_it() {
    let (mut app, _commands) = app_with_history(50);
    app.restore_workspace(ui_state::Workspace::new(
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
            flow_bars: "tick:377".to_owned(),
            time_bars: None,
            flow_legend_collapsed: false,
            time_legend_collapsed: false,
        }],
        None,
    ));
    let pane = &app.active_tab().flow_pane;
    assert_eq!(pane.state.spec(), &BarSpec::Tick(377));
    assert_eq!(pane.tick_n, 377, "the selector moved with the rule");
    assert_eq!(pane.kind, crate::state::BarKind::Tick);
}

/// The interval a reply carries is tagged rather than assumed. A base this
/// fold was not written for is refused, not folded wrongly.
#[test]
fn a_reply_at_an_unexpected_base_interval_is_refused() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);

    events
        .try_send(FeedEvent::OhlcvHistory {
            // Five-minute candles, from a venue that one day changes its
            // mind about what it serves.
            interval_ms: 5 * crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        0,
        "bars at an unknown base are not folded as if they were minutes"
    );
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "the reply still answered the request"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(has_price_axis(&texts), "and the pane draws: {texts:?}");
}

#[test]
fn fmt_time_applies_the_offset() {
    // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
    assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
    // UTC midnight in UTC+05:30 is 05:30.
    assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
}
