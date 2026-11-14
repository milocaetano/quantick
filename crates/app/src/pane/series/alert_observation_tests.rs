use super::ChartPane;
use quantick_engine::{Side, Trade, bar_registry::BUILTIN_BARS};
use rust_decimal::Decimal;

#[test]
fn committed_observer_excludes_held_history_and_preserves_equal_time_closes() {
    let mut pane = ChartPane::flow(1, BUILTIN_BARS.parse("renko:3").unwrap(), "TEST".to_owned());
    let trade = |id: u64, price: i64| Trade {
        agg_id: id,
        timestamp_ms: 1000 + id as i64,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    };
    let tape: Vec<_> = (100..164)
        .enumerate()
        .map(|(id, price)| trade(id as u64, price))
        .collect();
    pane.ingest_backfill(&tape[..10]);
    let mut closes = Vec::new();
    for print in &tape[10..] {
        pane.ingest_live_trade_observed(print, &mut |_, bar, slot| closes.push((bar.close, slot)));
    }
    assert!(closes.is_empty());
    pane.ingest_live_trade_observed(&trade(64, 170), &mut |_, bar, slot| {
        closes.push((bar.close, slot))
    });
    assert_eq!(
        closes.iter().map(|(_, slot)| *slot).collect::<Vec<_>>(),
        [31, 32, 33]
    );
    assert_eq!(pane.state.backfill_boundary(), Some(31));
    assert!(
        pane.strategies.pending.is_empty(),
        "observer never steals or creates strategy work"
    );
}
