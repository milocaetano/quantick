//! Every pane's series lives in the tab session, keyed by the pane's id.

use super::*;
use quantick_chart::state::{BarSpec, ChartState};
use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

const MINUTE: i64 = 60 * 1_000;

fn trade(ms: i64, price: i64) -> Trade {
    Trade {
        agg_id: ms as u64,
        timestamp_ms: ms,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn candle(open_time: i64, interval: i64) -> Bar {
    Bar {
        open_time,
        close_time: open_time + interval - 1,
        open: Decimal::ONE,
        high: Decimal::ONE,
        low: Decimal::ONE,
        close: Decimal::ONE,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ZERO,
        trade_count: 1,
    }
}

/// What a pane composed for the same input before the session owned it: the
/// prefix, then the engine's closed bars, then the forming one.
fn composed(prefix: &[Bar], state: &ChartState) -> (Vec<Bar>, Option<Bar>) {
    let mut bars = prefix.to_vec();
    bars.extend_from_slice(state.bars());
    (bars, state.partial().cloned())
}

#[test]
fn series_are_owned_per_pane_id() {
    let mut session = TabSession::new();
    let panes = [(11_u64, MINUTE), (22_u64, 5 * MINUTE)];
    for (id, interval) in panes {
        session.open_pane(id, BarSpec::Time(interval));
    }
    let backfill: Vec<_> = (0..40)
        .map(|i| trade(60 * MINUTE + i * 30_000, 100 + i))
        .collect();
    let live = trade(60 * MINUTE + 40 * 30_000, 99);
    for (id, interval) in panes {
        let prefix: Vec<_> = (0..6)
            .map(|i| candle(30 * MINUTE + i * interval, interval))
            .collect();
        let series = session.series_mut(id).expect("opened");
        *series.prefix_mut() = prefix.iter().cloned().collect();
        series.state_mut().ingest_backfill(&backfill);
        series.ingest_live(&live);

        let mut alone = ChartState::new(BarSpec::Time(interval));
        alone.ingest_backfill(&backfill);
        alone.ingest_live(&live);
        let series = session.series(id).expect("opened");
        assert_eq!(
            composed(series.prefix(), series.state()),
            composed(&prefix, &alone),
            "pane {id}"
        );
        assert_eq!(series.closed_bars(), composed(&prefix, &alone).0);
    }
    let one = session.series(11).unwrap().state().bars().len();
    let five = session.series(22).unwrap().state().bars().len();
    assert!(
        one > five,
        "each id keeps its own interval: {one} vs {five}"
    );

    assert!(session.close_pane(22).is_some());
    assert!(session.series(22).is_none());
    assert!(
        session.series(11).is_some(),
        "closing one pane keeps the other"
    );
}

#[test]
fn opening_a_pane_twice_keeps_its_bars() {
    let mut session = TabSession::new();
    session.open_pane(3, BarSpec::Time(MINUTE));
    session
        .series_mut(3)
        .unwrap()
        .state_mut()
        .ingest_backfill(&[trade(0, 1), trade(MINUTE, 2)]);
    let kept = session
        .open_pane(3, BarSpec::Time(5 * MINUTE))
        .state()
        .bars()
        .len();
    assert_eq!(kept, 1);
}

#[test]
fn a_live_close_behind_the_boundary_reads_as_history() {
    let mut series = PaneSeries::new(BarSpec::Time(MINUTE));
    series.state_mut().ingest_backfill(&[trade(0, 1)]);
    let closes = series.ingest_live(&trade(MINUTE, 2));
    assert_eq!(closes.history_until, None);
    assert_eq!(closes.closed, 0..1);
}
