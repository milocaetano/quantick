use quantick_alerts::*;

fn definition() -> AlertSpec {
    AlertSpec {
        version: FORMAT_VERSION,
        id: AlertId(0),
        name: "Idle lifecycle".into(),
        market: MarketRef {
            source: SourceContext::Live {
                feed: "fixture".into(),
            },
            symbol: "TEST".into(),
            aggregation: "tick:1".into(),
        },
        conditions: vec![Predicate {
            operator: Operator::CrossingUp,
            target: TargetRef::Value(10.0),
        }],
        frequency: Frequency::Once,
        expiration: Expiration::Never,
        message: String::new(),
        notification: Notification {
            popup: false,
            toast: true,
            sound: None,
            sound_seconds: None,
        },
    }
}
fn input(spec: &AlertSpec, price: f64, kind: ObservationKind) -> AlertInput<'_> {
    AlertInput {
        market: &spec.market,
        utc_ms: 1,
        session_ms: 1,
        bar: BarIdentity {
            epoch: 1,
            sequence: 1,
        },
        kind,
        coordinate: 1.0,
        price,
        availability: Ok(()),
        drawings: &[],
    }
}
#[test]
fn idle_preserves_price_baseline_once_and_expiry() {
    let spec = definition();
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(spec.clone()), 0).unwrap();
    let mut out = BoundedEvents::new(8);
    book.evaluate(&input(&spec, 9.0, ObservationKind::Trade), &mut out);
    for _ in 0..5 {
        book.evaluate(&input(&spec, 11.0, ObservationKind::Lifecycle), &mut out);
    }
    assert!(
        out.is_empty(),
        "lifecycle must never fire or consume the crossing"
    );
    book.evaluate(&input(&spec, 11.0, ObservationKind::Trade), &mut out);
    assert_eq!(out.len(), 1);
    book.evaluate(&input(&spec, 9.0, ObservationKind::Lifecycle), &mut out);
    book.evaluate(&input(&spec, 11.0, ObservationKind::Trade), &mut out);
    assert_eq!(
        out.len(),
        1,
        "completed once-only definitions remain complete"
    );
    let mut expiring = spec.clone();
    expiring.expiration = Expiration::At {
        utc_ms: 2,
        display_offset_minutes: 0,
    };
    book.command(AlertCommand::Create(expiring.clone()), 0)
        .unwrap();
    let mut idle = input(&expiring, 9.0, ObservationKind::Lifecycle);
    idle.utc_ms = 2;
    book.evaluate(&idle, &mut out);
    assert_eq!(book.views().unwrap()[1].status, AlertStatus::Expired);
}
#[test]
fn idle_missing_restore_and_geometry_edit_reseed_without_firing() {
    let mut spec = definition();
    let target = DurableDrawingRef {
        workspace: "w".into(),
        origin: "o".into(),
        pane: "p".into(),
        drawing: 1,
        incarnation: "i".into(),
    };
    spec.conditions[0].target = TargetRef::Drawing(target.clone());
    let mut facts = [DrawingFact {
        target: &target,
        revision: 0,
        geometry: Ok(Geometry::Line(Line {
            first: Point {
                coordinate: 0.0,
                price: 10.0,
            },
            second: Point {
                coordinate: 2.0,
                price: 10.0,
            },
            extend_left: false,
            extend_right: false,
        })),
    }];
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(spec.clone()), 0).unwrap();
    let mut out = BoundedEvents::new(8);
    let mut tick = input(&spec, 9.0, ObservationKind::Trade);
    tick.drawings = &facts;
    book.evaluate(&tick, &mut out);
    book.evaluate(&input(&spec, 9.0, ObservationKind::Lifecycle), &mut out);
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::MissingDrawing)
    );
    let mut idle = input(&spec, 11.0, ObservationKind::Lifecycle);
    idle.drawings = &facts;
    book.evaluate(&idle, &mut out);
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Monitoring);
    let mut restored = input(&spec, 11.0, ObservationKind::Trade);
    restored.drawings = &facts;
    book.evaluate(&restored, &mut out);
    assert!(out.is_empty(), "restoration seeds from an actual trade");
    facts[0].revision = 1;
    let mut edited = input(&spec, 9.0, ObservationKind::Lifecycle);
    edited.drawings = &facts;
    book.evaluate(&edited, &mut out);
    edited.kind = ObservationKind::Trade;
    edited.price = 11.0;
    book.evaluate(&edited, &mut out);
    assert!(
        out.is_empty(),
        "idle geometry edit cannot manufacture a crossing"
    );
}
