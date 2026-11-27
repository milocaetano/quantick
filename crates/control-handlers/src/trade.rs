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
use quantick_control_host::actions::ReplayTrace;
use quantick_control_host::journal::NewEvent;
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::{PaperAccount, format::PositionSummary, risk_sizing::RiskState};
use quantick_session::paper::{
    BRACKET_EVENT_KIND, BracketInput, CANCEL_EVENT_KIND, CancelInput, FLATTEN_EVENT_KIND,
    FlattenInput, PLACE_EVENT_KIND, PlaceInput, REVERSE_EVENT_KIND, ReverseInput, SharedChange,
    TradeCommand, TradeOutcome, TradeRefusal, TradeResult,
};
use quantick_sim::{Order, VenueEvent};
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::call::{Call, CallCx, CallSpec, dock_call};
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
/// Every ticket call and, through [`Call::entry`], every order call records,
/// accepted or refused by the venue, and the actor rides
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
    let payload = entry(asked, &json!(result));
    access.record_event(NewEvent::by(TRADE_MODULE_ID, kind, actor, payload));
}

/// The journal payload: what was asked and what the venue answered, from the
/// encoded [`TradeResult`].
fn entry(asked: Value, answer: &Value) -> Value {
    json!({
        "asked": asked,
        "accepted": answer["accepted"],
        "rejected_because": answer["rejected_because"],
        "order_id": answer["order_id"],
        "simulated": true,
    })
}

/// Run `command` on the active desk and refuse what the tab refused.
fn run(
    paper: &mut dyn PaperOwner,
    actor: &ActorContext,
    command: TradeCommand,
) -> Result<TradeResult, ControlError> {
    // A broker order from anyone but the trader's hand is the config's to
    // allow; refused before it reaches the desk.
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
    answer(paper, &outcome.events)
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

/// Every order and position call runs the command its input states where a
/// click runs it.
fn order<C, H: PaperHost + ?Sized>(
    call: C,
    host: &mut H,
    actor: &ActorContext,
) -> Result<TradeResult, ControlError>
where
    TradeCommand: TryFrom<C, Error = String>,
{
    let command = TradeCommand::try_from(call).map_err(ControlError::invalid_request)?;
    run(&mut *host.paper(), actor, command)
}

/// The order and position calls' posture: the family's, with the two fields
/// each call states.
fn posture(spec: &CallSpec, risk_reducing: bool, stale_input_safety: &str) -> CapabilityDescriptor {
    descriptor(
        spec.id,
        spec.title,
        spec.description,
        Value::Null,
        risk_reducing,
        stale_input_safety,
    )
}

impl<H: PaperHost + ?Sized> Call<H> for PlaceInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: PLACE_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Place a simulated order",
        description: "Places one market, limit or stop order on the charted symbol, optionally with the protective stop and target that arm when it fills. The kind is stated, never inferred from where the market happens to be - a kind that cannot rest at the given price is refused with the reason, in the venue's own words.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(PLACE_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            posture(
                spec,
                false,
                "Every price is judged against the market at the moment the order lands, not at the moment it was written: a limit that would fill at once, or a stop that would trigger at once, is refused with the reason. A caller working from a stale chart can therefore place an order at a level that has gone stale, but never one the venue would not have accepted from a caller reading the live tape.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        order(self, host, cx.actor)
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for BracketInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: BRACKET_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Set a working order's protective prices",
        description: "Replaces the stop and target riding a working order, which arm the moment it fills. Both legs are replaced wholesale: an absent leg is a cleared leg, exactly as dragging one off the chart clears it. The levels are judged against the order's own resting price, not the market.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(BRACKET_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            posture(
                spec,
                false,
                "The order is named by id, and ids are never reused. A stale caller amends the order it meant or nothing at all - an id that has since filled or been cancelled is reported as unknown rather than resolved to whatever is working now.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        order(self, host, cx.actor)
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for CancelInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: CANCEL_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Cancel a working order",
        description: "Removes one working order without trading. Risk-reducing: it can only ever take exposure off, never add any. Re-placing the same order afterwards is a new order at the back of the queue.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(CANCEL_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            posture(
                spec,
                true,
                "The order is named by id, and ids are never reused, so a stale cancel removes the order it meant or reports that there is no such order. It can never cancel a different one that happens to be working now.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        order(self, host, cx.actor)
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for FlattenInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: FLATTEN_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Go flat: close the position and cancel every working order",
        description: "Exits the open position at the next print and removes every working order, in one call, so the account is never half-flat between two calls. Risk-reducing: it only ever takes exposure off. Flat with nothing working, it changes nothing.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(FLATTEN_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            posture(
                spec,
                true,
                "Names no order and no size: it flattens whatever the account holds when it lands, so a caller working from a stale read still ends flat.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        order(self, host, cx.actor)
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for ReverseInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: REVERSE_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Reverse the simulated position",
        description: "Flips the open position in one market order for twice its size, protected by the ticket's offsets for the new side. The opening leg is measured by the risk per trade like any entry and refused with its sentence. Refused while flat: there is nothing to reverse.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(REVERSE_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            posture(
                spec,
                false,
                "The size is the account's at the moment the call lands, never a size the caller read earlier. Not safe to retry: a second reversal flips the position back.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        order(self, host, cx.actor)
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
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
    dock_call::<PlaceInput, _, _, _>(registry)?;
    dock_call::<BracketInput, _, _, _>(registry)?;
    dock_call::<CancelInput, _, _, _>(registry)?;
    dock_call::<FlattenInput, _, _, _>(registry)?;
    dock_call::<ReverseInput, _, _, _>(registry)?;
    crate::trade_account::register(registry)
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod trade_tests;
