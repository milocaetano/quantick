//! The `trade.*` action family: placing, bracketing and cancelling an
//! order, and flattening or reversing the position, as named calls rather
//! than only as chart gestures.
//!
//! `CLAUDE.md`'s *operable without a hand* rule, applied to the one class of
//! capability that had no registry entry at all. Everything the chart's
//! order-entry gestures do now also exists as an action with an actor in its
//! signature, so a hotkey, a test, a control trace and — when the trader
//! decides so — an authorized operator all arrive at the same handler.
//!
//! # Why nothing can invoke these remotely yet
//!
//! Every action here sits behind its own effect and its own permission,
//! ceilinged at a `trader` profile that **nothing hands out**: the access
//! panel does not offer the scope and `configured_profile` never returns
//! that profile, so no connection can reach one. (The ceiling itself is not
//! the gate — a permission with no ceiling is not even representable.) Not
//! an oversight:
//! `annotate`'s own description promises it "never [affects] a position", so
//! a trade cannot borrow it, and inventing a profile that may trade is a
//! decision about a real account rather than a detail of this change. Until
//! such a profile exists the gateway refuses these before dispatch, exactly
//! as it refuses any capability outside a connection's ceiling — while the
//! in-process operator (a hotkey, the harness hooks, a deterministic test)
//! reaches them normally.
//!
//! The orders themselves are still simulated, and every surface still says
//! `SIM`. What is real here is the *shape*: when a broker implements
//! `quantick_trading::TradingVenue`, these actions reach it unchanged, and
//! the permission that guards them is already carved out.
//!
//! The family reaches the desk through the window's [`PaperOwner`], lent by
//! [`PaperHost`]: the active tab's paper desk as [`TradeDesk`] — the account,
//! the ruler and the one `execute` the tab's own buttons trade through — and
//! the persistence a ticket edit makes. The trade rules (armed bracket, risk
//! lock, ruler) are the tab's, in `quantick_session::paper`; what is here is
//! parsing, refusals and the journal.

pub use quantick_control_host::authority::CAPABILITY_VERSION;
pub use quantick_control_schema::trade::*;

use quantick_civil::TzOffset;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_engine::Side;
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::{PaperAccount, format::PositionSummary, risk_sizing::RiskState};
use quantick_session::paper::{SharedChange, TradeCommand, TradeOutcome, TradeRefusal};
use quantick_sim::{Bracket, EntryKind, Order, OrderId, OrderIntent, VenueEvent};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::dock::ActionDock;

pub use quantick_control_host::authority::{TRADE_MODULE_ID, TRADE_PERMISSION_ID};

/// The active tab's paper desk as a trade action reaches it: the account the
/// orders rest in, the ruler that shapes one order, and the bracket the
/// ticket has armed.
pub trait TradeDesk {
    /// Install a validated report through the same model transition as the context menu.
    fn show_report(&mut self, report: quantick_paper::report::scope::PreparedReport);
    /// The simulated account.
    fn account(&self) -> &PaperAccount;
    /// The simulated account, to place into.
    fn account_mut(&mut self) -> &mut PaperAccount;
    /// How many ticks the ruler is set to.
    fn ruler_ticks(&self) -> u32;
    /// Set the ruler to `ticks`.
    fn set_ruler_ticks(&mut self, ticks: u32);
    /// Run one trade command exactly as the tab's own surfaces run it:
    /// `quantick_session::paper::TabPaper::execute`.
    fn execute(&mut self, command: TradeCommand) -> TradeOutcome;
    /// The last mark price, if the account has seen a print.
    fn mark_price(&self) -> Option<Decimal>;
    /// The resting orders.
    fn working_orders(&self) -> &[Order];
    /// The risk sizing's state and whether it blocks an entry, asked once.
    fn risk_report(&self) -> (RiskState, bool);
    /// The open position, as the desk's own surfaces summarise it.
    fn position_summary(&self) -> Option<PositionSummary>;
    /// Which closed-trade row the trader selected in the ledger.
    fn selected_trade_index(&self) -> Option<usize>;
    /// Apply `change` to the desk's Trades ledger — an empty one only reads —
    /// and answer what it shows and whether that moved.
    fn ledger_view(&mut self, change: &LedgerViewChange) -> (LedgerView, bool);
}

/// The workspace's paper trading as the trade, P&L and journal calls reach
/// it: the active tab's desk, the settings a ticket edit persists, the route
/// orders take, and the trades journal every tab writes into.
pub trait PaperOwner {
    /// The active tab's desk, `None` when no chart is open.
    fn active_desk(&self) -> Option<&dyn TradeDesk>;
    /// The active tab's desk, to act on.
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk>;
    /// Persist the active desk's copy of one shared setting and fan it out
    /// to every tab.
    fn persist(&mut self, change: SharedChange);
    /// The trades folder and every file a tab is still journaling to.
    fn journal_scope(&self) -> JournalScope;
    /// The display timezone the ledger stamps civil days and labels with.
    fn journal_timezone(&self) -> TzOffset;
    /// Apply one edit exactly as the ledger's gesture does, and have every
    /// open ledger and report re-read the folder.
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError>;
    /// Where orders go: the route session joined with the tabs it routes
    /// for, `None` where the host keeps no route. The three calls below are
    /// answered through it.
    fn route_call(&mut self) -> Option<quantick_paper::route::RouteCall<'_>> {
        None
    }
    /// Where orders go and the account list, as the Trading panel shows
    /// them; `None` where the host keeps no route.
    fn account_route(
        &mut self,
    ) -> Option<quantick_control_schema::trade_account::AccountRouteResult> {
        let read = self.route_call()?.read();
        Some(crate::trade_account::route_result(&read))
    }
    /// Ask the route for what `request` names.
    ///
    /// # Errors
    ///
    /// The route's refusal.
    fn request_route(
        &mut self,
        request: quantick_paper::route::RouteRequest,
    ) -> Result<(), quantick_paper::route::Refusal> {
        self.route_call()
            .ok_or(quantick_paper::route::Refusal::BrokerOff)?
            .request(request)
    }
    /// Why an order from `actor` is refused on the route in force, if it is.
    fn broker_write_block(
        &mut self,
        actor: quantick_paper::route::Actor,
    ) -> Option<quantick_paper::route::Refusal> {
        self.route_call()?.write_block(actor)
    }
}

/// The window lends its [`PaperOwner`] for one call.
pub trait PaperHost {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_>;
}

/// Turn the venue's answer into the result, whatever the call was.
fn answer(paper: &dyn PaperOwner, events: &[VenueEvent]) -> TradeResult {
    let rejected_because = events.iter().find_map(|event| match event {
        VenueEvent::Rejected(reason) => Some(reason.to_string()),
        _ => None,
    });
    let order_id = events.iter().find_map(|event| match event {
        VenueEvent::Placed(order) | VenueEvent::Updated(order) => Some(order.id.0),
        VenueEvent::Cancelled { order, .. } => Some(order.id.0),
        _ => None,
    });
    let desk = paper.active_desk();
    TradeResult {
        selected_strategy: desk
            .and_then(|desk| desk.account().selected_order_strategy())
            .map(|strategy| strategy.name.clone()),
        ruler_ticks: desk.map_or(0, TradeDesk::ruler_ticks),
        accepted: rejected_because.is_none(),
        rejected_because,
        order_id,
        mark_price: desk
            .and_then(TradeDesk::mark_price)
            .map(|price| price.to_string()),
        working_orders: desk
            .map(TradeDesk::working_orders)
            .unwrap_or_default()
            .iter()
            .map(|order| WorkingOrderView {
                order_id: order.id.0,
                side: match order.side {
                    Side::Buy => "buy".to_owned(),
                    Side::Sell => "sell".to_owned(),
                },
                kind: order.kind.as_str().to_owned(),
                price: order.price.map(|price| price.to_string()),
                quantity: order.quantity.to_string(),
                stop_loss: order.bracket.stop_loss().map(|level| level.to_string()),
                take_profit: order.bracket.take_profit().map(|level| level.to_string()),
            })
            .collect(),
        simulated: true,
    }
}

/// Append one journal event naming who asked and what the venue answered.
///
/// Every `trade.*` action records, accepted or refused, and the actor rides
/// in the event rather than beside it. An order placed by an operator that
/// looked exactly like one the trader placed would be the authorship half of
/// the honesty contract quietly dropped — and a refusal is worth recording
/// too, since "the agent tried to buy here and was told no" is precisely the
/// line a trader reviewing a session wants to find.
fn journal<A: JournalAccess>(
    access: &mut A,
    actor: &ActorContext,
    kind: &str,
    result: &TradeResult,
    asked: Value,
) {
    let payload = json!({
        "asked": asked,
        "accepted": result.accepted,
        "rejected_because": result.rejected_because,
        "order_id": result.order_id,
        "simulated": true,
    });
    access.record_event(NewEvent::by(TRADE_MODULE_ID, kind, actor, payload));
}

fn place_order<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: PlaceInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let quantity = parse_price("quantity", &input.quantity)?;
    let side = input.side.into_engine();
    let price = input
        .price
        .as_deref()
        .map(|text| parse_price("price", text))
        .transpose()?;
    let intent = match (input.kind.into_engine(), price) {
        (EntryKind::Market, None) => OrderIntent::market(side, quantity),
        (EntryKind::Limit, Some(price)) => OrderIntent::limit(side, quantity, price),
        (EntryKind::Stop, Some(trigger)) => OrderIntent::stop(side, quantity, trigger),
        (EntryKind::Market, Some(_)) => {
            return Err(ControlError::invalid_request(
                "a market order has no price of its own - drop `price`, or ask for a limit or a stop"
                    .to_owned(),
            ));
        }
        (EntryKind::Limit | EntryKind::Stop, None) => {
            return Err(ControlError::invalid_request(
                "a limit or stop needs the price it rests at".to_owned(),
            ));
        }
    };
    let named = Bracket::whole(
        input
            .stop_loss
            .as_deref()
            .map(|text| parse_price("stop_loss", text))
            .transpose()?,
        input
            .take_profit
            .as_deref()
            .map(|text| parse_price("take_profit", text))
            .transpose()?,
    );
    // The armed bracket, the risk per trade and the ruler are the tab's
    // decisions, made where a click makes them. A refusal comes back as an
    // error carrying the sentence the ticket shows, never as an empty
    // answer the caller has to interpret.
    let command = TradeCommand::Place {
        intent: Box::new(intent.with_bracket(named)),
    };
    run(paper, access, actor, command, PLACE_EVENT_KIND, asked)
}

/// Run `command` on the active desk, refuse what the tab refused, and
/// journal what the venue answered under `kind`.
fn run<A: JournalAccess>(
    paper: &mut dyn PaperOwner,
    access: &mut A,
    actor: &ActorContext,
    command: TradeCommand,
    kind: &str,
    asked: Value,
) -> Result<Value, ControlError> {
    // A broker order from anyone but the trader's hand is the config's to
    // allow; refused before it reaches the desk, and journaled as asked.
    if let Some(refusal) = paper.broker_write_block(crate::trade_account::actor_of(actor)) {
        return Err(crate::trade_account::refusal_error(refusal));
    }
    let outcome = paper
        .active_desk_mut()
        .ok_or_else(no_chart_open)?
        .execute(command);
    if let Some(refusal) = outcome.refusal {
        return Err(match refusal {
            TradeRefusal::Paused(gate) => paused_error(&gate),
            TradeRefusal::Ticket(sentence) => ControlError::invalid_request(sentence),
        });
    }
    let result = answer(paper, &outcome.events);
    journal(access, actor, kind, &result, asked);
    to_value(result)
}

/// An entry refused while the chart is out of sync with its feed. Nothing
/// was sent, so the same call is safe to retry once it catches up; the
/// details name the reading, as the `health` scope's `trade_gate` does.
fn paused_error(gate: &quantick_sources::sync::Desync) -> ControlError {
    let code = quantick_control::error::codes::CAPABILITY_UNAVAILABLE;
    let mut error = quantick_control_host::admission::known_error(code, gate.sentence(), true);
    error.context.details = Some(json!({
        "trade_gate": gate.reading.word(),
        "link": gate.link.word(),
        "latency_ms": gate.latency_ms,
        "tape_age_ms": gate.tape_age_ms,
    }));
    error.context.next_steps = vec![format!(
        "retry once the chart catches up, or call {} first",
        quantick_control_schema::recovery::RECONNECT_CAPABILITY_ID
    )];
    error
}

fn bracket_order<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: BracketInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let bracket = Bracket::whole(
        input
            .stop_loss
            .as_deref()
            .map(|text| parse_price("stop_loss", text))
            .transpose()?,
        input
            .take_profit
            .as_deref()
            .map(|text| parse_price("take_profit", text))
            .transpose()?,
    );
    let command = TradeCommand::Amend {
        id: OrderId(input.order_id),
        bracket: Box::new(bracket),
    };
    run(paper, access, actor, command, BRACKET_EVENT_KIND, asked)
}

fn cancel_order<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: CancelInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let command = TradeCommand::Cancel {
        id: OrderId(input.order_id),
    };
    run(paper, access, actor, command, CANCEL_EVENT_KIND, asked)
}

/// Close the position and cancel every working order in one call.
fn flatten_position<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let _: PositionInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let asked = input.clone();
    run(
        paper,
        access,
        actor,
        TradeCommand::Flatten,
        FLATTEN_EVENT_KIND,
        asked,
    )
}

/// Flip the open position in one call; refused while flat, by the tab,
/// because a reversal of nothing is a caller working from a stale read.
fn reverse_position<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let _: PositionInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let asked = input.clone();
    run(
        paper,
        access,
        actor,
        TradeCommand::Reverse,
        REVERSE_EVENT_KIND,
        asked,
    )
}

fn select_strategy<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: SelectStrategyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let desk = paper.active_desk_mut().ok_or_else(no_chart_open)?;
    let strategies = desk.account().order_strategies().to_vec();
    if let Some(name) = input.name.as_deref()
        && !strategies.iter().any(|strategy| strategy.name == name)
    {
        return Err(ControlError::invalid_request(format!(
            "no exit strategy is named `{name}`"
        )));
    }
    desk.account_mut()
        .set_order_strategies(strategies, input.name.as_deref());
    paper.persist(SharedChange::OrderStrategies);
    let result = answer(paper, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_ruler<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: SetRulerInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    paper
        .active_desk_mut()
        .ok_or_else(no_chart_open)?
        .set_ruler_ticks(input.ticks);
    let result = answer(paper, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_risk<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: SetRiskInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let edit = crate::ticket_money::risk_edit(&input)?;
    paper
        .active_desk_mut()
        .ok_or_else(no_chart_open)?
        .account_mut()
        .apply_risk_edit(edit);
    paper.persist(SharedChange::RiskSettings);
    let result = answer(paper, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_instrument_money<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let asked = input.clone();
    let input: SetInstrumentMoneyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let desk = paper.active_desk_mut().ok_or_else(no_chart_open)?;
    crate::ticket_money::declare_money(desk.account_mut(), &input)?;
    paper.persist(SharedChange::RiskSettings);
    let result = answer(paper, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

/// Dock the family. One call from the window's action registry, nothing
/// else opens.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: PaperHost + ?Sized,
    A: JournalAccess,
{
    registry.register(select_strategy_descriptor(), select_strategy::<H, A>)?;
    registry.register(set_ruler_descriptor(), set_ruler::<H, A>)?;
    registry.register(set_risk_descriptor(), set_risk::<H, A>)?;
    registry.register(
        set_instrument_money_descriptor(),
        set_instrument_money::<H, A>,
    )?;
    registry.register(place_descriptor(), place_order::<H, A>)?;
    registry.register(bracket_descriptor(), bracket_order::<H, A>)?;
    registry.register(cancel_descriptor(), cancel_order::<H, A>)?;
    registry.register(flatten_descriptor(), flatten_position::<H, A>)?;
    registry.register(reverse_descriptor(), reverse_position::<H, A>)?;
    crate::trade_account::register(registry)
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod trade_tests;
