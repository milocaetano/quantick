//! Fixture-first specification of the partial exit ladder.
//!
//! Written before the implementation, per the engine/determinism rule in
//! `CLAUDE.md`: the trades and the fills they must produce are stated here,
//! and the simulator is changed until this file is green.
//!
//! The ladder is what a trader means by "half off at the first target, the
//! rest runs". One entry, its quantity split into ordered parts, each part
//! carrying its own take profit and stop loss. The two legs of a part are an
//! OCO pair: whichever fills first cancels its sibling, and the other parts
//! carry on untouched.

use quantick_engine::{Side, Trade};
use quantick_sim::{Command, Simulator};
use quantick_trading::{
    Bracket, CancelReason, ExitPart, Fill, FillRole, Order, OrderId, OrderRole, VenueEvent,
};
use rust_decimal::Decimal;

fn dec(value: i64) -> Decimal {
    Decimal::from(value)
}

/// A print: the id doubles as a readable timestamp (`ts = id * 1000`).
fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("test ids are small") * 1000,
        price: dec(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn seeded(price: i64) -> Simulator {
    let mut sim = Simulator::new();
    sim.seed(&print(0, price));
    sim
}

fn fills(events: &[VenueEvent]) -> Vec<Fill> {
    events
        .iter()
        .filter_map(|event| match event {
            VenueEvent::Filled(fill) => Some(*fill),
            _ => None,
        })
        .collect()
}

fn cancels(events: &[VenueEvent]) -> Vec<(OrderId, CancelReason)> {
    events
        .iter()
        .filter_map(|event| match event {
            VenueEvent::Cancelled { order, reason } => Some((order.id, *reason)),
            _ => None,
        })
        .collect()
}

/// Every working order that is not the entry: the protection.
fn protective(sim: &Simulator) -> Vec<Order> {
    sim.orders()
        .iter()
        .filter(|order| order.role != OrderRole::Entry)
        .cloned()
        .collect()
}

/// The ladder the trader configured: two contracts, split one and one.
///
/// Part A takes ten points and risks ten. Part B runs for twenty and gives
/// the trade five more points of room, which is the shape of "take half off
/// early, let the rest breathe".
fn two_part_ladder() -> Bracket {
    Bracket::ladder(&[
        ExitPart {
            quantity: Some(dec(1)),
            take_profit: Some(dec(5000)),
            stop_loss: Some(dec(4980)),
        },
        ExitPart {
            quantity: Some(dec(1)),
            take_profit: Some(dec(5010)),
            stop_loss: Some(dec(4975)),
        },
    ])
    .expect("two parts is within the ladder maximum")
}

fn place_laddered_entry(sim: &mut Simulator) -> Vec<VenueEvent> {
    sim.apply(Command::PlaceLimit {
        side: Side::Buy,
        quantity: dec(2),
        price: dec(4990),
        bracket: two_part_ladder(),
        cancel_at: None,
        flat_only: false,
    })
}

#[test]
fn a_laddered_entry_rests_with_its_parts_and_places_nothing_until_it_fills() {
    let mut sim = seeded(5000);
    let events = place_laddered_entry(&mut sim);
    assert!(
        matches!(events.as_slice(), [VenueEvent::Placed(_)]),
        "placing a laddered entry is one event, not one per part: {events:?}"
    );

    // The protective legs are contingent on a fill. Until the entry fills
    // there is exactly one working order, and it carries the ladder so the
    // chart can draw the parts ahead of time.
    let working = sim.orders();
    assert_eq!(working.len(), 1, "only the entry works before the fill");
    assert_eq!(working[0].role, OrderRole::Entry);
    assert_eq!(
        working[0].bracket.parts().count(),
        2,
        "the resting entry carries both parts for the chart to project"
    );
    assert!(sim.position().is_none());
}

#[test]
fn the_fill_turns_each_part_into_a_working_oco_pair() {
    let mut sim = seeded(5000);
    place_laddered_entry(&mut sim);

    let events = sim.on_trade(&print(1, 4990));
    let filled = fills(&events);
    assert_eq!(filled.len(), 1, "one entry fill, whatever the ladder says");
    assert_eq!(filled[0].price, dec(4990));
    assert_eq!(filled[0].quantity, dec(2));

    let position = sim.position().expect("the entry opened a position");
    assert_eq!(position.quantity, dec(2));
    assert_eq!(position.avg_price, dec(4990));

    // Four protective orders now work: a take profit and a stop loss for
    // each part, every one of them a sell, because they reduce a long.
    let legs = protective(&sim);
    assert_eq!(legs.len(), 4, "two parts produce two OCO pairs: {legs:?}");
    assert!(
        legs.iter().all(|order| order.side == Side::Sell),
        "a long is reduced by sells"
    );
    assert!(
        legs.iter().all(|order| order.reduce_only),
        "a protective leg may never open or reverse a position"
    );
    assert!(
        legs.iter().all(|order| order.quantity == Decimal::ONE),
        "each leg carries its own part's quantity, not the whole position"
    );

    // Each pair shares an OCO group, and the two pairs do not share one.
    let take_profits: Vec<_> = legs
        .iter()
        .filter(|order| order.role == OrderRole::TakeProfit)
        .collect();
    let stops: Vec<_> = legs
        .iter()
        .filter(|order| order.role == OrderRole::StopLoss)
        .collect();
    assert_eq!(take_profits.len(), 2);
    assert_eq!(stops.len(), 2);
    for take_profit in &take_profits {
        let sibling = stops
            .iter()
            .find(|stop| stop.oco == take_profit.oco)
            .expect("every take profit is paired with a stop");
        assert_ne!(
            sibling.id, take_profit.id,
            "a leg is never its own OCO sibling"
        );
    }
    assert_ne!(
        take_profits[0].oco, take_profits[1].oco,
        "separate parts are separate OCO groups"
    );
}

#[test]
fn one_parts_target_closes_only_that_part_and_cancels_only_its_own_stop() {
    let mut sim = seeded(5000);
    place_laddered_entry(&mut sim);
    sim.on_trade(&print(1, 4990));

    // Part A's target is 5000; part B's is 5010 and neither stop is near.
    let events = sim.on_trade(&print(2, 5000));

    let filled = fills(&events);
    assert_eq!(filled.len(), 1, "only part A's target was traded through");
    assert_eq!(filled[0].role, FillRole::TakeProfit);
    assert_eq!(
        filled[0].price,
        dec(5000),
        "a take profit is a resting limit and fills at its own price"
    );
    assert_eq!(filled[0].quantity, dec(1), "part A's quantity, not the two");

    let cancelled = cancels(&events);
    assert_eq!(
        cancelled.len(),
        1,
        "exactly part A's stop stands down: {cancelled:?}"
    );
    assert_eq!(cancelled[0].1, CancelReason::OcoFilled);

    let position = sim.position().expect("part B is still open");
    assert_eq!(position.quantity, dec(1));
    assert_eq!(
        position.avg_price,
        dec(4990),
        "a partial exit leaves the average entry alone"
    );

    let legs = protective(&sim);
    assert_eq!(legs.len(), 2, "part B's pair survives untouched: {legs:?}");
    assert!(
        legs.iter().all(|order| order.oco == legs[0].oco),
        "and both survivors belong to the one remaining part"
    );
}

#[test]
fn the_ladder_runs_to_flat_and_books_one_trade_per_part() {
    let mut sim = seeded(5000);
    place_laddered_entry(&mut sim);
    sim.on_trade(&print(1, 4990));
    sim.on_trade(&print(2, 5000)); // Part A takes +10.

    // Part B's stop is 4975; the print trades through it, so the stop fills
    // at the print, honestly worse than the trigger.
    let events = sim.on_trade(&print(3, 4970));
    let filled = fills(&events);
    assert_eq!(filled.len(), 1);
    assert_eq!(filled[0].role, FillRole::StopLoss);
    assert_eq!(
        filled[0].price,
        dec(4970),
        "a stop is a market order once touched: the gap is reported, not hidden"
    );

    assert!(sim.position().is_none(), "the ladder ran to flat");
    assert!(
        sim.orders().is_empty(),
        "no protective leg outlives the position it protects"
    );

    let closed = sim.closed_trades();
    assert_eq!(closed.len(), 2, "one closed trade per part: {closed:?}");
    assert_eq!(closed[0].exit_price, dec(5000));
    assert_eq!(closed[1].exit_price, dec(4970));
    assert_eq!(
        sim.realized_points(),
        dec(10) + dec(-20),
        "+10 on part A, -20 on part B"
    );
}

#[test]
fn closing_by_hand_sweeps_every_protective_leg() {
    let mut sim = seeded(5000);
    place_laddered_entry(&mut sim);
    sim.on_trade(&print(1, 4990));
    assert_eq!(sim.orders().len(), 4);

    sim.apply(Command::ClosePosition);
    sim.on_trade(&print(2, 4995));

    assert!(sim.position().is_none());
    assert!(
        sim.orders().is_empty(),
        "a hand close leaves no orphan protection behind: {:?}",
        sim.orders()
    );
}

/// A plain bracket still arms the position's own pair - the answer the port
/// reports and every venue models. Only a ladder produces working legs.
#[test]
fn a_whole_bracket_arms_the_position_pair_and_makes_no_legs() {
    let mut sim = seeded(5000);
    sim.apply(Command::PlaceMarket {
        side: Side::Buy,
        quantity: dec(2),
        bracket: Bracket::whole(Some(dec(4980)), Some(dec(5020))),
    });
    sim.on_trade(&print(1, 5000));

    assert!(
        protective(&sim).is_empty(),
        "a whole bracket needs no legs of its own"
    );
    let position = sim.position().expect("opened");
    assert_eq!(position.stop_loss, Some(dec(4980)));
    assert_eq!(position.take_profit, Some(dec(5020)));

    let events = sim.on_trade(&print(2, 5020));
    let filled = fills(&events);
    assert_eq!(filled.len(), 1);
    assert_eq!(
        filled[0].quantity,
        dec(2),
        "the whole position exits at once"
    );
    assert!(sim.position().is_none());
    assert!(sim.orders().is_empty());
}

/// The determinism guard: the same prints and the same commands at the same
/// points in the stream produce identical events, twice over.
#[test]
fn the_ladder_is_deterministic_over_a_fixed_tape() {
    fn replay() -> Vec<String> {
        let mut sim = seeded(5000);
        let mut log = vec![format!("{:?}", place_laddered_entry(&mut sim))];
        for (id, price) in [
            (1_u64, 4995_i64),
            (2, 4990),
            (3, 4996),
            (4, 5000),
            (5, 4988),
            (6, 4970),
        ] {
            log.push(format!("{:?}", sim.on_trade(&print(id, price))));
        }
        log.push(format!("{:?}", sim.closed_trades()));
        log
    }

    assert_eq!(replay(), replay(), "same tape in, same events out");
}
