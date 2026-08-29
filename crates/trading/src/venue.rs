//! The port every execution backend implements.
//!
//! One trait stands between the surfaces a trader touches — the chart's
//! order-entry gestures, the ticket, the control-plane actions — and the
//! thing that actually executes. Today the only implementation is the
//! deterministic paper simulator in `quantick-sim`; a real broker adapter
//! docks here without any of those surfaces learning a second vocabulary.
//!
//! # What the trait is careful *not* to assume
//!
//! - **Not that the caller drives time.** [`TradingVenue::on_trade`] hands
//!   the venue the tape. A paper venue *is* the tape and fills from it; a
//!   real venue takes it as the market it quotes against and reports its own
//!   fills through the same event stream whenever they arrive. Either way the
//!   caller's loop is the same, and neither kind reads a wall clock.
//! - **Not that a command succeeds.** Every write returns the events it
//!   caused, and a refusal is one of them ([`VenueEvent::Rejected`]) rather
//!   than an error type the caller may forget to render. Refusals are the
//!   curriculum: they are meant to reach the trader verbatim.
//! - **Not that reads are cheap to build.** Every read borrows what the venue
//!   already holds. A chart repaints these per frame, per pane, so a port
//!   that made the caller allocate to ask "am I in?" would be the wrong port.

use quantick_engine::Trade;
use rust_decimal::Decimal;

use crate::events::VenueEvent;
use crate::intent::{BracketTarget, CloseAmount, OrderIntent};
use crate::order::{Bracket, Order, OrderId};
use crate::position::{ClosedTrade, Position};

/// An execution backend: orders in, fills and refusals out.
///
/// Object-safe on purpose — a chart holds one of these behind a `Box` and
/// must not be generic over which venue it is talking to.
pub trait TradingVenue {
    // --- writes -------------------------------------------------------

    /// Place an order. The returned events say whether it was accepted
    /// ([`VenueEvent::Placed`]) or refused ([`VenueEvent::Rejected`]).
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent>;

    /// Move a working order to a new price, validated as if it were being
    /// placed there.
    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent>;

    /// Replace the protective prices of a position or a working order,
    /// wholesale — a `None` leg clears that side.
    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent>;

    /// Remove a working order without filling it.
    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent>;

    /// Close all or part of the open position at the market.
    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent>;

    /// Cancel every working order and close the position.
    fn flatten(&mut self) -> Vec<VenueEvent>;

    // --- the market ---------------------------------------------------

    /// Show the venue a trade. See the module doc for why this is not
    /// "advance the simulation".
    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent>;

    /// Show the venue a price without letting it act on one: backfill and
    /// paged history establish where the market is, and filling against
    /// them would be trading on the past.
    fn seed(&mut self, trade: &Trade);

    /// Abandon everything: the timeline the position was built on is gone
    /// (a replay seek, a symbol switch). The venue closes at the last mark
    /// and says so rather than pretending a continuity it cannot have.
    fn reset(&mut self) -> Vec<VenueEvent>;

    // --- reads --------------------------------------------------------

    /// The last price the venue was shown, or `None` before the first one.
    fn mark_price(&self) -> Option<Decimal>;

    /// Venue time of that last price. Not a wall clock: it is the market's
    /// own timestamp, so a replay and a live session read the same way.
    fn mark_timestamp_ms(&self) -> Option<i64>;

    /// The open net position, if there is one.
    fn position(&self) -> Option<&Position>;

    /// Orders resting at a price, in placement order.
    fn working_orders(&self) -> &[Order];

    /// Actions accepted but not yet answered by the market — a market
    /// order between the click and the print that fills it, a close
    /// waiting for the same. A count rather than the actions themselves:
    /// nothing paints them, and most callers only ask whether the account
    /// is truly idle.
    fn in_flight(&self) -> usize;

    /// The *entries* among those, appended to `out`, oldest first. A
    /// caller-owned buffer rather than a returned `Vec` because the one
    /// surface that needs the ids ("cancel everything working") is also
    /// the one that must not allocate a vector per frame to find out
    /// there is nothing to cancel. Venues with nothing in flight append
    /// nothing.
    fn in_flight_entries(&self, out: &mut Vec<OrderId>);

    /// Round trips completed this session, oldest first.
    fn closed_trades(&self) -> &[ClosedTrade];

    /// Realized profit this session, in points.
    fn realized_points(&self) -> Decimal;

    // --- derived ------------------------------------------------------

    /// Nothing open, nothing working, nothing in flight.
    fn is_idle(&self) -> bool {
        self.position().is_none() && self.working_orders().is_empty() && self.in_flight() == 0
    }

    /// The working order with this id, if it is still working.
    fn working_order(&self, id: OrderId) -> Option<&Order> {
        self.working_orders().iter().find(|order| order.id == id)
    }
}
