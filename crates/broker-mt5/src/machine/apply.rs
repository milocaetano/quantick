//! What the bridge says, folded into the venue: the link's hello, each
//! reply, each pushed state and the deals inside it.

use quantick_engine::Side;
use quantick_trading::{
    Bracket, CancelReason, ClosedTrade, EntryKind, ExitReason, Fill, FillRole, Order, OrderId,
    OrderRole, Position, RejectReason, VenueEvent, VenueNotice, VenueText, signed_points,
};
use rust_decimal::Decimal;

use super::{
    BrokerCore, ForeignExposure, ForeignOrder, ForeignPosition, LinkState, NOT_NETTING, Pending,
    accepted,
};
use crate::account::facts_from_line;
use crate::wire::{
    self, ClosedOrderLine, DealLine, EXEC_SCHEMA_VERSION, Inbound, OrderLine, PositionLine, Reply,
    Request, State,
};

impl BrokerCore {
    /// Fold one line from the bridge into the venue.
    pub fn on_line(&mut self, line: &str) -> Vec<VenueEvent> {
        match wire::decode(line) {
            Some(Inbound::ExecHello { schema, instance }) => self.on_hello(schema, instance),
            Some(Inbound::Reply(reply)) => self.on_reply(&reply),
            Some(Inbound::State(state)) => self.on_state(&state),
            // The link drops next; the transport reports that on its own.
            Some(Inbound::Bye { .. }) => Vec::new(),
            None => {
                self.malformed_lines = self.malformed_lines.saturating_add(1);
                Vec::new()
            }
        }
    }

    /// The bridge's socket closed. Reads stay as they were, stale.
    pub fn on_disconnected(&mut self) -> Vec<VenueEvent> {
        self.has_state = false;
        if self.link != LinkState::Up {
            return Vec::new();
        }
        self.link = LinkState::Lost;
        vec![VenueEvent::Notice(VenueNotice::LinkLost)]
    }

    fn on_hello(&mut self, schema: u32, instance: String) -> Vec<VenueEvent> {
        self.has_state = false;
        if schema != EXEC_SCHEMA_VERSION {
            // Never written to: a request in a shape the bridge does not
            // read could be read as some other request.
            self.incompatible = true;
            return Vec::new();
        }
        self.incompatible = false;
        let mut events = Vec::new();
        let same_bridge = self.instance.as_deref() == Some(instance.as_str());
        if same_bridge {
            // The bridge still remembers every reply it gave, so a request
            // whose reply was lost is answered from that memory, never
            // executed twice. Same id, same line.
            self.outbox
                .extend(self.requests.values().map(|request| request.line.clone()));
        } else {
            self.abandon_unanswered(&mut events);
        }
        self.instance = Some(instance);
        if self.link == LinkState::Lost {
            events.push(VenueEvent::Notice(VenueNotice::LinkRestored));
        }
        self.link = LinkState::Up;
        self.queue(
            Pending::Watch,
            &Request::Watch {
                symbol: self.config.symbol.clone(),
            },
        );
        events
    }

    /// A new bridge process has no memory of what the old one answered, so
    /// resending could execute a request twice. Each one is dropped and
    /// reported as of unknown outcome; the next state shows what happened.
    fn abandon_unanswered(&mut self, events: &mut Vec<VenueEvent>) {
        for (_, request) in std::mem::take(&mut self.requests) {
            let id = match request.pending {
                Pending::Watch => continue,
                Pending::Submit(id) => {
                    self.remove_own(id);
                    Some(id)
                }
                Pending::Cancel { id, .. } => Some(id),
                Pending::Modify { id, .. } => {
                    self.changing.remove(&id);
                    Some(id)
                }
                Pending::PositionStops { .. } => {
                    self.stops_pending = false;
                    None
                }
                Pending::Close { .. } => None,
            };
            events.push(VenueEvent::Notice(VenueNotice::OutcomeUnknown { id }));
        }
    }

    /// Take an own order out of the working list or the in-flight entries.
    fn remove_own(&mut self, id: OrderId) -> Option<Order> {
        self.ids.retired.insert(id);
        if let Some(index) = self.working.iter().position(|order| order.id == id) {
            return Some(self.working.remove(index));
        }
        let index = self.entries.iter().position(|order| order.id == id)?;
        Some(self.entries.remove(index))
    }

    fn on_reply(&mut self, reply: &Reply) -> Vec<VenueEvent> {
        let prefix = format!("{}-", self.config.session_tag);
        let Some(number) = reply
            .id
            .as_deref()
            .and_then(|id| id.strip_prefix(prefix.as_str()))
            .and_then(|number| number.parse::<u64>().ok())
        else {
            return Vec::new();
        };
        // A reply nobody is waiting for — a repeat of one already handled —
        // changes nothing. That is the other half of the bridge's memory.
        let Some(request) = self.requests.remove(&number) else {
            return Vec::new();
        };
        let refusal = RejectReason::Venue {
            code: reply.retcode,
            message: VenueText::new(
                reply
                    .comment
                    .as_deref()
                    .or(reply.error.as_deref())
                    .unwrap_or_default(),
            ),
        };
        let ok = accepted(reply.retcode);
        let mut events = Vec::new();
        match request.pending {
            Pending::Watch => {}
            Pending::Submit(id) if ok => {
                if reply.order != 0 {
                    self.ids.bind(id, reply.order);
                    events.push(VenueEvent::Notice(VenueNotice::Acknowledged {
                        id,
                        venue_ref: reply.order,
                    }));
                    if let Some(reason) = self.cancel_on_ack.remove(&id) {
                        self.cancel_working(id, reason);
                    }
                }
            }
            Pending::Submit(id) => {
                self.remove_own(id);
                self.cancel_on_ack.remove(&id);
                events.push(VenueEvent::Rejected(refusal));
            }
            Pending::Cancel { id, reason } if ok => {
                if let Some(order) = self.remove_own(id) {
                    events.push(VenueEvent::Cancelled { order, reason });
                }
            }
            Pending::Modify { id, .. } if ok => {
                self.changing.remove(&id);
                if let Some(ticket) = self.ids.ticket(id) {
                    events.push(VenueEvent::Notice(VenueNotice::Acknowledged {
                        id,
                        venue_ref: ticket,
                    }));
                }
            }
            Pending::Modify { id, previous } => {
                self.changing.remove(&id);
                events.push(VenueEvent::Rejected(refusal));
                if let Some(order) = self.working.iter_mut().find(|order| order.id == id) {
                    *order = (*previous).clone();
                    events.push(VenueEvent::Updated(*previous));
                }
            }
            Pending::PositionStops { .. } if ok => self.stops_pending = false,
            Pending::PositionStops { previous } => {
                self.stops_pending = false;
                events.push(VenueEvent::Rejected(refusal));
                if let Some(position) = self.position.as_mut() {
                    (position.stop_loss, position.take_profit) = previous;
                    events.push(VenueEvent::BracketSet {
                        stop_loss: previous.0,
                        take_profit: previous.1,
                    });
                }
            }
            Pending::Close { volume } if ok => {
                if reply.order != 0 {
                    self.closing.insert(reply.order, volume);
                }
            }
            Pending::Cancel { .. } | Pending::Close { .. } => {
                events.push(VenueEvent::Rejected(refusal));
            }
        }
        events
    }

    fn on_state(&mut self, state: &State) -> Vec<VenueEvent> {
        if state.symbol != self.config.symbol {
            return Vec::new();
        }
        let mut events = Vec::new();
        self.apply_account(state, &mut events);
        if self.deals.baselined {
            for deal in &state.deals {
                if self.deals.seen.insert(deal.ticket) {
                    self.apply_deal(deal, &mut events);
                }
            }
        } else {
            self.deals
                .seen
                .extend(state.deals.iter().map(|deal| deal.ticket));
            self.deals.baselined = true;
        }
        self.foreign = ForeignExposure::default();
        self.apply_positions(&state.positions);
        self.apply_orders(&state.orders);
        self.apply_closed_orders(&state.closed_orders, &mut events);
        self.has_state = true;
        events
    }

    fn apply_account(&mut self, state: &State, events: &mut Vec<VenueEvent>) {
        let Some(line) = &state.account else {
            // Logged out: nothing can be traded, but who it was is kept so a
            // log-in to someone else is still seen as a change.
            self.account = None;
            return;
        };
        let facts = facts_from_line(line);
        let changed = self
            .known_account
            .as_ref()
            .is_some_and(|known| !known.same_account(&facts));
        if changed {
            self.forget_symbol_view();
            self.deals.baselined = false;
            events.push(VenueEvent::Notice(VenueNotice::AccountChanged));
        }
        let was_refused = self
            .account
            .as_ref()
            .is_some_and(|account| !account.margin_mode.nets());
        if !facts.margin_mode.nets() && (changed || !was_refused) {
            events.push(VenueEvent::Notice(VenueNotice::AccountRefused(NOT_NETTING)));
        }
        self.known_account = Some(facts.clone());
        self.account = Some(facts);
    }

    fn apply_positions(&mut self, lines: &[PositionLine]) {
        let mut own = None;
        for line in lines {
            let (Some(side), Some(quantity), Some(avg_price)) = (
                wire::side(&line.side),
                wire::decimal(&line.volume),
                wire::decimal(&line.price_open),
            ) else {
                continue;
            };
            if line.magic == self.config.magic {
                own = Some((line, side, quantity, avg_price));
            } else {
                self.foreign.positions.push(ForeignPosition {
                    ticket: line.ticket,
                    side,
                    quantity,
                    avg_price,
                    magic: line.magic,
                });
            }
        }
        let Some((line, side, quantity, avg_price)) = own else {
            self.position = None;
            self.position_ticket = None;
            self.stops_pending = false;
            return;
        };
        let previous = self.position.take().filter(|open| open.side == side);
        let mut position = Position {
            side,
            quantity,
            avg_price,
            opened_ms: previous
                .as_ref()
                .map_or(self.mark.ms, |open| open.opened_ms),
            opened_agg_id: previous
                .as_ref()
                .map_or(self.mark.agg_id, |open| open.opened_agg_id),
            low_price: previous.as_ref().map_or(avg_price, |open| open.low_price),
            high_price: previous.as_ref().map_or(avg_price, |open| open.high_price),
            stop_loss: line.sl.as_deref().and_then(wire::decimal),
            take_profit: line.tp.as_deref().and_then(wire::decimal),
        };
        position.observe(avg_price);
        if self.stops_pending
            && let Some(open) = previous
        {
            // A change is on its way: show it, not the snapshot taken
            // before the broker saw it.
            position.stop_loss = open.stop_loss;
            position.take_profit = open.take_profit;
        }
        self.position = Some(position);
        self.position_ticket = Some(line.ticket);
    }

    fn apply_orders(&mut self, lines: &[OrderLine]) {
        for line in lines {
            let (Some(quantity), Some(price)) =
                (wire::decimal(&line.volume), wire::decimal(&line.price))
            else {
                continue;
            };
            let side = wire::side(&line.side);
            if line.magic != self.config.magic {
                self.foreign.orders.push(ForeignOrder {
                    ticket: line.ticket,
                    side,
                    kind: line.kind.clone(),
                    quantity,
                    price,
                    magic: line.magic,
                });
                continue;
            }
            let kind = match line.kind.as_str() {
                "limit" => EntryKind::Limit,
                "stop" | "stop_limit" => EntryKind::Stop,
                // A market order or a close in flight: answered by its deal.
                _ => continue,
            };
            let Some(side) = side else { continue };
            let bracket = Bracket::whole(
                line.sl.as_deref().and_then(wire::decimal),
                line.tp.as_deref().and_then(wire::decimal),
            );
            let id = match self.ids.order(line.ticket) {
                Some(id) if self.changing.contains(&id) || self.ids.retired.contains(&id) => {
                    continue;
                }
                Some(id) => id,
                None => self.adopt(line.ticket),
            };
            let placed_ms = self.mark.ms;
            let Some(order) = self.working.iter_mut().find(|order| order.id == id) else {
                if self.entries.iter().any(|order| order.id == id) {
                    continue;
                }
                self.working.push(broker_order(
                    id, side, kind, price, quantity, bracket, placed_ms,
                ));
                continue;
            };
            order.side = side;
            order.kind = kind;
            order.price = Some(price);
            order.quantity = quantity;
            order.bracket = bracket;
        }
    }

    /// The oldest in-flight entry on `side` the broker answered without a
    /// ticket, now known by the deal's order ticket. A reply that carried no
    /// ticket must not leave its entry in flight forever.
    fn claim_unticketed_entry(&mut self, side: Side, ticket: u64) -> Option<OrderId> {
        let id = self
            .entries
            .iter()
            .find(|order| order.side == side && self.ids.ticket(order.id).is_none())?
            .id;
        self.ids.bind(id, ticket);
        Some(id)
    }

    /// An own order this session did not place — a previous session's, or
    /// one whose request's outcome was lost — gets an id and is shown.
    fn adopt(&mut self, ticket: u64) -> OrderId {
        let id = OrderId(self.next_order);
        self.next_order = self.next_order.saturating_add(1);
        self.ids.bind(id, ticket);
        id
    }

    fn apply_closed_orders(&mut self, lines: &[ClosedOrderLine], events: &mut Vec<VenueEvent>) {
        for line in lines {
            // A filled order is answered by its deal, which removes it.
            if matches!(line.state.as_str(), "filled" | "started" | "placed") {
                continue;
            }
            if self.closing.remove(&line.ticket).is_some() {
                // A close the broker accepted and then dropped unfilled: the
                // position is still open, and the trader has to know.
                events.push(VenueEvent::Rejected(RejectReason::Venue {
                    code: None,
                    message: VenueText::new(&format!(
                        "close order {} ended {} without filling",
                        line.ticket, line.state
                    )),
                }));
                continue;
            }
            let Some(id) = self.ids.order(line.ticket) else {
                continue;
            };
            if let Some(order) = self.remove_own(id) {
                self.cancel_on_ack.remove(&id);
                events.push(VenueEvent::Cancelled {
                    order,
                    reason: CancelReason::Venue,
                });
            }
        }
    }

    fn apply_deal(&mut self, deal: &DealLine, events: &mut Vec<VenueEvent>) {
        let (Some(side), Some(quantity), Some(price)) = (
            wire::side(&deal.side),
            wire::decimal(&deal.volume),
            wire::decimal(&deal.price),
        ) else {
            return;
        };
        let is_close = self.closing.remove(&deal.order).is_some();
        let mut order = self.ids.order(deal.order);
        if order.is_none() && !is_close && deal.magic == self.config.magic {
            order = self.claim_unticketed_entry(side, deal.order);
        }
        let ours = deal.magic == self.config.magic
            || order.is_some()
            || is_close
            || self.position_ticket == Some(deal.position_id);
        if !ours {
            return;
        }
        let (role, exit) = match deal.reason.as_str() {
            "sl" => (FillRole::StopLoss, ExitReason::StopLoss),
            "tp" => (FillRole::TakeProfit, ExitReason::TakeProfit),
            _ if is_close => (FillRole::Close, ExitReason::Manual),
            _ => match order {
                Some(id) => (FillRole::Entry(id), ExitReason::Reversal),
                None if deal.entry == "in" => (
                    FillRole::Entry(self.adopt(deal.order)),
                    ExitReason::Reversal,
                ),
                None => (FillRole::Close, ExitReason::Manual),
            },
        };
        events.push(VenueEvent::Filled(Fill {
            timestamp_ms: self.mark.ms,
            agg_id: self.mark.agg_id,
            side,
            price,
            quantity,
            role,
        }));
        if let FillRole::Entry(id) = role {
            self.consume_entry(id, quantity);
        }
        self.net(side, quantity, price, exit, events);
    }

    /// Take a fill off the entry it answers; a fully filled entry stops
    /// working and stops being in flight.
    fn consume_entry(&mut self, id: OrderId, quantity: Decimal) {
        let remaining = self
            .working
            .iter_mut()
            .chain(self.entries.iter_mut())
            .find(|order| order.id == id)
            .map(|order| {
                order.quantity = order.quantity.saturating_sub(quantity);
                order.quantity
            });
        if remaining.is_some_and(|left| left <= Decimal::ZERO) {
            self.remove_own(id);
        }
    }

    /// Fold one execution into the net position, closing before opening.
    /// The snapshot that follows replaces the result; this keeps the round
    /// trips of several deals in one state honest in between.
    fn net(
        &mut self,
        side: Side,
        quantity: Decimal,
        price: Decimal,
        exit: ExitReason,
        events: &mut Vec<VenueEvent>,
    ) {
        match self.position.take() {
            Some(open) if open.side != side => {
                let closed = open.quantity.min(quantity);
                self.record_round_trip(&open, closed, price, exit, events);
                let kept = open.quantity.saturating_sub(closed);
                let remainder = quantity.saturating_sub(closed);
                if kept > Decimal::ZERO {
                    self.position = Some(Position {
                        quantity: kept,
                        ..open
                    });
                } else if remainder > Decimal::ZERO {
                    self.position = Some(self.opened(side, remainder, price));
                } else {
                    self.position_ticket = None;
                }
            }
            Some(mut open) => {
                let total = open.quantity.saturating_add(quantity);
                if total > Decimal::ZERO {
                    open.avg_price = open
                        .avg_price
                        .saturating_mul(open.quantity)
                        .saturating_add(price.saturating_mul(quantity))
                        / total;
                }
                open.quantity = total;
                open.observe(price);
                self.position = Some(open);
            }
            None => self.position = Some(self.opened(side, quantity, price)),
        }
    }

    fn opened(&self, side: Side, quantity: Decimal, price: Decimal) -> Position {
        Position {
            side,
            quantity,
            avg_price: price,
            opened_ms: self.mark.ms,
            opened_agg_id: self.mark.agg_id,
            low_price: price,
            high_price: price,
            stop_loss: None,
            take_profit: None,
        }
    }

    /// One round trip against the average entry. The audit-trail print ids
    /// stay `None`: a broker deal is not proven by a print on this tape.
    fn record_round_trip(
        &mut self,
        open: &Position,
        quantity: Decimal,
        price: Decimal,
        reason: ExitReason,
        events: &mut Vec<VenueEvent>,
    ) {
        let pnl = signed_points(open.side, open.avg_price, price, quantity);
        let (adverse, favorable) = open.excursions(price);
        let trade = ClosedTrade {
            side: open.side,
            quantity,
            entry_price: open.avg_price,
            exit_price: price,
            opened_ms: open.opened_ms,
            closed_ms: self.mark.ms,
            pnl_points: pnl,
            exit_reason: reason,
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: Some(adverse.saturating_mul(quantity)),
            mfe_points: Some(favorable.saturating_mul(quantity)),
        };
        self.ledger.realized = self.ledger.realized.saturating_add(pnl);
        self.ledger.closed.push(trade.clone());
        events.push(VenueEvent::Closed(trade));
    }
}

fn broker_order(
    id: OrderId,
    side: Side,
    kind: EntryKind,
    price: Decimal,
    quantity: Decimal,
    bracket: Bracket,
    placed_ms: i64,
) -> Order {
    Order {
        id,
        side,
        kind,
        price: Some(price),
        quantity,
        bracket,
        cancel_at: None,
        flat_only: false,
        placed_ms,
        role: OrderRole::Entry,
        oco: None,
        reduce_only: false,
    }
}
