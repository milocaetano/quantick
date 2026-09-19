//! The registered frame and drain orders, replayed through the real stage
//! adapters with one declared dependency hoisted: each swap does visible
//! harm, which is what the declaration exists to refuse.

use super::*;
use quantick_chart_interaction::frame_plan::FrameStage;
use quantick_chart_interaction::stage_registry::hoisted;
use quantick_chart_interaction::tab_drain_plan::TabDrainStage;

fn dense_prints(start: u64, end: u64) -> Vec<quantick_engine::Trade> {
    (start..end).map(trade).collect()
}

const WINDOW: egui::Vec2 = egui::vec2(1600.0, 1000.0);

fn position<S: PartialEq>(order: &[S], stage: S) -> usize {
    order
        .iter()
        .position(|candidate| *candidate == stage)
        .expect("every stage is registered")
}

/// `order` with `later` moved directly in front of `earlier`, refused by
/// the registry before it is replayed.
fn frame_hoisting(earlier: FrameStage, later: FrameStage) -> [FrameStage; FrameStage::COUNT] {
    let order = FrameStage::ORDER;
    let moved = hoisted(order, position(&order, earlier), position(&order, later));
    assert!(
        !FrameStage::is_valid_order(&moved),
        "{later:?} declares {earlier:?}"
    );
    moved
}

fn staged_frame(app: &mut QuantickApp, ctx: &egui::Context, order: &[FrameStage]) {
    let _ = staged_frame_at(app, ctx, order, Instant::now());
}

/// `order` with `earlier` moved directly behind `later`, refused by the
/// registry before it is replayed. The mirror of [`frame_hoisting`], for a
/// prerequisite whose neighbours declare nothing on it: only the one
/// declaration under test can refuse the move.
fn frame_deferring(earlier: FrameStage, later: FrameStage) -> [FrameStage; FrameStage::COUNT] {
    let order = FrameStage::ORDER;
    let mut moved = order;
    moved[position(&order, earlier)..=position(&order, later)].rotate_left(1);
    assert!(
        !FrameStage::is_valid_order(&moved),
        "{later:?} declares {earlier:?}"
    );
    moved
}

fn staged_frame_at(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    order: &[FrameStage],
    now: Instant,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW)),
            ..Default::default()
        },
        |ctx| {
            app.draw_frame_stage_test_order(
                ctx,
                now,
                &mut quantick_feed::spawn_live,
                order.iter().copied(),
            );
        },
    )
}

fn lowest_pane_edge(app: &QuantickApp) -> f32 {
    app.active_tab()
        .panes()
        .filter_map(|(pane, _)| pane.frame.area)
        .map(|area| area.bottom())
        .fold(f32::MIN, f32::max)
}

#[test]
fn the_canvas_before_the_status_line_paints_under_the_bottom_chrome() {
    let measure = |order: &[FrameStage]| {
        let (mut app, _events, _commands, _book) = test_app();
        let ctx = egui::Context::default();
        staged_frame(&mut app, &ctx, order);
        staged_frame(&mut app, &ctx, order);
        lowest_pane_edge(&app)
    };
    let registered = measure(&FrameStage::ORDER);
    let hoisted = measure(&frame_hoisting(FrameStage::StatusLine, FrameStage::Canvas));
    assert!(
        registered < WINDOW.y && hoisted > registered,
        "the chart pays for the status line only when it is laid out after it: \
         registered bottom {registered}, hoisted bottom {hoisted}"
    );
}

#[test]
fn the_tail_before_the_canvas_never_publishes_the_offline_corner() {
    let corner = |order: &[FrameStage]| {
        let (mut app, _events, _commands, _book) = test_app();
        let ctx = egui::Context::default();
        app.active_tab_mut().forced_stall = Some(quantick_feed::stall::ForcedStall::Silent);
        staged_frame(&mut app, &ctx, order);
        staged_frame(&mut app, &ctx, order);
        app.chrome_reads().feed_chip_rect()
    };
    assert!(
        corner(&FrameStage::ORDER).is_some(),
        "a stalled feed shows the corner"
    );
    assert_eq!(
        corner(&frame_hoisting(FrameStage::Canvas, FrameStage::Tail)),
        None,
        "published before the canvas answered, the corner is lost every frame"
    );
}

#[test]
fn the_capture_heartbeat_before_the_source_drain_leaves_a_reset_market_unrecorded() {
    let order = TabDrainStage::ORDER;
    let premature = hoisted(
        order,
        position(&order, TabDrainStage::ReceiveSource),
        position(&order, TabDrainStage::BookCaptureHeartbeat),
    );
    assert!(!TabDrainStage::is_valid_order(&premature));
    let recording_after_reset = |order: &[TabDrainStage]| {
        let (mut app, events, mut commands, _book) = test_app();
        let _ = take_capture_start(&mut commands);
        assert!(app.active_tab().tape().enabled());
        events.try_send(FeedEvent::Reset).unwrap();
        let tab_id = app.tabs.active_id();
        let policy = crate::tab::HistoryPolicy {
            progressive: app.history.progressive_history,
            reach: app.history.history_reach,
            reach_span_minutes: app.history.history_reach_span_minutes,
            venue_lead_in: app.history.venue_lead_in,
        };
        with_config(&mut app, |tab, config| {
            tab.drain_frame_test_order(tab_id, config, policy, order.iter().copied());
        });
        app.active_tab().tape().enabled()
    };
    assert!(
        recording_after_reset(&order),
        "the heartbeat restarts capture on the frame the reset stopped it"
    );
    assert!(
        !recording_after_reset(&premature),
        "run first, the heartbeat sees a recorder the reset has yet to stop"
    );
}

#[test]
fn the_note_hook_before_the_expiry_draws_a_frame_with_an_empty_lane() {
    let ending = quantick_feed::history_reach::CampaignEnd::ALL
        .into_iter()
        .find(|end| end.notice().is_some())
        .expect("an ending with words");
    let note_after_frame = |order: &[FrameStage]| {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(12);
        run_frame(&mut app, &ctx);
        app.active_tab_mut()
            .raise_history_note(ending.notice().expect("chosen for its words"));
        app.chrome.harness.arm_history_note(ending, 8);
        // The frame on which the held note has outlived its linger.
        let past_linger = Instant::now() + crate::tab::HISTORY_NOTE_LINGER;
        staged_frame_at(&mut app, &ctx, order, past_linger);
        app.active_tab().history_note().is_some()
    };
    assert!(
        note_after_frame(&FrameStage::ORDER),
        "the hook re-raises the note the expiry just swept"
    );
    assert!(
        !note_after_frame(&frame_hoisting(
            FrameStage::ExpireHistoryNotes,
            FrameStage::HistoryNoteHook
        )),
        "run first, the hook finds the note still up and the expiry then \
         leaves the lane empty for the frame"
    );
}

#[test]
fn the_pinned_inspector_before_the_demo_misses_the_selection_it_made() {
    let canvas_width_on_the_demo_frame = |order: &[FrameStage]| {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(500);
        app.workspace
            .set_ui_state_path(scratch_ui_state("stage-order-demo-inspector"));
        app.drawings.chrome.set_inspector_pinned(true);
        app.drawings.chrome.set_inspector_pin_touched(true);
        app.drawings
            .chrome
            .demos_mut()
            .arm_drawings_demo(DrawingsDemo::default());
        for _ in 0..8 {
            staged_frame(&mut app, &ctx, order);
            if !app.drawings.chrome.demos().gallery_requested() {
                break;
            }
        }
        assert!(
            !app.drawings.chrome.demos().gallery_requested(),
            "the demo ran"
        );
        assert!(
            app.active_tab()
                .drawing_pane()
                .drawings
                .selected()
                .is_some(),
            "and selected what it placed"
        );
        app.active_tab()
            .drawing_pane()
            .frame
            .chart_area
            .expect("the canvas drew")
            .width()
    };
    let registered = canvas_width_on_the_demo_frame(&FrameStage::ORDER);
    let deferred = canvas_width_on_the_demo_frame(&frame_deferring(
        FrameStage::ScenarioHooks,
        FrameStage::PinnedInspector,
    ));
    assert!(
        registered < deferred,
        "the pinned panel shows the demo's selection on the frame it was made \
         only when it is drawn after the demo: registered {registered}, \
         deferred {deferred}"
    );
}

/// #480 A3 for the registries this branch added, counted rather than timed:
/// walking the frame and tab-drain plans allocates nothing, and a dense tab
/// intake driven by its plan does exactly the heap work of the same seven
/// stages listed by hand.
#[test]
fn the_frame_and_tab_drain_plans_add_no_heap_work_to_a_dense_intake() {
    use TabDrainStage::*;
    use quantick_chart_interaction::frame_plan::FramePlan;
    use quantick_chart_interaction::tab_drain_plan::TabDrainPlan;
    const BY_HAND: [TabDrainStage; 7] = [
        ReceiveSource,
        ApplyIndicatorResults,
        ReceiveBook,
        ReceiveNotices,
        BookCaptureHeartbeat,
        MirrorHistoryPolicy,
        PollCandleHistory,
    ];
    let before = crate::work_meter::tally();
    for _ in 0..1_000 {
        for stage in FramePlan::stages() {
            std::hint::black_box(stage);
        }
        for stage in TabDrainPlan::stages() {
            std::hint::black_box(stage);
        }
    }
    let walk = crate::work_meter::tally().since(before);
    assert_eq!((walk.allocs, walk.reallocs), (0, 0), "{walk:?}");

    let intake_work = |planned: bool| {
        let (mut app, events, _commands, _book) = test_app();
        let tab_id = app.tabs.active_id();
        events
            .try_send(FeedEvent::Backfilled(dense_prints(1, 8_001)))
            .unwrap();
        app.active_tab_mut().drain_feed(tab_id);
        events
            .try_send(FeedEvent::LiveBatch(dense_prints(8_001, 8_513)))
            .unwrap();
        let slots = app.active_tab().flow_pane.slots();
        let policy = crate::tab::HistoryPolicy {
            progressive: app.history.progressive_history,
            reach: app.history.history_reach,
            reach_span_minutes: app.history.history_reach_span_minutes,
            venue_lead_in: app.history.venue_lead_in,
        };
        let before = crate::work_meter::tally();
        with_config(&mut app, |tab, config| {
            if planned {
                tab.drain_frame(tab_id, config, policy);
            } else {
                tab.drain_frame_test_order(tab_id, config, policy, BY_HAND);
            }
        });
        let work = crate::work_meter::tally().since(before);
        assert!(slots > 80, "a dense series before the live batch");
        assert!(
            app.active_tab().flow_pane.slots() > slots,
            "the live batch landed in the intake being counted"
        );
        work
    };
    // One discarded run, so a lazily initialised global is not charged to
    // whichever variant happens to run first.
    intake_work(true);
    let planned = intake_work(true);
    let by_hand = intake_work(false);
    assert_eq!(
        (planned.allocs, planned.alloc_bytes, planned.reallocs),
        (by_hand.allocs, by_hand.alloc_bytes, by_hand.reallocs),
        "planned {planned:?} against by-hand {by_hand:?}"
    );
}

#[test]
fn the_mark_before_the_trace_loads_is_injected_back_as_a_second_mark() {
    use quantick_feed::replay::test_support as replay_test_support;
    let scenario_marks = |order: &[FrameStage]| {
        let ctx = egui::Context::default();
        let dir = crate::scratch::ScratchDir::new("stage-order-trace-mark");
        // A recording whose sidecar already holds one traced mark.
        let (mut app, _commands) = app_with_history(12);
        app.active_tab_mut().replay = Some(replay_test_support::detached_link(recording_at(&dir)));
        hover_bar(&mut app, &ctx, 6);
        app.take_mark(Some("recorded".to_owned()));
        drop(app);

        // Replayed again, with a scenario mark due on its first frame.
        let (mut app, _commands) = app_with_history(12);
        app.active_tab_mut().replay = Some(replay_test_support::detached_link(recording_at(&dir)));
        app.control.scenarios.queue_mark("scenario".to_owned());
        staged_frame(&mut app, &ctx, order);
        app.control
            .control_access
            .as_ref()
            .expect("control access is installed")
            .journal()
            .read(1, 64, 1 << 20)
            .events
            .iter()
            .filter(|event| {
                event.kind.as_str() == "attention.mark.created"
                    && event.payload["note"] == "scenario"
            })
            .count()
    };
    assert_eq!(
        scenario_marks(&FrameStage::ORDER),
        1,
        "the trace loads first, so this run's mark is known as its own"
    );
    assert_eq!(
        scenario_marks(&frame_hoisting(
            FrameStage::ReplayTrace,
            FrameStage::TakeMark
        )),
        2,
        "taken before the trace loads, the mark joins the sidecar as a recorded \
         action and is re-injected as a replayed one on the same pass"
    );
}

#[test]
fn the_evidence_hook_before_the_annotation_captures_a_window_without_it() {
    use super::control_launch_baselines::{RecordedEvents, hook_bundle};
    use tracing_subscriber::prelude::*;
    let bundle_holds_the_label = |order: &[FrameStage]| {
        let recorded = RecordedEvents::default();
        let subscriber = tracing_subscriber::registry().with(recorded.clone());
        tracing::subscriber::with_default(subscriber, || {
            let ctx = egui::Context::default();
            let (mut app, _commands) = app_with_history(8);
            run_frame(&mut app, &ctx);
            grant_annotate_for_test(&mut app, "all-reads,observe.evidence");
            app.control
                .scenarios
                .queue_annotation("stage-order label".to_owned());
            app.control.scenarios.queue_evidence("all".to_owned());
            staged_frame(&mut app, &ctx, order);
            assert_eq!(
                app.active_tab().drawing_pane().drawings.items().len(),
                1,
                "the annotation landed on this frame either way"
            );
            // The label's words are redacted from a bundle; its creation
            // event is what the capture either saw or did not.
            hook_bundle(&mut app, &recorded)["events"]["events"]
                .as_array()
                .expect("the bundle carries the journal page")
                .iter()
                .any(|event| event["kind"] == "annotate.object.created")
        })
    };
    assert!(
        bundle_holds_the_label(&FrameStage::ORDER),
        "the bundle describes the window the assistant already wrote on"
    );
    assert!(
        !bundle_holds_the_label(&frame_hoisting(
            FrameStage::AnnotateHooks,
            FrameStage::EvidenceHook
        )),
        "captured first, the bundle misses the label placed on the same frame"
    );
}

/// Whether the frame painted the unapplied-settings watermark.
fn painted_preview_watermark(output: &egui::FullOutput) -> bool {
    fn holds(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.text() == "PREVIEW — settings not applied",
            egui::Shape::Vec(shapes) => shapes.iter().any(holds),
            _ => false,
        }
    }
    output.shapes.iter().any(|clipped| holds(&clipped.shape))
}

#[test]
fn the_surfaces_before_the_indicator_dialog_paint_a_preview_it_just_replaced() {
    use quantick_indicators::InputValue;
    let watermark_on_the_reopen_frame = |order: &[FrameStage]| {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        app.apply_toolbar_action(crate::toolbar::ToolbarAction::AddNative("native.ema"));
        settle_indicators(&mut app);
        let side = PaneSide::Time(0);
        let slot = app.active_tab().pane(side).indicators.all()[0].slot;
        let tab = app.tabs.active_id();
        app.apply_indicator_legend_action(
            tab,
            side,
            crate::indicator_legend::LegendAction::OpenSettings(slot),
        );
        app.indicators.indicator_settings.as_mut().unwrap().draft[0] = InputValue::Int(37);
        let change = app.indicators.preview_settings();
        app.apply_indicator_settings_change(change);
        settle_indicators(&mut app);
        assert!(
            painted_preview_watermark(&staged_frame_at(
                &mut app,
                &ctx,
                &FrameStage::ORDER,
                Instant::now()
            )),
            "an unapplied preview carries the watermark"
        );
        // The dialog is replaced by a fresh one, with nothing previewed.
        app.chrome
            .harness
            .arm_settings_autostart(0, crate::indicator_panel::SettingsTab::default());
        painted_preview_watermark(&staged_frame_at(&mut app, &ctx, order, Instant::now()))
    };
    assert!(
        !watermark_on_the_reopen_frame(&FrameStage::ORDER),
        "the surfaces read the dialog the indicator stage just opened"
    );
    assert!(
        watermark_on_the_reopen_frame(&frame_deferring(
            FrameStage::IndicatorSurfaces,
            FrameStage::Surfaces
        )),
        "drawn first, the surfaces label a preview that is already gone"
    );
}

#[test]
fn the_report_before_the_seek_drains_misses_the_round_trip_it_closed() {
    use super::frame_tail_tests::print;
    // The drain and the housekeeping that reads it, moved behind the tail
    // together: only the tail's own declaration on the drain refuses this.
    let mut late_drain = FrameStage::ORDER.to_vec();
    for stage in [FrameStage::DrainSources, FrameStage::WindowHousekeeping] {
        late_drain.retain(|candidate| *candidate != stage);
        let tail = position(&late_drain, FrameStage::Tail);
        late_drain.insert(tail + 1, stage);
    }
    assert!(
        !FrameStage::is_valid_order(&late_drain),
        "Tail declares DrainSources"
    );
    let projected_rows_on_the_seek_frame = |order: &[FrameStage]| {
        let (mut app, events, _commands, _book) = test_app();
        let ctx = egui::Context::default();
        let dir = crate::scratch::ScratchDir::new("stage-order-seek-report");
        staged_frame(&mut app, &ctx, &FrameStage::ORDER);
        {
            let symbol = app.active_tab().symbol.clone();
            let paper = &mut app.active_tab_mut().paper;
            paper.redirect_history_dir(dir.path().to_path_buf());
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
        staged_frame(&mut app, &ctx, &FrameStage::ORDER);
        // A replay seek reaches the tab as a reset of its timeline.
        events.try_send(FeedEvent::Reset).unwrap();
        staged_frame(&mut app, &ctx, order);
        assert!(
            app.active_tab().paper.position_summary().is_none(),
            "the seek closed the position on this frame either way"
        );
        app.active_tab()
            .paper
            .report_state()
            .snapshot()
            .expect("the report is open")
            .rows
            .len()
    };
    assert_eq!(
        projected_rows_on_the_seek_frame(&FrameStage::ORDER),
        1,
        "the report projects the round trip the seek closed"
    );
    assert_eq!(
        projected_rows_on_the_seek_frame(&late_drain),
        0,
        "projected before the drain, the report misses it for the frame"
    );
}
