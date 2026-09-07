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
    pane.seed_from(&[print(1, 1), print(2, 2)], 1);
    assert_eq!(
        retained(&pane),
        2,
        "a pane opened mid-session seeds the whole partial"
    );
    assert_eq!(publish(&mut pane), 0);
}
