//! [`Mt5Venue`]: the state machine and a link, behind the
//! [`TradingVenue`] port.

use quantick_engine::Trade;
use quantick_trading::{
    Bracket, BracketTarget, BrokerVenue, CloseAmount, ClosedTrade, Order, OrderId, OrderIntent,
    Position, TradingVenue, VenueEvent,
};
use rust_decimal::Decimal;

use crate::account::AccountFacts;
use crate::link::{BridgeLink, LinkEvent};
use crate::machine::{BrokerConfig, BrokerCore, ForeignExposure, LinkState};

/// A [`TradingVenue`] that executes through the logged-in MetaTrader 5
/// terminal.
///
/// Every write returns at once with what was accepted and sent — a
/// [`VenueEvent::Placed`] order is on its way, not yet at the broker. The
/// broker's answers (acknowledgements, fills, refusals in its own words)
/// arrive later, from [`TradingVenue::on_trade`] or from [`Mt5Venue::poll`],
/// which a caller with no print to hand calls once per frame.
pub struct Mt5Venue {
    core: BrokerCore,
    link: Box<dyn BridgeLink>,
    scratch: Vec<LinkEvent>,
}

impl std::fmt::Debug for Mt5Venue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mt5Venue")
            .field("core", &self.core)
            .finish_non_exhaustive()
    }
}

impl Mt5Venue {
    #[must_use]
    pub fn new(config: BrokerConfig, link: Box<dyn BridgeLink>) -> Self {
        Self {
            core: BrokerCore::new(config),
            link,
            scratch: Vec::new(),
        }
    }

    /// Everything the bridge said since the last drain, folded in.
    pub fn poll(&mut self) -> Vec<VenueEvent> {
        let mut events = Vec::new();
        self.link.poll(&mut self.scratch);
        for event in self.scratch.drain(..) {
            events.extend(match event {
                LinkEvent::Line(line) => self.core.on_line(&line),
                LinkEvent::Disconnected => self.core.on_disconnected(),
            });
        }
        self.flush();
        events
    }

    /// The time, from a caller that owns a clock: a link silent past
    /// [`crate::machine::STALE_AFTER_MS`] is declared lost.
    pub fn tick(&mut self, now_ms: i64) -> Vec<VenueEvent> {
        self.link.clock(now_ms);
        self.core.on_clock(now_ms)
    }

    fn flush(&mut self) {
        for line in self.core.take_outbox() {
            self.link.send(&line);
        }
    }

    fn write(&mut self, act: impl FnOnce(&mut BrokerCore) -> Vec<VenueEvent>) -> Vec<VenueEvent> {
        let events = act(&mut self.core);
        self.flush();
        events
    }

    /// Trade and read another symbol; nothing at the broker is touched.
    pub fn set_symbol(&mut self, symbol: &str) -> Vec<VenueEvent> {
        self.write(|core| core.set_symbol(symbol))
    }

    /// The state machine, for every read the port does not cover: the
    /// account, the link, foreign exposure, tickets and acknowledgements.
    #[must_use]
    pub fn core(&self) -> &BrokerCore {
        &self.core
    }

    #[must_use]
    pub fn account(&self) -> Option<&AccountFacts> {
        self.core.account()
    }

    #[must_use]
    pub fn link_state(&self) -> LinkState {
        self.core.link_state()
    }

    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.core.is_stale()
    }

    #[must_use]
    pub fn foreign_exposure(&self) -> &ForeignExposure {
        self.core.foreign_exposure()
    }

    #[must_use]
    pub fn entry_block(&self) -> Option<&'static str> {
        self.core.entry_block()
    }

    #[must_use]
    pub fn ticket(&self, id: OrderId) -> Option<u64> {
        self.core.ticket(id)
    }

    #[must_use]
    pub fn is_acknowledged(&self, id: OrderId) -> bool {
        self.core.is_acknowledged(id)
    }
}

impl TradingVenue for Mt5Venue {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        self.write(|core| core.submit(intent))
    }

    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        self.write(|core| core.amend_price(id, price))
    }

    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        self.write(|core| core.amend_bracket(target, bracket))
    }

    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        self.write(|core| core.cancel(id))
    }

    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        self.write(|core| core.close(amount))
    }

    fn flatten(&mut self) -> Vec<VenueEvent> {
        self.write(|core| core.flatten())
    }

    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        self.core.seed(trade);
        self.poll()
    }

    fn seed(&mut self, trade: &Trade) {
        self.core.seed(trade);
    }

    fn reset(&mut self) -> Vec<VenueEvent> {
        self.write(BrokerCore::reset)
    }

    fn mark_price(&self) -> Option<Decimal> {
        self.core.mark_price()
    }

    fn mark_timestamp_ms(&self) -> Option<i64> {
        self.core.mark_timestamp_ms()
    }

    fn position(&self) -> Option<&Position> {
        self.core.position()
    }

    fn working_orders(&self) -> &[Order] {
        self.core.working_orders()
    }

    fn in_flight(&self) -> usize {
        self.core.in_flight()
    }

    fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        self.core.in_flight_entries(out);
    }

    fn in_flight_entry(&self, id: OrderId) -> Option<&Order> {
        self.core.in_flight_entry(id)
    }

    fn closed_trades(&self) -> &[ClosedTrade] {
        self.core.closed_trades()
    }

    fn realized_points(&self) -> Decimal {
        self.core.realized_points()
    }
}

/// The reads the account that routes orders needs, in the venue-neutral
/// vocabulary: it never names this crate.
impl BrokerVenue for Mt5Venue {
    fn poll(&mut self) -> Vec<VenueEvent> {
        Mt5Venue::poll(self)
    }

    fn tick(&mut self, now_ms: i64) -> Vec<VenueEvent> {
        Mt5Venue::tick(self, now_ms)
    }

    fn set_symbol(&mut self, symbol: &str) -> Vec<VenueEvent> {
        Mt5Venue::set_symbol(self, symbol)
    }

    fn account(&self) -> Option<&AccountFacts> {
        self.core.account()
    }

    fn link_state(&self) -> LinkState {
        self.core.link_state()
    }

    fn entry_block(&self) -> Option<&'static str> {
        self.core.entry_block()
    }

    fn foreign_counts(&self) -> (usize, usize) {
        let foreign = self.core.foreign_exposure();
        (foreign.positions.len(), foreign.orders.len())
    }

    fn order_ref(&self, id: OrderId) -> Option<u64> {
        self.core.ticket(id)
    }

    fn position_ref(&self) -> Option<u64> {
        self.core.position_ticket()
    }

    fn is_acknowledged(&self, id: OrderId) -> bool {
        self.core.is_acknowledged(id)
    }
}
