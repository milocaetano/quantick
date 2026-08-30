//! A second implementation of [`TradingVenue`], so the port has more than
//! one.
//!
//! Its whole job is to be *unlike* the paper simulator. It has no fill model
//! — a test says when an order fills, and at what price — so a caller that
//! works against this venue is a caller that assumed nothing about how
//! execution happens. Anything paper-specific that leaks into the port stops
//! compiling here, which is the point.
//!
//! It is shipped rather than hidden behind `#[cfg(test)]` for the same
//! reason `quantick_control::fake` is: the implementations that have to
//! prove themselves against it live in other crates.

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

use crate::events::{CancelReason, ExitReason, Fill, FillRole, RejectReason, VenueEvent};
use crate::intent::{BracketTarget, CloseAmount, OrderIntent};
use crate::order::{Bracket, Order, OrderId, OrderRole};
use crate::position::{ClosedTrade, Position, signed_points};
use crate::venue::TradingVenue;

/// A scriptable venue: it accepts what it is told to accept, and fills what
/// it is told to fill.
#[derive(Debug, Default)]
pub struct FakeVenue {
    next_id: u64,
    mark: Option<Decimal>,
    last_ms: i64,
    last_agg_id: u64,
    resting: Vec<Order>,
    queued: Vec<Order>,
    position: Option<Position>,
    closed: Vec<ClosedTrade>,
    realized: Decimal,
    /// Every intent this venue was handed, in order. What a test asserts
    /// the caller *asked for*, independently of what happened next.
    pub submitted: Vec<OrderIntent>,
    /// The refusal to answer the next write with, if a test armed one.
    refusal: Option<RejectReason>,
}

impl FakeVenue {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer the next write with this refusal instead of acting on it.
    pub fn refuse_next(&mut self, reason: RejectReason) {
        self.refusal = Some(reason);
    }

    /// Fill a working order at `price`, whatever the market has done — the
    /// fake has no tape and no opinion about when an order should fill.
    pub fn fill(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        let Some(index) = self.resting.iter().position(|order| order.id == id) else {
            return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
        };
        let order = self.resting.remove(index);
        let mut events = Vec::new();
        self.execute(&order, price, FillRole::Entry(order.id), &mut events);
        events
    }

    /// Take the refusal a test armed, if there is one.
    fn refused(&mut self) -> Option<Vec<VenueEvent>> {
        self.refusal
            .take()
            .map(|reason| vec![VenueEvent::Rejected(reason)])
    }

    fn allocate(&mut self, intent: OrderIntent) -> Order {
        let id = OrderId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        Order {
            id,
            side: intent.side,
            kind: intent.kind,
            price: intent.price,
            quantity: intent.quantity,
            bracket: intent.bracket,
            cancel_at: intent.cancel_at,
            flat_only: intent.flat_only,
            placed_ms: self.last_ms,
            // The fake models no ladder: it arms the position pair a plain
            // bracket carries and nothing else, which is exactly the part of
            // the contract the parity tests hold both venues to.
            role: OrderRole::Entry,
            oco: None,
            reduce_only: false,
        }
    }

    /// Apply one execution to the net position, closing before opening.
    fn execute(
        &mut self,
        order: &Order,
        price: Decimal,
        role: FillRole,
        events: &mut Vec<VenueEvent>,
    ) {
        events.push(VenueEvent::Filled(Fill {
            timestamp_ms: self.last_ms,
            agg_id: self.last_agg_id,
            side: order.side,
            price,
            quantity: order.quantity,
            role,
        }));
        match self.position.take() {
            Some(open) if open.side != order.side => {
                let closed = open.quantity.min(order.quantity);
                self.close_quantity(&open, closed, price, ExitReason::Reversal, events);
                let kept = open.quantity.saturating_sub(closed);
                let remainder = order.quantity.saturating_sub(closed);
                if kept > Decimal::ZERO {
                    let mut open = open;
                    open.quantity = kept;
                    self.position = Some(open);
                } else if remainder > Decimal::ZERO {
                    self.position = Some(self.opened(order, price, remainder));
                }
            }
            Some(mut open) => {
                let total = open.quantity.saturating_add(order.quantity);
                let weighted = open
                    .avg_price
                    .saturating_mul(open.quantity)
                    .saturating_add(price.saturating_mul(order.quantity));
                if total > Decimal::ZERO {
                    open.avg_price = weighted / total;
                }
                open.quantity = total;
                open.observe(price);
                self.position = Some(open);
            }
            None => self.position = Some(self.opened(order, price, order.quantity)),
        }
        if let Some(position) = self.position.as_mut()
            && !order.bracket.is_empty()
        {
            position.stop_loss = order.bracket.stop_loss();
            position.take_profit = order.bracket.take_profit();
        }
    }

    fn opened(&self, order: &Order, price: Decimal, quantity: Decimal) -> Position {
        Position {
            side: order.side,
            quantity,
            avg_price: price,
            opened_ms: self.last_ms,
            opened_agg_id: self.last_agg_id,
            low_price: price,
            high_price: price,
            stop_loss: None,
            take_profit: None,
        }
    }

    fn close_quantity(
        &mut self,
        position: &Position,
        quantity: Decimal,
        price: Decimal,
        reason: ExitReason,
        events: &mut Vec<VenueEvent>,
    ) {
        let pnl = signed_points(position.side, position.avg_price, price, quantity);
        self.realized = self.realized.saturating_add(pnl);
        let (adverse, favorable) = position.excursions(price);
        let trade = ClosedTrade {
            side: position.side,
            quantity,
            entry_price: position.avg_price,
            exit_price: price,
            opened_ms: position.opened_ms,
            closed_ms: self.last_ms,
            pnl_points: pnl,
            exit_reason: reason,
            entry_agg_id: Some(position.opened_agg_id),
            exit_agg_id: Some(self.last_agg_id),
            mae_points: Some(adverse.saturating_mul(quantity)),
            mfe_points: Some(favorable.saturating_mul(quantity)),
        };
        self.closed.push(trade.clone());
        events.push(VenueEvent::Closed(trade));
    }
}

impl TradingVenue for FakeVenue {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        self.submitted.push(intent);
        if let Some(refusal) = self.refused() {
            return refusal;
        }
        let order = self.allocate(intent);
        let placed = VenueEvent::Placed(order.clone());
        if intent.rests() {
            self.resting.push(order);
        } else {
            self.queued.push(order);
        }
        vec![placed]
    }

    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        if let Some(refusal) = self.refused() {
            return refusal;
        }
        let Some(order) = self.resting.iter_mut().find(|order| order.id == id) else {
            return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
        };
        order.price = Some(price);
        vec![VenueEvent::Updated(order.clone())]
    }

    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        if let Some(refusal) = self.refused() {
            return refusal;
        }
        match target {
            BracketTarget::Position => {
                let Some(position) = self.position.as_mut() else {
                    return vec![VenueEvent::Rejected(RejectReason::NoPosition)];
                };
                position.stop_loss = bracket.stop_loss();
                position.take_profit = bracket.take_profit();
                vec![VenueEvent::BracketSet {
                    stop_loss: bracket.stop_loss(),
                    take_profit: bracket.take_profit(),
                }]
            }
            BracketTarget::Order(id) => {
                let Some(order) = self.resting.iter_mut().find(|order| order.id == id) else {
                    return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
                };
                order.bracket = bracket;
                vec![VenueEvent::Updated(order.clone())]
            }
        }
    }

    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        if let Some(refusal) = self.refused() {
            return refusal;
        }
        let Some(index) = self.resting.iter().position(|order| order.id == id) else {
            return vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))];
        };
        vec![VenueEvent::Cancelled {
            order: self.resting.remove(index),
            reason: CancelReason::User,
        }]
    }

    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        if let Some(refusal) = self.refused() {
            return refusal;
        }
        let Some(position) = self.position.take() else {
            return vec![VenueEvent::Rejected(RejectReason::NoPosition)];
        };
        let price = self.mark.unwrap_or(position.avg_price);
        let wanted = match amount {
            CloseAmount::All => position.quantity,
            CloseAmount::Partial(quantity) => quantity.min(position.quantity),
        };
        let mut events = Vec::new();
        self.close_quantity(&position, wanted, price, ExitReason::Manual, &mut events);
        let kept = position.quantity.saturating_sub(wanted);
        if kept > Decimal::ZERO {
            let mut position = position;
            position.quantity = kept;
            self.position = Some(position);
        }
        events
    }

    fn flatten(&mut self) -> Vec<VenueEvent> {
        let mut events: Vec<VenueEvent> = self
            .resting
            .drain(..)
            .chain(self.queued.drain(..))
            .map(|order| VenueEvent::Cancelled {
                order,
                reason: CancelReason::Flatten,
            })
            .collect();
        if self.position.is_some() {
            events.extend(self.close(CloseAmount::All));
        }
        events
    }

    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        self.seed(trade);
        // The only automatic behaviour the fake has: a market order was
        // accepted for "the next print", so the next print is when it
        // happens. A resting order waits for an explicit `fill`.
        let queued = std::mem::take(&mut self.queued);
        let mut events = Vec::new();
        for order in queued {
            self.execute(&order, trade.price, FillRole::Entry(order.id), &mut events);
        }
        events
    }

    fn seed(&mut self, trade: &Trade) {
        self.mark = Some(trade.price);
        self.last_ms = trade.timestamp_ms;
        self.last_agg_id = trade.agg_id;
        if let Some(position) = self.position.as_mut() {
            position.observe(trade.price);
        }
    }

    fn reset(&mut self) -> Vec<VenueEvent> {
        let mut events: Vec<VenueEvent> = self
            .resting
            .drain(..)
            .chain(self.queued.drain(..))
            .map(|order| VenueEvent::Cancelled {
                order,
                reason: CancelReason::Reset,
            })
            .collect();
        if let Some(position) = self.position.take() {
            let price = self.mark.unwrap_or(position.avg_price);
            let quantity = position.quantity;
            self.close_quantity(&position, quantity, price, ExitReason::Reset, &mut events);
        }
        self.mark = None;
        events
    }

    fn mark_price(&self) -> Option<Decimal> {
        self.mark
    }

    fn mark_timestamp_ms(&self) -> Option<i64> {
        self.mark.map(|_| self.last_ms)
    }

    fn position(&self) -> Option<&Position> {
        self.position.as_ref()
    }

    fn working_orders(&self) -> &[Order] {
        &self.resting
    }

    fn in_flight(&self) -> usize {
        self.queued.len()
    }

    fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        // Everything this venue queues is an entry: it answers a close at
        // once, against its own last mark.
        out.extend(self.queued.iter().map(|order| order.id));
    }

    fn closed_trades(&self) -> &[ClosedTrade] {
        &self.closed
    }

    fn realized_points(&self) -> Decimal {
        self.realized
    }
}

/// A print, for tests that need to move a fake venue's market.
#[must_use]
pub fn print_at(agg_id: u64, price: Decimal, timestamp_ms: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}
