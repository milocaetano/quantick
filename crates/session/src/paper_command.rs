//! Every paper-trading action as one command, and the one place that runs it.
//!
//! A toolbar press, a menu shortcut, the position card, an order row's ✕ and
//! a named control call all used to compose the trade rules themselves: which
//! bracket an entry carries, whether the risk per trade refuses it, whether
//! the ruler is put away, what the venue answered. [`TabPaper::execute`] owns
//! those decisions now, so every surface asks for the same [`TradeCommand`]
//! and gets the same [`TradeOutcome`].

use std::borrow::BorrowMut;

use quantick_engine::Side;
use quantick_paper::PaperAccount;
use quantick_sim::{Bracket, OrderId, OrderIntent, VenueEvent};

use super::TabPaper;

/// Why a reversal of no position is refused.
pub const NOTHING_TO_REVERSE: &str = "the account is flat: there is no position to reverse";

/// One paper-trading action, said once for every surface. The two variants
/// that carry a ladder box it, so a bare press stays a small value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeCommand {
    /// Enter at the market with the ticket's quantity and offsets, sized by
    /// the risk per trade — the toolbar's BUY/SELL.
    Market {
        /// Which way to enter.
        side: Side,
    },
    /// Place one order stated in full — the named call's entry.
    Place {
        /// The order. Its bracket is the protection the caller named; an
        /// empty one takes the bracket the ticket has armed, as a click does.
        intent: Box<OrderIntent>,
    },
    /// Replace a working order's protective prices.
    Amend {
        /// The working order.
        id: OrderId,
        /// Its new protection.
        bracket: Box<Bracket>,
    },
    /// Remove one working order without trading.
    Cancel {
        /// The working order.
        id: OrderId,
    },
    /// Remove every working order without trading.
    CancelAll,
    /// Exit the open position at the market.
    Close,
    /// Flip the open position in one market order.
    Reverse,
    /// Exit the open position and remove every working order.
    Flatten,
}

/// What one [`TradeCommand`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[must_use]
pub struct TradeOutcome {
    /// What the venue answered, in order; empty when nothing reached it.
    pub events: Vec<VenueEvent>,
    /// The sentence that refused the command before it reached the venue —
    /// the same words the trader's toast carries.
    pub refusal: Option<String>,
}

impl TradeOutcome {
    fn sent(events: Vec<VenueEvent>) -> Self {
        Self {
            events,
            refusal: None,
        }
    }

    fn refused(sentence: String) -> Self {
        Self {
            events: Vec::new(),
            refusal: Some(sentence),
        }
    }
}

impl<A: BorrowMut<PaperAccount>> TabPaper<A> {
    /// Run one trade command: the bracket choice, the risk refusal, the
    /// ruler clear and the venue's answer, the same for every caller.
    pub fn execute(&mut self, command: TradeCommand) -> TradeOutcome {
        match command {
            TradeCommand::Market { side } => self.market_entry(side),
            TradeCommand::Place { intent } => self.place(*intent),
            TradeCommand::Amend { id, bracket } => {
                TradeOutcome::sent(self.core_mut().set_order_bracket(id, *bracket))
            }
            TradeCommand::Cancel { id } => TradeOutcome::sent(self.core_mut().cancel_order(id)),
            TradeCommand::CancelAll => TradeOutcome::sent(self.core_mut().cancel_all_orders()),
            TradeCommand::Close => TradeOutcome::sent(self.core_mut().close_position()),
            TradeCommand::Reverse => self.reverse(),
            TradeCommand::Flatten => TradeOutcome::sent(self.core_mut().flatten()),
        }
    }

    /// [`Self::execute`] for a surface whose answer the toast already
    /// carries.
    pub fn send(&mut self, command: TradeCommand) {
        let _ = self.execute(command);
    }

    /// The ruler's distance was for this trade, so an accepted entry puts it
    /// away; a refused one was no trade, and the distance stays for the retry.
    fn market_entry(&mut self, side: Side) -> TradeOutcome {
        let reference = self.core().mark_price().unwrap_or_default();
        let Some(ticket) = self.parse_bracket(side, reference) else {
            return self.refused_by_toast();
        };
        let env = self.account_env(side, reference);
        match self.core_mut().market_events(side, reference, ticket, &env) {
            Some(events) => {
                self.desk.ruler.clear();
                TradeOutcome::sent(events)
            }
            None => self.refused_by_toast(),
        }
    }

    /// Levels the caller named win; otherwise the order takes what the
    /// ticket is set to, as a click does. A refusal posts nothing: the caller
    /// reads the sentence from the outcome.
    fn place(&mut self, intent: OrderIntent) -> TradeOutcome {
        let named = intent.bracket;
        let bracket = if named.is_empty() {
            let reference = intent
                .price
                .or_else(|| self.core().mark_price())
                .unwrap_or_default();
            self.armed_bracket(intent.side, reference, intent.quantity)
        } else {
            named
        };
        match self
            .core_mut()
            .try_place_intent(intent.with_bracket(bracket))
        {
            Ok(events) => {
                // The ruler shaped this one order; the next starts bare.
                self.desk.ruler.clear();
                TradeOutcome::sent(events)
            }
            Err(refusal) => TradeOutcome::refused(refusal.sentence()),
        }
    }

    /// The account says which way and at what mark; the ticket resolves the
    /// protection for that side, because the offsets are its text. Flat, it
    /// is refused without a toast: a press on nothing stays silent, and a
    /// named caller learns its read went stale.
    fn reverse(&mut self) -> TradeOutcome {
        let Some((side, reference)) = self.core().reverse_aim() else {
            return TradeOutcome::refused(NOTHING_TO_REVERSE.to_owned());
        };
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return self.refused_by_toast();
        };
        match self.core_mut().try_reverse_position(bracket) {
            Ok(events) => TradeOutcome::sent(events),
            Err(refusal) => {
                let sentence = refusal.sentence();
                self.show_toast(format!("SIM: {sentence}"));
                TradeOutcome::refused(sentence)
            }
        }
    }

    /// A refusal the account already posted as the toast, answered with the
    /// same words.
    fn refused_by_toast(&self) -> TradeOutcome {
        TradeOutcome::refused(self.core().peek_toast().cloned().unwrap_or_default())
    }
}
