//! A second [`TradingVenue`](quantick_trading::TradingVenue): orders
//! executed by the MetaTrader 5 terminal the trader is already logged into.
//!
//! Every existing order surface — the chart's gestures, the ticket, the
//! control plane — talks to the port and never learns which venue answers.
//! This crate is the answer for a real (or demo) broker account, reached
//! through `bridge/mt5/quantick_bridge_exec.py` on its own loopback port.
//!
//! # Two halves
//!
//! - [`machine`] decides everything and touches nothing: intents become
//!   request lines, bridge lines become [`VenueEvent`]s and read state. No
//!   clock, no socket, no thread — so every rule is tested by scripting the
//!   bridge's lines ([`fake::FakeBridge`]).
//! - [`transport`] owns the one socket and its two threads, the way the
//!   `feed` level owns the market-data runtimes, and decides nothing.
//!
//! [`Mt5Venue`] joins them behind the port.
//!
//! # What this venue will not do
//!
//! - **Touch what it did not place.** A position or order on the symbol with
//!   another magic number is shown ([`ForeignExposure`]) and never modified;
//!   while one exists, every new entry is refused, because a netting account
//!   would merge it into someone else's position.
//! - **Close anything on a reset.** A symbol switch or a replay seek leaves
//!   the broker's orders and position where they are and says so
//!   ([`VenueNotice::ResetLeftOpen`]).
//! - **Emulate a stop.** Protective prices are the broker's own SL/TP on the
//!   position or the pending order. What the broker has no native way to
//!   hold — an exit ladder, a cancel-at price, a fill-only-when-flat
//!   condition — is refused, never imitated from this side.
//! - **Send an order twice.** Every request carries an id the bridge
//!   remembers; a request resent after a dropped link is answered from that
//!   memory. A bridge that restarted has no memory, so nothing is resent to
//!   it: the request is reported as of unknown outcome
//!   ([`VenueNotice::OutcomeUnknown`]) and the next state shows the truth.
//! - **Trade a hedging account.** Refused at connect, with the reason.
//!
//! [`VenueEvent`]: quantick_trading::VenueEvent
//! [`VenueNotice::ResetLeftOpen`]: quantick_trading::VenueNotice::ResetLeftOpen
//! [`VenueNotice::OutcomeUnknown`]: quantick_trading::VenueNotice::OutcomeUnknown

pub mod account;
pub mod fake;
pub mod link;
pub mod machine;
pub mod transport;
pub mod venue;
pub mod wire;

pub use account::{AccountFacts, MarginMode, TradeMode};
pub use link::{BridgeLink, LinkEvent};
pub use machine::{
    BrokerConfig, BrokerCore, ForeignExposure, ForeignOrder, ForeignPosition, LinkState,
};
pub use transport::TcpBridgeLink;
pub use venue::Mt5Venue;
