use super::*;
use crate::worker_progress::{Age, Phase, tests::Gate};
use quantick_engine::{Bar, Side};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use std::time::Duration;

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).unwrap()
}
fn print() -> Trade {
    Trade {
        agg_id: 1,
        timestamp_ms: 1001,
        price: Decimal::from(100),
        quantity: Decimal::from(2),
        side: Side::Buy,
    }
}
fn setup() -> BookCommand {
    BookCommand::SetEnabled {
        enabled: true,
        generation_floor: 10,
    }
}
fn request(first_bar_index: usize) -> ProjectionRequest {
    ProjectionRequest {
        timeline_revision: 1,
        first_bar_index,
        closed: vec![Bar {
            open_time: 999,
            close_time: 1002,
            open: Decimal::from(100),
            high: Decimal::from(100),
            low: Decimal::from(100),
            close: Decimal::from(100),
            buy_volume: Decimal::from(2),
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }],
        partial: None,
        lane: false,
        on_newest_bar: true,
        lane_reference_ms: None,
        price_range: (98.0, 102.0),
    }
}
fn replay(worker: &BookWorker) -> Receiver<()> {
    worker.send(BookCommand::ApplyVisualConfig(HeatmapConfig {
        show_aggressions: true,
        ..HeatmapConfig::default()
    }));
    worker.send(BookCommand::Depth {
        event: DepthEvent::Snapshot {
            symbol: "BTCUSDT".into(),
            generation: 10,
            observed_at_ms: 1000,
            effective_at_ms: 999,
            price_step: Some(Decimal::ONE),
            snapshot: BookSnapshot::new(
                10,
                vec![level(99, 5)],
                vec![level(101, 6)],
                BookCoverage::Limited { levels_per_side: 1 },
            ),
        },
        received_at_ms: 1000,
    });
    worker.send(BookCommand::Depth {
        event: DepthEvent::Update {
            symbol: "BTCUSDT".into(),
            generation: 10,
            event_time_ms: 1001,
            delta: BookDelta::new(11, 11, vec![level(99, 7)], vec![level(101, 9)]),
        },
        received_at_ms: 1003,
    });
    worker.send(BookCommand::Trade(print()));
    worker.send(BookCommand::Project(request(9)));
    worker.send(BookCommand::Project(request(0)));
    let (tx, rx) = channel();
    worker.send(BookCommand::Flush(tx));
    rx
}
fn assert_book(published: &BookPublished) {
    assert_eq!(published.health.generation, Some(10));
    assert_eq!(published.health.last_update_id, Some(11));
    assert_eq!(published.health.depth_updates, 1);
    assert_eq!(published.health.snapshots, 1);
    assert_eq!(published.health.gaps, 0);
    assert_eq!(published.health.aggression_count, 1);
    assert_eq!(published.health.arrival_latency_ms, Some(2));
    let ladder = published.ladder.as_ref().unwrap();
    assert_eq!(ladder.bids, vec![level(99, 7)]);
    assert_eq!(ladder.asks, vec![level(101, 9)]);
    let frame = published.frame.as_ref().unwrap();
    assert_eq!((frame.first_bar_index, frame.slot_count), (0, 1));
    // No live lane: archived quantities 5 and 6 span [999,1001]. The latest
    // book time is 1001, so new open runs 7 and 9 have no drawable duration.
    let mut cells: Vec<_> = frame
        .projection
        .cells
        .iter()
        .map(|cell| (cell.generation, cell.price_bucket, cell.quantity))
        .collect();
    cells.sort();
    assert_eq!(
        cells,
        vec![
            (10, Decimal::from(99), Decimal::from(5)),
            (10, Decimal::from(101), Decimal::from(6))
        ]
    );
}

#[test]
fn held_real_book_reports_backlog_publication_and_all_ordered_data() {
    // Prescribed batches: SetEnabled (1), then config/snapshot/update/trade/
    // two Project/Flush (7). Update 11 replaces bid 5->7, ask 6->9; +2 trade.
    let clock = Gate::new();
    let first = clock.hold(Phase::Applying);
    let second = clock.hold(Phase::Applying);
    let publication = clock.hold(Phase::Publishing);
    let progress = WorkerProgress::with_clock(clock.clone());
    let worker = BookWorker::spawn_with_progress("BTCUSDT", progress.clone());
    worker.send(setup());
    first.reached();
    clock.at(10);
    let ack = replay(&worker);
    clock.at(40);
    let held = worker.progress();
    assert!(held.valid);
    assert_eq!(
        (
            held.counts.accepted,
            held.counts.queued,
            held.counts.inflight,
            held.counts.retired
        ),
        (8, 7, 1, 0)
    );
    assert_eq!(held.oldest_wait, Age::Known(30));
    assert_eq!(held.processing_age, Age::Known(40));
    println!("Q8_BOOK_DEGRADED {}", serde_json::to_string(&held).unwrap());
    first.release();
    second.reached();
    clock.at(70);
    second.release();
    publication.reached();
    let publishing = worker.progress();
    assert_eq!(publishing.phase, Phase::Publishing);
    assert_eq!(publishing.counts.projects_superseded, 1);
    assert_eq!(publishing.counts.mailbox_replacements, 1);
    assert_eq!(publishing.processing_age, Age::Known(30));
    publication.release();
    ack.recv_timeout(Duration::from_secs(10))
        .expect("book fixture published");
    let done = worker.progress();
    assert_eq!(done.instance, held.instance);
    assert_eq!(
        (
            done.counts.retired,
            done.counts.queued,
            done.counts.inflight,
            done.counts.cycles,
            done.counts.mailbox_replacements
        ),
        (8, 0, 0, 2, 2)
    );
    assert_book(&worker.published());
    println!(
        "Q8_BOOK_RECOVERED {}",
        serde_json::to_string(&done).unwrap()
    );
    // Symbol reset is a live operation on this identity, never resurrection.
    worker.send(BookCommand::ResetForSymbol("ETHUSDT".into()));
    let (tx, rx) = channel();
    worker.send(BookCommand::Flush(tx));
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(worker.progress().instance, held.instance);
    assert_eq!(worker.published().health.generation, None);
    let closed = clock.hold(Phase::Closed);
    drop(worker);
    closed.reached();
    assert_eq!(progress.snapshot().phase, Phase::Closed);
    assert_eq!(progress.snapshot().counts.unfinished, 0);
    closed.release();
}

#[test]
fn book_unwind_failed_send_and_existing_constructor_recovery_are_attributable() {
    let clock = Gate::new();
    let first = clock.hold(Phase::Applying);
    let terminal = clock.hold(Phase::Unwound);
    let progress = WorkerProgress::with_clock(clock);
    let worker = BookWorker::spawn_with_progress("BTCUSDT", progress);
    worker.send(setup());
    first.reached();
    let ack = replay(&worker);
    first.unwind();
    terminal.reached();
    let stopped = worker.progress();
    assert_eq!(stopped.phase, Phase::Unwound);
    assert_eq!(
        (
            stopped.counts.accepted,
            stopped.counts.unfinished,
            stopped.counts.retired
        ),
        (8, 8, 0)
    );
    terminal.release();
    assert!(matches!(
        ack.recv_timeout(Duration::from_secs(10)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
    worker.send(setup());
    assert_eq!(worker.progress().counts.failed_sends, 1);
    let replacement = BookWorker::spawn("BTCUSDT");
    replacement.send(setup());
    replay(&replacement)
        .recv_timeout(Duration::from_secs(10))
        .expect("replacement replay published");
    assert_ne!(replacement.progress().instance, stopped.instance);
    assert_eq!(replacement.progress().counts.accepted, 8);
    assert_book(&replacement.published());
    println!(
        "Q8_BOOK_TERMINAL {}",
        serde_json::to_string(&worker.progress()).unwrap()
    );
}
