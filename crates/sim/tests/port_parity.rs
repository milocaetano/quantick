//! Two venues, one port.
//!
//! Every test here talks to `&mut dyn TradingVenue` and never names the
//! implementation behind it. That is the actual claim `quantick-trading`
//! makes: a surface written against the port works against a venue it has
//! never heard of. If something paper-specific creeps into the trait — a
//! fill rule, a tape assumption, a simulator-shaped read — these stop
//! compiling or stop passing against the fake, which is why the fake has no
//! fill model of its own.

use quantick_engine::Side;
use quantick_sim::Simulator;
use quantick_trading::fake::{FakeVenue, print_at};
use quantick_trading::{
    Bracket, BracketTarget, CloseAmount, OrderIntent, TradingVenue, VenueEvent,
};
use rust_decimal::Decimal;

fn dec(value: i64) -> Decimal {
    Decimal::from(value)
}

/// The two venues under test, each behind the port and nothing else.
fn both() -> [(&'static str, Box<dyn TradingVenue>); 2] {
    [
        ("paper simulator", Box::new(Simulator::new())),
        ("fake venue", Box::new(FakeVenue::new())),
    ]
}

/// A resting order is placed, read back, repriced, bracketed and cancelled
/// — the whole working-order lifecycle, without either venue's own API.
#[test]
fn the_working_order_lifecycle_runs_on_any_venue() {
    for (name, mut venue) in both() {
        venue.seed(&print_at(0, dec(100), 0));

        let events = venue.submit(OrderIntent::limit(Side::Buy, dec(2), dec(95)));
        assert!(
            matches!(events.as_slice(), [VenueEvent::Placed(_)]),
            "{name}: a limit below the market is placeable"
        );

        let id = venue.working_orders()[0].id;
        assert_eq!(venue.working_orders().len(), 1, "{name}: one order works");
        assert!(venue.position().is_none(), "{name}: nothing is open yet");
        assert!(!venue.is_idle(), "{name}: a working order is not idle");

        venue.amend_price(id, dec(93));
        assert_eq!(
            venue.working_order(id).and_then(|order| order.price),
            Some(dec(93)),
            "{name}: the order moved"
        );

        venue.amend_bracket(
            BracketTarget::Order(id),
            Bracket::whole(Some(dec(90)), Some(dec(110))),
        );
        assert_eq!(
            venue.working_order(id).map(|order| order.bracket),
            Some(Bracket::whole(Some(dec(90)), Some(dec(110)))),
            "{name}: the order carries its legs"
        );

        venue.cancel(id);
        assert!(
            venue.working_orders().is_empty(),
            "{name}: the order is gone"
        );
        assert!(venue.is_idle(), "{name}: nothing left working or open");
    }
}

/// The bracket a trader draws on a *working* order arms the position the
/// moment that order fills — on both venues, reached the same way, even
/// though only one of them decides for itself when the fill happens.
#[test]
fn a_working_orders_bracket_arms_the_position_it_opens() {
    // The paper venue fills from the tape: a print at the limit does it.
    let mut paper = Simulator::new();
    paper.seed(&print_at(0, dec(100), 0));
    drive_bracketed_entry(&mut paper);
    paper.on_trade(&print_at(1, dec(95), 1_000));
    assert_armed(&paper, "paper simulator");

    // The fake has no fill model, so the test says when. Same port calls
    // before it, same position after it.
    let mut fake = FakeVenue::new();
    fake.seed(&print_at(0, dec(100), 0));
    let id = drive_bracketed_entry(&mut fake);
    fake.seed(&print_at(1, dec(95), 1_000));
    fake.fill(id, dec(95));
    assert_armed(&fake, "fake venue");
}

/// The caller half of the test above: everything a chart gesture would do,
/// through the port, with no idea which venue is listening.
fn drive_bracketed_entry(venue: &mut dyn TradingVenue) -> quantick_trading::OrderId {
    venue.submit(OrderIntent::limit(Side::Buy, dec(1), dec(95)));
    let id = venue.working_orders()[0].id;
    venue.amend_bracket(
        BracketTarget::Order(id),
        Bracket::whole(Some(dec(90)), Some(dec(110))),
    );
    id
}

fn assert_armed(venue: &dyn TradingVenue, name: &str) {
    let position = venue
        .position()
        .unwrap_or_else(|| panic!("{name}: the limit filled"));
    assert_eq!(position.stop_loss, Some(dec(90)), "{name}: stop armed");
    assert_eq!(position.take_profit, Some(dec(110)), "{name}: target armed");
    assert!(
        venue.working_orders().is_empty(),
        "{name}: the entry stopped working once it filled"
    );
}

/// A market entry is in flight until the market answers, on both venues —
/// the state the position and the working orders both fail to describe.
#[test]
fn a_market_entry_is_in_flight_until_the_market_answers() {
    for (name, mut venue) in both() {
        venue.seed(&print_at(0, dec(100), 0));
        venue.submit(OrderIntent::market(Side::Buy, dec(1)));

        assert_eq!(venue.in_flight(), 1, "{name}: accepted, not yet done");
        assert!(venue.position().is_none(), "{name}: nothing open yet");
        assert!(!venue.is_idle(), "{name}: an in-flight order is not idle");

        venue.on_trade(&print_at(1, dec(101), 1_000));
        assert_eq!(venue.in_flight(), 0, "{name}: the print answered it");
        assert_eq!(
            venue.position().map(|position| position.quantity),
            Some(dec(1)),
            "{name}: the position opened"
        );
    }
}

/// Closing, flattening and the realized tally all read the same through the
/// port, and a round trip lands in the venue's own closed-trade list.
#[test]
fn closing_a_position_journals_a_round_trip_on_any_venue() {
    for (name, mut venue) in both() {
        venue.seed(&print_at(0, dec(100), 0));
        venue.submit(OrderIntent::market(Side::Buy, dec(2)));
        venue.on_trade(&print_at(1, dec(100), 1_000));
        venue.on_trade(&print_at(2, dec(110), 2_000));

        venue.close(CloseAmount::Partial(dec(1)));
        venue.on_trade(&print_at(3, dec(110), 3_000));
        assert_eq!(
            venue.position().map(|position| position.quantity),
            Some(dec(1)),
            "{name}: a partial close never reverses"
        );
        assert_eq!(venue.closed_trades().len(), 1, "{name}: one round trip");
        assert_eq!(
            venue.realized_points(),
            dec(10),
            "{name}: ten points banked"
        );

        venue.flatten();
        venue.on_trade(&print_at(4, dec(110), 4_000));
        assert!(venue.is_idle(), "{name}: flat and empty");
        assert_eq!(venue.closed_trades().len(), 2, "{name}: both round trips");
    }
}

/// A refusal is an event, not an error return — so a caller that renders
/// events renders the reason, and cannot forget to.
#[test]
fn a_refused_command_answers_with_an_event_on_any_venue() {
    for (name, mut venue) in both() {
        venue.seed(&print_at(0, dec(100), 0));
        let events = venue.close(CloseAmount::All);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, VenueEvent::Rejected(_))),
            "{name}: closing nothing is refused out loud"
        );
    }
}

/// A reset abandons everything: the timeline the position was built on is
/// gone, and neither venue pretends otherwise.
#[test]
fn a_reset_abandons_orders_and_position_on_any_venue() {
    for (name, mut venue) in both() {
        venue.seed(&print_at(0, dec(100), 0));
        venue.submit(OrderIntent::limit(Side::Buy, dec(1), dec(95)));
        venue.submit(OrderIntent::market(Side::Buy, dec(1)));
        venue.on_trade(&print_at(1, dec(100), 1_000));

        venue.reset();
        assert!(venue.is_idle(), "{name}: nothing survived the reset");
        assert!(
            venue.mark_price().is_none(),
            "{name}: the market is gone too"
        );
    }
}
