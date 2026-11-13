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
    let now = frame_instant(ctx);
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
                    now,
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
                    now,
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
    pin_frame_clock(&ctx);
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
    app.runtime
        .tabs
        .runtime_mut(app.runtime.tabs.active_index())
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
    app.active_tab_mut().link.notice =
        FeedNotice::attention("Fixture transport is paused", "Choose how to recover.");
    app.surfaces
        .feed_notice
        .update(quantick_ui_model::feed_notice::Intent::InitializeOpen(
            app.runtime.tabs.active_id(),
        ));
    for _ in 0..3 {
        let _ = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    }
    // The report and the popup's chip move with the book worker's
    // publication: land it before reading either, and click where two
    // consecutive frames agree the button is.
    settle_workers(&mut app);
    let mut output = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    assert!(
        report_text(&ctx, &output)
            .iter()
            .any(|s| s == "No saved trades for this filter.")
    );
    let before_click = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Simulated performance")))
        .unwrap();
    let at = painted_text_center(&output, "Reload").expect("actual popup Reload button");
    output = frame(&mut app, &ctx, Vec::new(), &mut spawn, late);
    assert_eq!(
        painted_text_center(&output, "Reload"),
        Some(at),
        "the Reload button holds still before the click"
    );
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
    assert_eq!(app.surfaces.feed_notice.model_state.open(), None);
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
                .model_state
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
            app.active_tab_mut().link.replay =
                Some(quantick_feed::replay::test_support::detached_link(
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
    let opening = app.runtime.tabs.plan_open();
    app.runtime
        .tabs
        .append(opening, other.runtime.tabs.into_single_runtime());
    let opening = app.runtime.tabs.plan_open();
    app.runtime
        .tabs
        .append(opening, third.runtime.tabs.into_single_runtime());
    app.runtime.tabs.select(0);
    app.runtime.tabs.runtime_mut(1).symbol = "BACKGROUND".to_owned();
    let run = |app: &mut QuantickApp| {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mut chip = None;
            app.surfaces
                .feed_notice
                .update(quantick_ui_model::feed_notice::Intent::InitializeOpen(7));
            super::super::frame_tail::FrameTailOwners {
                tabs: &mut app.runtime.tabs,
                config: &app.config,
                surfaces: &mut app.surfaces,
                chip_rect: &mut chip,
            }
            .execute(
                super::super::frame_tail::FrameTailInput {
                    presentation: Some(ctx),
                    now: Instant::now(),
                    tz: app.runtime.tz,
                    notice_action: feed_notice::NoticeAction::None,
                    popup_tab: 7,
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
            assert_eq!(app.surfaces.feed_notice.model_state.open(), Some(7));
        });
    };
    for (index, tab) in app.runtime.tabs.iter_mut().enumerate() {
        tab.paper.show_toast(format!("message{index}"));
    }
    run(&mut app);
    assert_eq!(app.surfaces.toast.model_state.message(), Some("message0"));
    assert!(
        app.runtime
            .tabs
            .iter_mut()
            .all(|tab| tab.paper.take_toast().is_none())
    );
    app.runtime
        .tabs
        .runtime_mut(1)
        .paper
        .show_toast("first".to_owned());
    app.runtime
        .tabs
        .runtime_mut(2)
        .paper
        .show_toast("second".to_owned());
    run(&mut app);
    assert_eq!(
        app.surfaces.toast.model_state.message(),
        Some("BACKGROUND \u{00b7} first")
    );
}

#[test]
fn feed_notice_settles_only_for_the_presented_view() {
    use quantick_chart_interaction::frame_tail_plan::FrameTailStage;
    use quantick_ui_model::feed_notice::Intent;
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let tab = app.runtime.tabs.active_id();
    app.surfaces.feed_notice.update(Intent::InitializeOpen(tab));
    let chip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 20.0));
    app.chrome.feed_chip_rect = Some(chip);
    for presented in [false, true] {
        super::super::frame_tail::FrameTailOwners {
            tabs: &mut app.runtime.tabs,
            config: &app.config,
            surfaces: &mut app.surfaces,
            chip_rect: &mut app.chrome.feed_chip_rect,
        }
        .execute(
            super::super::frame_tail::FrameTailInput {
                presentation: presented.then_some(&ctx),
                now: Instant::now(),
                tz: app.runtime.tz,
                notice_action: feed_notice::NoticeAction::None,
                popup_tab: tab,
                chip_clicked: false,
                dismissed: true,
                chip_rect: None,
            },
            &mut |_, _, _, _| panic!("popup publication must not execute recovery"),
            [FrameTailStage::PublishFeedPopup],
        );
        assert_eq!(
            app.surfaces.feed_notice.model_state.open(),
            (!presented).then_some(tab)
        );
        assert_eq!(app.chrome.feed_chip_rect, (!presented).then_some(chip));
        assert_eq!(app.chrome_reads().feed_popup_open(), !presented);
    }
}

#[test]
fn paper_tail_registered_second_row_settles_after_borrow_before_next_stage() {
    use quantick_chart_interaction::frame_tail_plan::FrameTailStage::*;
    use quantick_ui_model::paper_tail_probe::Intent;
    const REFUSAL: &str =
        "This trade is outside the bars on the flow chart - nothing to centre on.";
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    let (other, _events, _commands2, _book) = test_app();
    let opening = app.runtime.tabs.plan_open();
    app.runtime
        .tabs
        .append(opening, other.runtime.tabs.into_single_runtime());
    app.runtime.tabs.select(0);
    app.presentation.context = Some(ctx.clone());
    app.surfaces.dock.restore(false, None);
    assert!(!app.active_tab().paper.report_state().is_open());
    app.runtime
        .tabs
        .runtime_mut(1)
        .paper
        .show_toast("background untouched".to_owned());
    let arm = |app: &mut QuantickApp, mixed| {
        app.surfaces.paper_tail_probe.update(Intent::Arm {
            opened: -20_000,
            closed: -10_000,
            mixed,
        });
    };
    arm(&mut app, false);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        assert!(app.run_report_tail_test(ctx, [DrawPaperReport]).is_empty());
    });
    assert_eq!(
        app.surfaces.paper_tail_probe.model_state.report_open,
        Some(false)
    );
    assert_eq!(app.surfaces.paper_tail_probe.model_state.visits, 1);
    assert_eq!(
        app.active_tab_mut().paper.take_toast().as_deref(),
        Some(REFUSAL)
    );
    assert!(
        app.active_tab_mut().paper.take_toast().is_none(),
        "the supported request settles once"
    );
    assert_eq!(
        app.runtime
            .tabs
            .runtime_mut(1)
            .paper
            .take_toast()
            .as_deref(),
        Some("background untouched")
    );
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        assert!(app.run_report_tail_test(ctx, [DrawPaperReport]).is_empty());
    });
    assert_eq!(
        app.surfaces.paper_tail_probe.model_state.visits, 1,
        "unarmed row stays quiet"
    );
    assert!(app.active_tab_mut().paper.take_toast().is_none());

    // In the actual production order the refusal waits for the existing later
    // settlement. A reordered test iterator proves settlement precedes its next item.
    arm(&mut app, false);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        assert!(
            app.run_report_tail_test(ctx, [SettlePaperPanels, DrawPaperReport])
                .is_empty()
        );
    });
    assert_ne!(app.surfaces.toast.model_state.message(), Some(REFUSAL));
    assert_eq!(
        app.active_tab_mut().paper.take_toast().as_deref(),
        Some(REFUSAL)
    );
    arm(&mut app, false);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        assert!(
            app.run_report_tail_test(ctx, [DrawPaperReport, SettlePaperPanels])
                .is_empty()
        );
    });
    assert_eq!(
        app.surfaces.toast.model_state.message(),
        Some(REFUSAL),
        "the next stage sees host settlement"
    );
    assert!(app.active_tab_mut().paper.take_toast().is_none());

    arm(&mut app, false);
    let visits = app.surfaces.paper_tail_probe.model_state.visits;
    app.presentation
        .rebind(
            &app.runtime,
            egui::ViewportId::from_hash_of("paper-tail-elsewhere"),
        )
        .unwrap();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        assert!(app.run_report_tail_test(ctx, [DrawPaperReport]).is_empty());
    });
    assert_eq!(app.surfaces.paper_tail_probe.model_state.visits, visits);
    assert!(
        app.surfaces.paper_tail_probe.model_state.armed.is_some(),
        "presentation absence never consumes input"
    );
    assert!(app.active_tab_mut().paper.take_toast().is_none());
}

#[test]
fn paper_tail_unsupported_mixed_output_preserves_the_complete_response() {
    use crate::surfaces::replay_browser::ReplayBrowserExt as _;
    use crate::surfaces::{PaperPanelEffects, SurfaceResponse};
    use quantick_chart_interaction::frame_tail_plan::FrameTailStage::DrawPaperReport;
    use quantick_ui_model::paper_tail_probe::Intent;
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.presentation.context = Some(ctx.clone());
    app.active_tab_mut()
        .paper
        .show_toast("report refusal".to_owned());
    app.surfaces.paper_tail_probe.update(Intent::Arm {
        opened: -20_000,
        closed: -10_000,
        mixed: true,
    });
    let mut retained = Vec::new();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        retained = app.run_report_tail_test(ctx, [DrawPaperReport]);
    });
    let expected = SurfaceResponse {
        paper_panel: Some(Box::new(PaperPanelEffects {
            navigate_to_trade: Some((-20_000, -10_000)),
            ..Default::default()
        })),
        undo_drawing: true,
        ..Default::default()
    };
    assert_eq!(retained, vec![expected]);
    assert_eq!(
        app.surfaces.paper_tail_probe.model_state.visits, 1,
        "direct model mutation is retained, not rolled back"
    );
    assert_eq!(
        app.active_tab_mut().paper.take_toast().as_deref(),
        Some("report refusal"),
        "unsupported output cannot overwrite the report toast"
    );
    assert!(!app.surfaces.replay_browser.browser_is_open());
    let response = SurfaceResponse {
        paper_panel: Some(Box::new(PaperPanelEffects {
            open_replay_browser: true,
            navigate_to_trade: Some((-20_000, -10_000)),
            ..Default::default()
        })),
        undo_drawing: true,
        ..Default::default()
    };
    let retained = app.settle_paper_response_test(response).unwrap_err();
    assert!(retained.undo_drawing);
    assert!(retained.paper_panel.as_ref().unwrap().open_replay_browser);
    assert_eq!(
        retained.paper_panel.as_ref().unwrap().navigate_to_trade,
        Some((-20_000, -10_000))
    );
    assert!(
        !app.surfaces.replay_browser.browser_is_open(),
        "validation precedes the prefix too"
    );
    assert!(app.active_tab_mut().paper.take_toast().is_none());
}
