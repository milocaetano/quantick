//! The simulator core: prints and commands in, fills and closures out.

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

use crate::events::{CancelReason, ExitReason, Fill, FillRole, RejectReason, SimEvent};
use crate::order::{Bracket, EntryKind, Order, OrderId};
use crate::position::{Position, signed_points};

/// One completed round trip: an exit fill closing quantity against the
/// position's average entry at that moment. The unit persisted as history
/// and consumed by the performance report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTrade {
    /// Side of the position that closed: `Buy` was a long, `Sell` a short.
    pub side: Side,
    pub quantity: Decimal,
    /// Average entry price at the moment of the exit.
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    /// Venue time of the print that opened the position.
    pub opened_ms: i64,
    /// Venue time of the print that closed it.
    pub closed_ms: i64,
    /// Profit in points (price units × quantity), signed. Points, not
    /// currency: the workspace knows no per-instrument tick value, and a
    /// number the simulator cannot compute honestly is not shown.
    pub pnl_points: Decimal,
    pub exit_reason: ExitReason,
    /// Aggregate id of the print that opened the position — the audit trail
    /// back to the tape. `None` only on rows loaded from a version-1 history
    /// file, which did not record it; the simulator always fills it.
    pub entry_agg_id: Option<u64>,
    /// Aggregate id of the print that closed this quantity (see
    /// `entry_agg_id` for why it is optional).
    pub exit_agg_id: Option<u64>,
    /// Maximum adverse excursion in points (≥ 0): the worst the position ran
    /// against its average entry over every price it was exposed to — entry
    /// fills, marks while open, the exit fill — scaled by this trade's
    /// closed quantity. Measured against the average entry at close time,
    /// so a position that averaged in reports its excursion against the
    /// final average. `None` only for version-1 history rows: unknown is
    /// not zero.
    pub mae_points: Option<Decimal>,
    /// Maximum favorable excursion in points (≥ 0); see `mae_points`.
    pub mfe_points: Option<Decimal>,
}

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
    /// A market entry order.
    Entry(Order),
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
    /// [`SimEvent::Rejected`] whose reason says what to do instead.
    pub fn apply(&mut self, command: Command) -> Vec<SimEvent> {
        match self.dispatch(command) {
            Ok(events) => events,
            Err(reason) => vec![SimEvent::Rejected(reason)],
        }
    }

    /// Process one print. The processing order is fixed and documented in
    /// the crate doc: brackets, then queued market actions, then resting
    /// orders; the mark updates last, so entries filled by this print arm
    /// their brackets from the next one.
    pub fn on_trade(&mut self, trade: &Trade) -> Vec<SimEvent> {
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

        // 3) Resting orders, in placement order.
        let resting = std::mem::take(&mut self.resting);
        let mut kept = Vec::with_capacity(resting.len());
        for order in resting {
            // Resting orders always carry a price by construction.
            let Some(level) = order.price else {
                kept.push(order);
                continue;
            };
            let fill_at = match (order.kind, order.side) {
                (EntryKind::Limit, Side::Buy) if price <= level => Some(level),
                (EntryKind::Limit, Side::Sell) if price >= level => Some(level),
                (EntryKind::Stop, Side::Buy) if price >= level => Some(price),
                (EntryKind::Stop, Side::Sell) if price <= level => Some(price),
                _ => None,
            };
            match fill_at {
                Some(at) => self.fill_entry_at(&order, at, trade, &mut events),
                None => kept.push(order),
            }
        }
        self.resting = kept;

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
    pub fn reset(&mut self) -> Vec<SimEvent> {
        let mut events = Vec::new();
        for action in std::mem::take(&mut self.queue) {
            if let QueuedAction::Entry(order) = action {
                events.push(SimEvent::Cancelled {
                    order,
                    reason: CancelReason::Reset,
                });
            }
        }
        for order in std::mem::take(&mut self.resting) {
            events.push(SimEvent::Cancelled {
                order,
                reason: CancelReason::Reset,
            });
        }
        // A position can only exist after a print, so the mark is present
        // whenever the position is.
        if let (Some(position), Some(mark)) = (self.position.take(), self.mark) {
            events.push(SimEvent::Filled(Fill {
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

    fn dispatch(&mut self, command: Command) -> Result<Vec<SimEvent>, RejectReason> {
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
                    mark.timestamp_ms,
                );
                let placed = SimEvent::Placed(order.clone());
                self.queue.push(QueuedAction::Entry(order));
                Ok(vec![placed])
            }
            Command::PlaceLimit {
                side,
                quantity,
                price,
                bracket,
            } => {
                let mark = self.mark.ok_or(RejectReason::NoMarketPrice)?;
                require_positive_quantity(quantity)?;
                require_positive_price(price)?;
                validate_limit_side(side, price, mark.price)?;
                validate_bracket(side, price, bracket)?;
                let order = self.make_order(
                    side,
                    EntryKind::Limit,
                    Some(price),
                    quantity,
                    bracket,
                    mark.timestamp_ms,
                );
                let placed = SimEvent::Placed(order.clone());
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
                    mark.timestamp_ms,
                );
                let placed = SimEvent::Placed(order.clone());
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
                Ok(vec![SimEvent::Updated(order.clone())])
            }
            Command::CancelOrder { id } => {
                if let Some(index) = self.resting.iter().position(|order| order.id == id) {
                    let order = self.resting.remove(index);
                    return Ok(vec![SimEvent::Cancelled {
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
                    return Ok(vec![SimEvent::Cancelled {
                        order,
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
                    Bracket {
                        stop_loss,
                        take_profit,
                    },
                )?;
                position.stop_loss = stop_loss;
                position.take_profit = take_profit;
                Ok(vec![SimEvent::BracketSet {
                    stop_loss,
                    take_profit,
                }])
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
                        events.push(SimEvent::Cancelled {
                            order,
                            reason: CancelReason::Flatten,
                        });
                    }
                }
                for order in std::mem::take(&mut self.resting) {
                    events.push(SimEvent::Cancelled {
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

    fn make_order(
        &mut self,
        side: Side,
        kind: EntryKind,
        price: Option<Decimal>,
        quantity: Decimal,
        bracket: Bracket,
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
            placed_ms,
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
        events: &mut Vec<SimEvent>,
    ) -> Bracket {
        let mut kept = bracket;
        if let Some(level) = kept.stop_loss {
            let lying = match side {
                Side::Buy => level > at,
                Side::Sell => level < at,
            };
            if lying {
                events.push(SimEvent::BracketDropped {
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
                events.push(SimEvent::BracketDropped {
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
        events: &mut Vec<SimEvent>,
    ) {
        events.push(SimEvent::Filled(Fill {
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
                if bracket.stop_loss.is_some() {
                    position.stop_loss = bracket.stop_loss;
                }
                if bracket.take_profit.is_some() {
                    position.take_profit = bracket.take_profit;
                }
                self.position = Some(position);
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
                    position.quantity = position.quantity.saturating_sub(close_quantity);
                    position.observe(at);
                    self.position = Some(position);
                } else {
                    let remainder = order.quantity.saturating_sub(close_quantity);
                    if remainder > Decimal::ZERO {
                        self.position =
                            Some(opened_position(order.side, remainder, at, print, bracket));
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
        events: &mut Vec<SimEvent>,
    ) {
        let Some(position) = self.position.take() else {
            return;
        };
        events.push(SimEvent::Filled(Fill {
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
        events: &mut Vec<SimEvent>,
    ) {
        let Some(mut position) = self.position.take() else {
            return;
        };
        let close_quantity = position.quantity.min(quantity);
        events.push(SimEvent::Filled(Fill {
            timestamp_ms: print.timestamp_ms,
            agg_id: print.agg_id,
            side: opposite(position.side),
            price: at,
            quantity: close_quantity,
            role: FillRole::Close,
        }));
        self.record_close(
            &position,
            close_quantity,
            at,
            print.timestamp_ms,
            print.agg_id,
            ExitReason::Manual,
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
        events: &mut Vec<SimEvent>,
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
        events.push(SimEvent::Closed(closed.clone()));
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
        stop_loss: bracket.stop_loss,
        take_profit: bracket.take_profit,
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
    if let Some(level) = bracket.stop_loss {
        require_positive_price(level)?;
        let protective = match side {
            Side::Buy => level < reference,
            Side::Sell => level > reference,
        };
        if !protective {
            return Err(RejectReason::StopLossOnWrongSide(side));
        }
    }
    if let Some(level) = bracket.take_profit {
        require_positive_price(level)?;
        let winning = match side {
            Side::Buy => level > reference,
            Side::Sell => level < reference,
        };
        if !winning {
            return Err(RejectReason::TakeProfitOnWrongSide(side));
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

    fn fills(events: &[SimEvent]) -> Vec<Fill> {
        events
            .iter()
            .filter_map(|event| match event {
                SimEvent::Filled(fill) => Some(*fill),
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
        assert!(matches!(events.as_slice(), [SimEvent::Placed(_)]));
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
            vec![SimEvent::Rejected(RejectReason::NoMarketPrice)]
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
        });
        assert_eq!(
            events,
            vec![SimEvent::Rejected(RejectReason::LimitOnWrongSide(
                Side::Buy
            ))]
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
            vec![SimEvent::Rejected(RejectReason::StopOnWrongSide(Side::Buy))]
        );
    }

    #[test]
    fn bracket_attaches_on_fill_and_the_stop_exits_at_the_print() {
        let mut sim = seeded(100);
        sim.apply(Command::PlaceMarket {
            side: Side::Buy,
            quantity: dec(3),
            bracket: Bracket {
                stop_loss: Some(dec(97)),
                take_profit: Some(dec(110)),
            },
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
            bracket: Bracket {
                stop_loss: None,
                take_profit: Some(dec(101)),
            },
        });
        // The tape outruns the target before the fill: entry lands at 102,
        // above the 101 target that validated fine against the 100 mark.
        let events = sim.on_trade(&print(1, 102));
        assert!(
            events.iter().any(|event| matches!(
                event,
                SimEvent::BracketDropped {
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
            bracket: Bracket {
                stop_loss: Some(dec(104)),
                take_profit: None,
            },
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
            bracket: Bracket {
                stop_loss: None,
                take_profit: Some(dec(110)),
            },
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
            bracket: Bracket {
                stop_loss: Some(dec(99)),
                take_profit: None,
            },
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
            bracket: Bracket {
                stop_loss: Some(dec(98)),
                take_profit: None,
            },
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
        });
        let events = sim.apply(Command::Flatten);
        assert!(
            matches!(
                events.as_slice(),
                [SimEvent::Cancelled {
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
            vec![SimEvent::Rejected(RejectReason::StopLossOnWrongSide(
                Side::Buy
            ))]
        );

        let events = sim.apply(Command::SetBracket {
            stop_loss: Some(dec(96)),
            take_profit: Some(dec(107)),
        });
        assert_eq!(
            events,
            vec![SimEvent::BracketSet {
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
        assert!(matches!(events.as_slice(), [SimEvent::BracketSet { .. }]));
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
        });
        let id = sim.orders()[0].id;
        let events = sim.apply(Command::ModifyOrder { id, price: dec(93) });
        assert!(
            matches!(events.as_slice(), [SimEvent::Updated(order)] if order.price == Some(dec(93)))
        );

        let events = sim.apply(Command::CancelOrder { id });
        assert!(matches!(
            events.as_slice(),
            [SimEvent::Cancelled {
                reason: CancelReason::User,
                ..
            }]
        ));
        assert!(sim.orders().is_empty());

        let events = sim.apply(Command::CancelOrder { id });
        assert_eq!(
            events,
            vec![SimEvent::Rejected(RejectReason::UnknownOrder(id))]
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
        });
        let id = sim.orders()[0].id;
        let events = sim.apply(Command::ModifyOrder {
            id,
            price: dec(102),
        });
        assert_eq!(
            events,
            vec![SimEvent::Rejected(RejectReason::LimitOnWrongSide(
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
        });
        sim.on_trade(&print(2, 104));

        let events = sim.reset();
        assert!(
            matches!(
                events.first(),
                Some(SimEvent::Cancelled {
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
            vec![SimEvent::Rejected(RejectReason::QuantityNotPositive)]
        );
        let events = sim.apply(Command::PlaceLimit {
            side: Side::Sell,
            quantity: Decimal::ONE,
            price: Decimal::ZERO,
            bracket: Bracket::none(),
        });
        assert_eq!(
            events,
            vec![SimEvent::Rejected(RejectReason::PriceNotPositive)]
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
            bracket: Bracket {
                stop_loss: Some(dec(98)),
                take_profit: None,
            },
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
        assert_eq!(events, vec![SimEvent::Rejected(RejectReason::NoPosition)]);

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
            vec![SimEvent::Rejected(RejectReason::QuantityNotPositive)]
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
            bracket: Bracket {
                stop_loss: Some(dec(98)),
                take_profit: None,
            },
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
                        bracket: Bracket {
                            stop_loss: Some(dec(90)),
                            take_profit: Some(dec(104)),
                        },
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
