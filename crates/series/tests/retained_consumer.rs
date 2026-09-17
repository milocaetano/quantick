//! A second retained consumer: only the public headless series/engine ports.
use quantick_engine::bar_registry::{BUILTIN_BARS, BarRegistry};
use quantick_engine::{Bar, fixture};
use quantick_series::{DEFAULT_FOOTPRINT_GROUP, RetainedSeries, SeriesFold};
use rust_decimal::Decimal;

#[path = "../../engine/tests/support/seventh_bar.rs"]
mod seventh_bar;

const TAPE: &str = include_str!("../../engine/tests/fixtures/tick_trades.csv");
const GOLDEN: &str = include_str!("../../engine/tests/fixtures/tick_n3_expected.csv");

fn partial() -> Bar {
    Bar {
        open_time: 1600,
        close_time: 1600,
        open: Decimal::from(100),
        high: Decimal::from(100),
        low: Decimal::from(100),
        close: Decimal::from(100),
        buy_volume: Decimal::ZERO,
        sell_volume: Decimal::new(5, 1),
        trade_count: 1,
    }
}

#[test]
fn seventh_registered_family_traverses_streaming_and_retained_lifecycle_without_app() {
    // The extension edits one definition and this registry entry. No series,
    // pane or runner factory/identity branch knows "probe".
    let registry = BarRegistry::new(
        BUILTIN_BARS
            .definitions()
            .iter()
            .copied()
            .chain([&seventh_bar::SEVENTH]),
    )
    .unwrap();
    let spec = registry.parse("probe:3").unwrap();
    let tape = fixture::parse_trades(TAPE).unwrap();
    let expected = fixture::parse_bars(GOLDEN).unwrap();
    for capture in [false, true] {
        let mut fold = if capture {
            SeriesFold::with_footprints(spec, Decimal::ONE)
        } else {
            SeriesFold::new(spec)
        };
        let output: Vec<_> = tape.iter().filter_map(|trade| fold.push(trade)).collect();
        assert_eq!(
            output
                .iter()
                .map(|closed| closed.bar.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(fold.partial(), Some(&partial()));
        assert_eq!(fold.partial_footprint().is_some(), capture);
        for closed in &output {
            assert_eq!(closed.footprint.is_some(), capture);
            if let Some(ladder) = &closed.footprint {
                assert_eq!(
                    ladder
                        .levels()
                        .values()
                        .map(|level| level.trade_count)
                        .sum::<u64>(),
                    3
                );
                assert_eq!(
                    ladder
                        .levels()
                        .values()
                        .map(|level| level.buy)
                        .sum::<Decimal>(),
                    closed.bar.buy_volume
                );
                assert_eq!(
                    ladder
                        .levels()
                        .values()
                        .map(|level| level.sell)
                        .sum::<Decimal>(),
                    closed.bar.sell_volume
                );
            }
        }
        let mut retained = RetainedSeries::new(spec);
        retained.set_footprint_group(Decimal::ONE);
        retained.set_footprint_enabled(capture);
        retained.ingest_backfill(&tape[2..4]);
        for trade in &tape[4..] {
            retained.ingest_live(trade);
        }
        assert_eq!(retained.prepend_history(&tape[..2]), 1);
        assert_eq!(retained.bars(), expected);
        assert_eq!(retained.partial(), Some(&partial()));
        assert_eq!(retained.backfill_trade_count(), 4);
        assert_eq!(retained.backfill_boundary(), Some(1));
        assert_eq!(
            retained.tape_reference_price(),
            Some(Decimal::new(1005, 1)),
            "first seen, not first in time"
        );

        let mut second = RetainedSeries::new(spec);
        second.set_footprint_group(Decimal::ONE);
        second.set_footprint_enabled(capture);
        second.seed_from(
            retained.trades(),
            retained.backfill_trade_count(),
            retained.deal_samples(),
        );
        assert_eq!(second.bars(), expected);
        assert_eq!(second.backfill_boundary(), Some(1));
        assert_eq!(second.partial(), Some(&partial()));
        assert_eq!(second.bar_footprints(), retained.bar_footprints());
        assert_eq!(second.partial_footprint(), retained.partial_footprint());

        retained.set_spec(registry.parse("probe:2").unwrap());
        retained.set_spec(spec);
        retained.rebuild_bars();
        retained.set_footprint_group(Decimal::from(2));
        assert_eq!(retained.bars(), expected);
        assert_eq!(retained.backfill_boundary(), Some(1));
        assert_eq!(retained.partial(), Some(&partial()));
        assert_eq!(retained.bar_footprints().len(), if capture { 2 } else { 0 });
        if capture {
            let ladder = &retained.bar_footprints()[0];
            assert_eq!(ladder.group(), Decimal::from(2));
            assert_eq!(ladder.levels().len(), 1);
            assert_eq!(ladder.levels()[&50].buy, Decimal::from(3));
            assert_eq!(ladder.levels()[&50].sell, Decimal::new(15, 1));
        }
        retained.reset_series(spec);
        retained.ingest_backfill(&tape);
        assert_eq!(retained.bars(), expected);
        assert_eq!(retained.footprint_group(), DEFAULT_FOOTPRINT_GROUP);
        assert!(retained.bar_footprints().is_empty());
    }
}
