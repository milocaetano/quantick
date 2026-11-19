//! A broker double: a simulator wearing a broker account's reads, steered
//! by a script the test keeps. For tests here and, through the
//! `test-support` feature, for the crates that route a paper account to a
//! broker (`quantick-session`'s entry pause).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quantick_engine::Trade;
use quantick_sim::Simulator;
pub use quantick_trading::TradeMode;
use quantick_trading::{
    AccountFacts, Bracket, BracketTarget, BrokerVenue, CloseAmount, ClosedTrade, LinkState,
    MarginMode, Order, OrderId, OrderIntent, Position, TradingVenue, VenueEvent,
};
use rust_decimal::Decimal;

/// What the test changes about the broker after handing it over.
#[derive(Default)]
pub struct Script {
    pub link: Option<LinkState>,
    pub said: Vec<VenueEvent>,
    pub tickets: BTreeMap<OrderId, u64>,
    /// Every time the desk handed the venue, and what it says then.
    pub ticked: Vec<i64>,
    pub on_tick: Vec<VenueEvent>,
    /// The terminal logged into another account of the same kind.
    pub login: Option<u64>,
}

/// A simulator wearing a broker's reads.
struct FakeBroker {
    sim: Simulator,
    facts: AccountFacts,
    script: Rc<RefCell<Script>>,
}

impl TradingVenue for FakeBroker {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        self.sim.submit(intent)
    }
    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        self.sim.amend_price(id, price)
    }
    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        self.sim.amend_bracket(target, bracket)
    }
    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        self.sim.cancel(id)
    }
    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        self.sim.close(amount)
    }
    fn flatten(&mut self) -> Vec<VenueEvent> {
        self.sim.flatten()
    }
    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        // As the MetaTrader venue does: a print drains what the broker said.
        let mut events = self.sim.on_trade(trade);
        events.append(&mut self.script.borrow_mut().said);
        events
    }
    fn seed(&mut self, trade: &Trade) {
        self.sim.seed(trade);
    }
    fn reset(&mut self) -> Vec<VenueEvent> {
        Vec::new()
    }
    fn mark_price(&self) -> Option<Decimal> {
        self.sim.mark_price()
    }
    fn mark_timestamp_ms(&self) -> Option<i64> {
        self.sim.mark_timestamp_ms()
    }
    fn position(&self) -> Option<&Position> {
        self.sim.position()
    }
    fn working_orders(&self) -> &[Order] {
        self.sim.working_orders()
    }
    fn in_flight(&self) -> usize {
        self.sim.in_flight()
    }
    fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        self.sim.in_flight_entries(out);
    }
    fn in_flight_entry(&self, id: OrderId) -> Option<&Order> {
        self.sim.in_flight_entry(id)
    }
    fn closed_trades(&self) -> &[ClosedTrade] {
        self.sim.closed_trades()
    }
    fn realized_points(&self) -> Decimal {
        self.sim.realized_points()
    }
}

impl BrokerVenue for FakeBroker {
    fn poll(&mut self) -> Vec<VenueEvent> {
        std::mem::take(&mut self.script.borrow_mut().said)
    }
    fn tick(&mut self, now_ms: i64) -> Vec<VenueEvent> {
        let mut script = self.script.borrow_mut();
        if let Some(login) = script.login {
            self.facts.login = login;
        }
        script.ticked.push(now_ms);
        std::mem::take(&mut script.on_tick)
    }
    fn set_symbol(&mut self, _symbol: &str) -> Vec<VenueEvent> {
        Vec::new()
    }
    fn account(&self) -> Option<&AccountFacts> {
        // A terminal that has not said hello has not said which account.
        (self.script.borrow().link != Some(LinkState::NeverConnected)).then_some(&self.facts)
    }
    fn link_state(&self) -> LinkState {
        self.script.borrow().link.unwrap_or(LinkState::Up)
    }
    fn entry_block(&self) -> Option<&'static str> {
        None
    }
    fn foreign_counts(&self) -> (usize, usize) {
        (1, 0)
    }
    fn order_ref(&self, id: OrderId) -> Option<u64> {
        self.script.borrow().tickets.get(&id).copied()
    }
    fn position_ref(&self) -> Option<u64> {
        Some(88_412_094)
    }
    fn is_acknowledged(&self, id: OrderId) -> bool {
        self.script.borrow().tickets.contains_key(&id)
    }
}

/// A broker on an account of `mode`, and the script that steers it.
#[must_use]
pub fn broker(mode: TradeMode) -> (Box<dyn BrokerVenue>, Rc<RefCell<Script>>) {
    let script = Rc::new(RefCell::new(Script::default()));
    let venue = FakeBroker {
        sim: Simulator::new(),
        facts: AccountFacts {
            login: 51_234_567,
            server: "XPMT5-PRD".to_owned(),
            company: "XP Investimentos".to_owned(),
            trade_mode: mode,
            margin_mode: MarginMode::Exchange,
            balance: Some(Decimal::new(1_234_000, 2)),
            currency: "BRL".to_owned(),
        },
        script: Rc::clone(&script),
    };
    (Box::new(venue), script)
}
