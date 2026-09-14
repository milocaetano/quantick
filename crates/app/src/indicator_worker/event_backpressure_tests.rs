//! The output side of the indicator worker is bounded: a UI that stops
//! draining pauses the worker, counted, and loses nothing once it drains.

use super::tests::trade;
use super::*;
use rust_decimal::prelude::ToPrimitive;
use std::time::{Duration, Instant};

#[test]
fn a_ui_that_stops_reading_pauses_the_worker_and_loses_no_row() {
    // Two slots of output, so the first batch's events already fill it.
    let worker = IndicatorWorker::spawn_bounded(WorkerProgress::new(), 2);
    worker.send(IndicatorCommand::Add {
        slot: SlotId(1),
        source: IndicatorSource::Native {
            id: "native.cvd".to_owned(),
            values: Vec::new(),
        },
    });
    let bars: Vec<Bar> = (1..=100).map(|i| Bar::opened_by(&trade(i))).collect();
    for bar in &bars {
        worker.send(IndicatorCommand::BarClosed(bar.clone()));
    }
    // Nobody drains: the worker must end up waiting on its output, and say so.
    let deadline = Instant::now() + Duration::from_secs(10);
    while worker.progress().counts.output_blocked == 0 {
        assert!(
            Instant::now() < deadline,
            "the worker never waited on a full output"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    // The UI comes back and reads frame after frame: every row arrives.
    let (mut rows, mut last) = (0, None);
    let deadline = Instant::now() + Duration::from_secs(10);
    while rows < bars.len() {
        assert!(Instant::now() < deadline, "rows stopped arriving at {rows}");
        for event in worker.drain_events() {
            match event {
                IndicatorEvent::Rebuilt {
                    rows: all, columns, ..
                } => {
                    rows = all;
                    last = columns.first().and_then(|c| c.last().copied());
                }
                IndicatorEvent::Appended { row, .. } => {
                    rows += 1;
                    last = row.first().copied();
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let delta: f64 = bars
        .iter()
        .map(|b| {
            (b.buy_volume - b.sell_volume)
                .to_f64()
                .expect("small volume")
        })
        .sum();
    assert_eq!(rows, bars.len(), "one row per closed bar");
    assert_eq!(last, Some(delta), "the last row carries every bar's delta");
    let counts = worker.progress().counts;
    assert!(counts.output_blocked > 0);
    assert_eq!(counts.output_failures, 0, "a wait is not a failure");
}
