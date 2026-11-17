//! What a broker-backed venue says beyond the order port.
//!
//! [`TradingVenue`] is the lifecycle every surface trades through, and a
//! simulator answers all of it. A broker answers more: which account it is
//! logged into and whether real money is at stake, whether the link to it is
//! up, the broker's own ticket for an order, and whether the broker has
//! confirmed it yet. [`BrokerVenue`] is that extension, so the account that
//! routes orders and the surfaces that label them read one vocabulary and
//! never the adapter crate that implements it.
//!
//! The facts are read, never configured: a route the trader picks is checked
//! against [`AccountFacts::trade_mode`], not the other way round.

use rust_decimal::Decimal;

use crate::events::VenueEvent;
use crate::order::OrderId;
use crate::venue::TradingVenue;

/// Whether real money is at stake, as the terminal reports it. Read, never
/// configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeMode {
    Demo,
    Contest,
    Real,
    /// A value the adapter does not know. Treated as real by every caller
    /// that has to choose: guessing "demo" is the mistake that costs money.
    Unknown,
}

impl TradeMode {
    /// Whether a caller must treat the account as real money.
    #[must_use]
    pub fn is_real_money(self) -> bool {
        matches!(self, Self::Real | Self::Unknown)
    }
}

/// How the account holds positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginMode {
    /// One net position per symbol (retail netting).
    Netting,
    /// One net position per symbol, on an exchange (B3).
    Exchange,
    /// Several positions per symbol. Refused: every surface reads one net
    /// position, and a hedged account would show the trader a sum that is
    /// not what the broker closes.
    Hedging,
    Unknown,
}

impl MarginMode {
    /// Whether the account nets, the only kind a broker venue trades.
    #[must_use]
    pub fn nets(self) -> bool {
        matches!(self, Self::Netting | Self::Exchange)
    }
}

/// The account facts a surface shows beside a broker route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFacts {
    pub login: u64,
    pub server: String,
    pub company: String,
    pub trade_mode: TradeMode,
    pub margin_mode: MarginMode,
    /// `None` when the terminal sent something that is not a number.
    pub balance: Option<Decimal>,
    pub currency: String,
}

impl AccountFacts {
    /// Whether `other` is the same account: same login on the same server.
    #[must_use]
    pub fn same_account(&self, other: &Self) -> bool {
        self.login == other.login && self.server == other.server
    }

    /// The short name a label wears: the broker's first word and the login
    /// (`XP 51234567`). The company is the broker's own spelling; an empty
    /// one leaves the login alone.
    #[must_use]
    pub fn short_name(&self) -> String {
        match self.company.split_whitespace().next() {
            Some(word) => format!("{word} {}", self.login),
            None => self.login.to_string(),
        }
    }
}

/// Whether the link to the broker is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Nothing has connected yet.
    NeverConnected,
    Up,
    /// It was up and dropped. Reads are the last known state, stale.
    Lost,
}

/// A [`TradingVenue`] backed by a broker account: the order port plus the
/// reads only a broker has.
///
/// Object-safe for the reason the port is: an account holds one behind a
/// `Box` and must not learn which broker answers.
pub trait BrokerVenue: TradingVenue {
    /// Everything the broker said since the last drain, folded in. A caller
    /// with no print to hand calls this once per frame.
    fn poll(&mut self) -> Vec<VenueEvent>;

    /// Trade and read another symbol. Nothing at the broker is touched.
    fn set_symbol(&mut self, symbol: &str) -> Vec<VenueEvent>;

    /// The account the broker is logged into, once it has said.
    fn account(&self) -> Option<&AccountFacts>;

    /// Whether the link is up.
    fn link_state(&self) -> LinkState;

    /// Why a new entry would be refused right now, if it would.
    fn entry_block(&self) -> Option<&'static str>;

    /// Positions and working orders on the symbol that belong to someone
    /// else (another magic, the terminal's own hand): shown, never touched.
    fn foreign_counts(&self) -> (usize, usize);

    /// The broker's ticket for a working order, once it has one.
    fn order_ref(&self, id: OrderId) -> Option<u64>;

    /// The broker's ticket for the open position.
    fn position_ref(&self) -> Option<u64>;

    /// Whether the broker confirmed this order as it stands. `false` paints
    /// it dashed: sent, not yet held.
    fn is_acknowledged(&self, id: OrderId) -> bool;
}

/// What builds the one broker venue the first time a route needs it, with
/// whatever it needs running beside it (a bridge process, a socket).
///
/// The route owns the decision of *when*; the implementor owns *how*, so the
/// account that routes orders never learns which broker answers or what it
/// takes to reach it. Ending the implementor ends what it started.
pub trait BrokerConnector {
    /// A venue trading `symbol`, or `None` when it cannot be built now (the
    /// implementor says why in its log). Asked again the next time a route
    /// needs one.
    fn connect(&mut self, symbol: &str) -> Option<Box<dyn BrokerVenue>>;

    /// Cut the link on purpose: a staged capture's lost link. A real link
    /// only drops on its own, so the default does nothing.
    fn sever(&mut self) {}
}
