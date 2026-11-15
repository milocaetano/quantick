use super::*;
use quantick_alerts::*;

fn spec(target: TargetRef) -> AlertSpec {
    AlertSpec {
        version: FORMAT_VERSION,
        id: AlertId(0),
        name: "Price alert".into(),
        market: MarketRef {
            source: SourceContext::Live {
                feed: "fixture".into(),
            },
            symbol: "TEST".into(),
            aggregation: "tick:10".into(),
        },
        conditions: vec![Predicate {
            operator: Operator::CrossingUp,
            target,
        }],
        frequency: Frequency::Once,
        expiration: Expiration::Never,
        message: "Crossed".into(),
        notification: Notification {
            popup: false,
            toast: true,
            sound: None,
            sound_seconds: None,
        },
    }
}

#[test]
fn alert_document_roundtrip() {
    let dir = crate::scratch::ScratchDir::new("alerts-roundtrip");
    let path = dir.join(ALERTS_FILE);
    let mut runtime = AlertRuntime::open(path.clone(), 0);
    let target = DurableDrawingRef {
        workspace: "workspace".into(),
        origin: "document-a".into(),
        pane: "pane".into(),
        drawing: 1,
        incarnation: "mark-a".into(),
    };
    let created = runtime
        .command(
            AlertCommand::Create(spec(TargetRef::Drawing(target.clone()))),
            1,
        )
        .unwrap();
    runtime
        .command(
            AlertCommand::Pause {
                id: created.id.unwrap(),
                expected_revision: created.revision,
            },
            2,
        )
        .unwrap();
    let reopened = AlertRuntime::open(path.clone(), 3);
    assert_eq!(
        reopened.book.document().unwrap(),
        runtime.book.document().unwrap()
    );
    let view = reopened.book.views().unwrap().remove(0);
    assert_eq!(view.status, AlertStatus::Paused);
    assert_eq!(
        view.spec.conditions[0].target,
        TargetRef::Drawing(target.clone())
    );
    let mut overlapping = target.clone();
    overlapping.origin = "document-b".into();
    assert!(quantick_workspace::drawing_identity::resolve(&target, [&overlapping]).is_err());
    assert!(quantick_workspace::drawing_identity::resolve(&target, [&target, &target]).is_err());
    std::fs::write(
        &path,
        "version = 0\nnext_id = 1\nnext_event_sequence = 1\ndefinitions = []\nhistory = []\n",
    )
    .unwrap();
    let mut legacy = AlertRuntime::open(path.clone(), 4);
    assert_eq!(legacy.error, Some(AlertError::UnsupportedVersion(0)));
    assert!(
        legacy
            .command(AlertCommand::Create(spec(TargetRef::Value(1.0))), 4)
            .is_err()
    );
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("version = 0")
    );
    std::fs::write(&path, "broken").unwrap();
    assert!(legacy.reload(5).is_err());
}

#[test]
fn failed_alert_save_does_not_publish_a_definition() {
    let dir = crate::scratch::ScratchDir::new("alerts-write-refusal");
    let mut runtime = AlertRuntime::open(dir.join("absent").join(ALERTS_FILE), 0);
    assert!(
        runtime
            .command(AlertCommand::Create(spec(TargetRef::Value(1.0))), 1)
            .is_err()
    );
    assert!(runtime.book.views().unwrap().is_empty());
    assert!(runtime.error.is_some());
    assert_eq!(runtime.views().unwrap_err(), runtime.error.clone().unwrap());
    assert_eq!(
        runtime.history(0, 10).unwrap_err(),
        runtime.error.clone().unwrap()
    );
    runtime.reload(2).unwrap();
    assert!(runtime.views().unwrap().is_empty());
    assert!(runtime.history(0, 10).unwrap().is_empty());
}

/// C8: the live connection is the live feed's fact. A replay plays from its
/// recording, so a dropped live link never suspends a replay alert.
#[test]
fn replay_source_ignores_a_live_disconnect() {
    let status = |replay: Option<(&std::path::Path, bool)>| {
        let dir = crate::scratch::ScratchDir::new("alerts-cache-source");
        let mut runtime = AlertRuntime::open(dir.join(ALERTS_FILE), 0);
        let mut series = quantick_chart::state::ChartState::new(quantick_engine::BarSpec::Tick(10));
        // A priced series: an empty one is unavailable for want of a price.
        series.ingest_live(&quantick_engine::Trade {
            agg_id: 1,
            timestamp_ms: 1_000,
            price: rust_decimal::Decimal::ONE_HUNDRED,
            quantity: rust_decimal::Decimal::ONE,
            side: quantick_engine::Side::Buy,
        });
        let mut rule = spec(TargetRef::Value(1.0));
        rule.market.aggregation = series.spec().to_config_string();
        if let Some((path, _)) = replay {
            rule.market.source = SourceContext::Replay {
                recording: path.to_string_lossy().into_owned(),
            };
        }
        runtime.command(AlertCommand::Create(rule), 1).unwrap();
        let drawings = quantick_drawings::Drawings::default();
        let mut cache = Cache::default();
        cache.begin();
        cache.source(Source {
            feed: "fixture",
            symbol: "TEST",
            generation: 0,
            connected: false,
            hydrating: false,
            replay,
        });
        let facts = PaneFacts {
            id: 1,
            series: &series,
            drawings: &drawings,
            drawing_key: None,
            prefix_slots: 0,
            hydrating: false,
        };
        cache.pane(&mut runtime, facts, None, || 2, 2);
        cache.end(&mut runtime, 2, 2);
        runtime.book.views().unwrap().remove(0).status
    };
    let inactive = AlertStatus::Suspended(Unavailable::SourceInactive);
    assert_eq!(status(None), inactive, "a dropped live feed suspends");
    let recording = std::path::Path::new("session.qtr");
    assert_eq!(status(Some((recording, true))), AlertStatus::Monitoring);
    assert_eq!(
        status(Some((recording, false))),
        AlertStatus::Suspended(Unavailable::ReplayPaused)
    );
}
