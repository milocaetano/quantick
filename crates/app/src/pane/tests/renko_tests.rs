//! A Renko pane: no brick until its builder has read the price step, then
//! every brick the prints it held close, appended; and one print clearing
//! several levels lands every brick it closed.

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

/// `prices` as consecutive prints, the first one numbered `first_id`.
fn prints(first_id: u64, prices: Vec<i64>) -> Vec<quantick_engine::Trade> {
    prices
        .into_iter()
        .enumerate()
        .map(|(i, price)| print(first_id + i as u64, price))
        .collect()
}

/// A flow pane cutting 3-tick bricks: on a one-point grid, two points tall.
fn renko_pane() -> ChartPane {
    let renko = BUILTIN_BARS.parse("renko:3").unwrap();
    ChartPane::flow(1, renko, "WINV26".to_owned())
}

#[test]
fn a_renko_pane_appends_every_brick_the_print_that_reads_the_step_cuts() {
    let mut pane = renko_pane();
    // A climb of one point a print: sixty-three distances read no step.
    let climb = prints(0, (100..=164).collect());
    for trade in &climb[..64] {
        pane.ingest_live_trade(trade);
    }
    assert_eq!(pane.closed_slots(), 0, "the prints are held");
    let appended = pane.pagination_revision();
    // The sixty-fourth distance reads it and cuts every brick the climb
    // closed; then 170 clears 164, 166 and 168 by a step in one print.
    pane.ingest_live_trade(&climb[64]);
    assert_eq!(pane.closed_slots(), 31);
    pane.ingest_live_trade(&print(65, 170));
    assert_eq!(pane.closed_slots(), 34);
    assert_eq!(
        pane.pagination_revision(),
        appended,
        "appended, never rewritten"
    );
    let closes: Vec<Decimal> = (31..34)
        .map(|slot| pane.closed_bar(slot).expect("a closed brick").close)
        .collect();
    assert_eq!(closes, [164, 166, 168].map(Decimal::from), "one per level");
}
