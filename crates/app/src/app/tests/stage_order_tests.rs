//! The registered frame and drain orders, replayed through the real stage
//! adapters with one declared dependency hoisted: each swap does visible
//! harm, which is what the declaration exists to refuse.

use super::*;
use quantick_chart_interaction::frame_plan::FrameStage;
use quantick_chart_interaction::stage_registry::hoisted;
use quantick_chart_interaction::tab_drain_plan::TabDrainStage;

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
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW)),
            ..Default::default()
        },
        |ctx| {
            app.draw_frame_stage_test_order(
                ctx,
                Instant::now(),
                &mut quantick_feed::spawn_live,
                order.iter().copied(),
            );
        },
    );
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
        app.control_feed_chip_rect()
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
