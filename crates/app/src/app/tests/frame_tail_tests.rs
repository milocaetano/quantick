use super::*;

const WINDOW: egui::Vec2 = egui::vec2(2000.0, 1200.0);

pub(super) fn print(id: u64, price: i64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        agg_id: id,
        timestamp_ms: 1_700_000_000_000 + id as i64 * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    }
}

fn frame(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    spawn: &mut crate::tab::LiveFeedSpawn<'_>,
    late: bool,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW)),
            events,
            ..Default::default()
        },
        |ctx| {
            if late {
                use quantick_chart_interaction::frame_tail_plan::FrameTailStage::*;
                app.draw_frame_test_order(
                    ctx,
                    Instant::now(),
                    spawn,
                    [
                        SettlePaperPanels,
                        DrawPaperReport,
                        ApplyNoticeAction,
                        PublishFeedPopup,
                    ],
                );
            } else {
                app.draw_frame_test_order(
                    ctx,
                    Instant::now(),
                    spawn,
                    quantick_chart_interaction::frame_tail_plan::FrameTailPlan::stages(),
                );
            }
        },
    )
}

fn report_text(ctx: &egui::Context, output: &egui::FullOutput) -> Vec<String> {
    let area = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Simulated performance")))
        .expect("the real report window is visible");
    assert!(area.is_positive(), "actual report area: {area:?}");
    fn collect(shape: &egui::Shape, area: egui::Rect, clip: egui::Rect, found: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text)
                if area.contains(text.pos) && clip.contains_rect(text.visual_bounding_rect()) =>
            {
                found.push(text.galley.text().to_owned());
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, area, clip, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, area, shape.clip_rect, &mut found);
    }
    assert!(found.iter().any(|text| text == "Simulated performance"));
    found
}

fn reload_case(late: bool, failed: bool) -> egui::Rect {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let dir = crate::scratch::ScratchDir::new("frame-tail-reload");
    let calls = std::cell::Cell::new(0);
    let mut endpoints = Vec::new();
    let mut spawn =
        |provider, symbol: &str, _settings: &quantick_feed::config::MetaTraderSettings, _cache| {
            assert_eq!(provider, ProviderKind::Binance);
            assert_eq!(symbol, "TESTUSDT");
            calls.set(calls.get() + 1);
            let (events, event_rx) = mpsc::channel(64);
            let (book, book_rx) = mpsc::channel(64);
            let (commands, command_rx) = mpsc::channel(16);
            endpoints.push((events, book, command_rx));
            FeedHandle {
                events: event_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                latency: feed::unsplit_latency(),
                commands,
                replay: None,
            }
        };
    let _ = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    let drawing;
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let rectangle = drawing_tool("rectangle");
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(0.0, 90.0));
        pane.drawings
            .place(rectangle, drawings::ChartPoint::at(60.0, 120.0));
        drawing = pane.drawings.items().last().unwrap().id;
    }
    let form = crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    app.tabs
        .runtime_mut(app.tabs.active_index())
        .arm_strategy_instance(
            &mut *app.audio.alerts,
            pane::PaneSide::Flow,
            drawing,
            &form,
            "tail reset".to_owned(),
        )
        .unwrap();
    {
        let symbol = app.active_tab().symbol.clone();
        let paper = &mut app.active_tab_mut().paper;
        paper.redirect_history_dir(dir.path().to_path_buf());
        if failed {
            std::fs::write(dir.path().join(&symbol), b"not a directory").unwrap();
        }
        paper.set_symbol(&symbol);
        paper.seed(&print(0, 100));
        paper.market(quantick_engine::Side::Buy);
        paper.on_trade(&print(1, 100));
        paper.on_trade(&print(2, 105));
        assert!(paper.position_summary().is_some());
        let (report, env) = paper.report_parts();
        report.set_report_list_open(true);
        report.open(&env);
    }
    app.active_tab_mut().notice =
        FeedNotice::attention("Fixture transport is paused", "Choose how to recover.");
    app.chrome.feed_popup_tab = Some(app.tabs.active_id());
    let mut output = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    for _ in 0..2 {
        output = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    }
    assert!(
        report_text(&ctx, &output)
            .iter()
            .any(|s| s == "No saved trades for this filter.")
    );
    let before_click = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Simulated performance")))
        .unwrap();
    let at = painted_text_center(&output, "Reload").expect("actual popup Reload button");
    let click = vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let clicked = frame(&mut app, &ctx, click, &mut spawn, late);
    assert!(app.active_tab().paper.position_summary().is_none());
    assert_eq!(app.chrome.feed_popup_tab, None);
    assert!(matches!(
        app.active_tab()
            .flow_pane
            .strategies
            .anchors
            .for_drawing(drawing)
            .unwrap()
            .armed
            .state(),
        quantick_strategy::ArmedState::Disarmed {
            reason: quantick_strategy::DisarmReason::TimelineReset
        }
    ));
    let saved = quantick_paper::report::load_history(dir.path(), Some("TESTUSDT"), &[]);
    assert_eq!(saved.rows.len(), usize::from(!failed));
    if !failed {
        let closed = &saved.rows[0].trade;
        assert_eq!(closed.entry_price, Decimal::from(100));
        assert_eq!(closed.exit_price, Decimal::from(105));
        assert_eq!(closed.closed_ms, print(2, 105).timestamp_ms);
        assert_eq!(closed.exit_reason.as_str(), "reset");
    }
    let visible = report_text(&ctx, &clicked);
    if late || failed {
        assert!(
            visible
                .iter()
                .any(|s| s == "No saved trades for this filter.")
        );
        assert!(!visible.iter().any(|s| s == "100 \u{2192} 105"));
    } else {
        assert!(
            visible.iter().any(|s| s == "TRADES BEHIND THIS CURVE"),
            "{visible:?}"
        );
        assert!(
            visible.iter().any(|s| s == "100 \u{2192} 105"),
            "actual saved round trip is painted: {visible:?}"
        );
        assert!(
            visible.iter().any(|s| s == "reset"),
            "actual reset reason is painted"
        );
        assert!(
            !visible
                .iter()
                .any(|s| s == "No saved trades for this filter.")
        );
    }
    assert_eq!(
        app.active_tab()
            .paper
            .report_state()
            .snapshot()
            .unwrap()
            .rows
            .len(),
        usize::from(!late && !failed)
    );
    if failed {
        assert!(
            app.surfaces
                .toast
                .message()
                .unwrap()
                .contains("could not save")
        );
    }
    let following = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    let following_text = report_text(&ctx, &following);
    assert_eq!(
        following_text.iter().any(|s| s == "100 \u{2192} 105"),
        !failed
    );
    assert_eq!(
        app.active_tab()
            .paper
            .report_state()
            .snapshot()
            .unwrap()
            .rows
            .len(),
        usize::from(!failed)
    );
    assert_eq!(calls.get(), 1);
    before_click
}

#[test]
fn frame_tail_reload_paints_the_close_now_and_the_same_interpreter_mutant_paints_it_late() {
    let canonical = reload_case(false, false);
    let mutant = reload_case(true, false);
    assert_eq!(canonical, mutant, "identical pre-click report geometry");
}

#[test]
fn frame_tail_failed_reset_write_shows_no_saved_row_or_healthy_toast() {
    reload_case(false, true);
}

#[test]
fn frame_tail_recovery_refusals_never_spawn_or_reset_and_reconnect_keeps_the_position() {
    for refused in ["replay", "missing-provider", "reconnect"] {
        let (mut app, _commands) = app_with_history(50);
        let dir = crate::scratch::ScratchDir::new("tail-recovery-outcomes");
        let paper = &mut app.active_tab_mut().paper;
        paper.redirect_history_dir(dir.path().to_path_buf());
        paper.seed(&print(0, 100));
        paper.market(quantick_engine::Side::Buy);
        paper.on_trade(&print(1, 100));
        let tape_before = app.active_tab().flow_pane.state.trades().len();
        if refused == "replay" {
            app.active_tab_mut().replay = Some(quantick_feed::replay::test_support::detached_link(
                recording_at(&dir.join("recording")),
            ));
        } else if refused == "missing-provider" {
            app.active_tab_mut().feed_id = "not-configured".to_owned();
        }
        let calls = std::cell::Cell::new(0);
        let mut endpoints = Vec::new();
        let mut spawn = |_, _: &str, _: &quantick_feed::config::MetaTraderSettings, _| {
            calls.set(calls.get() + 1);
            let (tx, rx) = mpsc::channel(64);
            let (book, books) = mpsc::channel(64);
            let (commands, command_rx) = mpsc::channel(16);
            endpoints.push((tx, book, command_rx));
            FeedHandle {
                events: rx,
                book_events: books,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                latency: feed::unsplit_latency(),
                commands,
                replay: None,
            }
        };
        let (tab, config) = app.active_with_config();
        if refused == "reconnect" {
            assert!(tab.reconnect_feed_with_spawn(config, &mut spawn));
        } else {
            assert!(!tab.reload_feed_with_spawn(config, &mut spawn));
            assert!(!tab.reconnect_feed_with_spawn(config, &mut spawn));
        }
        assert_eq!(calls.get(), usize::from(refused == "reconnect"));
        assert!(app.active_tab().paper.position_summary().is_some());
        assert_eq!(app.active_tab().flow_pane.state.trades().len(), tape_before);
        assert!(app.active_tab().paper.session_trades().is_empty());
        assert!(
            !app.active_tab_mut()
                .paper
                .account_mut()
                .take_journal_changed()
        );
    }
}

#[test]
fn frame_tail_a_closed_report_consumes_the_hint_and_reads_the_saved_close_on_open() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let dir = crate::scratch::ScratchDir::new("tail-closed-report");
    let paper = &mut app.active_tab_mut().paper;
    paper.redirect_history_dir(dir.path().to_path_buf());
    paper.seed(&print(0, 100));
    paper.market(quantick_engine::Side::Buy);
    paper.on_trade(&print(1, 100));
    paper.on_trade(&print(2, 105));
    assert!(!paper.report_state().is_open());
    paper.on_timeline_reset();
    paper.settle();
    assert!(!paper.account_mut().take_journal_changed());
    assert!(paper.report_state().snapshot().is_none());
    let (report, env) = paper.report_parts();
    report.open(&env);
    let output = frame(
        &mut app,
        &ctx,
        Vec::new(),
        &mut |_, _, _, _| panic!("None action must not spawn"),
        false,
    );
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
    // The first opening may be a sizing frame; the view itself must already be cut.
    assert!(!output.shapes.is_empty());
}

#[test]
fn frame_tail_none_settles_every_tab_and_preserves_toast_priority() {
    use quantick_chart_interaction::frame_tail_plan::FrameTailPlan;
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let (other, _events, _commands2, _book) = test_app();
    let (third, _events3, _commands3, _book3) = test_app();
    let opening = app.tabs.plan_open();
    app.tabs.append(opening, other.tabs.into_single_runtime());
    let opening = app.tabs.plan_open();
    app.tabs.append(opening, third.tabs.into_single_runtime());
    app.tabs.select(0);
    app.tabs.runtime_mut(1).symbol = "BACKGROUND".to_owned();
    let run = |app: &mut QuantickApp| {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mut chip = None;
            let mut popup = None;
            super::super::frame_tail::FrameTailOwners {
                tabs: &mut app.tabs,
                config: &app.config,
                toast: &mut app.surfaces.toast,
                chip_rect: &mut chip,
                popup_tab: &mut popup,
            }
            .execute(
                super::super::frame_tail::FrameTailInput {
                    ctx,
                    now: Instant::now(),
                    tz: app.tz,
                    notice_action: feed_notice::NoticeAction::None,
                    popup_tab: 7,
                    popup_open: true,
                    chip_clicked: false,
                    dismissed: false,
                    chip_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(20.0, 20.0),
                    )),
                },
                &mut |_, _, _, _| panic!("None action must not spawn"),
                FrameTailPlan::stages(),
            );
            assert!(chip.is_some());
            assert_eq!(popup, Some(7));
        });
    };
    for (index, tab) in app.tabs.iter_mut().enumerate() {
        tab.paper.show_toast(format!("message{index}"));
    }
    run(&mut app);
    assert_eq!(app.surfaces.toast.message(), Some("message0"));
    assert!(
        app.tabs
            .iter_mut()
            .all(|tab| tab.paper.take_toast().is_none())
    );
    app.tabs.runtime_mut(1).paper.show_toast("first".to_owned());
    app.tabs
        .runtime_mut(2)
        .paper
        .show_toast("second".to_owned());
    run(&mut app);
    assert_eq!(
        app.surfaces.toast.message(),
        Some("BACKGROUND \u{00b7} first")
    );
}
