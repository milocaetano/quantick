//! Proposed identical baseline/candidate Q8 harness; not built or measured yet.
//! Register this file as `#[cfg(test)] mod worker_progress_bench;` at app root.
//! Register the separate `worker_progress_bench_observer` adapter beside it.
//! Exact ignored test name: `worker_progress_bench::dense_workers`.
//! Oracles are independently specified in Q8-measurement-plan.md, pending run.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use quantick_engine::{Bar, BarBuilder, DollarBarBuilder, Side, TimeBarBuilder, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot, DepthEvent};
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::engine::{BookPublished, ProjectionRequest};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use crate::indicator_worker::{
    IndicatorCommand, IndicatorEvent, IndicatorSource, IndicatorWorker, LaneSample, SlotId,
};
use crate::indicators::IndicatorViews;
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::worker_progress_bench_observer::{book_snapshot, indicator_snapshot};

const FIXTURE_ID: &str = "q8-dense-workers-v1";
const WARMUP: usize = 30;
const MEASURED: usize = 600;
const STEPS: usize = 64;
const TOTAL_TRADES: u64 = 40_320;
const ACK_LIMIT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
enum Spec {
    Time,
    Dollar,
}

impl Spec {
    fn name(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Dollar => "dollar",
        }
    }

    fn counts(self) -> (usize, usize) {
        match self {
            Self::Time => (40_953, 38_400),
            Self::Dollar => (40_955, 38_402),
        }
    }
}

struct Samples {
    admission_ns: Vec<u64>,
    admission_kind: Vec<&'static str>,
    completion_ns: Vec<u64>,
}

impl Samples {
    fn new(commands: usize) -> Self {
        Self {
            admission_ns: Vec::with_capacity(commands),
            admission_kind: Vec::with_capacity(commands),
            completion_ns: Vec::with_capacity(MEASURED),
        }
    }

    fn record(&mut self, elapsed: Duration, kind: &'static str) {
        self.admission_ns.push(nanos(elapsed));
        self.admission_kind.push(kind);
    }

    fn publish(
        self,
        variant: &str,
        total: usize,
        measured: usize,
        wall: Duration,
        telemetry: Value,
        oracle: Value,
    ) {
        assert_eq!(self.admission_ns.len(), measured);
        assert_eq!(self.admission_kind.len(), measured);
        assert_eq!(self.completion_ns.len(), MEASURED);
        println!(
            "\nQ8_WORKER_BENCH {}",
            json!({
                "variant": variant,
                "fixture_id": FIXTURE_ID,
                "oracle_pass": true,
                "counts": {"total_commands": total, "measured_commands": measured,
                           "total_trades": TOTAL_TRADES, "measured_trades": 38400,
                           "warmup_bursts": WARMUP, "measured_bursts": MEASURED},
                "admission_ns": self.admission_ns,
                "admission_kind": self.admission_kind,
                "completion_ns": self.completion_ns,
                "wall_ns": nanos(wall),
                "wall_scope": "Measured bursts plus output drain/read and between-burst bookkeeping",
                "completion_scope": "First command admission through checked publication acknowledgement",
                "telemetry": telemetry,
                "oracle": oracle,
            })
        );
    }
}

fn nanos(duration: Duration) -> u64 {
    duration
        .as_nanos()
        .try_into()
        .expect("fixture duration fits u64 nanoseconds")
}

fn indicator_print(i: u64) -> Trade {
    Trade {
        agg_id: i,
        timestamp_ms: i.try_into().expect("fixture timestamp"),
        price: Decimal::from(60_000),
        quantity: Decimal::new(1, 2),
        side: if i.is_multiple_of(3) {
            Side::Sell
        } else {
            Side::Buy
        },
    }
}

fn assert_indicator_bar(bar: &Bar, first: u64, last: u64) {
    let count = last - first + 1;
    let sells = last / 3 - (first - 1) / 3;
    assert_eq!(bar.open_time, i64::try_from(first).unwrap());
    assert_eq!(bar.close_time, i64::try_from(last).unwrap());
    assert_eq!(bar.trade_count, count);
    for price in [bar.open, bar.high, bar.low, bar.close] {
        assert_eq!(price, Decimal::from(60_000));
    }
    assert_eq!(bar.sell_volume, Decimal::new(sells.try_into().unwrap(), 2));
    assert_eq!(
        bar.buy_volume,
        Decimal::new((count - sells).try_into().unwrap(), 2)
    );
}

fn indicator_fixture(spec: Spec) -> Vec<Vec<IndicatorCommand>> {
    let mut builder: Box<dyn BarBuilder> = match spec {
        Spec::Time => Box::new(TimeBarBuilder::new(60_000)),
        Spec::Dollar => Box::new(DollarBarBuilder::new(Decimal::from(12_000_000))),
    };
    let mut bursts = Vec::with_capacity(WARMUP + MEASURED);
    let mut closed_count = 0_u64;
    for burst in 0..WARMUP + MEASURED {
        let mut commands = Vec::with_capacity(STEPS + 1);
        for offset in 0..STEPS {
            let i = u64::try_from(burst * STEPS + offset + 1).unwrap();
            let trade = indicator_print(i);
            if let Some(closed) = builder.push(&trade) {
                assert!(matches!(spec, Spec::Dollar), "time fixture has no close");
                assert!(i == 20_000 || i == 40_000, "independent dollar close index");
                assert_indicator_bar(&closed, closed_count * 20_000 + 1, i);
                closed_count += 1;
                commands.push(IndicatorCommand::BarClosed(closed));
            }
            let partial = builder.partial().cloned();
            let suffix = if partial.is_some() {
                vec![trade]
            } else {
                Vec::new()
            };
            commands.push(IndicatorCommand::PartialUpdated {
                partial,
                run: suffix,
                rungs: 16,
            });
        }
        bursts.push(commands);
    }
    match spec {
        Spec::Time => {
            assert_eq!(closed_count, 0);
            assert_indicator_bar(builder.partial().unwrap(), 1, TOTAL_TRADES);
        }
        Spec::Dollar => {
            assert_eq!(closed_count, 2);
            assert_indicator_bar(builder.partial().unwrap(), 40_001, TOTAL_TRADES);
        }
    }
    let emitted: usize = bursts.iter().map(Vec::len).sum();
    assert_eq!(emitted + 2 + WARMUP + MEASURED + 1, spec.counts().0);
    bursts
}

fn collect_indicator(
    worker: &IndicatorWorker,
    views: &mut IndicatorViews,
    last_lane: &mut Vec<LaneSample>,
) {
    for event in worker.drain_events() {
        assert!(
            !matches!(&event, IndicatorEvent::Error { .. }),
            "indicator emitted an error"
        );
        if let IndicatorEvent::Lane { samples, .. } = &event
            && !samples.is_empty()
        {
            last_lane.clone_from(samples);
        }
        views.apply(event);
    }
}

fn near(actual: f64, expected: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= 1e-9,
        "actual {actual}, independent expected {expected}"
    );
}

fn signed_cvd_at(id: u64) -> f64 {
    // Integer arithmetic over the specified side pattern, not the worker/host.
    let hundredths = id - 2 * (id / 3);
    f64::from(u32::try_from(hundredths).unwrap()) / 100.0
}

fn assert_indicator_output(spec: Spec, views: &IndicatorViews, lane: &[LaneSample]) -> Value {
    assert_eq!(views.all().len(), 1);
    let view = &views.all()[0];
    assert_eq!(view.columns.len(), 1);
    match spec {
        Spec::Time => assert!(view.columns[0].is_empty()),
        Spec::Dollar => {
            assert_eq!(view.columns[0].len(), 2);
            near(view.columns[0][0], 66.68);
            near(view.columns[0][1], 133.34);
        }
    }
    let preview = view.preview.as_ref().expect("actual CVD preview");
    assert_eq!(preview.values.len(), 1);
    near(preview.values[0], 134.40);
    assert_eq!(lane.len(), 16);
    for (index, rung) in lane.iter().enumerate() {
        let k = u64::try_from(index + 1).unwrap();
        let id = match spec {
            Spec::Time => 2520 * k,
            Spec::Dollar => 40_000 + 20 * k,
        };
        assert_eq!(rung.close_time, i64::try_from(id).unwrap());
        assert_eq!(rung.values.len(), 1);
        near(rung.values[0], signed_cvd_at(id));
    }
    json!({"cvd_columns": view.columns, "cvd_preview": preview.values,
           "lane": lane.iter().map(|r| json!({"id": r.close_time, "values": r.values}))
                       .collect::<Vec<_>>()})
}

fn measure_indicator(spec: Spec) {
    let bursts = indicator_fixture(spec);
    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    assert_eq!(views.allocate_slot("native.cvd"), SlotId(0));
    let mut last_lane = Vec::new();
    worker.send(IndicatorCommand::Add {
        slot: SlotId(0),
        source: IndicatorSource::Native {
            id: "native.cvd".to_owned(),
            values: Vec::new(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(Vec::new()));
    let (setup_tx, setup_rx) = channel();
    worker.send(IndicatorCommand::Flush(setup_tx));
    setup_rx
        .recv_timeout(ACK_LIMIT)
        .expect("indicator setup was published");
    collect_indicator(&worker, &mut views, &mut last_lane);
    let initial = indicator_snapshot(&worker);
    let (total, measured) = spec.counts();
    let mut samples = Samples::new(measured);
    let mut wall_start = None;
    for (index, commands) in bursts.into_iter().enumerate() {
        let (ack_tx, ack_rx) = channel();
        if index == WARMUP {
            wall_start = Some(Instant::now());
        }
        let burst_start = Instant::now();
        for command in commands {
            let kind = match &command {
                IndicatorCommand::BarClosed(_) => "bar_closed",
                IndicatorCommand::PartialUpdated { .. } => "partial_updated",
                _ => panic!("unexpected measured indicator command"),
            };
            let send_start = Instant::now();
            worker.send(command);
            let elapsed = send_start.elapsed();
            if index >= WARMUP {
                samples.record(elapsed, kind);
            }
        }
        worker.send(IndicatorCommand::Flush(ack_tx));
        ack_rx
            .recv_timeout(ACK_LIMIT)
            .expect("indicator burst was published");
        let elapsed = burst_start.elapsed();
        if index >= WARMUP {
            samples.completion_ns.push(nanos(elapsed));
        }
        collect_indicator(&worker, &mut views, &mut last_lane);
    }
    let wall = wall_start.unwrap().elapsed();
    let final_snapshot = indicator_snapshot(&worker);
    let oracle = assert_indicator_output(spec, &views, &last_lane);
    samples.publish(
        spec.name(),
        total,
        measured,
        wall,
        json!({"after_setup": initial, "after_workload": final_snapshot}),
        oracle,
    );
}

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).unwrap()
}

fn book_initial_snapshot() -> DepthEvent {
    DepthEvent::Snapshot {
        symbol: "BTCUSDT".to_owned(),
        generation: 10,
        observed_at_ms: 1000,
        effective_at_ms: 999,
        price_step: Some(Decimal::ONE),
        snapshot: BookSnapshot::new(
            10,
            (1..=128).map(|j| level(10_000 - j, 5)).collect(),
            (1..=128).map(|j| level(10_000 + j, 6)).collect(),
            BookCoverage::Limited {
                levels_per_side: 128,
            },
        ),
    }
}

fn book_request(
    burst: usize,
    current: u64,
    first_bar_index: usize,
    price_range: (f64, f64),
) -> ProjectionRequest {
    // This bar is an independently constructed display timeline, not a worker result.
    let count = i64::try_from(current).unwrap();
    ProjectionRequest {
        timeline_revision: u64::try_from(burst + 1).unwrap(),
        first_bar_index,
        closed: vec![Bar {
            open_time: 999,
            close_time: 1000 + count,
            open: Decimal::from(10_000),
            high: Decimal::from(10_000),
            low: Decimal::from(10_000),
            close: Decimal::from(10_000),
            buy_volume: Decimal::from((count + 1) / 2),
            sell_volume: Decimal::from(count / 2),
            trade_count: current,
        }],
        partial: None,
        lane: false,
        on_newest_bar: true,
        lane_reference_ms: None,
        price_range,
    }
}

fn book_fixture() -> Vec<Vec<BookCommand>> {
    let mut bursts = Vec::with_capacity(WARMUP + MEASURED);
    for burst in 0..WARMUP + MEASURED {
        let mut commands = Vec::with_capacity(2 * STEPS + 2);
        for offset in 0..STEPS {
            let i = u64::try_from(burst * STEPS + offset + 1).unwrap();
            let j = i64::try_from(1 + (i - 1) % 128).unwrap();
            let timestamp = 1000 + i64::try_from(i).unwrap();
            commands.push(BookCommand::Depth {
                event: DepthEvent::Update {
                    symbol: "BTCUSDT".to_owned(),
                    generation: 10,
                    event_time_ms: timestamp,
                    delta: BookDelta::new(
                        10 + i,
                        10 + i,
                        vec![level(10_000 - j, 7 + i64::try_from(i % 5).unwrap())],
                        vec![level(10_000 + j, 9 + i64::try_from(i % 7).unwrap())],
                    ),
                },
                received_at_ms: timestamp + 2,
            });
            commands.push(BookCommand::Trade(Trade {
                agg_id: i,
                timestamp_ms: timestamp,
                price: Decimal::from(10_000),
                quantity: Decimal::ONE,
                side: if i.is_multiple_of(2) {
                    Side::Sell
                } else {
                    Side::Buy
                },
            }));
        }
        let current = u64::try_from((burst + 1) * STEPS).unwrap();
        commands.push(BookCommand::Project(book_request(
            burst,
            current,
            1000,
            (9990.0, 10010.0),
        )));
        commands.push(BookCommand::Project(book_request(
            burst,
            current,
            0,
            (9871.0, 10129.0),
        )));
        bursts.push(commands);
    }
    assert_eq!(
        bursts.iter().map(Vec::len).sum::<usize>() + WARMUP + MEASURED + 4,
        82_534
    );
    bursts
}

fn assert_book_output(published: &BookPublished) -> Value {
    let health = &published.health;
    assert_eq!(health.generation, Some(10));
    assert_eq!(health.last_update_id, Some(40_330));
    assert_eq!(health.depth_updates, 40_320);
    assert_eq!(health.snapshots, 1);
    assert_eq!(health.gaps, 0);
    assert_eq!(
        (health.bid_levels, health.ask_levels, health.active_levels),
        (128, 128, 256)
    );
    assert_eq!(health.archived_runs, 80_640);
    assert_eq!(health.aggression_count, 40_320);
    assert_eq!(health.arrival_latency_ms, Some(2));
    let ladder = published
        .ladder
        .as_ref()
        .expect("actual published book ladder");
    assert_eq!((ladder.bids.len(), ladder.asks.len()), (128, 128));
    for j in 1..=128_i64 {
        let index = usize::try_from(j - 1).unwrap();
        let last_write = 40_192 + j;
        assert_eq!(ladder.bids[index], level(10_000 - j, 7 + last_write % 5));
        assert_eq!(ladder.asks[index], level(10_000 + j, 9 + last_write % 7));
    }
    assert_eq!(ladder.best_bid, Some(level(9999, 10)));
    assert_eq!(ladder.best_ask, Some(level(10001, 15)));
    let frame = published.frame.as_ref().expect("actual projected frame");
    assert_eq!((frame.first_bar_index, frame.slot_count), (0, 1));
    assert!(frame.projection.enabled);
    assert!(!frame.projection.cells.is_empty());
    for cell in frame.projection.cells.iter() {
        assert!(
            [cell.x0, cell.x1, cell.y0, cell.y1]
                .into_iter()
                .all(f64::is_finite)
        );
        assert!(cell.x0 <= cell.x1 && cell.y0 <= cell.y1);
        assert!(cell.quantity > Decimal::ZERO);
    }
    json!({"generation": health.generation, "last_update_id": health.last_update_id,
           "depth_updates": health.depth_updates, "archived_runs": health.archived_runs,
           "aggression_count": health.aggression_count, "active_levels": health.active_levels,
           "projection_cells": frame.projection.cells.len(),
           "selected_first_bar_index": frame.first_bar_index,
           "all_256_ladder_levels_match_last_write": true})
}

fn measure_book() {
    let bursts = book_fixture();
    let worker = BookWorker::spawn("BTCUSDT");
    let config = HeatmapConfig {
        show_aggressions: true,
        ..HeatmapConfig::default()
    };
    worker.send(BookCommand::ApplyVisualConfig(config));
    worker.send(BookCommand::SetEnabled {
        enabled: true,
        generation_floor: 10,
    });
    worker.send(BookCommand::Depth {
        event: book_initial_snapshot(),
        received_at_ms: 1000,
    });
    let (setup_tx, setup_rx) = channel();
    worker.send(BookCommand::Flush(setup_tx));
    setup_rx
        .recv_timeout(ACK_LIMIT)
        .expect("book setup was published");
    let initial = book_snapshot(&worker);
    let mut samples = Samples::new(78_000);
    let mut wall_start = None;
    for (index, commands) in bursts.into_iter().enumerate() {
        let (ack_tx, ack_rx) = channel();
        if index == WARMUP {
            wall_start = Some(Instant::now());
        }
        let burst_start = Instant::now();
        for command in commands {
            let kind = match &command {
                BookCommand::Depth { .. } => "depth",
                BookCommand::Trade(_) => "trade",
                BookCommand::Project(_) => "project",
                _ => panic!("unexpected measured book command"),
            };
            let send_start = Instant::now();
            worker.send(command);
            let elapsed = send_start.elapsed();
            if index >= WARMUP {
                samples.record(elapsed, kind);
            }
        }
        worker.send(BookCommand::Flush(ack_tx));
        ack_rx
            .recv_timeout(ACK_LIMIT)
            .expect("book burst was published");
        let elapsed = burst_start.elapsed();
        if index >= WARMUP {
            samples.completion_ns.push(nanos(elapsed));
        }
    }
    let wall = wall_start.unwrap().elapsed();
    let final_snapshot = book_snapshot(&worker);
    let oracle = assert_book_output(&worker.published());
    samples.publish(
        "depth",
        82_534,
        78_000,
        wall,
        json!({"after_setup": initial, "after_workload": final_snapshot}),
        oracle,
    );
}

#[test]
#[ignore = "manual serialized Q8 dense time/dollar/depth worker comparison"]
fn dense_workers() {
    measure_indicator(Spec::Time);
    measure_indicator(Spec::Dollar);
    measure_book();
}
