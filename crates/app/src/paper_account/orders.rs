//! Placing, amending and cancelling what the account has working.
//!
//! The venue-facing half: every command the ticket can issue goes through
//! [`PaperAccount::dispatch`] here, and every bracket amendment resolves its
//! owner here too. The prices these methods place are computed in
//! [`super::risk`]; this file is about getting them to the simulator and
//! back.

use quantick_engine::Side;
use quantick_sim::{Bracket, BracketTarget, Command, EntryKind, OrderId, OrderIntent, VenueEvent};
use rust_decimal::Decimal;

use super::{AccountEnv, Leg, PaperAccount};

impl PaperAccount {
    /// Rest an entry at `price`, protected by `ticket`. Answers whether
    /// anything is actually resting, which is what the caller paints.
    pub(crate) fn place_resting(
        &mut self,
        side: Side,
        kind: EntryKind,
        price: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> bool {
        let Some((quantity, bracket)) = self.entry_size(side, price, ticket, env) else {
            return false;
        };
        let command = match kind {
            EntryKind::Limit => Command::PlaceLimit {
                side,
                quantity,
                price,
                bracket,
                cancel_at: None,
                flat_only: false,
            },
            EntryKind::Stop => Command::PlaceStop {
                side,
                quantity,
                trigger: price,
                bracket,
            },
            // Market never rests; the buttons fire it directly.
            EntryKind::Market => return false,
        };
        let events = self.dispatch(command);
        let placed = events
            .iter()
            .any(|event| matches!(event, VenueEvent::Placed(_)));
        self.handle_events(events);
        placed
    }

    /// The side and mark a reversal would be taken at, or `None` when flat.
    /// Split out because the caller must know the side *before* it can
    /// resolve the protection for it.
    pub(crate) fn reverse_aim(&self) -> Option<(Side, Decimal)> {
        let position = self.venue.position()?;
        let side = match position.side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        Some((side, self.venue.mark_price().unwrap_or_default()))
    }

    /// Turn the open position around: close it and take the same size the
    /// other way, in one order, protected by `bracket`.
    pub(crate) fn reverse_position(&mut self, bracket: Bracket) {
        let (Some(position), Some((side, _))) =
            (self.venue.position().cloned(), self.reverse_aim())
        else {
            return;
        };
        let events = self.venue.submit(
            OrderIntent::market(side, position.quantity.saturating_add(position.quantity))
                .with_bracket(bracket),
        );
        self.handle_events(events);
    }

    /// Place one order, stated in full — the control plane's entry point,
    /// and the shape a hotkey or a hook uses too.
    ///
    /// Unlike the chart's aim this takes the kind rather than inferring it:
    /// a caller with no pointer has no "where I am relative to the mark" to
    /// infer from, and an action whose meaning depends on the market at the
    /// instant it lands is an action nobody can replay.
    pub(crate) fn place_intent(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        // The risk per trade is a ceiling on the account, not on the mouse.
        // A named call is exactly the operator `CLAUDE.md` treats as
        // first-class, so a lock the ticket enforces and this path does not
        // would be a ceiling that holds only while a human is clicking.
        if let Some(refusal) = self.risk_refusal_for(&intent) {
            // Nothing is fabricated onto the venue's own event stream: the
            // lock is this application's policy, not a fact the venue
            // reported, and a `RejectReason` variant for it would put that
            // policy inside the domain crate. The trader gets the toast; a
            // named caller gets the same sentence as an error, because
            // `control::trade` asks this same function first.
            self.set_toast(format!("SIM: {refusal}"));
            return Vec::new();
        }
        let events = self.venue.submit(intent);
        self.handle_events(events.clone());
        events
    }

    /// Replace a working order's protective prices — the chart's drag, said
    /// in words.
    pub(crate) fn set_order_bracket(&mut self, id: OrderId, bracket: Bracket) -> Vec<VenueEvent> {
        let events = self.venue.amend_bracket(BracketTarget::Order(id), bracket);
        self.handle_events(events.clone());
        events
    }

    /// Remove one working order without trading.
    pub(crate) fn cancel_order(&mut self, id: OrderId) -> Vec<VenueEvent> {
        let events = self.venue.cancel(id);
        self.handle_events(events.clone());
        events
    }

    /// Hand one [`Command`] to the attached venue.
    ///
    /// The chart's own gestures build [`OrderIntent`]s and call the port
    /// directly; this exists for the callers that already speak `Command`
    /// — the strategy kernel, the scripted demo, and the tests that drive
    /// this host the way the kernel does.
    pub(crate) fn dispatch(&mut self, command: Command) -> Vec<VenueEvent> {
        command.dispatch(self.venue.as_mut())
    }

    /// What a bracket owner looks like to every gesture in this module:
    /// the side it trades, the price its legs are judged against, the
    /// bracket it carries today, and the size that turns a level into
    /// points.
    ///
    /// The position's reference is its average entry; a working order's is
    /// its own resting price. That is the same reference the venue
    /// validates against, so a leg the chart lets you drop is a leg the
    /// venue accepts — the two never disagree about which side of the
    /// entry is protective.
    pub(crate) fn bracket_owner(
        &self,
        owner: BracketTarget,
    ) -> Option<(Side, Decimal, Bracket, Decimal)> {
        match owner {
            BracketTarget::Position => self.venue.position().map(|position| {
                (
                    position.side,
                    position.avg_price,
                    self.position_bracket(position),
                    position.quantity,
                )
            }),
            BracketTarget::Order(id) => self
                .venue
                .working_orders()
                .iter()
                .find(|order| order.id == id)
                .and_then(|order| {
                    order
                        .price
                        .map(|price| (order.side, price, order.bracket, order.quantity))
                }),
        }
    }

    /// Set or clear one protective leg, keeping the other — the tag cross's
    /// command and the drop of a leg drag, which are the same amendment.
    pub(crate) fn amend_leg(&mut self, owner: BracketTarget, leg: Leg, level: Option<Decimal>) {
        let Some((.., bracket, _)) = self.bracket_owner(owner) else {
            return;
        };
        let events = self.venue.amend_bracket(owner, leg.applied(bracket, level));
        self.handle_events(events);
    }

    /// Everything that can carry a bracket right now, in the order a press
    /// should consider it — which is **the reverse of the paint**, topmost
    /// first.
    ///
    /// `draw_layer` paints the working orders and then the position, so the
    /// position's lines and tag sit on top of them. A press has to resolve
    /// the same way or the two disagree wherever they overlap: a position
    /// stopped at 90 and a resting entry whose own stop is also 90 show the
    /// position's solid leg, and a hit-test that reached the order first
    /// would clear the entry's protection while the trader was looking at
    /// the position's.
    ///
    /// An iterator and not a `Vec`, because both callers run **per frame**,
    /// not per press: `hover_cursor` asks `control_at` and `line_at` what is
    /// under the pointer on every frame the hand is over the chart, and
    /// `compute_cmd_preview` asks both again. A vector here was four small
    /// allocations a frame for a list that is usually empty. Everything it
    /// borrows is borrowed immutably, so a caller can walk it while asking
    /// `self` about each entry.
    pub(crate) fn bracket_owners(&self) -> impl Iterator<Item = BracketTarget> + '_ {
        self.venue
            .position()
            .is_some()
            .then_some(BracketTarget::Position)
            .into_iter()
            .chain(
                self.venue
                    .working_orders()
                    .iter()
                    .rev()
                    // A protective leg is not an entry: it *is* protection,
                    // and offering it a bracket of its own would put SL/TP
                    // handles beside a rung the trader is already using as
                    // one. The venue refuses such an amendment anyway; the
                    // chart must not offer the gesture that earns a refusal.
                    .filter(|order| !order.is_protective())
                    .map(|order| BracketTarget::Order(order.id)),
            )
    }

    /// Set one rung's leg on a resting entry, or clear it with `None`.
    ///
    /// Every other rung is carried through untouched, and the named
    /// strategy is never written to: an order on the chart is the trader's,
    /// and the ladder that shaped it is a template they can still reuse.
    /// A rung left protecting nothing is dropped rather than rested empty.
    pub(crate) fn amend_rung(
        &mut self,
        order: OrderId,
        index: usize,
        leg: Leg,
        level: Option<Decimal>,
    ) {
        let Some(entry) = self
            .venue
            .working_orders()
            .iter()
            .find(|working| working.id == order)
        else {
            return;
        };
        let mut parts: Vec<quantick_sim::ExitPart> = entry.bracket.parts().copied().collect();
        let Some(part) = parts.get_mut(index) else {
            return;
        };
        match leg {
            Leg::StopLoss => part.stop_loss = level,
            Leg::TakeProfit => part.take_profit = level,
        }
        parts.retain(|part| !part.is_empty());
        let Ok(bracket) = Bracket::ladder(&parts) else {
            return;
        };
        let events = self
            .venue
            .amend_bracket(BracketTarget::Order(order), bracket);
        self.handle_events(events);
    }

    /// Cancel every working order (resting and queued), trading nothing.
    pub(crate) fn cancel_all_orders(&mut self) {
        let mut ids: Vec<OrderId> = self
            .venue
            .working_orders()
            .iter()
            .map(|order| order.id)
            .collect();
        self.venue.in_flight_entries(&mut ids);
        for id in ids {
            let events = self.venue.cancel(id);
            self.handle_events(events);
        }
    }

    /// The strategies the trader keeps, in their own order.
    pub(crate) fn order_strategies(&self) -> &[crate::order_strategies::OrderStrategy] {
        &self.strategies
    }

    /// The strategy the ticket is set to, if any.
    pub(crate) fn selected_order_strategy(
        &self,
    ) -> Option<&crate::order_strategies::OrderStrategy> {
        self.strategies.get(self.selected_strategy?)
    }

    /// Replace the kept strategies and the selection, by name.
    ///
    /// A name this build no longer knows selects nothing: telling the trader
    /// their strategy is gone beats silently arming a different one.
    pub(crate) fn set_order_strategies(
        &mut self,
        strategies: Vec<crate::order_strategies::OrderStrategy>,
        selected: Option<&str>,
    ) {
        self.selected_strategy =
            selected.and_then(|name| strategies.iter().position(|item| item.name == name));
        self.strategies = strategies;
    }

    /// Apply a strategy-issued command through the same funnel manual
    /// orders use — journal, toasts, everything — and hand the simulator's
    /// immediate answer back for the instance to attribute.
    pub(crate) fn apply_strategy_command(&mut self, command: Command) -> Vec<VenueEvent> {
        let events = self.dispatch(command);
        self.handle_events(events.clone());
        events
    }
}
