use super::*;
use quantick_alerts::{
    AlertCommand, AlertId, AlertSpec, DeliveryOutcome, Expiration, Frequency, Notification,
    Operator, Predicate, TargetRef,
};
use quantick_control_handlers::alerts::AlertDefinitionsPort;

fn spec(app: &QuantickApp) -> AlertSpec {
    let tab = app.active_tab();
    let context =
        super::super::alerts_runtime::context(tab, &tab.flow_pane, app.runtime.tabs.active_id(), 0);
    AlertSpec {
        version: quantick_alerts::FORMAT_VERSION,
        id: AlertId(0),
        name: "Close alert".into(),
        market: context.market,
        conditions: vec![Predicate {
            operator: Operator::GreaterThan,
            target: TargetRef::Value(100.0),
        }],
        frequency: Frequency::PerBarClose,
        expiration: Expiration::Never,
        message: "Price crossed".into(),
        notification: Notification {
            popup: true,
            toast: true,
            sound: Some("beep".into()),
            sound_seconds: None,
        },
    }
}
fn ready(app: &mut QuantickApp) {
    let tab = app.active_tab_mut();
    tab.link.feed_connection = quantick_feed::FeedConnectionState::Connected;
    tab.flow_pane.spec.retain(BarSpec::Tick(1));
    tab.apply_spec_changes();
    tab.apply_spec_changes();
}
#[test]
fn alert_end_to_end() {
    use quantick_alerts::delivery::{AlertSink, AlertState};
    use quantick_alerts::sound::Cue;
    use std::{cell::RefCell, rc::Rc};
    struct Sounds(Rc<RefCell<Vec<Cue>>>);
    impl AlertSink for Sounds {
        fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str> {
            self.0.borrow_mut().extend_from_slice(cues);
            Ok(())
        }
    }
    let (mut app, events, _commands, _book) = test_app();
    ready(&mut app);
    events
        .try_send(FeedEvent::Backfilled(vec![trade(1)]))
        .unwrap();
    app.drain_tabs();
    let rule = spec(&app);
    let created = app.alert_command(AlertCommand::Create(rule)).unwrap();
    for id in 2..=4 {
        let mut print = trade(id);
        print.timestamp_ms = 2000;
        print.price = Decimal::from(102);
        events.try_send(FeedEvent::Live(print)).unwrap();
    }
    app.drain_tabs();
    let history = app.runtime.alerts.core.book.history(0, 10).unwrap();
    assert_eq!(
        history.len(),
        3,
        "each equal-timestamp close remains distinct"
    );
    assert!(
        history
            .windows(2)
            .all(|pair| pair[0].bar_sequence != pair[1].bar_sequence)
    );
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "alerts never place orders"
    );
    let sounds = Rc::new(RefCell::new(Vec::new()));
    let mut audio = AlertState {
        alerts: Box::new(Sounds(sounds.clone())),
        alert_failure: None,
    };
    super::super::alerts_runtime::deliver(
        &mut app.runtime.alerts,
        &mut audio,
        &mut app.surfaces.toast,
        &mut app.surfaces.agent_popup,
        std::iter::empty(),
    );
    assert!(
        !sounds.borrow().is_empty(),
        "existing sound sink receives the requested cue"
    );
    assert!(
        app.runtime
            .alerts
            .core
            .book
            .history(0, 10)
            .unwrap()
            .iter()
            .all(|event| event.delivery == DeliveryOutcome::Delivered)
    );
    let reopened =
        quantick_stores::alerts::AlertRuntime::open(app.runtime.alerts.core.store.path.clone(), 0);
    assert_eq!(reopened.book.history(0, 10).unwrap().len(), 3);
    assert_eq!(
        reopened.book.views().unwrap()[0].spec.id,
        created.id.unwrap()
    );
    assert!(
        reopened.events.is_empty(),
        "restoration never replays notifications"
    );
    let ctx = egui::Context::default();
    let mirror = open_second_tab(&mut app, &ctx, "TESTUSDT");
    ready(&mut app);
    mirror
        .events
        .try_send(FeedEvent::Backfilled((1..=4).map(trade).collect()))
        .unwrap();
    app.drain_tabs();
    mirror.events.try_send(FeedEvent::Live(trade(5))).unwrap();
    events.try_send(FeedEvent::Live(trade(5))).unwrap();
    app.drain_tabs();
    assert_eq!(
        app.runtime.alerts.core.book.history(0, 10).unwrap().len(),
        4,
        "mirrored observations do not reset or double-fire"
    );
}

#[test]
fn alert_target_binding_end_to_end() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let index = place_drawing(
        &mut app,
        &ctx,
        "rectangle",
        &[egui::pos2(500.0, 300.0), egui::pos2(650.0, 420.0)],
    );
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(index));
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let button = painted_text_center(&output, "Create alert").expect("drawing alert action");
    click_chart(&mut app, &ctx, button);
    run_frame(&mut app, &ctx);
    let target = app.active_tab().drawing_pane().drawings.items()[index]
        .durable_identity
        .clone()
        .unwrap();
    let selector = quantick_control_schema::alerts::DrawingSelector {
        tab_id: app.runtime.tabs.active_id(),
        pane_id: app.active_tab().drawing_pane().id,
        drawing_id: app.active_tab().drawing_pane().drawings.items()[index].id.0,
    };
    assert_eq!(app.alert_resolve_target(selector).unwrap(), target);
    let mut rule = spec(&app);
    rule.conditions[0] = Predicate {
        operator: Operator::Inside,
        target: TargetRef::Drawing(target.clone()),
    };
    app.alert_command(AlertCommand::Create(rule)).unwrap();
    let reopened =
        quantick_stores::alerts::AlertRuntime::open(app.runtime.alerts.core.store.path.clone(), 0);
    assert_eq!(
        reopened.book.views().unwrap()[0].spec.conditions[0].target,
        TargetRef::Drawing(target.clone())
    );
    let saved = std::fs::read_to_string(app.layout_adapter().store.market().path()).unwrap();
    assert!(
        saved.contains(&target.incarnation),
        "UI and host share the persisted target"
    );
    super::super::alerts_runtime::apply_alert_request(
        &mut app,
        crate::surfaces::alerts::Request {
            locate: Some(target),
            ..Default::default()
        },
    );
    assert_eq!(
        app.active_tab().drawing_pane().drawings.selected(),
        Some(index)
    );
    // A second tab of the same persisted source is a known mirror, not an
    // ambiguous identity. Locate keeps the currently focused valid copy.
    let target = app.active_tab().drawing_pane().drawings.items()[index]
        .durable_identity
        .clone()
        .unwrap();
    let symbol = app.active_tab().active.1.clone();
    let _mirror = open_second_tab(&mut app, &ctx, &symbol);
    run_frame(&mut app, &ctx);
    let focused = app.runtime.tabs.active_id();
    let mirror_index = app
        .active_tab()
        .drawing_pane()
        .drawings
        .items()
        .iter()
        .position(|drawing| drawing.durable_identity.as_ref() == Some(&target))
        .expect("persisted mirror carries the same target");
    app.active_tab_mut()
        .focused_pane_mut()
        .drawings
        .select(None);
    super::super::alerts_runtime::apply_alert_request(
        &mut app,
        crate::surfaces::alerts::Request {
            locate: Some(target),
            ..Default::default()
        },
    );
    assert_eq!(app.runtime.tabs.active_id(), focused);
    assert_eq!(
        app.active_tab().drawing_pane().drawings.selected(),
        Some(mirror_index)
    );
}

#[test]
fn alert_native_dense_tape_no_fire() {
    use crate::tab::alerts::{Observation, Observer};
    let (mut app, events, _commands, _book) = test_app();
    ready(&mut app);
    events
        .try_send(FeedEvent::Backfilled(vec![trade(1)]))
        .unwrap();
    app.drain_tabs();
    let mut rule = spec(&app);
    rule.frequency = Frequency::PerBar;
    rule.conditions[0].target = TargetRef::Value(10_000.0);
    app.alert_command(AlertCommand::Create(rule)).unwrap();
    let tab = &app.runtime.tabs[0];
    let runtime = &mut app.runtime.alerts;
    tab.observe_alert_lifecycle(runtime);
    let print = trade(1);
    let observation = || {
        Some(Observation {
            trade: &print,
            received_at_ms: 2000,
            close: None,
        })
    };
    runtime.pane((&tab.flow_pane).into(), observation());
    assert_eq!(
        runtime.core.book.views().unwrap()[0].status,
        quantick_alerts::AlertStatus::Monitoring
    );
    let before = crate::work_meter::tally();
    let started = std::time::Instant::now();
    for _ in 0..100_000 {
        runtime.pane((&tab.flow_pane).into(), observation());
    }
    let elapsed = started.elapsed();
    let spent = crate::work_meter::tally().since(before);
    assert_eq!(
        spent.allocs, 0,
        "cached native no-fire observations allocate nothing"
    );
    assert!(runtime.core.events.is_empty());
    // The scaffold/base observer is the unchanged unit implementation.
    let mut baseline = ();
    let base_started = std::time::Instant::now();
    for _ in 0..100_000 {
        std::hint::black_box(&mut baseline).pane((&tab.flow_pane).into(), observation());
    }
    eprintln!(
        "alerts_native_dense_tape observations=100000 adapter_us={} scaffold_us={} allocations={} bytes={}",
        elapsed.as_micros(),
        base_started.elapsed().as_micros(),
        spent.allocs,
        spent.alloc_bytes
    );
}

#[test]
fn alert_shared_sound_batch_and_failure() {
    use quantick_alerts::delivery::{AlertSink, AlertState};
    use quantick_alerts::sound::{AlertSound, Cue};
    use std::{cell::RefCell, rc::Rc};
    struct Sounds {
        batches: Rc<RefCell<Vec<Vec<Cue>>>>,
        fail: bool,
    }
    impl AlertSink for Sounds {
        fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str> {
            self.batches.borrow_mut().push(cues.to_vec());
            if self.fail {
                Err("test device refusal")
            } else {
                Ok(())
            }
        }
    }
    for fail in [false, true] {
        let (mut app, events, ..) = test_app();
        ready(&mut app);
        events
            .try_send(FeedEvent::Backfilled(vec![trade(1)]))
            .unwrap();
        app.drain_tabs();
        let rule = spec(&app);
        app.alert_command(AlertCommand::Create(rule.clone()))
            .unwrap();
        let mut silent = rule;
        silent.notification.sound = None;
        app.alert_command(AlertCommand::Create(silent)).unwrap();
        for id in 2..=4 {
            let mut print = trade(id);
            print.price = Decimal::from(102);
            events.try_send(FeedEvent::Live(print)).unwrap();
        }
        app.drain_tabs();
        let batches = Rc::new(RefCell::new(Vec::new()));
        let mut audio = AlertState {
            alerts: Box::new(Sounds {
                batches: batches.clone(),
                fail,
            }),
            alert_failure: None,
        };
        let strategy = Cue::new(AlertSound::Question, None);
        super::super::alerts_runtime::deliver(
            &mut app.runtime.alerts,
            &mut audio,
            &mut app.surfaces.toast,
            &mut app.surfaces.agent_popup,
            [strategy, strategy],
        );
        assert_eq!(
            *batches.borrow(),
            vec![vec![strategy, Cue::new(AlertSound::Beep, None)]]
        );
        let history = app.runtime.alerts.core.book.history(0, 10).unwrap();
        assert_eq!(history.len(), 6);
        for event in history {
            let expected = if fail && event.spec.notification.sound.is_some() {
                DeliveryOutcome::Failed
            } else {
                DeliveryOutcome::Delivered
            };
            assert_eq!(event.delivery, expected);
        }
    }
}

#[test]
fn alert_post_close_trade_preserves_bar_budget() {
    for aggregation in [BarSpec::Tick(1), BarSpec::Volume(Decimal::ONE)] {
        for frequency in [Frequency::PerBar, Frequency::PerBarClose] {
            let (mut app, events, ..) = test_app();
            ready(&mut app);
            app.active_tab_mut().flow_pane.spec.retain(aggregation);
            app.active_tab_mut().apply_spec_changes();
            app.active_tab_mut().apply_spec_changes();
            events
                .try_send(FeedEvent::Backfilled(vec![trade(1)]))
                .unwrap();
            app.drain_tabs();
            let mut rule = spec(&app);
            rule.frequency = frequency;
            app.alert_command(AlertCommand::Create(rule)).unwrap();
            for id in 2..=4 {
                let mut print = trade(id);
                print.price = Decimal::from(102);
                events.try_send(FeedEvent::Live(print)).unwrap();
            }
            app.drain_tabs();
            let history = app.runtime.alerts.core.book.history(0, 10).unwrap();
            assert_eq!(
                history.len(),
                3,
                "{aggregation:?} {frequency:?}: post-close trades share the committed budget"
            );
            assert!(
                history
                    .windows(2)
                    .all(|pair| pair[0].bar_sequence + 1 == pair[1].bar_sequence)
            );
        }
    }
}

#[test]
fn alert_native_lifecycle_reset_and_hydration_are_silent() {
    let (mut app, events, ..) = test_app();
    ready(&mut app);
    events
        .try_send(FeedEvent::Backfilled(vec![trade(1)]))
        .unwrap();
    app.drain_tabs();
    app.alert_command(AlertCommand::Create(spec(&app))).unwrap();
    app.active_tab_mut().link.feed_connection = quantick_feed::FeedConnectionState::Reconnecting;
    app.drain_tabs();
    assert_eq!(
        app.runtime.alerts.core.book.views().unwrap()[0].status,
        quantick_alerts::AlertStatus::Suspended(quantick_alerts::Unavailable::SourceInactive)
    );
    app.active_tab_mut().link.feed_connection = quantick_feed::FeedConnectionState::Connected;
    events.try_send(FeedEvent::Reset).unwrap();
    events
        .try_send(FeedEvent::Backfilled((1..=20).map(trade).collect()))
        .unwrap();
    app.drain_tabs();
    assert!(
        app.runtime
            .alerts
            .core
            .book
            .history(0, 10)
            .unwrap()
            .is_empty(),
        "reconnect/seek hydration never replays historical notifications"
    );
    let mut print = trade(21);
    print.price = Decimal::from(102);
    events.try_send(FeedEvent::Live(print)).unwrap();
    app.drain_tabs();
    assert!(
        app.runtime
            .alerts
            .core
            .book
            .history(0, 10)
            .unwrap()
            .is_empty(),
        "the first live close reseeds after reset; lifecycle cannot consume that baseline"
    );
    let mut print = trade(22);
    print.price = Decimal::from(102);
    events.try_send(FeedEvent::Live(print)).unwrap();
    app.drain_tabs();
    assert_eq!(
        app.runtime.alerts.core.book.history(0, 10).unwrap().len(),
        1
    );
}

/// Native C11 found the selected drawing's context bar painted over the open
/// alert editor: hovering the Trigger combo offered "Delete rectangle". The
/// bar stands down while the alert surface owns the pointer, as it already
/// does for the drawing manager, and returns when the surface closes.
#[test]
fn the_context_bar_yields_to_the_open_alert_surface() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let rectangle = place_drawing(
        &mut app,
        &ctx,
        "rectangle",
        &[egui::pos2(500.0, 300.0), egui::pos2(700.0, 420.0)],
    );
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(rectangle));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.drawings.chrome.context_bar_rect().is_some(),
        "the selection raised the bar"
    );

    app.surfaces.alerts.open_drawing(None);
    app.drawings.chrome.forget_context_bar_rect();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.drawings.chrome.context_bar_rect().is_none(),
        "the bar stands down while the alert surface is open"
    );

    app.surfaces.alerts = Default::default();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.drawings.chrome.context_bar_rect().is_some(),
        "the bar returns once the alert surface closes"
    );
}
