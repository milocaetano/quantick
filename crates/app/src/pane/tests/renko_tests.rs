//! A Renko pane: nothing is cut before the tape shows its grid, the grid's
//! arrival re-cuts the series as a rewrite, and one print that clears
//! several brick levels lands every brick it closed.

use super::*;
use quantick_engine::bar_registry::BUILTIN_BARS;
use rust_decimal::Decimal;

fn print(agg_id: u64, price: i64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        agg_id,
        timestamp_ms: 1_700_000_000_000 + agg_id as i64 * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    }
}

#[test]
fn a_renko_pane_waits_for_the_grid_then_takes_every_brick_a_print_closes() {
    let renko = BUILTIN_BARS.parse("renko:3").unwrap();
    let mut pane = ChartPane::flow(1, renko, "WINV26".to_owned());
    // Eight one-point moves inside the first brick's cell: the eighth names
    // the grid, and no level is broken yet.
    for (i, price) in [100, 101, 100, 101, 100, 101, 100, 101].iter().enumerate() {
        pane.ingest_live_trade(&print(i as u64, *price));
    }
    assert_eq!(pane.state.tape_price_step(), None);
    let before_grid = pane.pagination_revision();
    pane.ingest_live_trade(&print(8, 100));
    assert_eq!(pane.state.tape_price_step(), Some(Decimal::ONE));
    assert_ne!(
        pane.pagination_revision(),
        before_grid,
        "the grid's arrival re-cut the series, a rewrite and not an append"
    );
    assert_eq!(pane.closed_slots(), 0, "nothing broke the cell [100, 102]");

    // 108 clears 102, 104 and 106 by a step: three bricks from one print,
    // appended without rewriting anything closed.
    let appended = pane.pagination_revision();
    pane.ingest_live_trade(&print(9, 108));
    assert_eq!(pane.closed_slots(), 3);
    assert_eq!(pane.pagination_revision(), appended);
    let closes: Vec<Decimal> = (0..3)
        .map(|slot| pane.closed_bar(slot).expect("a closed brick").close)
        .collect();
    assert_eq!(
        closes,
        [102, 104, 106].map(Decimal::from),
        "one brick per level"
    );
}
