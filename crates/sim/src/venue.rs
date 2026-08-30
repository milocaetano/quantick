//! The paper venue: [`Simulator`] as an implementation of
//! [`TradingVenue`].
//!
//! There is no wrapper type here on purpose. A `PaperVenue(Simulator)`
//! newtype would add a layer with no behaviour of its own — the simulator
//! *is* the paper venue, and every method below is the translation from a
//! venue-neutral request into the [`Command`] the simulator already
//! understood. Keeping [`Command`] is deliberate too: the strategy kernel
//! and the backtest harness speak it directly, and they are not talking to
//! a venue through a port — they are driving this specific simulator.
//!
//! What the translation is allowed to do is *nothing*. Every arm below maps
//! one request to one command; there is no validation, no defaulting and no
//! clamping in this file, because a second implementation of the port that
//! silently corrected its caller would make the two venues disagree about
//! the same gesture.

use quantick_engine::Trade;
use rust_decimal::Decimal;

use quantick_trading::{
    Bracket, BracketTarget, CloseAmount, ClosedTrade, EntryKind, Order, OrderId, OrderIntent,
    Position, RejectReason, TradingVenue, VenueEvent,
};

use crate::simulator::{Command, QueuedAction, Simulator};

impl Command {
    /// Deliver this command to whatever venue is attached.
    ///
    /// The strategy kernel and the backtest harness emit [`Command`]s, and
    /// the chart hosts a [`TradingVenue`] it does not name. This is the one
    /// place the two meet, so a command reaching a broker one day travels
    /// the same road as one reaching the simulator — there is no second
    /// translation to keep in step with this one.
    #[must_use]
    pub fn dispatch(self, venue: &mut dyn TradingVenue) -> Vec<VenueEvent> {
        match self {
            Self::PlaceMarket {
                side,
                quantity,
                bracket,
            } => venue.submit(OrderIntent::market(side, quantity).with_bracket(bracket)),
            Self::PlaceLimit {
                side,
                quantity,
                price,
                bracket,
                cancel_at,
                flat_only,
            } => {
                let mut intent = OrderIntent::limit(side, quantity, price)
                    .with_bracket(bracket)
                    .with_cancel_at(cancel_at);
                if flat_only {
                    intent = intent.only_when_flat();
                }
                venue.submit(intent)
            }
            Self::PlaceStop {
                side,
                quantity,
                trigger,
                bracket,
            } => venue.submit(OrderIntent::stop(side, quantity, trigger).with_bracket(bracket)),
            Self::ModifyOrder { id, price } => venue.amend_price(id, price),
            Self::CancelOrder { id } => venue.cancel(id),
            Self::SetOrderBracket { id, bracket } => {
                venue.amend_bracket(BracketTarget::Order(id), bracket)
            }
            Self::SetBracket {
                stop_loss,
                take_profit,
            } => venue.amend_bracket(
                BracketTarget::Position,
                Bracket::whole(stop_loss, take_profit),
            ),
            Self::ClosePosition => venue.close(CloseAmount::All),
            Self::ClosePartial { quantity } => venue.close(CloseAmount::Partial(quantity)),
            Self::Flatten => venue.flatten(),
        }
    }
}

impl TradingVenue for Simulator {
    fn submit(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        let command = match (intent.kind, intent.price) {
            (EntryKind::Market, _) => Command::PlaceMarket {
                side: intent.side,
                quantity: intent.quantity,
                bracket: intent.bracket,
            },
            (EntryKind::Limit, Some(price)) => Command::PlaceLimit {
                side: intent.side,
                quantity: intent.quantity,
                price,
                bracket: intent.bracket,
                cancel_at: intent.cancel_at,
                flat_only: intent.flat_only,
            },
            (EntryKind::Stop, Some(trigger)) => Command::PlaceStop {
                side: intent.side,
                quantity: intent.quantity,
                trigger,
                bracket: intent.bracket,
            },
            // `OrderIntent`'s constructors cannot build a priceless resting
            // order, so this is unreachable through the public API. It
            // refuses rather than panicking: an intent that arrived over a
            // wire one day must not be able to take a session down.
            (EntryKind::Limit | EntryKind::Stop, None) => {
                return vec![VenueEvent::Rejected(RejectReason::PriceNotPositive)];
            }
        };
        self.apply(command)
    }

    fn amend_price(&mut self, id: OrderId, price: Decimal) -> Vec<VenueEvent> {
        self.apply(Command::ModifyOrder { id, price })
    }

    fn amend_bracket(&mut self, target: BracketTarget, bracket: Bracket) -> Vec<VenueEvent> {
        let command = match target {
            BracketTarget::Position => Command::SetBracket {
                stop_loss: bracket.stop_loss(),
                take_profit: bracket.take_profit(),
            },
            // The whole bracket travels, ladder and all: flattening it to
            // two levels here would make a ladder unreachable through the
            // very port that exists so callers need one vocabulary.
            BracketTarget::Order(id) => Command::SetOrderBracket { id, bracket },
        };
        self.apply(command)
    }

    fn cancel(&mut self, id: OrderId) -> Vec<VenueEvent> {
        self.apply(Command::CancelOrder { id })
    }

    fn close(&mut self, amount: CloseAmount) -> Vec<VenueEvent> {
        self.apply(match amount {
            CloseAmount::All => Command::ClosePosition,
            CloseAmount::Partial(quantity) => Command::ClosePartial { quantity },
        })
    }

    fn flatten(&mut self) -> Vec<VenueEvent> {
        self.apply(Command::Flatten)
    }

    fn on_trade(&mut self, trade: &Trade) -> Vec<VenueEvent> {
        Simulator::on_trade(self, trade)
    }

    fn seed(&mut self, trade: &Trade) {
        Simulator::seed(self, trade);
    }

    fn reset(&mut self) -> Vec<VenueEvent> {
        Simulator::reset(self)
    }

    fn mark_price(&self) -> Option<Decimal> {
        Simulator::mark_price(self)
    }

    fn mark_timestamp_ms(&self) -> Option<i64> {
        Simulator::mark_timestamp_ms(self)
    }

    fn position(&self) -> Option<&Position> {
        Simulator::position(self)
    }

    fn working_orders(&self) -> &[Order] {
        self.orders()
    }

    fn in_flight(&self) -> usize {
        self.queued().len()
    }

    fn in_flight_entries(&self, out: &mut Vec<OrderId>) {
        out.extend(self.queued().iter().filter_map(|action| match action {
            QueuedAction::Entry(order) => Some(order.id),
            QueuedAction::Close | QueuedAction::ClosePartial { .. } => None,
        }));
    }

    fn closed_trades(&self) -> &[ClosedTrade] {
        Simulator::closed_trades(self)
    }

    fn realized_points(&self) -> Decimal {
        Simulator::realized_points(self)
    }
}

#[cfg(test)]
mod tests {
    use quantick_engine::Side;
    use quantick_trading::fake::print_at;

    use super::*;

    fn dec(value: i64) -> Decimal {
        Decimal::from(value)
    }

    /// The port reaches the same simulator the `Command` API does — one
    /// engine behind both doors, never two code paths that can drift.
    #[test]
    fn the_port_places_the_same_order_the_command_api_does() {
        let mut through_port = Simulator::new();
        through_port.seed(&print_at(0, dec(100), 0));
        let events = TradingVenue::submit(
            &mut through_port,
            OrderIntent::limit(Side::Buy, dec(2), dec(95)),
        );
        assert!(matches!(events.as_slice(), [VenueEvent::Placed(_)]));

        let mut through_command = Simulator::new();
        through_command.seed(&print_at(0, dec(100), 0));
        through_command.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: dec(2),
            price: dec(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });

        assert_eq!(through_port.orders(), through_command.orders());
    }

    /// A refusal travels through the port unchanged: the messages are the
    /// curriculum, and a port that swallowed them would teach nothing.
    #[test]
    fn a_refusal_reaches_the_caller_through_the_port() {
        let mut sim = Simulator::new();
        sim.seed(&print_at(0, dec(100), 0));
        let events =
            TradingVenue::submit(&mut sim, OrderIntent::limit(Side::Buy, dec(1), dec(105)));
        assert_eq!(
            events,
            vec![VenueEvent::Rejected(RejectReason::LimitOnWrongSide(
                Side::Buy
            ))]
        );
    }

    /// `BracketTarget` is the whole point of the enum: the same call reaches
    /// a working order's legs or the position's, and never the wrong one.
    #[test]
    fn bracket_target_separates_the_order_from_the_position() {
        let mut sim = Simulator::new();
        sim.seed(&print_at(0, dec(100), 0));
        TradingVenue::submit(&mut sim, OrderIntent::limit(Side::Buy, dec(1), dec(95)));
        let id = sim.orders()[0].id;

        TradingVenue::amend_bracket(
            &mut sim,
            BracketTarget::Order(id),
            Bracket::whole(Some(dec(90)), Some(dec(110))),
        );
        assert_eq!(sim.orders()[0].bracket.stop_loss(), Some(dec(90)));
        // No position exists yet, so the position-targeted call refuses
        // rather than quietly addressing the order.
        let events = TradingVenue::amend_bracket(
            &mut sim,
            BracketTarget::Position,
            Bracket::whole(Some(dec(90)), None),
        );
        assert_eq!(events, vec![VenueEvent::Rejected(RejectReason::NoPosition)]);
    }

    /// `in_flight` counts what neither `position` nor `working_orders`
    /// shows: a market order between the click and the print.
    #[test]
    fn a_market_order_is_in_flight_until_the_next_print() {
        let mut sim = Simulator::new();
        sim.seed(&print_at(0, dec(100), 0));
        TradingVenue::submit(&mut sim, OrderIntent::market(Side::Buy, dec(1)));
        assert_eq!(TradingVenue::in_flight(&sim), 1);
        assert!(!TradingVenue::is_idle(&sim));

        TradingVenue::on_trade(&mut sim, &print_at(1, dec(100), 1_000));
        assert_eq!(TradingVenue::in_flight(&sim), 0);
        assert!(TradingVenue::position(&sim).is_some());
    }

    /// Every command reaches the same state through the port as through
    /// `apply` — the guarantee that makes `dispatch` safe to route the
    /// strategy kernel through.
    #[test]
    fn dispatch_and_apply_agree_on_every_command() {
        let script = |place_through_port: bool| {
            let mut sim = Simulator::new();
            sim.seed(&print_at(0, dec(100), 0));
            fn run(sim: &mut Simulator, through_port: bool, command: Command) {
                if through_port {
                    let _ = command.dispatch(sim as &mut dyn TradingVenue);
                } else {
                    let _ = sim.apply(command);
                }
            }
            run(
                &mut sim,
                place_through_port,
                Command::PlaceLimit {
                    side: Side::Buy,
                    quantity: dec(2),
                    price: dec(95),
                    bracket: Bracket::none(),
                    cancel_at: Some(dec(120)),
                    flat_only: true,
                },
            );
            run(
                &mut sim,
                place_through_port,
                Command::PlaceStop {
                    side: Side::Buy,
                    quantity: dec(1),
                    trigger: dec(105),
                    bracket: Bracket::whole(Some(dec(101)), None),
                },
            );
            run(
                &mut sim,
                place_through_port,
                Command::SetOrderBracket {
                    id: OrderId(0),
                    bracket: Bracket::whole(Some(dec(90)), Some(dec(99))),
                },
            );
            run(
                &mut sim,
                place_through_port,
                Command::ModifyOrder {
                    id: OrderId(0),
                    price: dec(94),
                },
            );
            run(
                &mut sim,
                place_through_port,
                Command::PlaceMarket {
                    side: Side::Buy,
                    quantity: dec(1),
                    bracket: Bracket::none(),
                },
            );
            sim.on_trade(&print_at(1, dec(100), 1_000));
            run(
                &mut sim,
                place_through_port,
                Command::SetBracket {
                    stop_loss: Some(dec(95)),
                    take_profit: Some(dec(115)),
                },
            );
            run(
                &mut sim,
                place_through_port,
                Command::ClosePartial { quantity: dec(1) },
            );
            sim.on_trade(&print_at(2, dec(102), 2_000));
            sim
        };

        let through_port = script(true);
        let through_apply = script(false);
        assert_eq!(through_port.orders(), through_apply.orders());
        assert_eq!(through_port.position(), through_apply.position());
        assert_eq!(
            through_port.closed_trades(),
            through_apply.closed_trades(),
            "the same commands close the same round trips"
        );
    }
}
