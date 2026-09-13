//! Observe the actual pane-to-worker producer, including reset and seed traffic.

use super::*;
use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

fn print(id: u64, timestamp_ms: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn publish(pane: &mut ChartPane) -> usize {
    let before = pane.indicator_worker.lane_traffic_for_test();
    pane.publish_partial();
    pane.indicator_worker.lane_traffic_for_test() - before
}

fn retained(pane: &ChartPane) -> usize {
    pane.indicator_worker.flush();
    pane.indicator_worker.retained_lane_for_test().0
}

#[test]
fn producer_transport_is_new_prints_independent_of_old_history() {
    for old_count in [0_u64, 1_024, 8_192] {
        for count in [128_u64, 256] {
            let mut pane = ChartPane::time(1, 60_000);
            let old: Vec<_> = (0..old_count).map(|id| print(id, id as i64)).collect();
            pane.ingest_backfill(&old);
            assert!(pane.lane.set_rungs(8));
            let cold_seed = publish(&mut pane);
            assert_eq!(cold_seed, old_count as usize);
            let mut transported = 0;
            for id in 1..=count {
                pane.ingest_live_trade(&print(old_count + id, 60_000 + id as i64));
                let sent = publish(&mut pane);
                assert_eq!(sent, 1, "each one-print drain sends exactly its new print");
                transported += sent;
                assert_eq!(
                    publish(&mut pane),
                    0,
                    "no-new-data publication sends nothing"
                );
            }
            assert_eq!(transported, count as usize);
            assert_eq!(retained(&pane), count as usize);
            println!(
                "Q3_PRODUCER old_history={old_count} drains={count} new_entries={transported} cold_seed_entries={cold_seed} retained_entries={count}"
            );
        }
    }
}

#[test]
fn producer_close_rebuild_lane_toggle_and_empty_updates_preserve_the_run() {
    let mut pane = ChartPane::time(1, 60_000);
    pane.lane.set_rungs(8);
    pane.ingest_backfill(&(1..=3).map(|id| print(id, id as i64)).collect::<Vec<_>>());
    assert_eq!(retained(&pane), 3, "backfill cold-seeds once");
    for id in 4..=5 {
        pane.ingest_live_trade(&print(id, id as i64));
    }
    assert_eq!(publish(&mut pane), 2);
    assert_eq!(publish(&mut pane), 0);
    assert_eq!(retained(&pane), 5);

    pane.set_spec(BarSpec::Tick(4));
    pane.send_indicator_rebuild();
    assert_eq!(
        retained(&pane),
        1,
        "the recut seeds only its new forming bar"
    );
    assert_eq!(publish(&mut pane), 0);
    for id in 6..=7 {
        pane.ingest_live_trade(&print(id, id as i64));
    }
    assert_eq!(publish(&mut pane), 2);
    assert_eq!(retained(&pane), 3);
    pane.ingest_live_trade(&print(8, 8));
    assert!(pane.state.partial().is_none());
    assert_eq!(publish(&mut pane), 0);
    assert_eq!(
        retained(&pane),
        0,
        "a close that vanishes the partial resets the run"
    );
    pane.ingest_live_trade(&print(9, 9));
    assert_eq!(publish(&mut pane), 1);
    assert_eq!(retained(&pane), 1);

    pane.lane.set_rungs(0);
    assert_eq!(publish(&mut pane), 0);
    assert_eq!(retained(&pane), 0, "disabling releases the worker run");
    pane.ingest_live_trade(&print(10, 10));
    assert_eq!(publish(&mut pane), 0);
    pane.lane.set_rungs(8);
    assert_eq!(
        publish(&mut pane),
        2,
        "enable cold-seeds all current forming prints"
    );
    assert_eq!(publish(&mut pane), 0);
    pane.lane.set_rungs(16);
    assert_eq!(publish(&mut pane), 0, "resize does not duplicate the seed");
    assert_eq!(retained(&pane), 2);
    assert_eq!(
        pane.state.trades().len(),
        10,
        "transport never changes trade retention"
    );
}

#[test]
fn producer_prepend_rewind_and_repeated_rebuild_seed_the_recut_partial_once() {
    let mut pane = ChartPane::time(1, 60_000);
    pane.lane.set_rungs(8);
    pane.ingest_backfill(&(3..=5).map(|id| print(id, id as i64)).collect::<Vec<_>>());
    assert_eq!(retained(&pane), 3);
    pane.prepend_history(&[print(1, 1), print(2, 2)]);
    assert_eq!(retained(&pane), 5);
    assert_eq!(publish(&mut pane), 0);
    for _ in 0..2 {
        let before = pane.indicator_worker.lane_traffic_for_test();
        pane.send_indicator_rebuild();
        assert_eq!(pane.indicator_worker.lane_traffic_for_test() - before, 5);
        assert_eq!(retained(&pane), 5);
        assert_eq!(publish(&mut pane), 0);
    }
    pane.reset_series();
    assert_eq!(retained(&pane), 0, "rewind discards the previous epoch");
    assert_eq!(publish(&mut pane), 0);
    pane.seed_from(&[print(1, 1), print(2, 2)].into_iter().collect(), 1);
    assert_eq!(
        retained(&pane),
        2,
        "a pane opened mid-session seeds the whole partial"
    );
    assert_eq!(publish(&mut pane), 0);
}

/// `LaneTransport` as it stood at `eb60ed41`, over the contiguous tape it
/// sliced, kept verbatim as the oracle for the chunked one.
#[derive(Default)]
struct SliceLane {
    rungs: usize,
    sent: usize,
}

impl SliceLane {
    fn reset(&mut self) {
        self.sent = 0;
    }

    fn set_rungs(&mut self, rungs: usize) -> bool {
        let changed = self.rungs != rungs;
        if rungs == 0 {
            self.reset();
        }
        self.rungs = rungs;
        changed
    }

    fn command(&mut self, partial: Option<quantick_engine::Bar>, trades: &[Trade]) -> Vec<Trade> {
        let count = partial
            .as_ref()
            .filter(|_| self.rungs > 0)
            .map_or(0, |bar| {
                usize::try_from(bar.trade_count)
                    .unwrap_or(usize::MAX)
                    .min(trades.len())
            });
        let start = trades.len() - count + self.sent.min(count);
        let run = trades[start..].to_vec();
        self.sent = count;
        run
    }
}

/// The lane transport sends the worker exactly what it sent when the tape
/// was one slice: frame after frame of live prints, bar closes, a lane
/// switched off and on, and a forming bar whose run spans several chunks
/// sent whole after a reset.
#[test]
fn the_chunked_tape_sends_the_worker_what_the_slice_sent() {
    use quantick_engine::trade_tape::CHUNK_TRADES;
    for spec in [BarSpec::Tick(50), BarSpec::Time(86_400_000)] {
        let mut state = crate::state::ChartState::new(spec.clone());
        let mut contiguous: Vec<Trade> = Vec::new();
        let mut lane = LaneTransport::default();
        let mut oracle = SliceLane::default();
        assert_eq!(lane.set_rungs(16), oracle.set_rungs(16));
        let mut next = 0_u64;
        let mut frame = 0_u64;
        let mut crossed = 0_usize;
        while contiguous.len() < 3 * CHUNK_TRADES + 1_000 {
            frame += 1;
            // Mostly a handful of prints; now and then a burst that crosses
            // a chunk boundary inside one frame; and a frame that ends just
            // past each boundary, so a short run straddles it too.
            let to_boundary = CHUNK_TRADES - contiguous.len() % CHUNK_TRADES;
            let prints = if frame.is_multiple_of(4_999) {
                CHUNK_TRADES / 2 + 3
            } else if to_boundary <= 20 {
                to_boundary + 3
            } else {
                (frame * 7_919 % 23) as usize
            };
            let mut closed = false;
            for _ in 0..prints {
                let trade = print(next + 1, 1_720_051_200_000 + next as i64);
                next += 1;
                let before = state.bars().len();
                state.ingest_live(&trade);
                contiguous.push(trade);
                closed |= state.bars().len() > before;
            }
            if closed {
                lane.reset();
                oracle.reset();
            }
            // The lane switched off for a few frames now and then.
            if frame % 211 < 2 {
                let rungs = if frame.is_multiple_of(211) { 0 } else { 16 };
                assert_eq!(lane.set_rungs(rungs), oracle.set_rungs(rungs));
            }
            let want = oracle.command(state.partial().cloned(), &contiguous);
            match lane.command(state.partial().cloned(), state.trades()) {
                crate::indicator_worker::IndicatorCommand::PartialUpdated { run, .. } => {
                    assert_eq!(
                        format!("{run:?}"),
                        format!("{want:?}"),
                        "{spec:?}, frame {frame}"
                    );
                    let start = contiguous.len() - run.len();
                    if !run.is_empty()
                        && start / CHUNK_TRADES != (contiguous.len() - 1) / CHUNK_TRADES
                    {
                        crossed += 1;
                    }
                }
                _ => unreachable!("the lane transport only builds forming-bar updates"),
            }
        }
        assert!(crossed > 0, "{spec:?}: some run spans a chunk boundary");
    }
}
