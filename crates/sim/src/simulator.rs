//! The simulator core: prints and commands in, fills and closures out.

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

use quantick_trading::{
    Bracket, CancelReason, ClosedTrade, EntryKind, ExitPart, ExitReason, Fill, FillRole, OcoId,
    Order, OrderId, OrderRole, Position, RejectReason, VenueEvent, signed_points,
};

/// A user command, applied between prints. Commands and prints interleave
/// deterministically: same tape + same commands at the same points → same
/// fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Buy or sell at the next print.
    PlaceMarket {
        side: Side,
        quantity: Decimal,
        bracket: Bracket,
    },
    /// Rest at `price` until the tape trades at or through it. A limit that
    /// would fill immediately is rejected with advice instead.
    PlaceLimit {
        side: Side,
        quantity: Decimal,
        price: Decimal,
        bracket: Bracket,
        /// Optional price-cancel level: a print trading at or through it
        /// before the fill removes the order
        /// ([`CancelReason::PriceTouched`]). Must sit on the far side of
        /// the market from `price` — above it for a buy, below it for a
        /// sell — so fill and cancel can never share a print.
        cancel_at: Option<Decimal>,
        /// Fill only into an account with no open position; if the fill
        /// print arrives while one is open, the order cancels instead
        /// ([`CancelReason::AccountOccupied`]). For entries whose owner
        /// (the strategy kernel) promised never to trade against a
        /// position it did not open.
        flat_only: bool,
    },
    /// Arm at `trigger` until the tape trades at or through it, then fill
    /// at that print's price.
    PlaceStop {
        side: Side,
        quantity: Decimal,
        trigger: Decimal,
        bracket: Bracket,
    },
    /// Move a resting order to a new price (same validation as placing it).
    ModifyOrder { id: OrderId, price: Decimal },
    /// Replace a *working* order's protective prices wholesale — `None`
    /// clears that side.
    ///
    /// The levels are validated against the order's **own resting price**,
    /// not the mark: the bracket protects a fill that has not happened yet,
    /// and the price it will happen at is the order's. That is the same
    /// reference [`Command::PlaceLimit`] and [`Command::PlaceStop`] already
    /// validate their brackets against, so an order amended here obeys
    /// exactly the rules it was placed under.
    ///
    /// Unlike [`Command::SetBracket`] this touches no position. The legs
    /// ride the order and arm the moment it fills — and are re-checked
    /// against the *actual* fill price then, so a level the tape outran
    /// while the order rested is dropped and reported
    /// ([`crate::VenueEvent::BracketDropped`]) rather than kept wearing a
    /// lying label.
    SetOrderBracket { id: OrderId, bracket: Bracket },
    /// Remove a pending order without filling it.
    CancelOrder { id: OrderId },
    /// Replace the open position's protective prices wholesale — `None`
    /// clears that side.
    SetBracket {
        stop_loss: Option<Decimal>,
        take_profit: Option<Decimal>,
    },
    /// Close the open position at the next print. Emits no event of its
    /// own; the fill (or its quiet dissolution, if a bracket got there
    /// first) is the answer.
    ClosePosition,
    /// Close up to `quantity` of the open position at the next print. Closes
    /// at most what is open — a partial close never reverses — and the
    /// remainder keeps its average entry price, protective prices and
    /// opening time. Like [`Command::ClosePosition`] it emits no event of
    /// its own and dissolves quietly if the position is gone by the time
    /// the print arrives.
    ClosePartial { quantity: Decimal },
    /// Cancel every pending order and close the position at the next print.
    Flatten,
}

/// A market action waiting for the next print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueuedAction {
    /// A market entry order. Boxed: an entry carries its exit ladder and
    /// dwarfs the other variants, and the queue is walked once per print.
    Entry(Box<Order>),
    /// A user close. If the position is already gone when the print
    /// arrives (a bracket fired first), the close dissolves — there is
    /// nothing left for it to close.
    Close,
    /// A user partial close, clamped to the open quantity at fill time.
    /// Dissolves like [`QueuedAction::Close`] when the position is gone.
    ClosePartial { quantity: Decimal },
}

/// The last print seen — the only "now" the simulator knows.
#[derive(Debug, Clone, Copy)]
struct Mark {
    timestamp_ms: i64,
    agg_id: u64,
    price: Decimal,
}

/// Deterministic paper-trading state machine for one instrument. See the
/// crate doc for the fill model and the per-print processing order.
#[derive(Debug, Default)]
pub struct Simulator {
    next_id: u64,
    next_oco: u64,
    mark: Option<Mark>,
    /// Market actions awaiting the next print, in command order.
    queue: Vec<QueuedAction>,
    /// Resting limit/stop entries, in placement order.
    resting: Vec<Order>,
    position: Option<Position>,
    closed: Vec<ClosedTrade>,
    realized_points: Decimal,
}

impl Simulator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the mark from history without ever filling against it —
    /// backfill happened before the session; trading on it would be
    /// look-ahead.
    pub fn seed(&mut self, trade: &Trade) {
        self.mark = Some(Mark {
            timestamp_ms: trade.timestamp_ms,
            agg_id: trade.agg_id,
            price: trade.price,
        });
    }

    /// Last price seen (live or seeded), if any.
    #[must_use]
    pub fn mark_price(&self) -> Option<Decimal> {
        self.mark.map(|mark| mark.price)
    }

    /// Venue time of the last print seen (live or seeded), if any — the
    /// only "now" an open trade's age can honestly be measured against.
    #[must_use]
    pub fn mark_timestamp_ms(&self) -> Option<i64> {
        self.mark.map(|mark| mark.timestamp_ms)
    }

    #[must_use]
    pub fn position(&self) -> Option<&Position> {
        self.position.as_ref()
    }

    /// Resting limit/stop entries, in placement order.
    #[must_use]
    pub fn orders(&self) -> &[Order] {
        &self.resting
    }

    /// Market actions awaiting the next print, in command order.
    #[must_use]
    pub fn queued(&self) -> &[QueuedAction] {
        &self.queue
    }

    /// Every round trip completed this session, oldest first.
    #[must_use]
    pub fn closed_trades(&self) -> &[ClosedTrade] {
        &self.closed
    }

    /// Sum of closed-trade profits, in points.
    #[must_use]
    pub fn realized_points(&self) -> Decimal {
        self.realized_points
    }

    /// Apply a user command. Refusals come back as a single
    /// [`VenueEvent::Rejected`] whose reason says what to do instead.
    pub fn apply(&mut self, command: Command) -> Vec<VenueEvent> {
        match self.dispatch(command) {
            Ok(events) => events,
            Err(reason) => vec![VenueEvent::Rejected(reason)],
        }
    }

    /// Process one print. The processing order is fixed and documented in
    /// the crate doc: brackets, then queued market actions, then resting
    /// orders; the mark updates last, so entries filled by this print arm
    /// their brackets from the next one.
    pub fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        let mut events = Vec::new();
        let price = trade.price;

        // 1) Position brackets, armed on prints before this one. The stop
        //    is checked first; validation keeps stop < take profit apart,
        //    so one print can never satisfy both.
        if let Some(position) = &self.position {
            let stop_hit = position.stop_loss.is_some_and(|level| match position.side {
                Side::Buy => price <= level,
                Side::Sell => price >= level,
            });
            let target = position.take_profit.filter(|level| match position.side {
                Side::Buy => price >= *level,
                Side::Sell => price <= *level,
            });
            if stop_hit {
                // A stop is a market order once touched: it fills at the
                // print, so a gap fills honestly worse than the trigger.
                self.exit_position(
                    price,
                    trade,
                    ExitReason::StopLoss,
                    FillRole::StopLoss,
                    &mut events,
                );
            } else if let Some(level) = target {
                // A take profit is a resting limit: it fills at its own
                // price.
                self.exit_position(
                    level,
                    trade,
                    ExitReason::TakeProfit,
                    FillRole::TakeProfit,
                    &mut events,
                );
            }
        }

        // 1b) The ladder's protective legs, armed on prints before this
        //     one and walked in the order the trader listed their parts. A
        //     print can reach several - a gap through two stops fires both -
        //     and each reduces the position by its own part's quantity.
        self.fire_protective_legs(price, trade, &mut events);

        // 2) Queued market actions, in command order.
        for action in std::mem::take(&mut self.queue) {
            match action {
                QueuedAction::Entry(order) => {
                    self.fill_entry_at(&order, price, trade, &mut events);
                }
                QueuedAction::Close => {
                    if self.position.is_some() {
                        self.exit_position(
                            price,
                            trade,
                            ExitReason::Manual,
                            FillRole::Close,
                            &mut events,
                        );
                    }
                }
                QueuedAction::ClosePartial { quantity } => {
                    self.close_partial_at(quantity, price, trade, &mut events);
                }
            }
        }

        // 3) Resting orders, in placement order. The cancel-at check comes
        //    first, but never races the fill: placement validation keeps the
        //    two levels on opposite sides of the market, so one print price
        //    cannot satisfy both.
        let resting = std::mem::take(&mut self.resting);
        let mut kept = Vec::with_capacity(resting.len());
        for order in resting {
            // Protective legs had their turn in phase 1b; they never fill as
            // entries, whatever their price says.
            if order.is_protective() {
                kept.push(order);
                continue;
            }
            // Resting orders always carry a price by construction.
            let Some(level) = order.price else {
                kept.push(order);
                continue;
            };
            let price_cancelled = order.kind == EntryKind::Limit
                && order.cancel_at.is_some_and(|cancel| match order.side {
                    Side::Buy => price >= cancel,
                    Side::Sell => price <= cancel,
                });
            if price_cancelled {
                events.push(VenueEvent::Cancelled {
                    order,
                    reason: CancelReason::PriceTouched,
                });
                continue;
            }
            let fill_at = match (order.kind, order.side) {
                (EntryKind::Limit, Side::Buy) if price <= level => Some(level),
                (EntryKind::Limit, Side::Sell) if price >= level => Some(level),
                (EntryKind::Stop, Side::Buy) if price >= level => Some(price),
                (EntryKind::Stop, Side::Sell) if price <= level => Some(price),
                _ => None,
            };
            match fill_at {
                // A flat-only order whose moment arrives over an open
                // position stands down at that moment — never before it
                // (the position may close first), never after (filling
                // would trade against a position its owner never saw).
                Some(_) if order.flat_only && self.position.is_some() => {
                    events.push(VenueEvent::Cancelled {
                        order,
                        reason: CancelReason::AccountOccupied,
                    });
                }
                Some(at) => self.fill_entry_at(&order, at, trade, &mut events),
                None => kept.push(order),
            }
        }
        // An entry filled by this print installed its legs into the list
        // while it was taken, so they are collected here rather than
        // overwritten. They are the newest orders, so they sit last.
        kept.extend(std::mem::take(&mut self.resting));
        self.resting = kept;

        // No protective leg outlives the position it protects: an orphan
        // would exit a position nobody holds on some later print.
        if self.position.is_none() {
            self.sweep_protective(CancelReason::PositionClosed, &mut events);
        }

        // 4) The mark updates last; a position that survived the print has
        //    been exposed to its price, which the excursions must remember.
        if let Some(position) = self.position.as_mut() {
            position.observe(price);
        }
        self.mark = Some(Mark {
            timestamp_ms: trade.timestamp_ms,
            agg_id: trade.agg_id,
            price,
        });
        events
    }

    /// Discard pending state and flatten at the last mark. For when the
    /// source rebuilds its timeline (a replay seek): closed bars cannot be
    /// un-closed, and a position cannot honestly survive into a rewritten
    /// past. Completed trades and realized points are kept — they happened.
    pub fn reset(&mut self) -> Vec<VenueEvent> {
        let mut events = Vec::new();
        for action in std::mem::take(&mut self.queue) {
            if let QueuedAction::Entry(order) = action {
                events.push(VenueEvent::Cancelled {
                    order: *order,
                    reason: CancelReason::Reset,
                });
            }
        }
        for order in std::mem::take(&mut self.resting) {
            events.push(VenueEvent::Cancelled {
                order,
                reason: CancelReason::Reset,
            });
        }
        // A position can only exist after a print, so the mark is present
        // whenever the position is.
        if let (Some(position), Some(mark)) = (self.position.take(), self.mark) {
            events.push(VenueEvent::Filled(Fill {
                timestamp_ms: mark.timestamp_ms,
                agg_id: mark.agg_id,
                side: opposite(position.side),
                price: mark.price,
                quantity: position.quantity,
                role: FillRole::Reset,
            }));
            self.record_close(
                &position,
                position.quantity,
                mark.price,
                mark.timestamp_ms,
                mark.agg_id,
                ExitReason::Reset,
                &mut events,
            );
        }
        self.mark = None;
        events
    }

    fn dispatch(&mut self, command: Command) -> Result<Vec<VenueEvent>, RejectReason> {
        match command {
            Command::PlaceMarket {
                side,
                quantity,
                bracket,
            } => {
                let mark = self.mark.ok_or(RejectReason::NoMarketPrice)?;
                require_positive_quantity(quantity)?;
                validate_bracket(side, mark.price, bracket)?;
                let order = self.make_order(
                    side,
                    EntryKind::Market,
                    None,
                    quantity,
                    bracket,
                    None,
                    false,
                    mark.timestamp_ms,
                );
                let placed = VenueEvent::Placed(order.clone());
                self.queue.push(QueuedAction::Entry(Box::new(order)));
                Ok(vec![placed])
            }
            Command::PlaceLimit {
                side,
                quantity,
                price,
                bracket,
                cancel_at,
                flat_only,
            } => {
                let mark = self.mark.ok_or(RejectReason::NoMarketPrice)?;
                require_positive_quantity(quantity)?;
                require_positive_price(price)?;
                validate_limit_side(side, price, mark.price)?;
                validate_bracket(side, price, bracket)?;
                if let Some(cancel) = cancel_at {
                    require_positive_price(cancel)?;
                    validate_cancel_at_side(side, cancel, mark.price)?;
                }
                let order = self.make_order(
                    side,
                    EntryKind::Limit,
                    Some(price),
                    quantity,
                    bracket,
                    cancel_at,
                    flat_only,
                    mark.timestamp_ms,
                );
                let placed = VenueEvent::Placed(order.clone());
                self.resting.push(order);
                Ok(vec![placed])
            }
            Command::PlaceStop {
                side,
                quantity,
                trigger,
                bracket,
            } => {
                let mark = self.mark.ok_or(RejectReason::NoMarketPrice)?;
                require_positive_quantity(quantity)?;
                require_positive_price(trigger)?;
                validate_stop_side(side, trigger, mark.price)?;
                validate_bracket(side, trigger, bracket)?;
                let order = self.make_order(
                    side,
                    EntryKind::Stop,
                    Some(trigger),
                    quantity,
                    bracket,
                    None,
                    false,
                    mark.timestamp_ms,
                );
                let placed = VenueEvent::Placed(order.clone());
                self.resting.push(order);
                Ok(vec![placed])
            }
            Command::ModifyOrder { id, price } => {
                let mark = self.mark.ok_or(RejectReason::NoMarketPrice)?;
                require_positive_price(price)?;
                let Some(order) = self.resting.iter_mut().find(|order| order.id == id) else {
                    return Err(RejectReason::UnknownOrder(id));
                };
                match order.kind {
                    EntryKind::Limit => validate_limit_side(order.side, price, mark.price)?,
                    EntryKind::Stop => validate_stop_side(order.side, price, mark.price)?,
                    // Market orders never rest, so they cannot be here.
                    EntryKind::Market => return Err(RejectReason::UnknownOrder(id)),
                }
                validate_bracket(order.side, price, order.bracket)?;
                order.price = Some(price);
                Ok(vec![VenueEvent::Updated(order.clone())])
            }
            Command::SetOrderBracket { id, bracket } => {
                let Some(order) = self.resting.iter_mut().find(|order| order.id == id) else {
                    return Err(RejectReason::UnknownOrder(id));
                };
                // A resting order always carries a price — only a market
                // order is priceless, and a market order never rests. The
                // `else` is the unreachable arm written out rather than
                // unwrapped, so an impossible state refuses instead of
                // panicking a live session.
                let Some(reference) = order.price else {
                    return Err(RejectReason::UnknownOrder(id));
                };
                validate_bracket(order.side, reference, bracket)?;
                order.bracket = bracket;
                Ok(vec![VenueEvent::Updated(order.clone())])
            }
            Command::CancelOrder { id } => {
                if let Some(index) = self.resting.iter().position(|order| order.id == id) {
                    let order = self.resting.remove(index);
                    return Ok(vec![VenueEvent::Cancelled {
                        order,
                        reason: CancelReason::User,
                    }]);
                }
                let queued = self.queue.iter().position(
                    |action| matches!(action, QueuedAction::Entry(order) if order.id == id),
                );
                if let Some(index) = queued
                    && let QueuedAction::Entry(order) = self.queue.remove(index)
                {
                    return Ok(vec![VenueEvent::Cancelled {
                        order: *order,
                        reason: CancelReason::User,
                    }]);
                }
                Err(RejectReason::UnknownOrder(id))
            }
            Command::SetBracket {
                stop_loss,
                take_profit,
            } => {
                let mark = self.mark.ok_or(RejectReason::NoMarketPrice)?;
                let Some(position) = self.position.as_mut() else {
                    return Err(RejectReason::NoPosition);
                };
                validate_bracket(
                    position.side,
                    mark.price,
                    Bracket::whole(stop_loss, take_profit),
                )?;
                position.stop_loss = stop_loss;
                position.take_profit = take_profit;
                // Replacing the protection replaces all of it. A ladder left
                // armed beside a new whole-position pair would protect the
                // same quantity twice, and whichever fired first would
                // surprise a trader who believed they had just said what
                // protects this position.
                let mut events = Vec::new();
                self.sweep_protective(CancelReason::BracketReplaced, &mut events);
                events.push(VenueEvent::BracketSet {
                    stop_loss,
                    take_profit,
                });
                Ok(events)
            }
            Command::ClosePosition => {
                if self.position.is_none() {
                    return Err(RejectReason::NoPosition);
                }
                self.queue.push(QueuedAction::Close);
                Ok(Vec::new())
            }
            Command::ClosePartial { quantity } => {
                require_positive_quantity(quantity)?;
                if self.position.is_none() {
                    return Err(RejectReason::NoPosition);
                }
                self.queue.push(QueuedAction::ClosePartial { quantity });
                Ok(Vec::new())
            }
            Command::Flatten => {
                let mut events = Vec::new();
                for action in std::mem::take(&mut self.queue) {
                    if let QueuedAction::Entry(order) = action {
                        events.push(VenueEvent::Cancelled {
                            order: *order,
                            reason: CancelReason::Flatten,
                        });
                    }
                }
                for order in std::mem::take(&mut self.resting) {
                    events.push(VenueEvent::Cancelled {
                        order,
                        reason: CancelReason::Flatten,
                    });
                }
                if self.position.is_some() {
                    self.queue.push(QueuedAction::Close);
                }
                Ok(events)
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one order has one natural field list; bundling it into a struct would name nothing"
    )]
    fn make_order(
        &mut self,
        side: Side,
        kind: EntryKind,
        price: Option<Decimal>,
        quantity: Decimal,
        bracket: Bracket,
        cancel_at: Option<Decimal>,
        flat_only: bool,
        placed_ms: i64,
    ) -> Order {
        self.next_id += 1;
        Order {
            id: OrderId(self.next_id),
            side,
            kind,
            price,
            quantity,
            bracket,
            cancel_at,
            flat_only,
            placed_ms,
            role: OrderRole::Entry,
            oco: None,
            reduce_only: false,
        }
    }

    /// One protective leg of a ladder part, resting on the reducing side.
    #[expect(
        clippy::too_many_arguments,
        reason = "one leg has one natural field list; bundling it into a struct would name nothing"
    )]
    fn make_leg(
        &mut self,
        side: Side,
        kind: EntryKind,
        price: Decimal,
        quantity: Decimal,
        role: OrderRole,
        oco: OcoId,
        placed_ms: i64,
    ) -> Order {
        self.next_id += 1;
        Order {
            id: OrderId(self.next_id),
            side,
            kind,
            price: Some(price),
            quantity,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
            placed_ms,
            role,
            oco: Some(oco),
            reduce_only: true,
        }
    }

    /// Turn a filled entry's ladder into working protective legs.
    ///
    /// Only a ladder gets legs. A plain whole-fill bracket arms the
    /// position's own pair, which is what the port reports and what every
    /// venue already models - splitting it into orders would change an
    /// answer nothing asked to change.
    ///
    /// Each part becomes an OCO pair on the reducing side: a take profit
    /// resting at its level and a stop armed at its own, sharing one
    /// [`OcoId`] so whichever fills first removes the other.
    fn install_protection(
        &mut self,
        side: Side,
        filled_quantity: Decimal,
        placed_ms: i64,
        bracket: Bracket,
        events: &mut Vec<VenueEvent>,
    ) {
        if !bracket.is_laddered() {
            return;
        }
        let reducing = opposite(side);
        for part in fitted_parts(bracket, filled_quantity) {
            let Some(quantity) = part.quantity else {
                continue;
            };
            self.next_oco += 1;
            let oco = OcoId(self.next_oco);
            if let Some(level) = part.take_profit {
                let leg = self.make_leg(
                    reducing,
                    EntryKind::Limit,
                    level,
                    quantity,
                    OrderRole::TakeProfit,
                    oco,
                    placed_ms,
                );
                events.push(VenueEvent::Placed(leg.clone()));
                self.resting.push(leg);
            }
            if let Some(level) = part.stop_loss {
                let leg = self.make_leg(
                    reducing,
                    EntryKind::Stop,
                    level,
                    quantity,
                    OrderRole::StopLoss,
                    oco,
                    placed_ms,
                );
                events.push(VenueEvent::Placed(leg.clone()));
                self.resting.push(leg);
            }
        }
    }

    /// Fire every protective leg this print reaches, in placement order.
    ///
    /// A leg reduces the position by its own quantity - clamped to what is
    /// actually still open, because a hand close may have got there first -
    /// and cancels the sibling it shares an OCO group with. Parts the print
    /// did not reach carry on untouched: that is the whole point of a
    /// ladder.
    ///
    /// The scan restarts after each fill. An OCO cancellation removes a
    /// sibling from anywhere in the list, and a cursor walking past that is
    /// how a leg gets silently skipped; a ladder is at most four pairs long,
    /// so the rescan costs nothing a per-trade path would notice.
    fn fire_protective_legs(
        &mut self,
        price: Decimal,
        print: &Trade,
        events: &mut Vec<VenueEvent>,
    ) {
        while self.position.is_some() {
            let Some(index) = self.resting.iter().position(|order| {
                order.is_protective()
                    && order
                        .price
                        .is_some_and(|level| leg_reached(order.role, order.side, level, price))
            }) else {
                return;
            };
            let leg = self.resting.remove(index);
            // A stop is a market order once touched and fills at the print,
            // so a gap fills honestly worse than the trigger; a take profit
            // is a resting limit and fills at its own level.
            let level = leg.price.unwrap_or(price);
            let (at, reason, role) = match leg.role {
                OrderRole::StopLoss => (price, ExitReason::StopLoss, FillRole::StopLoss),
                _ => (level, ExitReason::TakeProfit, FillRole::TakeProfit),
            };
            self.close_quantity_at(leg.quantity, at, print, reason, role, events);
            if let Some(oco) = leg.oco {
                self.cancel_oco_sibling(oco, events);
            }
        }
    }

    /// Remove the other leg of `oco`: its part is closed, and a lone
    /// survivor would exit a quantity that is no longer open.
    fn cancel_oco_sibling(&mut self, oco: OcoId, events: &mut Vec<VenueEvent>) {
        if let Some(index) = self
            .resting
            .iter()
            .position(|order| order.is_protective() && order.oco == Some(oco))
        {
            let order = self.resting.remove(index);
            events.push(VenueEvent::Cancelled {
                order,
                reason: CancelReason::OcoFilled,
            });
        }
    }

    /// Cancel every protective leg, saying why. User entries are untouched:
    /// an order waiting for its own moment is not protection.
    fn sweep_protective(&mut self, reason: CancelReason, events: &mut Vec<VenueEvent>) {
        let mut index = 0;
        while index < self.resting.len() {
            if self.resting[index].is_protective() {
                let order = self.resting.remove(index);
                events.push(VenueEvent::Cancelled { order, reason });
            } else {
                index += 1;
            }
        }
    }

    /// Keep only the attached protective prices the fill has not strictly
    /// outrun. A market (or stop) order validates its bracket against the
    /// mark it was placed at, but fills at a later print — a level the
    /// tape ran *past* in between would exit on the very next print with a
    /// lying label (`take_profit` on a loss, `stop_loss` on a profit), so
    /// it is dropped and reported; the user re-places it from the fill
    /// they actually got. A level exactly at the fill is kept: it exits at
    /// zero points, an honest break-even, not a lie.
    fn admissible_bracket(
        side: Side,
        at: Decimal,
        bracket: Bracket,
        events: &mut Vec<VenueEvent>,
    ) -> Bracket {
        let parts: Vec<ExitPart> = bracket
            .parts()
            .map(|part| Self::admissible_part(side, at, *part, events))
            .filter(|part| !part.is_empty())
            .collect();
        Bracket::ladder(&parts).unwrap_or_else(|_| Bracket::none())
    }

    /// One part of a ladder, with the levels the fill has already outrun
    /// dropped and reported. See [`Self::admissible_bracket`].
    fn admissible_part(
        side: Side,
        at: Decimal,
        part: ExitPart,
        events: &mut Vec<VenueEvent>,
    ) -> ExitPart {
        let mut kept = part;
        if let Some(level) = kept.stop_loss {
            let lying = match side {
                Side::Buy => level > at,
                Side::Sell => level < at,
            };
            if lying {
                events.push(VenueEvent::BracketDropped {
                    reason: RejectReason::StopLossOnWrongSide(side),
                });
                kept.stop_loss = None;
            }
        }
        if let Some(level) = kept.take_profit {
            let lying = match side {
                Side::Buy => level < at,
                Side::Sell => level > at,
            };
            if lying {
                events.push(VenueEvent::BracketDropped {
                    reason: RejectReason::TakeProfitOnWrongSide(side),
                });
                kept.take_profit = None;
            }
        }
        kept
    }

    /// Execute an entry order at `at` against the print that caused it,
    /// netting against any opposite position.
    fn fill_entry_at(
        &mut self,
        order: &Order,
        at: Decimal,
        print: &Trade,
        events: &mut Vec<VenueEvent>,
    ) {
        events.push(VenueEvent::Filled(Fill {
            timestamp_ms: print.timestamp_ms,
            agg_id: print.agg_id,
            side: order.side,
            price: at,
            quantity: order.quantity,
            role: FillRole::Entry(order.id),
        }));
        // Re-check the attached levels against the real fill: a limit's
        // reference *is* its fill price so nothing changes for it, but a
        // market/stop order may have been outrun by the tape.
        let bracket = Self::admissible_bracket(order.side, at, order.bracket, events);
        match self.position.take() {
            None => {
                self.position = Some(opened_position(
                    order.side,
                    order.quantity,
                    at,
                    print,
                    bracket,
                ));
                self.install_protection(
                    order.side,
                    order.quantity,
                    print.timestamp_ms,
                    bracket,
                    events,
                );
            }
            Some(mut position) if position.side == order.side => {
                // Average in. The newest entry's bracket wins where it sets
                // a side; sides it leaves unset keep their level.
                let total = position.quantity.saturating_add(order.quantity);
                let weighted = position
                    .avg_price
                    .saturating_mul(position.quantity)
                    .saturating_add(at.saturating_mul(order.quantity));
                if total > Decimal::ZERO {
                    position.avg_price = weighted / total;
                }
                position.quantity = total;
                position.observe(at);
                if bracket.stop_loss().is_some() {
                    position.stop_loss = bracket.stop_loss();
                }
                if bracket.take_profit().is_some() {
                    position.take_profit = bracket.take_profit();
                }
                let total_open = position.quantity;
                self.position = Some(position);
                // A bracketed add re-protects the whole position: a ladder's
                // parts are slices of one entry, and old legs left beside
                // new ones would protect some quantity twice and some not at
                // all. An add carrying no ladder leaves the legs alone.
                if bracket.is_laddered() {
                    self.sweep_protective(CancelReason::BracketReplaced, events);
                    self.install_protection(
                        order.side,
                        total_open,
                        print.timestamp_ms,
                        bracket,
                        events,
                    );
                }
            }
            Some(mut position) => {
                // Netting: the opposite entry closes quantity first, then
                // opens the remainder as a new position. No exit fill of its
                // own — the entry execution above covers both legs.
                let close_quantity = position.quantity.min(order.quantity);
                self.record_close(
                    &position,
                    close_quantity,
                    at,
                    print.timestamp_ms,
                    print.agg_id,
                    ExitReason::Reversal,
                    events,
                );
                if position.quantity > close_quantity {
                    // Part of the old position survives, and so does its
                    // protection: the legs still reduce the right side, and
                    // each clamps to what is open when it fires.
                    position.quantity = position.quantity.saturating_sub(close_quantity);
                    position.observe(at);
                    self.position = Some(position);
                } else {
                    // The old position is gone and its protection goes with
                    // it: legs that reduced a long are the wrong side of the
                    // short now opening, and left working they would exit it
                    // at a price nothing armed for.
                    self.sweep_protective(CancelReason::PositionClosed, events);
                    let remainder = order.quantity.saturating_sub(close_quantity);
                    if remainder > Decimal::ZERO {
                        self.position =
                            Some(opened_position(order.side, remainder, at, print, bracket));
                        self.install_protection(
                            order.side,
                            remainder,
                            print.timestamp_ms,
                            bracket,
                            events,
                        );
                    }
                }
            }
        }
    }

    /// Exit the whole position at `at` against the print that caused it.
    fn exit_position(
        &mut self,
        at: Decimal,
        print: &Trade,
        reason: ExitReason,
        role: FillRole,
        events: &mut Vec<VenueEvent>,
    ) {
        let Some(position) = self.position.take() else {
            return;
        };
        events.push(VenueEvent::Filled(Fill {
            timestamp_ms: print.timestamp_ms,
            agg_id: print.agg_id,
            side: opposite(position.side),
            price: at,
            quantity: position.quantity,
            role,
        }));
        self.record_close(
            &position,
            position.quantity,
            at,
            print.timestamp_ms,
            print.agg_id,
            reason,
            events,
        );
    }

    /// Close up to `quantity` of the position at `at` against the print that
    /// caused it, keeping the remainder (average entry, protective prices
    /// and opening time untouched). Dissolves when there is no position.
    fn close_partial_at(
        &mut self,
        quantity: Decimal,
        at: Decimal,
        print: &Trade,
        events: &mut Vec<VenueEvent>,
    ) {
        self.close_quantity_at(
            quantity,
            at,
            print,
            ExitReason::Manual,
            FillRole::Close,
            events,
        );
    }

    /// The one path out of a position: reduce it by up to `quantity` at
    /// `at`, book the closed trade, and keep whatever remains with its
    /// average entry and opening time untouched.
    ///
    /// A ladder's leg, a hand close and a flatten all run through here, so a
    /// rung and a button can never disagree about what closing means.
    /// Dissolves quietly when there is no position: a queued close whose
    /// position a stop already took has nothing left to do.
    fn close_quantity_at(
        &mut self,
        quantity: Decimal,
        at: Decimal,
        print: &Trade,
        reason: ExitReason,
        role: FillRole,
        events: &mut Vec<VenueEvent>,
    ) {
        let Some(mut position) = self.position.take() else {
            return;
        };
        let close_quantity = position.quantity.min(quantity);
        events.push(VenueEvent::Filled(Fill {
            timestamp_ms: print.timestamp_ms,
            agg_id: print.agg_id,
            side: opposite(position.side),
            price: at,
            quantity: close_quantity,
            role,
        }));
        self.record_close(
            &position,
            close_quantity,
            at,
            print.timestamp_ms,
            print.agg_id,
            reason,
            events,
        );
        if position.quantity > close_quantity {
            position.quantity = position.quantity.saturating_sub(close_quantity);
            position.observe(at);
            self.position = Some(position);
        }
    }

    /// Record `close_quantity` of `position` exiting at `at`: the
    /// [`ClosedTrade`] with its excursion and tape-audit fields, the
    /// realized-points total, and the `Closed` event. The caller owns the
    /// fill event and whatever remains of the position.
    #[expect(
        clippy::too_many_arguments,
        reason = "one exit has one natural argument list; bundling it into a struct would name nothing"
    )]
    fn record_close(
        &mut self,
        position: &Position,
        close_quantity: Decimal,
        at: Decimal,
        closed_ms: i64,
        exit_agg_id: u64,
        reason: ExitReason,
        events: &mut Vec<VenueEvent>,
    ) {
        let (adverse, favorable) = position.excursions(at);
        let closed = ClosedTrade {
            side: position.side,
            quantity: close_quantity,
            entry_price: position.avg_price,
            exit_price: at,
            opened_ms: position.opened_ms,
            closed_ms,
            pnl_points: signed_points(position.side, position.avg_price, at, close_quantity),
            exit_reason: reason,
            entry_agg_id: Some(position.opened_agg_id),
            exit_agg_id: Some(exit_agg_id),
            mae_points: Some(adverse.saturating_mul(close_quantity)),
            mfe_points: Some(favorable.saturating_mul(close_quantity)),
        };
        self.realized_points = self.realized_points.saturating_add(closed.pnl_points);
        events.push(VenueEvent::Closed(closed.clone()));
        self.closed.push(closed);
    }
}

/// A brand-new position opened by an entry fill at `at`: its exposure range
/// starts at the fill price, and its audit trail starts at the causing print.
fn opened_position(
    side: Side,
    quantity: Decimal,
    at: Decimal,
    print: &Trade,
    bracket: Bracket,
) -> Position {
    Position {
        side,
        quantity,
        avg_price: at,
        opened_ms: print.timestamp_ms,
        opened_agg_id: print.agg_id,
        low_price: at,
        high_price: at,
        // The single-pair view the port reports. A ladder has several stops
        // and no single one of them is true, so it answers `None` here and
        // the legs in `working_orders` carry the truth instead.
        stop_loss: bracket.stop_loss(),
        take_profit: bracket.take_profit(),
    }
}

/// The ladder's parts resized to cover `total` exactly.
///
/// A ladder is written against the entry that carried it, and the quantity
/// it ends up protecting is not always that entry: averaging in enlarges the
/// position, and a partial fill would shrink it. Parts are taken in order up
/// to what is left, and the last one takes the remainder — the same rule the
/// strategy's own resolution follows, and for the same reason. A ladder that
/// summed short would leave a sliver of the position naked, which is the one
/// outcome a protective ladder may never produce.
fn fitted_parts(bracket: Bracket, total: Decimal) -> Vec<ExitPart> {
    let mut parts: Vec<ExitPart> = bracket
        .parts()
        .copied()
        .filter(|part| !part.is_empty())
        .collect();
    let Some(last) = parts.len().checked_sub(1) else {
        return parts;
    };
    let mut assigned = Decimal::ZERO;
    for (index, part) in parts.iter_mut().enumerate() {
        let left = total.saturating_sub(assigned).max(Decimal::ZERO);
        let share = if index == last {
            left
        } else {
            part.quantity.unwrap_or(total).min(left)
        };
        assigned = assigned.saturating_add(share);
        part.quantity = (share > Decimal::ZERO).then_some(share);
    }
    parts
}

/// True when `price` reaches a protective leg resting at `level`.
///
/// The leg's own side is the reducing one - a sell protects a long - so the
/// direction a stop is reached from is the opposite of the direction a take
/// profit is reached from.
fn leg_reached(role: OrderRole, side: Side, level: Decimal, price: Decimal) -> bool {
    match (role, side) {
        (OrderRole::StopLoss, Side::Sell) => price <= level,
        (OrderRole::StopLoss, Side::Buy) => price >= level,
        (OrderRole::TakeProfit, Side::Sell) => price >= level,
        (OrderRole::TakeProfit, Side::Buy) => price <= level,
        (OrderRole::Entry, _) => false,
    }
}

fn opposite(side: Side) -> Side {
    match side {
        Side::Buy => Side::Sell,
        Side::Sell => Side::Buy,
    }
}

fn require_positive_quantity(quantity: Decimal) -> Result<(), RejectReason> {
    if quantity > Decimal::ZERO {
        Ok(())
    } else {
        Err(RejectReason::QuantityNotPositive)
    }
}

fn require_positive_price(price: Decimal) -> Result<(), RejectReason> {
    if price > Decimal::ZERO {
        Ok(())
    } else {
        Err(RejectReason::PriceNotPositive)
    }
}

/// A limit must rest away from the market: a buy below it, a sell above it.
fn validate_limit_side(side: Side, price: Decimal, mark: Decimal) -> Result<(), RejectReason> {
    let rests = match side {
        Side::Buy => price < mark,
        Side::Sell => price > mark,
    };
    if rests {
        Ok(())
    } else {
        Err(RejectReason::LimitOnWrongSide(side))
    }
}

/// A limit's cancel-at level must sit on the far side of the market from
/// the limit price — above it for a buy (which rests below), below it for a
/// sell. That keeps the two levels on opposite sides of every future print,
/// so no single print can both fill and cancel the order.
fn validate_cancel_at_side(side: Side, cancel: Decimal, mark: Decimal) -> Result<(), RejectReason> {
    let far_side = match side {
        Side::Buy => cancel > mark,
        Side::Sell => cancel < mark,
    };
    if far_side {
        Ok(())
    } else {
        Err(RejectReason::CancelAtOnWrongSide(side))
    }
}

/// A stop must arm beyond the market: a buy above it, a sell below it.
fn validate_stop_side(side: Side, trigger: Decimal, mark: Decimal) -> Result<(), RejectReason> {
    let arms = match side {
        Side::Buy => trigger > mark,
        Side::Sell => trigger < mark,
    };
    if arms {
        Ok(())
    } else {
        Err(RejectReason::StopOnWrongSide(side))
    }
}

/// Protective prices must sit on the correct side of the reference (the
/// entry's own price, or the mark for market orders and open positions):
/// the stop on the losing side, the take profit on the winning side.
fn validate_bracket(side: Side, reference: Decimal, bracket: Bracket) -> Result<(), RejectReason> {
    for part in bracket.parts() {
        if let Some(level) = part.stop_loss {
            require_positive_price(level)?;
            let protective = match side {
                Side::Buy => level < reference,
                Side::Sell => level > reference,
            };
            if !protective {
                return Err(RejectReason::StopLossOnWrongSide(side));
            }
        }
        if let Some(level) = part.take_profit {
            require_positive_price(level)?;
            let winning = match side {
                Side::Buy => level > reference,
                Side::Sell => level < reference,
            };
            if !winning {
                return Err(RejectReason::TakeProfitOnWrongSide(side));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(value: i64) -> Decimal {
        Decimal::from(value)
    }

    /// A print: id doubles as a readable timestamp (`ts = id * 1000`).
    fn print(agg_id: u64, price: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("test ids are small") * 1000,
            price: dec(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    fn seeded(price: i64) -> Simulator {
        let mut sim = Simulator::new();
        sim.seed(&print(0, price));
        sim
    }

    fn fills(events: &[VenueEvent]) -> Vec<Fill> {
        events
            .iter()
            .filter_map(|event| match event {
                VenueEvent::Filled(fill) => Some(*fill),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn market_fills_at_the_next_print_not_the_last_one() {
        let mut sim = seeded(100);
        let events = sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        assert!(matches!(events.as_slice(), [VenueEvent::Placed(_)]));
        assert_eq!(sim.queued().len(), 1);

        let events = sim.on_trade(&print(1, 103));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(
            filled[0].price,
            dec(103),
            "fills at the next print, not the seeded 100"
        );
        assert_eq!(filled[0].quantity, dec(2));
        let position = sim.position().expect("position opened");
        assert_eq!(position.side, Side::Buy);
        assert_eq!(position.avg_price, dec(103));
        assert!(sim.queued().is_empty());
    }

    #[test]
    fn nothing_places_before_the_first_print() {
        let mut sim = Simulator::new();
        let events = sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::NoMarketPrice)]
        );
    }

    #[test]
    fn backfill_seed_never_fills() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
        });
        // More backfill arrives (say, paging older history) — no fill.
        sim.seed(&print(1, 90));
        assert!(sim.position().is_none());
        assert_eq!(sim.queued().len(), 1);
    }

    #[test]
    fn limit_buy_rests_and_fills_at_its_own_price_even_on_a_gap() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        assert!(
            fills(&sim.on_trade(&print(1, 96))).is_empty(),
            "96 does not touch 95"
        );

        // The tape gaps through the level: the limit still fills at 95 —
        // the resting order was in the book at 95, not at the gap price.
        let events = sim.on_trade(&print(2, 93));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].price, dec(95));
        assert!(sim.orders().is_empty());
    }

    #[test]
    fn marketable_limit_is_rejected_with_advice() {
        let mut sim = seeded(100);
        let events = sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::LimitOnWrongSide(
                Side::Buy
            ))]
        );
    }

    /// The retest scenario the strategy kernel places: the market broke
    /// below a region, a sell limit rests at the broken edge, and the
    /// cancel-at level names the projected target below. Whichever the tape
    /// reaches first decides the order's fate.
    #[test]
    fn a_print_through_the_cancel_at_level_removes_the_limit_unfilled() {
        // Market at 95 after a break: sell limit back up at 100 (the region
        // edge), cancelled if 88 (the target) trades first.
        let mut sim = seeded(95);
        sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: Some(dec(88)),
            flat_only: false,
        });
        // Prints between the two levels leave the order resting.
        assert!(sim.on_trade(&print(1, 94)).is_empty());
        assert_eq!(sim.orders().len(), 1);

        // The tape gaps through the target: the order goes, no fill.
        let events = sim.on_trade(&print(2, 87));
        assert!(
            matches!(
                events.as_slice(),
                [VenueEvent::Cancelled {
                    reason: CancelReason::PriceTouched,
                    ..
                }]
            ),
            "the target print cancels the resting limit: {events:?}"
        );
        assert!(sim.orders().is_empty());
        assert!(sim.position().is_none());
    }

    #[test]
    fn the_retest_fill_still_wins_when_the_tape_returns_first() {
        let mut sim = seeded(95);
        sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::whole(Some(dec(104)), Some(dec(88))),
            cancel_at: Some(dec(88)),
            flat_only: false,
        });
        // The tape returns to the edge before reaching the target: a normal
        // limit fill at its own price, bracket armed, cancel-at forgotten.
        let events = sim.on_trade(&print(1, 100));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].price, dec(100));
        let position = sim.position().expect("short opened at the edge");
        assert_eq!(position.side, Side::Sell);
        assert_eq!(position.take_profit, Some(dec(88)));

        // The later target print is now the position's take profit, not a
        // cancel of anything.
        let events = sim.on_trade(&print(2, 88));
        assert_eq!(fills(&events).len(), 1);
        assert!(sim.position().is_none());
    }

    /// Mirrored for a buy: the limit rests below, the cancel-at sits above.
    #[test]
    fn a_buy_retest_limit_cancels_upward_and_fills_downward() {
        let mut sim = seeded(105);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: Some(dec(112)),
            flat_only: false,
        });
        let events = sim.on_trade(&print(1, 112));
        assert!(
            matches!(
                events.as_slice(),
                [VenueEvent::Cancelled {
                    reason: CancelReason::PriceTouched,
                    ..
                }]
            ),
            "the upward target print cancels the buy limit: {events:?}"
        );

        let mut sim = seeded(105);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: Some(dec(112)),
            flat_only: false,
        });
        let events = sim.on_trade(&print(1, 100));
        assert_eq!(fills(&events).len(), 1, "the retest fills as usual");
    }

    /// A flat-only order's moment arriving over an open position stands the
    /// order down instead of filling it — the position a human opened while
    /// the order rested is never traded against, closed, or re-bracketed.
    #[test]
    fn a_flat_only_limit_stands_down_when_its_moment_finds_a_position() {
        let mut sim = seeded(95);
        sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::whole(Some(dec(104)), Some(dec(88))),
            cancel_at: Some(dec(88)),
            flat_only: true,
        });
        // A human buys at market while the order rests.
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 96));
        assert_eq!(sim.position().expect("manual long").quantity, dec(2));

        // The tape returns to the edge: the flat-only order stands down —
        // no fill, and the human's position is untouched.
        let events = sim.on_trade(&print(2, 100));
        assert!(
            matches!(
                events.as_slice(),
                [VenueEvent::Cancelled {
                    reason: CancelReason::AccountOccupied,
                    ..
                }]
            ),
            "the occupied account cancels the flat-only order: {events:?}"
        );
        let position = sim.position().expect("the human's long survives");
        assert_eq!(position.side, Side::Buy);
        assert_eq!(position.quantity, dec(2));
        assert_eq!(position.stop_loss, None, "their bracket is untouched");
        assert!(sim.orders().is_empty());
    }

    /// The stand-down judges the *fill moment*, not history: a position
    /// that opened and closed while the order rested changes nothing.
    #[test]
    fn a_position_that_closed_before_the_moment_does_not_stand_the_order_down() {
        let mut sim = seeded(95);
        sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: true,
        });
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 96));
        sim.apply(Command::ClosePosition);
        sim.on_trade(&print(2, 97));
        assert!(sim.position().is_none(), "the round trip closed");

        let events = sim.on_trade(&print(3, 100));
        assert_eq!(fills(&events).len(), 1, "flat again, the order fills");
        assert_eq!(sim.position().expect("short opened").side, Side::Sell);
    }

    /// A cancel-at level on the fill side of the market would cancel
    /// instantly (or race its own fill) — refused with advice, like every
    /// other level the simulator cannot honestly hold.
    #[test]
    fn a_cancel_at_on_the_wrong_side_is_rejected_whole() {
        let mut sim = seeded(95);
        let events = sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: Some(dec(97)),
            flat_only: false,
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::CancelAtOnWrongSide(
                Side::Sell
            ))],
            "a sell's cancel-at above the market is refused"
        );
        assert!(sim.orders().is_empty());

        let events = sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: dec(100),
            bracket: Bracket::none(),
            cancel_at: Some(Decimal::ZERO),
            flat_only: false,
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::PriceNotPositive)],
            "a non-positive cancel-at is refused"
        );
    }

    #[test]
    fn stop_entry_fills_at_the_print_so_a_gap_fills_worse() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceStop {
            side: Side::Buy,
            quantity: Decimal::ONE,
            trigger: dec(105),
            bracket: Bracket::none(),
        });
        // Gap over the trigger: the stop fills at the print's 108, not at
        // the trigger's 105 — a stop chases, it does not rest.
        let events = sim.on_trade(&print(1, 108));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].price, dec(108));
    }

    #[test]
    fn stop_on_the_wrong_side_is_rejected() {
        let mut sim = seeded(100);
        let events = sim.apply(Command::PlaceStop {
            side: Side::Buy,
            quantity: Decimal::ONE,
            trigger: dec(99),
            bracket: Bracket::none(),
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::StopOnWrongSide(
                Side::Buy
            ))]
        );
    }

    #[test]
    fn bracket_attaches_on_fill_and_the_stop_exits_at_the_print() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(3),
            bracket: Bracket::whole(Some(dec(97)), Some(dec(110))),
        });
        sim.on_trade(&print(1, 100));
        let position = sim.position().expect("opened");
        assert_eq!(position.stop_loss, Some(dec(97)));
        assert_eq!(position.take_profit, Some(dec(110)));

        // Gap through the stop: exits at the print's 95, honestly worse.
        let events = sim.on_trade(&print(2, 95));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].price, dec(95));
        assert_eq!(filled[0].role, FillRole::StopLoss);
        let closed = sim.closed_trades().last().expect("round trip recorded");
        assert_eq!(closed.exit_reason, ExitReason::StopLoss);
        assert_eq!(closed.pnl_points, dec(-15), "(95 - 100) × 3");
        assert!(sim.position().is_none());
    }

    #[test]
    fn a_target_outrun_by_the_fill_is_dropped_and_reported() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::whole(None, Some(dec(101))),
        });
        // The tape outruns the target before the fill: entry lands at 102,
        // above the 101 target that validated fine against the 100 mark.
        let events = sim.on_trade(&print(1, 102));
        assert!(
            events.iter().any(|event| matches!(
                event,
                VenueEvent::BracketDropped {
                    reason: RejectReason::TakeProfitOnWrongSide(Side::Buy)
                }
            )),
            "the drop is reported, never silent"
        );
        let position = sim.position().expect("opened without the target");
        assert_eq!(position.take_profit, None);
        // The next print must not exit at a "take profit" below the entry.
        let events = sim.on_trade(&print(2, 102));
        assert!(fills(&events).is_empty(), "no lying take_profit exit");
    }

    #[test]
    fn a_stop_that_survives_the_gap_is_kept() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceStop {
            side: Side::Buy,
            quantity: Decimal::ONE,
            trigger: dec(105),
            bracket: Bracket::whole(Some(dec(104)), None),
        });
        // Gap to 108: the fill is worse than the trigger, but 104 still
        // protects a long entered at 108 — kept, not dropped.
        sim.on_trade(&print(1, 108));
        assert_eq!(sim.position().expect("long").stop_loss, Some(dec(104)));
    }

    #[test]
    fn take_profit_fills_at_its_own_price() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::whole(None, Some(dec(110))),
        });
        sim.on_trade(&print(1, 100));
        // Gap above the target: a resting limit fills at 110, not 115.
        let events = sim.on_trade(&print(2, 115));
        let filled = fills(&events);
        assert_eq!(filled[0].price, dec(110));
        assert_eq!(filled[0].role, FillRole::TakeProfit);
    }

    #[test]
    fn the_entry_print_never_triggers_its_own_bracket() {
        let mut sim = seeded(100);
        // A tight stop right under the fill price…
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::whole(Some(dec(99)), None),
        });
        // …fills at 99: at or below the stop, but on the same print.
        let events = sim.on_trade(&print(1, 99));
        assert_eq!(fills(&events).len(), 1, "entry only");
        assert!(
            sim.position().is_some(),
            "not stopped out by its own entry print"
        );
        // The next print at the stop level does exit.
        let events = sim.on_trade(&print(2, 99));
        assert_eq!(fills(&events)[0].role, FillRole::StopLoss);
    }

    #[test]
    fn averaging_in_moves_the_average_price() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(2, 104));
        let position = sim.position().expect("still long");
        assert_eq!(position.quantity, dec(2));
        assert_eq!(position.avg_price, dec(102));
    }

    #[test]
    fn reversal_closes_the_position_and_opens_the_remainder() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        // Sell 5 while long 2: closes 2, opens short 3.
        sim.apply(Command::PlaceMarket {
            side: Side::Sell,
            quantity: dec(5),
            bracket: Bracket::none(),
        });
        let events = sim.on_trade(&print(2, 106));
        let closed = sim
            .closed_trades()
            .last()
            .expect("reversal closed the long");
        assert_eq!(closed.exit_reason, ExitReason::Reversal);
        assert_eq!(closed.quantity, dec(2));
        assert_eq!(closed.pnl_points, dec(12), "(106 - 100) × 2");
        let position = sim.position().expect("remainder opened");
        assert_eq!(position.side, Side::Sell);
        assert_eq!(position.quantity, dec(3));
        assert_eq!(position.avg_price, dec(106));
        assert_eq!(fills(&events).len(), 1, "one execution covers both legs");
    }

    #[test]
    fn partial_close_keeps_the_position_and_its_average() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(5),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::PlaceMarket {
            side: Side::Sell,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(2, 103));
        let position = sim.position().expect("3 remain");
        assert_eq!(position.side, Side::Buy);
        assert_eq!(position.quantity, dec(3));
        assert_eq!(position.avg_price, dec(100));
        assert_eq!(
            sim.closed_trades()
                .last()
                .expect("partial recorded")
                .quantity,
            dec(2)
        );
    }

    #[test]
    fn close_position_fills_at_the_next_print() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        assert!(sim.apply(Command::ClosePosition).is_empty());
        let events = sim.on_trade(&print(2, 101));
        assert_eq!(fills(&events)[0].role, FillRole::Close);
        assert_eq!(
            sim.closed_trades().last().expect("closed").exit_reason,
            ExitReason::Manual
        );
        assert!(sim.position().is_none());
    }

    #[test]
    fn a_close_dissolves_when_the_stop_got_there_first() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::whole(Some(dec(98)), None),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::ClosePosition);
        // The same print hits the stop; the queued close then finds nothing
        // and dissolves instead of opening a phantom short.
        let events = sim.on_trade(&print(2, 97));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].role, FillRole::StopLoss);
        assert!(sim.position().is_none());
        assert_eq!(sim.closed_trades().len(), 1);
    }

    #[test]
    fn flatten_cancels_everything_and_closes_at_the_next_print() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(1),
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        let events = sim.apply(Command::Flatten);
        assert!(
            matches!(
                events.as_slice(),
                [VenueEvent::Cancelled {
                    reason: CancelReason::Flatten,
                    ..
                }]
            ),
            "the resting limit is swept"
        );
        let events = sim.on_trade(&print(2, 102));
        assert_eq!(fills(&events)[0].role, FillRole::Close);
        assert!(sim.position().is_none());
        assert!(sim.orders().is_empty());
    }

    #[test]
    fn set_bracket_validates_against_the_mark() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        let events = sim.apply(Command::SetBracket {
            stop_loss: Some(dec(101)),
            take_profit: None,
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::StopLossOnWrongSide(
                Side::Buy
            ))]
        );

        let events = sim.apply(Command::SetBracket {
            stop_loss: Some(dec(96)),
            take_profit: Some(dec(107)),
        });
        assert_eq!(
            events,
            vec![VenueEvent::BracketSet {
                stop_loss: Some(dec(96)),
                take_profit: Some(dec(107))
            }]
        );
        let position = sim.position().expect("long");
        assert_eq!(position.stop_loss, Some(dec(96)));
        assert_eq!(position.take_profit, Some(dec(107)));
    }

    #[test]
    fn a_breakeven_stop_is_legal_once_the_market_moved_up() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.on_trade(&print(2, 106));
        // Stop above the entry (locking profit in) but below the mark.
        let events = sim.apply(Command::SetBracket {
            stop_loss: Some(dec(103)),
            take_profit: None,
        });
        assert!(matches!(events.as_slice(), [VenueEvent::BracketSet { .. }]));
        let events = sim.on_trade(&print(3, 103));
        assert_eq!(
            sim.closed_trades().last().expect("stopped").pnl_points,
            dec(3)
        );
        assert_eq!(fills(&events)[0].role, FillRole::StopLoss);
    }

    #[test]
    fn modify_and_cancel_address_only_pending_orders() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(1),
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        let id = sim.orders()[0].id;
        let events = sim.apply(Command::ModifyOrder { id, price: dec(93) });
        assert!(
            matches!(events.as_slice(), [VenueEvent::Updated(order)] if order.price == Some(dec(93)))
        );

        let events = sim.apply(Command::CancelOrder { id });
        assert!(matches!(
            events.as_slice(),
            [VenueEvent::Cancelled {
                reason: CancelReason::User,
                ..
            }]
        ));
        assert!(sim.orders().is_empty());

        let events = sim.apply(Command::CancelOrder { id });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::UnknownOrder(id))]
        );
    }

    /// The whole point of a bracket on a *working* order: the trader draws
    /// the stop and the target while the entry is still waiting, and the
    /// position opens already protected — no window between the fill and
    /// the hand that would have placed them.
    #[test]
    fn a_bracket_set_on_a_resting_limit_arms_the_position_on_fill() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(1),
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        let id = sim.orders()[0].id;

        let events = sim.apply(Command::SetOrderBracket {
            id,
            bracket: Bracket::whole(Some(dec(90)), Some(dec(110))),
        });
        assert!(matches!(
            events.as_slice(),
            [VenueEvent::Updated(order)]
                if order.bracket
                    == Bracket::whole(Some(dec(90)), Some(dec(110)))
        ));

        // The tape reaches the limit; the position it opens wears both legs.
        sim.on_trade(&print(1, 95));
        let position = sim.position().expect("the limit filled");
        assert_eq!(position.stop_loss, Some(dec(90)));
        assert_eq!(position.take_profit, Some(dec(110)));

        // And they are live, not decorative.
        let events = sim.on_trade(&print(2, 110));
        assert_eq!(fills(&events)[0].role, FillRole::TakeProfit);
    }

    /// The reference is the order's own resting price, never the mark. At
    /// mark 100 a stop of 96 is below the market and would pass a
    /// mark-based check; against the buy limit resting at 95 it sits on the
    /// *profit* side and is refused.
    #[test]
    fn a_working_order_bracket_is_judged_against_the_orders_own_price() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(1),
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        let id = sim.orders()[0].id;

        let events = sim.apply(Command::SetOrderBracket {
            id,
            bracket: Bracket::whole(Some(dec(96)), None),
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::StopLossOnWrongSide(
                Side::Buy
            ))]
        );
        // Refused means unchanged, not partially applied.
        assert_eq!(sim.orders()[0].bracket, Bracket::none());
    }

    /// `None` clears a leg, and the order keeps the other one — the same
    /// wholesale replacement `SetBracket` performs on a position.
    #[test]
    fn setting_a_working_order_bracket_replaces_both_legs_wholesale() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceStop {
            side: Side::Sell,
            quantity: dec(1),
            trigger: dec(95),
            bracket: Bracket::whole(Some(dec(99)), Some(dec(90))),
        });
        let id = sim.orders()[0].id;

        sim.apply(Command::SetOrderBracket {
            id,
            bracket: Bracket::whole(None, Some(dec(88))),
        });
        assert_eq!(sim.orders()[0].bracket, Bracket::whole(None, Some(dec(88))));
    }

    /// An id that never rested — a filled, cancelled or market order — is
    /// reported, never silently ignored.
    #[test]
    fn bracketing_an_unknown_order_is_rejected() {
        let mut sim = seeded(100);
        let events = sim.apply(Command::SetOrderBracket {
            id: OrderId(42),
            bracket: Bracket::whole(Some(dec(90)), None),
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::UnknownOrder(OrderId(
                42
            )))]
        );
    }

    #[test]
    fn modifying_a_limit_through_the_market_is_rejected() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(1),
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        let id = sim.orders()[0].id;
        let events = sim.apply(Command::ModifyOrder {
            id,
            price: dec(102),
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::LimitOnWrongSide(
                Side::Buy
            ))]
        );
        assert_eq!(
            sim.orders()[0].price,
            Some(dec(95)),
            "the order is untouched"
        );
    }

    #[test]
    fn reset_flattens_at_the_mark_and_keeps_the_history() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(1),
            price: dec(90),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        sim.on_trade(&print(2, 104));

        let events = sim.reset();
        assert!(
            matches!(
                events.first(),
                Some(VenueEvent::Cancelled {
                    reason: CancelReason::Reset,
                    ..
                })
            ),
            "pending orders are swept"
        );
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].role, FillRole::Reset);
        assert_eq!(filled[0].price, dec(104), "flattened at the last mark");
        let closed = sim.closed_trades().last().expect("reset closes the trade");
        assert_eq!(closed.exit_reason, ExitReason::Reset);
        assert_eq!(closed.pnl_points, dec(8), "(104 - 100) × 2");
        assert!(sim.position().is_none());
        assert_eq!(sim.mark_price(), None, "the old timeline's mark is gone");
        assert_eq!(sim.realized_points(), dec(8), "history survives the reset");
    }

    #[test]
    fn quantity_and_price_must_be_positive() {
        let mut sim = seeded(100);
        let events = sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ZERO,
            bracket: Bracket::none(),
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::QuantityNotPositive)]
        );
        let events = sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: Decimal::ZERO,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::PriceNotPositive)]
        );
    }

    #[test]
    fn close_partial_keeps_the_remainder_and_its_average() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(5),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        assert!(
            sim.apply(Command::ClosePartial { quantity: dec(2) })
                .is_empty()
        );

        let events = sim.on_trade(&print(2, 103));
        let filled = fills(&events);
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].role, FillRole::Close);
        assert_eq!(filled[0].quantity, dec(2));
        let closed = sim.closed_trades().last().expect("partial recorded");
        assert_eq!(closed.quantity, dec(2));
        assert_eq!(closed.pnl_points, dec(6), "(103 - 100) × 2");
        assert_eq!(closed.exit_reason, ExitReason::Manual);
        let position = sim.position().expect("3 remain");
        assert_eq!(position.quantity, dec(3));
        assert_eq!(position.avg_price, dec(100), "the average is untouched");
        assert_eq!(position.opened_ms, 1000, "so is the opening time");
        assert_eq!(sim.realized_points(), dec(6));
    }

    #[test]
    fn close_partial_clamps_to_the_open_quantity_and_never_reverses() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::ClosePartial { quantity: dec(5) });
        let events = sim.on_trade(&print(2, 101));
        assert_eq!(
            fills(&events)[0].quantity,
            dec(2),
            "clamped to what is open"
        );
        assert!(
            sim.position().is_none(),
            "closed, not reversed into a short"
        );
        assert_eq!(sim.closed_trades().last().expect("closed").quantity, dec(2));
    }

    #[test]
    fn close_partial_dissolves_when_a_bracket_got_there_first() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::whole(Some(dec(98)), None),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::ClosePartial { quantity: dec(1) });
        let events = sim.on_trade(&print(2, 97));
        let filled = fills(&events);
        assert_eq!(
            filled.len(),
            1,
            "the stop exit only — the partial dissolved"
        );
        assert_eq!(filled[0].role, FillRole::StopLoss);
        assert_eq!(sim.closed_trades().len(), 1);
    }

    #[test]
    fn close_partial_needs_a_position_and_a_positive_quantity() {
        let mut sim = seeded(100);
        let events = sim.apply(Command::ClosePartial { quantity: dec(1) });
        assert_eq!(events, vec![VenueEvent::Rejected(RejectReason::NoPosition)]);

        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        let events = sim.apply(Command::ClosePartial {
            quantity: Decimal::ZERO,
        });
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::QuantityNotPositive)]
        );
    }

    #[test]
    fn excursions_and_agg_ids_come_from_the_tape() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.on_trade(&print(2, 96));
        sim.on_trade(&print(3, 104));
        sim.apply(Command::ClosePosition);
        sim.on_trade(&print(4, 103));
        let closed = sim.closed_trades().last().expect("closed");
        assert_eq!(closed.entry_agg_id, Some(1), "the print that opened it");
        assert_eq!(closed.exit_agg_id, Some(4), "the print that closed it");
        assert_eq!(closed.mae_points, Some(dec(4)), "worst mark was 96");
        assert_eq!(closed.mfe_points, Some(dec(4)), "best mark was 104");
    }

    #[test]
    fn a_stop_gap_counts_toward_the_adverse_excursion() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::whole(Some(dec(98)), None),
        });
        sim.on_trade(&print(1, 100));
        sim.on_trade(&print(2, 95));
        let closed = sim.closed_trades().last().expect("stopped");
        assert_eq!(
            closed.mae_points,
            Some(dec(5)),
            "the gap fill at 95 is part of the excursion"
        );
        assert_eq!(closed.mfe_points, Some(dec(0)));
        assert_eq!(closed.exit_agg_id, Some(2));
    }

    #[test]
    fn averaging_in_measures_excursions_against_the_final_average() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(1),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(2, 104));
        sim.apply(Command::ClosePosition);
        sim.on_trade(&print(3, 104));
        let closed = sim.closed_trades().last().expect("closed");
        assert_eq!(closed.entry_price, dec(102), "the volume-weighted average");
        assert_eq!(closed.entry_agg_id, Some(1), "the print that opened it");
        assert_eq!(
            closed.mae_points,
            Some(dec(4)),
            "(102 - 100) per unit against the final average, × 2"
        );
        assert_eq!(closed.mfe_points, Some(dec(4)));
    }

    #[test]
    fn reset_stamps_the_mark_as_the_exit_print() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(2),
            bracket: Bracket::none(),
        });
        sim.on_trade(&print(1, 100));
        sim.on_trade(&print(2, 104));
        sim.reset();
        let closed = sim.closed_trades().last().expect("reset closes");
        assert_eq!(closed.entry_agg_id, Some(1));
        assert_eq!(closed.exit_agg_id, Some(2), "the mark it flattened at");
        assert_eq!(closed.mae_points, Some(dec(0)));
        assert_eq!(closed.mfe_points, Some(dec(8)), "(104 - 100) × 2");
    }

    /// The determinism contract, exercised end to end: one fixed tape, one
    /// fixed command script, run twice from scratch — every event and every
    /// closed trade must match. Mirrors `engine::golden`'s double-run idea.
    #[test]
    fn same_tape_and_commands_produce_identical_output() {
        fn run() -> (Vec<String>, Vec<ClosedTrade>, Decimal) {
            let tape = [
                print(1, 100),
                print(2, 98),
                print(3, 95),
                print(4, 99),
                print(5, 104),
                print(6, 101),
                print(7, 108),
            ];
            let mut sim = Simulator::new();
            sim.seed(&print(0, 100));
            let mut log = Vec::new();
            for trade in &tape {
                if trade.agg_id == 1 {
                    for event in sim.apply(Command::PlaceLimit {
                        side: Side::Buy,
                        quantity: dec(2),
                        price: dec(96),
                        bracket: Bracket::whole(Some(dec(90)), Some(dec(104))),
                        cancel_at: None,
                        flat_only: false,
                    }) {
                        log.push(format!("{event:?}"));
                    }
                }
                if trade.agg_id == 6 {
                    for event in sim.apply(Command::PlaceStop {
                        side: Side::Buy,
                        quantity: dec(1),
                        trigger: dec(106),
                        bracket: Bracket::none(),
                    }) {
                        log.push(format!("{event:?}"));
                    }
                }
                for event in sim.on_trade(trade) {
                    log.push(format!("{event:?}"));
                }
            }
            (log, sim.closed_trades().to_vec(), sim.realized_points())
        }
        let first = run();
        let second = run();
        assert_eq!(
            first, second,
            "a paper trade must be replayable to the letter"
        );
        assert!(!first.0.is_empty(), "the script actually traded");
    }
}
