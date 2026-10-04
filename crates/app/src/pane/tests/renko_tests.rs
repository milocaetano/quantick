//! A Renko pane: no brick until its builder has read the price step, then
//! every brick the prints it held close, appended; one print clearing several
//! levels lands every brick it closed; and older history that reshapes the
//! first bricks keeps the marks and the view on the market time they carry.

use super::*;
use crate::viewport::Viewport;
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

/// The trader's mark and view sit on market time. Older history that
/// reshapes the bricks a series opened on must not move them onto a brick
/// that ended before that time — which is where a shift by the count of
/// bricks the history added would put them.
#[test]
fn older_history_that_reshapes_bricks_keeps_marks_and_view_on_their_market_time() {
    // Older: seventy prints at 120/121 read the step, then a fall to 99
    // cuts eleven bricks down to 100.
    let mut older: Vec<i64> = (0..70).map(|i| 120 + i % 2).collect();
    older.extend((99..=119).rev());
    let older = prints(0, older);
    // Held: a tape opening fresh at 100 — seventy prints at 100/101 read the
    // step, then a climb cuts 100->102 at 103 and 102->104 at 105.
    let mut held: Vec<i64> = (0..70).map(|i| 100 + i % 2).collect();
    held.extend(102..=105);
    let held = prints(older.len() as u64, held);

    let mut pane = renko_pane();
    pane.ingest_backfill(&held);
    assert_eq!(pane.closed_slots(), 2);
    let first = pane.closed_bar(0).expect("a brick").clone();
    assert_eq!(
        (first.open, first.close),
        (Decimal::from(100), Decimal::from(102))
    );
    let mark_time = first.open_time;
    let line = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("the horizontal line is registered");
    assert!(
        pane.drawings
            .place(line, ChartPoint::at_time(0.0, 101.0, Some(mark_time)))
    );
    // The view parked on that brick, off the live edge.
    let slots = pane.slots();
    pane.viewport.pan_pixels(1.0e6, slots);
    assert_eq!(pane.right_edge_time(), Some(mark_time));

    let added = pane.prepend_history(&older);
    // From the older tape the series reaches 100 already falling, so the
    // climb only reverses at 105: 100->102 is gone, and 102->104 opens on
    // the fall's last print and holds the mark's instant.
    assert_eq!((pane.closed_slots(), added), (12, 10));
    let holder = 11;
    let brick = pane.closed_bar(holder).expect("a brick");
    assert_eq!(
        (brick.open, brick.close),
        (Decimal::from(102), Decimal::from(104))
    );
    assert!(brick.open_time <= mark_time && mark_time <= brick.close_time);
    let mark = pane.drawings.items()[0].points[0];
    assert_eq!(Viewport::slot_of(mark.bar), Some(holder), "the mark");
    #[allow(clippy::cast_precision_loss)]
    let holder_bar = holder as f32;
    assert_eq!(
        pane.viewport.right_edge_bar(pane.slots()),
        holder_bar,
        "and the view stay on their market time"
    );
    assert_ne!(added, holder, "a shift by the count lands a brick early");
}
