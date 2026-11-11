use quantick_alerts::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
struct Counter;
fn note() {
    TRACK.with(|track| {
        if track.get() {
            ALLOCATIONS.with(|n| n.set(n.get() + 1));
        }
    });
}
// SAFETY: every operation delegates the exact pointer/layout contract to System.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller supplied a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies the pointer and original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        note();
        // SAFETY: the caller supplies a live allocation and valid new size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

use quantick_alerts::observer::{Drawing, Observation, Observer, Pane, Source, SourceKey};

fn definition(target: TargetRef, frequency: Frequency) -> AlertSpec {
    AlertSpec {
        version: FORMAT_VERSION,
        id: AlertId(0),
        name: "Observer regression".into(),
        market: MarketRef {
            source: SourceContext::Live {
                feed: "fixture".into(),
            },
            symbol: "TEST".into(),
            aggregation: "tick:1".into(),
        },
        conditions: vec![Predicate {
            operator: Operator::GreaterThan,
            target,
        }],
        frequency,
        expiration: Expiration::Never,
        message: String::new(),
        notification: Notification {
            popup: false,
            toast: false,
            sound: None,
            sound_seconds: None,
        },
    }
}
fn target(id: u64) -> DurableDrawingRef {
    DurableDrawingRef {
        workspace: "workspace".into(),
        origin: "origin".into(),
        pane: "pane".into(),
        drawing: id,
        incarnation: format!("incarnation-{id}"),
    }
}
fn line(price: f64) -> Result<Geometry, Unavailable> {
    Ok(Geometry::Line(Line {
        first: Point {
            coordinate: 0.0,
            price,
        },
        second: Point {
            coordinate: 10.0,
            price,
        },
        extend_left: true,
        extend_right: true,
    }))
}
fn source(observer: &mut Observer<u8>, generation: u64) {
    observer.source(Source {
        key: SourceKey::Live("fixture"),
        symbol: "TEST",
        generation,
        availability: Ok(()),
    });
}
fn observation(kind: ObservationKind, slot: usize, price: f64) -> Observation {
    Observation {
        kind,
        prefix_slots: 0,
        committed_slots: slot + 1,
        has_partial: false,
        close_slot: (kind == ObservationKind::CommittedClose).then_some(slot),
        price: Some(price),
        utc_ms: 1,
        session_ms: 1,
    }
}
fn pane(
    observer: &mut Observer<u8>,
    book: &mut AlertBook,
    out: &mut BoundedEvents,
    id: u64,
    revision: u64,
    input: Observation,
    drawings: &[Drawing<'_>],
) {
    observer.pane(
        book,
        out,
        Pane {
            id,
            spec: &1,
            drawing_revision: revision,
            hydrating: false,
        },
        input,
        || "tick:1".into(),
        drawings.iter().cloned(),
    );
}
#[test]
fn post_close_trade_uses_last_committed_bar_for_both_cadences() {
    for frequency in [Frequency::PerBar, Frequency::PerBarClose] {
        let mut book = AlertBook::default();
        book.command(
            AlertCommand::Create(definition(TargetRef::Value(100.0), frequency)),
            0,
        )
        .unwrap();
        let mut out = BoundedEvents::new(8);
        let mut observer = Observer::default();
        observer.begin();
        source(&mut observer, 0);
        for slot in 0..3 {
            pane(
                &mut observer,
                &mut book,
                &mut out,
                1,
                0,
                observation(ObservationKind::CommittedClose, slot, 101.0),
                &[],
            );
        }
        pane(
            &mut observer,
            &mut book,
            &mut out,
            1,
            0,
            observation(ObservationKind::Trade, 2, 101.0),
            &[],
        );
        assert_eq!(
            out.len(),
            3,
            "the trade after multiple closes does not invent a fourth bar"
        );
        pane(
            &mut observer,
            &mut book,
            &mut out,
            1,
            0,
            observation(ObservationKind::CommittedClose, 3, 101.0),
            &[],
        );
        assert_eq!(out.len(), 4, "the next actual close retains its budget");
    }
    let mut input = observation(ObservationKind::Trade, 3, 101.0);
    input.prefix_slots = 20;
    assert_eq!(input.slot(), 23);
    input.has_partial = true;
    assert_eq!(input.slot(), 24);
}
#[test]
fn relevant_target_after_unrelated_drawings_and_duplicate_identity() {
    let referenced = target(3000);
    let mut book = AlertBook::default();
    book.command(
        AlertCommand::Create(definition(
            TargetRef::Drawing(referenced.clone()),
            Frequency::PerBar,
        )),
        0,
    )
    .unwrap();
    let unrelated: Vec<_> = (0..3000).map(target).collect();
    let mut facts: Vec<_> = unrelated
        .iter()
        .map(|target| Drawing {
            target,
            known_mirror: true,
            geometry: line(100.0),
        })
        .collect();
    facts.push(Drawing {
        target: &referenced,
        known_mirror: true,
        geometry: line(100.0),
    });
    let mut observer = Observer::default();
    let mut out = BoundedEvents::new(8);
    observer.begin();
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 0, 101.0),
        &facts,
    );
    assert_eq!(
        out.len(),
        1,
        "unrelated identities cannot exhaust relevant target storage"
    );
    facts.push(*facts.last().unwrap());
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        1,
        observation(ObservationKind::Trade, 1, 101.0),
        &facts,
    );
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::AmbiguousDrawing)
    );
    assert_eq!(
        out.len(),
        1,
        "even equal duplicate geometry in one owner is ambiguous"
    );
}
#[test]
fn mirror_reset_invalidates_geometry_without_resetting_owner_crossing() {
    let referenced = target(1);
    let mut rule = definition(TargetRef::Drawing(referenced.clone()), Frequency::PerBar);
    rule.conditions[0].operator = Operator::CrossingUp;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(rule), 0).unwrap();
    let mut observer = Observer::default();
    let mut out = BoundedEvents::new(8);
    let mut facts = [Drawing {
        target: &referenced,
        known_mirror: true,
        geometry: line(100.0),
    }];
    observer.begin();
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 0, 99.0),
        &facts,
    );
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &facts,
    );
    observer.reset(&mut book, ResetReason::Hydration);
    facts[0].geometry = line(110.0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &facts,
    );
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &[],
    );
    observer.end(&mut book, 1, 1);
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::AmbiguousDrawing),
        "mirror reanchoring must refresh even with unchanged drawing revision"
    );
    // Restore the mirror to the owner's unchanged geometry; after preflight
    // clears ambiguity, the next price seeds once and the following crosses.
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        1,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &[],
    );
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 1, 99.0),
        &[],
    );
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 2, 101.0),
        &[],
    );
    assert_eq!(out.len(), 1);
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        1,
        observation(ObservationKind::Lifecycle, 2, 101.0),
        &[],
    );
    observer.reset(&mut book, ResetReason::Hydration);
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 3, 99.0),
        &[],
    );
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 4, 101.0),
        &[],
    );
    assert_eq!(
        out.len(),
        2,
        "a mirror reset cannot repeatedly erase owner baselines"
    );
}
#[test]
fn cached_no_fire_observations_allocate_nothing_and_do_not_visit_drawings() {
    let referenced = target(1);
    let mut book = AlertBook::default();
    book.command(
        AlertCommand::Create(definition(
            TargetRef::Drawing(referenced.clone()),
            Frequency::PerBar,
        )),
        0,
    )
    .unwrap();
    let mut observer = Observer::default();
    let mut out = BoundedEvents::new(8);
    observer.begin();
    source(&mut observer, 0);
    let facts = [Drawing {
        target: &referenced,
        known_mirror: true,
        geometry: line(100.0),
    }];
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 0, 99.0),
        &facts,
    );
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Monitoring);
    let mut visits = 0;
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
    for slot in 1..1000 {
        observer.begin();
        source(&mut observer, 0);
        observer.pane(
            &mut book,
            &mut out,
            Pane {
                id: 1,
                spec: &1,
                drawing_revision: 0,
                hydrating: false,
            },
            observation(ObservationKind::Trade, slot, 99.0),
            || panic!("binding text must stay cached"),
            facts.iter().cloned().inspect(|_| visits += 1),
        );
        observer.end(&mut book, 1, 1);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(visits, 0);
    assert!(out.is_empty());
}

#[test]
fn idle_census_observes_mirror_removal_before_owner_preflight() {
    let referenced = target(1);
    let mut book = AlertBook::default();
    book.command(
        AlertCommand::Create(definition(
            TargetRef::Drawing(referenced.clone()),
            Frequency::PerBar,
        )),
        0,
    )
    .unwrap();
    let mut observer = Observer::default();
    let mut out = BoundedEvents::new(8);
    let facts = [Drawing {
        target: &referenced,
        geometry: line(100.0),
        known_mirror: true,
    }];
    observer.begin();
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &[],
    );
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &facts,
    );
    observer.end(&mut book, 1, 1);
    assert_eq!(book.views().unwrap()[0].status, AlertStatus::Monitoring);
    observer.begin();
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &[],
    );
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        1,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &[],
    );
    observer.end(&mut book, 1, 1);
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::MissingDrawing)
    );
    assert!(out.is_empty());
}
#[test]
fn equal_geometry_requires_known_mirror_provenance() {
    let referenced = target(1);
    let mut book = AlertBook::default();
    book.command(
        AlertCommand::Create(definition(
            TargetRef::Drawing(referenced.clone()),
            Frequency::PerBar,
        )),
        0,
    )
    .unwrap();
    let mut observer = Observer::default();
    let mut out = BoundedEvents::new(8);
    let mut facts = [Drawing {
        target: &referenced,
        geometry: line(100.0),
        known_mirror: true,
    }];
    observer.begin();
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &facts,
    );
    facts[0].known_mirror = false;
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        2,
        0,
        observation(ObservationKind::Lifecycle, 0, 99.0),
        &facts,
    );
    observer.end(&mut book, 1, 1);
    assert_eq!(
        book.views().unwrap()[0].status,
        AlertStatus::Suspended(Unavailable::AmbiguousDrawing)
    );
}
#[test]
fn cosmetic_refresh_preserves_crossing_and_geometry_change_reseeds() {
    let referenced = target(1);
    let mut rule = definition(TargetRef::Drawing(referenced.clone()), Frequency::PerBar);
    rule.conditions[0].operator = Operator::CrossingUp;
    let mut book = AlertBook::default();
    book.command(AlertCommand::Create(rule), 0).unwrap();
    let mut observer = Observer::default();
    let mut out = BoundedEvents::new(8);
    let mut facts = [Drawing {
        target: &referenced,
        geometry: line(100.0),
        known_mirror: true,
    }];
    observer.begin();
    source(&mut observer, 0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        0,
        observation(ObservationKind::Trade, 0, 99.0),
        &facts,
    );
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        1,
        observation(ObservationKind::Lifecycle, 0, 101.0),
        &facts,
    );
    observer.end(&mut book, 1, 1);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        1,
        observation(ObservationKind::Trade, 1, 101.0),
        &facts,
    );
    assert_eq!(
        out.len(),
        1,
        "selection/style revision does not consume a crossing"
    );
    facts[0].geometry = line(90.0);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        2,
        observation(ObservationKind::Lifecycle, 1, 101.0),
        &facts,
    );
    observer.end(&mut book, 1, 1);
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        2,
        observation(ObservationKind::Trade, 2, 101.0),
        &facts,
    );
    assert_eq!(
        out.len(),
        1,
        "moving a line through the price reseeds without firing"
    );
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        2,
        observation(ObservationKind::Trade, 3, 89.0),
        &facts,
    );
    pane(
        &mut observer,
        &mut book,
        &mut out,
        1,
        2,
        observation(ObservationKind::Trade, 4, 91.0),
        &facts,
    );
    assert_eq!(out.len(), 2);
}

#[test]
fn unavailable_owner_hands_off_once_and_recovery_does_not_steal() {
    for unavailable in [
        Unavailable::SourceInactive,
        Unavailable::Hydrating,
        Unavailable::ReplayPaused,
    ] {
        let mut rule = definition(TargetRef::Value(100.0), Frequency::PerBar);
        rule.conditions[0].operator = Operator::CrossingUp;
        let mut book = AlertBook::default();
        book.command(AlertCommand::Create(rule), 0).unwrap();
        let mut observer = Observer::default();
        let mut out = BoundedEvents::new(8);
        observer.begin();
        source(&mut observer, 0);
        pane(
            &mut observer,
            &mut book,
            &mut out,
            1,
            0,
            observation(ObservationKind::Trade, 0, 99.0),
            &[],
        );
        source(&mut observer, 0);
        pane(
            &mut observer,
            &mut book,
            &mut out,
            2,
            0,
            observation(ObservationKind::Trade, 0, 99.0),
            &[],
        );
        observer.end(&mut book, 1, 1);
        observer.begin();
        observer.source(Source {
            key: SourceKey::Live("fixture"),
            symbol: "TEST",
            generation: 0,
            availability: Err(unavailable),
        });
        pane(
            &mut observer,
            &mut book,
            &mut out,
            1,
            0,
            observation(ObservationKind::Lifecycle, 0, 99.0),
            &[],
        );
        source(&mut observer, 0);
        pane(
            &mut observer,
            &mut book,
            &mut out,
            2,
            0,
            observation(ObservationKind::Trade, 1, 101.0),
            &[],
        );
        assert!(
            out.is_empty(),
            "handover must seed the new subscriber's tape"
        );
        pane(
            &mut observer,
            &mut book,
            &mut out,
            2,
            0,
            observation(ObservationKind::Trade, 2, 99.0),
            &[],
        );
        pane(
            &mut observer,
            &mut book,
            &mut out,
            2,
            0,
            observation(ObservationKind::Trade, 3, 101.0),
            &[],
        );
        assert_eq!(out.len(), 1, "a healthy mirror must keep monitoring");
        observer.end(&mut book, 1, 1);
        observer.begin();
        source(&mut observer, 1);
        pane(
            &mut observer,
            &mut book,
            &mut out,
            1,
            0,
            observation(ObservationKind::Trade, 4, 99.0),
            &[],
        );
        pane(
            &mut observer,
            &mut book,
            &mut out,
            1,
            0,
            observation(ObservationKind::Trade, 5, 101.0),
            &[],
        );
        assert_eq!(
            out.len(),
            1,
            "recovered former owner cannot duplicate the mirror's observation"
        );
        source(&mut observer, 0);
        pane(
            &mut observer,
            &mut book,
            &mut out,
            2,
            0,
            observation(ObservationKind::Trade, 4, 99.0),
            &[],
        );
        pane(
            &mut observer,
            &mut book,
            &mut out,
            2,
            0,
            observation(ObservationKind::Trade, 5, 101.0),
            &[],
        );
        observer.end(&mut book, 1, 1);
        assert_eq!(out.len(), 2);
        let events = book.history(0, 8).unwrap();
        assert_eq!(
            events[0].bar_epoch, events[1].bar_epoch,
            "handover happens once; recovery preserves the new owner's baseline"
        );
    }
}
