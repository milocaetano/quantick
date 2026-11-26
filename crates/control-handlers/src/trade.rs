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
//! lock, ruler) and each call's input and answer are the tab's, in
//! `quantick_session::paper`; what is here is decoding, refusals and the
//! journal.

pub use quantick_control_host::authority::CAPABILITY_VERSION;
pub use quantick_control_schema::trade::*;

use quantick_civil::TzOffset;
use quantick_control::{
    error::ControlError,
    registry::{CapabilityDescriptor, RegistryError},
    wire::ActorContext,
};
use quantick_control_host::journal::NewEvent;
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::{PaperAccount, format::PositionSummary, risk_sizing::RiskState};
use quantick_session::paper::{SharedChange, TradeCommand, TradeOutcome, TradeRefusal};
use quantick_sim::{Order, VenueEvent};
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
    /// Restore the ordinary report for the chart symbol.
    fn reset_report(&mut self);
    fn report_change(
        &mut self,
        change: Option<quantick_paper::report::snapshot::ReportChange>,
        tz: TzOffset,
    ) -> quantick_paper::report::snapshot::ReportReadout;
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

/// The active desk's answer to `events`, whatever the call was.
fn answer(paper: &dyn PaperOwner, events: &[VenueEvent]) -> Result<TradeResult, ControlError> {
    let desk = paper.active_desk().ok_or_else(no_chart_open)?;
    Ok(TradeResult::of(desk.account(), desk.ruler_ticks(), events))
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

/// Every order and position call, one adapter: decode the owner's input,
/// take the command it states, and run it where a click runs it.
fn trade_call<C: TradeCall, H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let mut owner = host.paper();
    let call: C = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let command = call.command().map_err(ControlError::invalid_request)?;
    run(&mut *owner, access, actor, command, C::EVENT_KIND, input.clone())
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
    let result = answer(paper, &outcome.events)?;
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
    let result = answer(paper, &[])?;
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
    let result = answer(paper, &[])?;
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
    let result = answer(paper, &[])?;
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
    let result = answer(paper, &[])?;
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

/// Dock one order or position call: the schema it publishes is the one its
/// adapter decodes, so the two cannot disagree.
fn dock<C, D, H, A>(registry: &mut D, descriptor: CapabilityDescriptor) -> Result<(), RegistryError>
where
    C: TradeCall,
    D: ActionDock<H, A>,
    H: PaperHost + ?Sized,
    A: JournalAccess,
{
    let descriptor = CapabilityDescriptor {
        input_schema: quantick_control::schema::generated_schema::<C>(),
        ..descriptor
    };
    registry.register(descriptor, trade_call::<C, H, A>)
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
    dock::<PlaceInput, _, _, _>(registry, place_descriptor())?;
    dock::<BracketInput, _, _, _>(registry, bracket_descriptor())?;
    dock::<CancelInput, _, _, _>(registry, cancel_descriptor())?;
    dock::<FlattenInput, _, _, _>(registry, flatten_descriptor())?;
    dock::<ReverseInput, _, _, _>(registry, reverse_descriptor())?;
    crate::trade_account::register(registry)
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod trade_tests;
