use quantick_alerts::*;
fn spec(op: Operator) -> AlertSpec {
    AlertSpec {
        version: FORMAT_VERSION,
        id: AlertId(0),
        name: "test".into(),
        market: MarketRef {
            source: SourceContext::Live {
                feed: "test".into(),
            },
            symbol: "BTC".into(),
            aggregation: "tick:1".into(),
        },
        conditions: vec![Predicate {
            operator: op,
            target: TargetRef::Value(10.0),
        }],
        frequency: Frequency::PerBar,
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
fn observe(book: &mut AlertBook, draft: &AlertSpec, prices: &[f64]) -> Vec<AlertEvent> {
    let mut out = BoundedEvents::new(MAX_ALERTS);
    for (i, price) in prices.iter().enumerate() {
        book.evaluate(
            &AlertInput {
                market: &draft.market,
                utc_ms: 0,
                session_ms: i as u64,
                bar: BarIdentity {
                    epoch: 0,
                    sequence: i as u64,
                },
                kind: ObservationKind::Trade,
                coordinate: i as f64,
                price: *price,
                availability: Ok(()),
                drawings: &[],
            },
            &mut out,
        );
    }
    out.drain().collect()
}
#[test]
fn predicate_matrix() {
    let draft = spec(Operator::Crossing);
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    assert_eq!(
        observe(&mut book, &draft, &[9., 10., 11., 10., 9.]).len(),
        2
    );
}
#[test]
fn cadence_and_expiration() {
    let mut draft = spec(Operator::GreaterThan);
    draft.frequency = Frequency::Once;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    assert_eq!(observe(&mut book, &draft, &[11., 12., 13.]).len(), 1);
}
#[test]
fn lifecycle_and_resume() {
    let draft = spec(Operator::Crossing);
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    observe(&mut book, &draft, &[9.]);
    let doc = book.document().unwrap();
    book.restore(doc, 0).unwrap();
    assert!(observe(&mut book, &draft, &[11.]).is_empty());
}
#[test]
fn headless_extension_and_budget() {
    let draft = spec(Operator::Crossing);
    let run = || {
        let mut book = AlertBook::default();
        book.command(AlertCommand::Create(draft.clone()), 0)
            .unwrap();
        observe(&mut book, &draft, &[9., 11., 9., 11.])
    };
    assert_eq!(run(), run());
}
fn snapshot<'a>(draft: &'a AlertSpec, sequence: u64, price: f64) -> AlertInput<'a> {
    AlertInput {
        market: &draft.market,
        utc_ms: sequence as i64,
        session_ms: sequence,
        bar: BarIdentity { epoch: 1, sequence },
        kind: ObservationKind::Trade,
        coordinate: sequence as f64,
        price,
        availability: Ok(()),
        drawings: &[],
    }
}
fn drawing() -> DurableDrawingRef {
    DurableDrawingRef {
        workspace: "desk".into(),
        origin: "one".into(),
        pane: "main".into(),
        drawing: 1,
        incarnation: "mark".into(),
    }
}
#[test]
fn region_matrix_and_time_bounds() {
    let target = drawing();
    for (op, expected) in [
        (Operator::Entering, 2),
        (Operator::Exiting, 1),
        (Operator::Inside, 3),
        (Operator::Outside, 2),
        (Operator::GreaterThan, 1),
        (Operator::LessThan, 1),
    ] {
        let mut draft = spec(op);
        draft.conditions[0].target = TargetRef::Drawing(target.clone());
        let mut book = AlertBook::default();
        book.command(AlertCommand::Create(draft.clone()), 0)
            .unwrap();
        let mut out = BoundedEvents::new(10);
        let facts = [DrawingFact {
            target: &target,
            revision: 1,
            geometry: Ok(Geometry::Rectangle {
                first: Point {
                    coordinate: 0.,
                    price: 10.,
                },
                second: Point {
                    coordinate: 4.,
                    price: 20.,
                },
            }),
        }];
        for (i, price) in [9., 10., 20., 21., 15., 30.].iter().enumerate() {
            let mut input = snapshot(&draft, i as u64, *price);
            input.drawings = &facts;
            book.evaluate(&input, &mut out);
        }
        assert_eq!(out.len(), expected, "{op:?}");
        assert_eq!(
            book.views().unwrap()[0].status,
            AlertStatus::Suspended(Unavailable::OutsideTimeDomain)
        );
    }
}
#[test]
fn crossing_operators_and_all_have_no_latch() {
    for (op, count) in [
        (Operator::Crossing, 2),
        (Operator::CrossingUp, 1),
        (Operator::CrossingDown, 1),
        (Operator::GreaterThan, 1),
        (Operator::LessThan, 2),
    ] {
        let draft = spec(op);
        let mut book = AlertBook::default();
        book.command(AlertCommand::Create(draft.clone()), 0)
            .unwrap();
        assert_eq!(
            observe(&mut book, &draft, &[9., 10., 11., 10., 9.]).len(),
            count
        );
    }
    let mut draft = spec(Operator::CrossingUp);
    draft.conditions.push(Predicate {
        operator: Operator::GreaterThan,
        target: TargetRef::Value(12.),
    });
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    assert!(observe(&mut book, &draft, &[9., 11., 13.]).is_empty());
}
#[test]
fn sloped_channel_and_geometry_revision() {
    let target = drawing();
    let line = Line {
        first: Point {
            coordinate: 0.,
            price: 10.,
        },
        second: Point {
            coordinate: 10.,
            price: 20.,
        },
        extend_left: false,
        extend_right: false,
    };
    let mut draft = spec(Operator::CrossingUp);
    draft.conditions[0].target = TargetRef::Drawing(target.clone());
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let mut out = BoundedEvents::new(10);
    for (i, price, revision) in [
        (0, 9., 1),
        (1, 12., 1),
        (2, 11., 1),
        (3, 14., 2),
        (4, 13., 2),
        (5, 16., 2),
    ] {
        let facts = [DrawingFact {
            target: &target,
            revision,
            geometry: Ok(Geometry::Line(line)),
        }];
        let mut input = snapshot(&draft, i, price);
        input.drawings = &facts;
        book.evaluate(&input, &mut out);
    }
    assert_eq!(out.len(), 2);
    draft.conditions[0].operator = Operator::Entering;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let facts = [DrawingFact {
        target: &target,
        revision: 1,
        geometry: Ok(Geometry::Channel { line, offset: 2. }),
    }];
    let mut out = BoundedEvents::new(10);
    for (i, price) in [(0, 9.), (1, 14.), (2, 12.)] {
        let mut input = snapshot(&draft, i, price);
        input.drawings = &facts;
        book.evaluate(&input, &mut out);
    }
    assert_eq!(
        out.len(),
        1,
        "jump over region never infers an entering visit"
    );
}
#[test]
fn close_only_does_not_latch_trades_or_duplicate_bars() {
    let mut draft = spec(Operator::CrossingUp);
    draft.frequency = Frequency::PerBarClose;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let mut out = BoundedEvents::new(10);
    for (sequence, price, kind) in [
        (0, 9., ObservationKind::CommittedClose),
        (1, 11., ObservationKind::Trade),
        (1, 9., ObservationKind::CommittedClose),
        (2, 11., ObservationKind::CommittedClose),
        (2, 9., ObservationKind::CommittedClose),
        (3, 9., ObservationKind::CommittedClose),
        (4, 11., ObservationKind::CommittedClose),
    ] {
        let mut input = snapshot(&draft, sequence, price);
        input.utc_ms = 100;
        input.kind = kind;
        book.evaluate(&input, &mut out);
    }
    assert_eq!(out.len(), 2);
}
#[test]
fn minute_interval_and_expiry_cutoff() {
    let mut draft = spec(Operator::GreaterThan);
    draft.frequency = Frequency::PerMinute;
    draft.expiration = Expiration::At {
        utc_ms: 180_000,
        display_offset_minutes: 0,
    };
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let mut out = BoundedEvents::new(10);
    for time in [0, 59_999, 60_000, 179_999, 180_000, 900_000] {
        book.evaluate(&snapshot(&draft, time, 11.), &mut out);
    }
    assert_eq!(out.len(), 3);
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Expired);
    let tz = quantick_civil::TzOffset::new(-180);
    let jan = quantick_civil::CivilDate::from_ymd(2024, 1, 31).start_ms(tz) + 1234;
    let feb = quantick_civil::CivilDate::from_ymd(2024, 2, 29).start_ms(tz) + 1234;
    assert_eq!(
        book.resolve_expiration(ExpirationChoice::CalendarMonth, jan, -180)
            .unwrap(),
        Expiration::At {
            utc_ms: feb,
            display_offset_minutes: -180
        }
    );
    assert_eq!(
        book.resolve_expiration(ExpirationChoice::EndOfDay, jan, -180)
            .unwrap(),
        Expiration::At {
            utc_ms: quantick_civil::CivilDate::from_ymd(2024, 2, 1).start_ms(tz),
            display_offset_minutes: -180
        }
    );
}
#[test]
fn lifecycle_commands_restore_and_refusal_history() {
    let mut draft = spec(Operator::GreaterThan);
    draft.frequency = Frequency::Once;
    let mut book = AlertBook::default();
    let created = book
        .command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    draft.id = created.id.unwrap();
    let mut sink = BoundedEvents::new(0);
    book.evaluate(&snapshot(&draft, 0, 11.), &mut sink);
    assert_eq!(
        book.history(0, 10).unwrap()[0].delivery,
        DeliveryOutcome::Suppressed
    );
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Completed);
    book.command(
        AlertCommand::Edit {
            expected_revision: 1,
            spec: draft.clone(),
        },
        0,
    )
    .unwrap();
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Completed);
    book.command(
        AlertCommand::Rearm {
            id: draft.id,
            expected_revision: 2,
        },
        0,
    )
    .unwrap();
    book.evaluate(&snapshot(&draft, 1, 11.), &mut sink);
    assert_eq!(book.history(0, 10).unwrap().len(), 1);
    book.evaluate(&snapshot(&draft, 2, 11.), &mut sink);
    assert_eq!(book.history(0, 10).unwrap().len(), 2);
    book.command(
        AlertCommand::Delete {
            id: draft.id,
            expected_revision: 3,
        },
        0,
    )
    .unwrap();
    assert_eq!(book.history(0, 10).unwrap()[0].spec.name, "test");
    let mut restored = AlertBook::default();
    restored.restore(book.document().unwrap(), 0).unwrap();
    assert_eq!(restored.document().unwrap(), book.document().unwrap());
}
#[test]
fn suspension_and_pause_reseed_without_burst() {
    let draft = spec(Operator::CrossingUp);
    let mut book = AlertBook::default();
    let id = book
        .command(AlertCommand::Create(draft.clone()), 0)
        .unwrap()
        .id
        .unwrap();
    let mut out = BoundedEvents::new(10);
    book.evaluate(&snapshot(&draft, 0, 9.), &mut out);
    book.command(
        AlertCommand::Pause {
            id,
            expected_revision: 1,
        },
        0,
    )
    .unwrap();
    book.evaluate(&snapshot(&draft, 1, 11.), &mut out);
    book.command(
        AlertCommand::Resume {
            id,
            expected_revision: 2,
        },
        0,
    )
    .unwrap();
    book.evaluate(&snapshot(&draft, 2, 11.), &mut out);
    assert!(out.is_empty());
    for (index, reason) in [
        ResetReason::Hydration,
        ResetReason::Reconnect,
        ResetReason::ReplaySeek,
    ]
    .into_iter()
    .enumerate()
    {
        book.reset(
            &draft.market,
            reason,
            BarIdentity {
                epoch: 1,
                sequence: 0,
            },
        );
        book.evaluate(&snapshot(&draft, 3 + index as u64 * 2, 9.), &mut out);
        book.evaluate(&snapshot(&draft, 4 + index as u64 * 2, 11.), &mut out);
    }
    assert_eq!(out.len(), 3);
    let mut unavailable = snapshot(&draft, 5, 9.);
    unavailable.availability = Err(Unavailable::ReplayPaused);
    book.evaluate(&unavailable, &mut out);
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::ReplayPaused)
    );
    book.evaluate(&snapshot(&draft, 6, 11.), &mut out);
    assert_eq!(out.len(), 3);
}
#[test]
fn validation_and_atomic_restore() {
    let book = AlertBook::default();
    let draft = spec(Operator::Crossing);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut d = draft.clone();
        d.conditions[0].target = TargetRef::Value(invalid);
        assert!(book.validate(&d).is_err());
    }
    let mut d = draft.clone();
    d.conditions.clear();
    assert!(book.validate(&d).is_err());
    d.conditions = vec![draft.conditions[0].clone(); MAX_CONDITIONS + 1];
    assert!(book.validate(&d).is_err());
    d = draft.clone();
    d.version = 99;
    assert!(matches!(
        book.validate(&d),
        Err(AlertError::UnsupportedVersion(99))
    ));
    d = draft.clone();
    d.notification.sound = Some("missing".into());
    assert!(book.validate(&d).is_err());
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft), 0).unwrap();
    let before = book.document().unwrap();
    let mut broken = before.clone();
    broken.definitions.push(broken.definitions[0].clone());
    assert!(book.restore(broken, 0).is_err());
    assert_eq!(before, book.document().unwrap());
}
#[test]
fn geometry_edits_reseed_compound_and_close_only_status() {
    let target = drawing();
    let mut draft = spec(Operator::CrossingUp);
    draft.conditions.push(Predicate {
        operator: Operator::Inside,
        target: TargetRef::Drawing(target.clone()),
    });
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let mut out = BoundedEvents::new(10);
    for (sequence, price, revision) in [(0, 9., 1), (1, 11., 2)] {
        let facts = [DrawingFact {
            target: &target,
            revision,
            geometry: Ok(Geometry::Rectangle {
                first: Point {
                    coordinate: 0.,
                    price: 0.,
                },
                second: Point {
                    coordinate: 10.,
                    price: 20.,
                },
            }),
        }];
        let mut input = snapshot(&draft, sequence, price);
        input.drawings = &facts;
        book.evaluate(&input, &mut out);
    }
    assert!(
        out.is_empty(),
        "editing any condition reseeds all crossings"
    );
    draft.frequency = Frequency::PerBarClose;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let facts = [DrawingFact {
        target: &target,
        revision: 1,
        geometry: Ok(Geometry::Rectangle {
            first: Point {
                coordinate: 0.,
                price: 0.,
            },
            second: Point {
                coordinate: 10.,
                price: 20.,
            },
        }),
    }];
    let mut input = snapshot(&draft, 0, 9.);
    input.drawings = &facts;
    input.kind = ObservationKind::CommittedClose;
    book.evaluate(&input, &mut out);
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Monitoring);
    book.evaluate(&snapshot(&draft, 1, 9.), &mut out);
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::MissingDrawing)
    );
}
#[test]
fn restart_and_reconnect_minute_cooldown_uses_only_session_time() {
    let mut draft = spec(Operator::GreaterThan);
    draft.frequency = Frequency::PerMinute;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(draft.clone()), 0)
        .unwrap();
    let mut out = BoundedEvents::new(10);
    let mut input = snapshot(&draft, 0, 11.);
    input.utc_ms = 1_000_000;
    input.session_ms = 99_000;
    book.evaluate(&input, &mut out);
    assert_eq!(out.len(), 1);
    book.restore(book.document().unwrap(), 1_000_010).unwrap();
    input.session_ms = 0;
    input.utc_ms = 1_000_010;
    book.evaluate(&input, &mut out);
    input.session_ms = 1;
    input.utc_ms = 9_000_000;
    book.evaluate(&input, &mut out);
    assert_eq!(out.len(), 1);
    input.session_ms = 60_000;
    book.evaluate(&input, &mut out);
    assert_eq!(out.len(), 2);
    input.bar.epoch = 2;
    input.session_ms = 0;
    book.evaluate(&input, &mut out);
    input.session_ms = 1;
    input.utc_ms = 99_000_000;
    book.evaluate(&input, &mut out);
    assert_eq!(out.len(), 2);
    input.session_ms = 60_000;
    book.evaluate(&input, &mut out);
    assert_eq!(out.len(), 3);
}
