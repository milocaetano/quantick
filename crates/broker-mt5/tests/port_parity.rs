//! The broker venue and the fake venue, through the same port calls.
//!
//! The caller half of each test talks to `&mut dyn TradingVenue` and never
//! names the venue; the only difference is what "the market answers"
//! means — for the fake, a print; for the broker, a bridge answering what
//! it was sent ([`support::AutoBroker`]). Where the broker differs on
//! purpose (a reset closes nothing), the parity stops and
//! `tests/safety.rs` says why.

mod support;

use quantick_broker_mt5::Mt5Venue;
use quantick_broker_mt5::fake::FakeBridgeHandle;
use quantick_engine::Side;
use quantick_trading::fake::{FakeVenue, print_at};
use quantick_trading::{
    Bracket, BracketTarget, CloseAmount, OrderIntent, TradingVenue, VenueEvent,
};
use support::{AutoBroker, connected, dec};

/// One venue under test and how its market answers.
trait Market {
    fn venue(&mut self) -> &mut dyn TradingVenue;
    /// Let the market answer everything sent so far, at `price`.
    fn answer(&mut self, price: i64, at_ms: i64);
}

struct Fake(FakeVenue);

impl Market for Fake {
    fn venue(&mut self) -> &mut dyn TradingVenue {
        &mut self.0
    }

    fn answer(&mut self, price: i64, at_ms: i64) {
        self.0.on_trade(&print_at(0, dec(price), at_ms));
    }
}

struct Broker {
    venue: Mt5Venue,
    bridge: FakeBridgeHandle,
    broker: AutoBroker,
}

impl Market for Broker {
    fn venue(&mut self) -> &mut dyn TradingVenue {
        &mut self.venue
    }

    fn answer(&mut self, price: i64, at_ms: i64) {
        self.broker.fill_price = price;
        self.broker.settle(&self.bridge);
        self.venue.on_trade(&print_at(0, dec(price), at_ms));
    }
}

fn both() -> [(&'static str, Box<dyn Market>); 2] {
    let (venue, bridge) = connected();
    [
        ("fake venue", Box::new(Fake(FakeVenue::new()))),
        (
            "broker venue",
            Box::new(Broker {
                venue,
                bridge,
                broker: AutoBroker::new(100),
            }),
        ),
    ]
}

#[test]
fn the_working_order_lifecycle_runs_on_both_venues() {
    for (name, mut market) in both() {
        market.venue().seed(&print_at(0, dec(100), 0));
        let events = market
            .venue()
            .submit(OrderIntent::limit(Side::Buy, dec(2), dec(95)));
        assert!(
            matches!(events.as_slice(), [VenueEvent::Placed(_)]),
            "{name}: placed"
        );
        market.answer(100, 1);
        let id = market.venue().working_orders()[0].id;
        assert!(
            !market.venue().is_idle(),
            "{name}: a working order is not idle"
        );

        market.venue().amend_price(id, dec(93));
        market.answer(100, 2);
        assert_eq!(
            market
                .venue()
                .working_order(id)
                .and_then(|order| order.price),
            Some(dec(93)),
            "{name}: moved"
        );

        market.venue().amend_bracket(
            BracketTarget::Order(id),
            Bracket::whole(Some(dec(90)), Some(dec(110))),
        );
        market.answer(100, 3);
        assert_eq!(
            market.venue().working_order(id).map(|order| order.bracket),
            Some(Bracket::whole(Some(dec(90)), Some(dec(110)))),
            "{name}: carries its legs"
        );

        market.venue().cancel(id);
        market.answer(100, 4);
        assert!(market.venue().is_idle(), "{name}: nothing left");
    }
}

#[test]
fn a_market_entry_is_in_flight_until_the_market_answers_on_both_venues() {
    for (name, mut market) in both() {
        market.venue().seed(&print_at(0, dec(100), 0));
        market
            .venue()
            .submit(OrderIntent::market(Side::Buy, dec(1)));
        assert_eq!(market.venue().in_flight(), 1, "{name}: accepted, not done");
        assert!(
            market.venue().position().is_none(),
            "{name}: nothing open yet"
        );
        market.answer(101, 1_000);
        assert_eq!(market.venue().in_flight(), 0, "{name}: answered");
        assert_eq!(
            market.venue().position().map(|position| position.quantity),
            Some(dec(1)),
            "{name}: opened"
        );
    }
}

#[test]
fn closing_journals_round_trips_on_both_venues() {
    for (name, mut market) in both() {
        market.venue().seed(&print_at(0, dec(100), 0));
        market
            .venue()
            .submit(OrderIntent::market(Side::Buy, dec(2)));
        market.answer(100, 1_000);

        market.venue().seed(&print_at(0, dec(110), 2_000));
        market.venue().close(CloseAmount::Partial(dec(1)));
        market.answer(110, 3_000);
        assert_eq!(
            market.venue().position().map(|position| position.quantity),
            Some(dec(1)),
            "{name}: a partial close never reverses"
        );
        assert_eq!(
            market.venue().closed_trades().len(),
            1,
            "{name}: one round trip"
        );
        assert_eq!(
            market.venue().realized_points(),
            dec(10),
            "{name}: ten points"
        );

        market.venue().flatten();
        market.answer(110, 4_000);
        assert!(market.venue().is_idle(), "{name}: flat and empty");
        assert_eq!(
            market.venue().closed_trades().len(),
            2,
            "{name}: both trips"
        );
    }
}

#[test]
fn a_refused_command_answers_with_an_event_on_both_venues() {
    for (name, mut market) in both() {
        market.venue().seed(&print_at(0, dec(100), 0));
        let events = market.venue().close(CloseAmount::All);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, VenueEvent::Rejected(_))),
            "{name}: closing nothing is refused out loud"
        );
    }
}
