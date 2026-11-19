//! New entries wait while the chart is out of sync with its feed.
//!
//! The host judges its feed (`quantick_feed::sync`) and tells the tab's
//! paper before anything can trade in a frame ([`TabPaper::set_trade_gate`]).
//! While a gate stands, every door an order comes through — a command, a
//! chart placement or reprice, an armed strategy — is asked one question:
//! would it open or grow exposure? An order on the other side of the open
//! position goes while the position still covers it after every reducing
//! order already working or in flight; everything else is refused whole,
//! never trimmed to its reducing part, and a reversal always opens the
//! other side. Protective legs, cancels and closes are not
//! orders that open anything and never wait.

use std::borrow::BorrowMut;

use quantick_engine::Side;
use quantick_paper::PaperAccount;
use quantick_paper::desk::ChartCommand;
use quantick_sim::{Command, OrderId, RejectReason, VenueEvent};
use quantick_sources::sync::{Desync, STRATEGY_ENTRY_PAUSED, SyncReading};
use rust_decimal::Decimal;

use super::{TabPaper, TradeCommand};

impl<A: BorrowMut<PaperAccount>> TabPaper<A> {
    /// Pause new entries for `gate`, or resume them with `None`: the host's
    /// judgement of its feed.
    pub fn set_trade_gate(&mut self, gate: Option<Desync>) {
        self.gate = gate;
    }

    #[must_use]
    pub fn trade_gate(&self) -> Option<&Desync> {
        self.gate.as_ref()
    }

    /// Carry out what a chart press asked for, through the desk. A placement
    /// is an entry and a reprice moves one; each waits out a pause unless
    /// it only reduces the position.
    ///
    /// # Errors
    ///
    /// The ticket's complaint when a placement's offsets do not parse.
    pub fn carry_out(&mut self, command: ChartCommand) -> Result<(), String> {
        let paused = match command {
            ChartCommand::PlaceResting { side, .. } => self.pauses_ticket_entry(side),
            ChartCommand::PlaceArmed { armed, .. } => self.pauses_ticket_entry(armed.side),
            ChartCommand::AmendOrder { id, .. } => self.order_grows(id) && self.refuse(),
            _ => false,
        };
        if paused {
            return Ok(());
        }
        self.desk.carry_out(self.account.borrow_mut(), command)
    }

    /// The pause that refuses `command`, toasted, when it would open or
    /// grow exposure: a market press at the size the ticket would send, a
    /// placement as stated, and every reversal.
    pub(super) fn entry_paused(&mut self, command: &TradeCommand) -> Option<Desync> {
        self.gate.as_ref()?;
        let grows = match command {
            TradeCommand::Market { side } => self.grows(*side, self.ticket_quantity(), None),
            TradeCommand::Place { intent } => self.grows(intent.side, Some(intent.quantity), None),
            TradeCommand::Reverse => true,
            TradeCommand::Amend { .. }
            | TradeCommand::Cancel { .. }
            | TradeCommand::CancelAll
            | TradeCommand::Close
            | TradeCommand::Flatten => false,
        };
        grows.then(|| self.refusal()).flatten()
    }

    /// Whether a ticket entry on `side` waits out a pause; toasts when it
    /// does.
    pub(super) fn pauses_ticket_entry(&mut self, side: Side) -> bool {
        self.grows(side, self.ticket_quantity(), None) && self.refuse()
    }

    /// A print reaching the desk is the tape arriving: a pause for its age
    /// is over now, not at the next judgement, so a strategy this print
    /// fires is not refused on the silence it just ended.
    pub(super) fn tape_arrived(&mut self) {
        if (self.gate.as_ref()).is_some_and(|gate| gate.reading == SyncReading::TapeAge) {
            self.gate = None;
        }
    }

    /// The venue's answer to an armed strategy's command that waits out a
    /// pause, or `None` when it may go.
    pub(super) fn strategy_refusal(&self, command: &Command) -> Option<Vec<VenueEvent>> {
        let blocked = RejectReason::Blocked(STRATEGY_ENTRY_PAUSED);
        self.strategy_entry_paused(command)
            .then(|| vec![VenueEvent::Rejected(blocked)])
    }

    /// Whether an armed strategy's command waits out a pause: an entry that
    /// would grow exposure, or a reprice of one. No toast — a strategy acts
    /// on every print, and its refusal is the venue's answer.
    fn strategy_entry_paused(&self, command: &Command) -> bool {
        if self.gate.is_none() {
            return false;
        }
        match command {
            Command::PlaceMarket { side, quantity, .. }
            | Command::PlaceLimit { side, quantity, .. }
            | Command::PlaceStop { side, quantity, .. } => self.grows(*side, Some(*quantity), None),
            Command::ModifyOrder { id, .. } => self.order_grows(*id),
            _ => false,
        }
    }

    /// Whether an order of `quantity` on `side` would open or grow exposure:
    /// anything but the other side of the open position, at no more than
    /// what is left of it once every order on that side already working or
    /// in flight — `except` the one being judged — has filled. An unknown
    /// size is treated as growing.
    fn grows(&self, side: Side, quantity: Option<Decimal>, except: Option<OrderId>) -> bool {
        let venue = self.core().venue();
        let (Some(open), Some(quantity)) = (venue.position(), quantity) else {
            return true;
        };
        let mut in_flight = Vec::new();
        venue.in_flight_entries(&mut in_flight);
        let in_flight = in_flight
            .into_iter()
            .filter_map(|id| venue.in_flight_entry(id));
        let reducing: Decimal = (venue.working_orders().iter().chain(in_flight))
            .filter(|order| order.side == side && Some(order.id) != except)
            .map(|order| order.quantity)
            .sum();
        open.side == side || quantity + reducing > open.quantity
    }

    /// Whether the working order `id` would grow exposure if it filled.
    fn order_grows(&self, id: OrderId) -> bool {
        let orders = self.core().working_orders();
        let order = orders.iter().find(|order| order.id == id);
        self.gate.is_some()
            && order.is_some_and(|order| self.grows(order.side, Some(order.quantity), Some(id)))
    }

    fn ticket_quantity(&self) -> Option<Decimal> {
        self.desk.entry_quantity(self.core())
    }

    /// Toast the pause in the route's word; whether one stands.
    fn refuse(&mut self) -> bool {
        self.refusal().is_some()
    }

    /// While entries are paused: post the cause as the toast, in the route's
    /// word, and answer with the pause.
    fn refusal(&mut self) -> Option<Desync> {
        let gate = self.gate.clone()?;
        let word = self.core().route_kind().word();
        self.show_toast(format!("{word}: {}", gate.sentence()));
        Some(gate)
    }
}
