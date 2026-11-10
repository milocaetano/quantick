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
}
