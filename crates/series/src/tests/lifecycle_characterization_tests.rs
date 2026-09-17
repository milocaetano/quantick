//! Pre-extraction contracts not pinned by the original golden/tape fixtures.
use super::*;
use quantick_engine::Side;

fn print(id: u64, quantity: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: 1_000 + id as i64 * 100,
        price: Decimal::from(100),
        quantity: Decimal::from(quantity),
        side: if id.is_multiple_of(2) {
            Side::Sell
        } else {
            Side::Buy
        },
    }
}

fn expected(first: i64, last: i64, buys: i64, sells: i64, count: u64) -> Bar {
    Bar {
        open_time: first,
        close_time: last,
        open: Decimal::from(100),
        high: Decimal::from(100),
        low: Decimal::from(100),
        close: Decimal::from(100),
        buy_volume: Decimal::from(buys),
        sell_volume: Decimal::from(sells),
        trade_count: count,
    }
}

#[test]
fn weighted_imbalance_elephant_closes_with_its_exact_ladder_live_and_rebuilt() {
    // Four unit prints warm the rule; the fifth weighs 100 units against
    // the preceding expectation and must close alone. The sixth forms.
    // Constant price makes dollar weighting exactly 100 times volume.
    let tape = [
        print(1, 1),
        print(2, 1),
        print(3, 1),
        print(4, 1),
        print(5, 100),
        print(6, 1),
    ];
    let closed = [
        expected(1_100, 1_400, 2, 2, 4),
        expected(1_500, 1_500, 100, 0, 1),
    ];
    let partial = expected(1_600, 1_600, 0, 1, 1);
    for unit in [ImbalanceUnit::Volume, ImbalanceUnit::Dollar] {
        for backfill in [false, true] {
            let mut state = ChartState::new(BarSpec::Imbalance(unit, 4));
            state.set_footprint_group(Decimal::ONE);
            state.set_footprint_enabled(true);
            if backfill {
                state.ingest_backfill(&tape);
            } else {
                for trade in &tape {
                    state.ingest_live(trade);
                }
            }
            for rebuild in [false, true] {
                if rebuild {
                    state.rebuild_bars();
                }
                assert_eq!(
                    state.bars(),
                    closed,
                    "{unit:?}, backfill={backfill}, rebuild={rebuild}"
                );
                assert_eq!(state.partial(), Some(&partial));
                assert_eq!(state.bar_footprints().len(), 2);
                for (ladder, (buy, sell, count)) in
                    state.bar_footprints().iter().zip([(2, 2, 4), (100, 0, 1)])
                {
                    assert_eq!(ladder.group(), Decimal::ONE);
                    assert_eq!(ladder.levels().len(), 1);
                    let level = &ladder.levels()[&100];
                    assert_eq!(
                        (level.buy, level.sell, level.trade_count),
                        (Decimal::from(buy), Decimal::from(sell), count)
                    );
                }
                let forming = state.partial_footprint().unwrap();
                assert_eq!(forming.levels().len(), 1);
                assert_eq!(forming.levels()[&100].sell, Decimal::ONE);
                assert_eq!(forming.levels()[&100].trade_count, 1);
            }
        }
    }
}

#[test]
fn reset_keeps_only_readings_and_restores_the_original_footprint_defaults() {
    let mut state = ChartState::new(BarSpec::Tick(2));
    let reading = DealSample {
        time_ms: 999,
        session_deals: 50,
    };
    state.observe_deals(reading);
    state.set_footprint_group(Decimal::from(5));
    state.set_footprint_enabled(true);
    state.ingest_backfill(&[print(1, 1), print(2, 1)]);
    state.ingest_live(&print(3, 1));
    assert_eq!((state.timeline_revision(), state.series_revision()), (4, 3));
    state.reset_series(BarSpec::Tick(3));
    assert_eq!(state.deal_samples(), &[reading]);
    assert_eq!(state.spec(), &BarConfiguration::from(BarSpec::Tick(3)));
    assert!(state.trades().is_empty());
    assert!(state.bars().is_empty());
    assert!(state.partial().is_none());
    assert_eq!(state.backfill_trade_count(), 0);
    assert_eq!(state.backfill_boundary(), None);
    assert_eq!(state.tape_reference_price(), None);
    assert_eq!(state.tape_price_step(), None);
    assert_eq!((state.timeline_revision(), state.series_revision()), (0, 0));
    assert_eq!(state.footprint_group(), Decimal::new(1, 2));
    state.ingest_live(&print(4, 1));
    assert!(state.bar_footprints().is_empty());
    assert!(
        state.partial_footprint().is_none(),
        "reset disables capture"
    );
    assert_eq!((state.timeline_revision(), state.series_revision()), (1, 0));
}

#[test]
fn disabled_group_and_capture_toggles_preserve_exact_revision_semantics() {
    let mut state = ChartState::new(BarSpec::Tick(2));
    state.ingest_live(&print(1, 1));
    state.set_footprint_group(Decimal::from(2));
    assert_eq!((state.timeline_revision(), state.series_revision()), (2, 1));
    assert!(state.partial_footprint().is_none());
    for group in [Decimal::ZERO, -Decimal::ONE, Decimal::from(2)] {
        state.set_footprint_group(group);
    }
    state.set_footprint_enabled(false);
    state.set_spec(BarSpec::Tick(2));
    state.prepend_history(&[]);
    assert_eq!((state.timeline_revision(), state.series_revision()), (2, 1));
    state.set_footprint_enabled(true);
    assert_eq!((state.timeline_revision(), state.series_revision()), (3, 2));
    assert_eq!(state.partial_footprint().unwrap().group(), Decimal::from(2));
    state.set_footprint_enabled(true);
    state.ingest_live(&print(2, 1));
    assert_eq!((state.timeline_revision(), state.series_revision()), (4, 2));
    state.set_footprint_enabled(false);
    assert_eq!((state.timeline_revision(), state.series_revision()), (5, 3));
    assert_eq!(state.bars(), &[expected(1_100, 1_200, 1, 1, 2)]);
    assert!(state.bar_footprints().is_empty());
    assert!(state.partial_footprint().is_none());
    assert_eq!(state.footprint_group(), Decimal::from(2));
}

#[test]
fn held_reading_refold_has_two_revision_steps_and_recuts_the_exact_prints() {
    let mut state = ChartState::new(BarSpec::Trades(2));
    state.set_footprint_enabled(true);
    for (time_ms, session_deals) in [(1_099, 1), (1_299, 6)] {
        state.observe_deals(DealSample {
            time_ms,
            session_deals,
        });
    }
    let tape = [print(1, 1), print(2, 1), print(3, 1)];
    for trade in &tape {
        state.ingest_live(trade);
    }
    assert_eq!((state.timeline_revision(), state.series_revision()), (4, 1));
    assert_eq!(state.bars(), &[expected(1_300, 1_300, 1, 0, 1)]);
    assert_eq!(state.uncounted_trades(), 2);
    state.observe_deals(DealSample {
        time_ms: 1_199,
        session_deals: 4,
    });
    assert_eq!((state.timeline_revision(), state.series_revision()), (4, 1));
    state.set_footprint_group(Decimal::from(2));
    assert_eq!((state.timeline_revision(), state.series_revision()), (6, 3));
    assert_eq!(
        state.bars(),
        &[
            expected(1_200, 1_200, 0, 1, 1),
            expected(1_300, 1_300, 1, 0, 1)
        ]
    );
    assert_eq!(state.uncounted_trades(), 1);
    assert_eq!(state.bar_footprints().len(), 2);
    assert_eq!(state.bar_footprints()[0].levels()[&50].sell, Decimal::ONE);
    assert_eq!(state.bar_footprints()[1].levels()[&50].buy, Decimal::ONE);
    assert!(state.partial().is_none());
    assert!(state.partial_footprint().is_none());
    state.set_footprint_group(Decimal::from(4));
    assert_eq!(
        (state.timeline_revision(), state.series_revision()),
        (7, 4),
        "the held flag was consumed"
    );
}
