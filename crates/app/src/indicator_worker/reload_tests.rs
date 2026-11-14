use super::*;
use crate::indicators::IndicatorViews;

const GOOD_V1: &str = "//@version=5\nindicator(\"r\")\nplot(close)\n";
const GOOD_V2: &str = "//@version=5\nindicator(\"r2\")\nplot(close * 2)\n";
const BROKEN: &str = "//@version=5\nindicator(\"r\")\nplot(request.security(close))\n";

fn drive() -> (
    IndicatorWorker,
    IndicatorViews,
    SlotId,
    Vec<quantick_engine::Bar>,
) {
    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "r.pine".to_owned(),
            text: GOOD_V1.to_owned(),
        },
    });
    let trades: Vec<quantick_engine::Trade> = (1..=8).map(tests::trade).collect();
    let mut builder = quantick_engine::TickBarBuilder::new(2);
    let bars = quantick_engine::golden::replay(&mut builder, &trades);
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    // Drain the initial publication (a frame passes before any reload
    // in reality; batching them would let the initial Rebuilt clear the
    // stale flag the reload sets).
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    (worker, views, slot, bars)
}

#[test]
fn a_good_reload_recompiles_and_replays() {
    let (worker, mut views, slot, bars) = drive();
    worker.send(IndicatorCommand::Reload {
        slot,
        source: IndicatorSource::Script {
            name: "r.pine".to_owned(),
            text: GOOD_V2.to_owned(),
        },
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    assert_eq!(view.descriptor.title, "r2", "the new version runs");
    assert!(view.stale.is_none());
    assert_eq!(view.rows, bars.len(), "replayed over the full history");
}

#[test]
fn a_broken_reload_keeps_the_last_good_version_and_flags_stale() {
    let (worker, mut views, slot, bars) = drive();
    worker.send(IndicatorCommand::Reload {
        slot,
        source: IndicatorSource::Script {
            name: "r.pine".to_owned(),
            text: BROKEN.to_owned(),
        },
    });
    worker.send(IndicatorCommand::BarClosed(bars[0].clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    assert_eq!(view.descriptor.title, "r", "the old version keeps running");
    assert!(view.error.is_none(), "stale is not a runtime error");
    let stale = view.stale.as_ref().expect("flagged stale");
    assert!(stale.contains("PINE_NO_SECURITY"), "{stale}");
    assert_eq!(
        view.rows,
        bars.len() + 1,
        "the running version even saw the new bar"
    );
}

/// The flag belongs to the worker, so an unrelated rebuild cannot clear
/// it. Before this, scrolling back to prepend history dropped the amber
/// dot while the pre-edit script was still the one running — and it never
/// came back, because the poll had already advanced its mtime.
#[test]
fn a_rebuild_does_not_clear_the_stale_flag() {
    let (worker, mut views, slot, bars) = drive();
    worker.send(IndicatorCommand::Reload {
        slot,
        source: IndicatorSource::Script {
            name: "r.pine".to_owned(),
            text: BROKEN.to_owned(),
        },
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    assert!(views.all()[0].stale.is_some(), "flagged after a bad edit");

    // Exactly what scrolling left does.
    worker.send(IndicatorCommand::Rebuild(bars.clone(), None));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    assert!(
        view.stale.is_some(),
        "the file on disk still has errors; the dot must stay"
    );
    assert_eq!(view.descriptor.title, "r", "and the old version still runs");
}

/// The branch the commit message advertises — "a slot whose first load
/// failed is healed by its first good reload" — reachable whenever a
/// script reads but does not compile.
#[test]
fn a_slot_that_never_loaded_is_healed_by_a_good_reload() {
    let trades: Vec<quantick_engine::Trade> = (1..=6).map(tests::trade).collect();
    let mut builder = quantick_engine::TickBarBuilder::new(2);
    let bars = quantick_engine::golden::replay(&mut builder, &trades);

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "r.pine".to_owned(),
            text: BROKEN.to_owned(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    assert!(views.all()[0].error.is_some(), "the first load failed");

    worker.send(IndicatorCommand::Reload {
        slot,
        source: IndicatorSource::Script {
            name: "r.pine".to_owned(),
            text: GOOD_V1.to_owned(),
        },
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    assert!(view.error.is_none(), "the good reload healed the slot");
    assert_eq!(
        view.rows,
        bars.len(),
        "and the healed instance caught up over the existing history"
    );
}
