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
pub use quantick_control_host::authority::{TRADE_MODULE_ID, TRADE_PERMISSION_ID};

use std::collections::BTreeSet;

use quantick_civil::TzOffset;
use quantick_control::{
    error::{ControlError, codes},
    id::{CapabilityId, CostClassId, ModuleId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::ActorContext,
};
use quantick_control_host::actions::ReplayTrace;
use quantick_control_host::admission::known_error;
use quantick_control_host::authority::{NO_CONFIRMATION_ID, TRADE_EFFECT_ID, UI_BOUNDED_COST_ID};
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
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::call::{Call, CallCx, CallSpec, dock_call};
use crate::dock::ActionDock;
use crate::wire::readback::{Readback, TRADE_UNREACHABLE_TEST, journal, snapshot};
use crate::wire::session::PAPER_SCOPE_ID;
use quantick_control::registry::IdempotencyPolicy::{Forbidden, Optional};

pub const PLACE_CAPABILITY_ID: &str = "trade.order.place";

pub const BRACKET_CAPABILITY_ID: &str = "trade.order.bracket";

/// What the two shaping calls journal under. They place nothing, so an
/// order-placed event would put a phantom order in the trail the constants
/// above exist to keep honest.
pub const TICKET_EVENT_KIND: &str = "trade.ticket.changed";

pub const SELECT_STRATEGY_CAPABILITY_ID: &str = "trade.strategy.select";

pub const SET_RULER_CAPABILITY_ID: &str = "trade.ruler.set";

pub const SET_RISK_CAPABILITY_ID: &str = "trade.risk.set";

pub const SET_INSTRUMENT_MONEY_CAPABILITY_ID: &str = "trade.instrument.set_money";

pub const CANCEL_CAPABILITY_ID: &str = "trade.order.cancel";

/// The two one-shot position calls.
pub const FLATTEN_CAPABILITY_ID: &str = "trade.position.flatten";

pub const REVERSE_CAPABILITY_ID: &str = "trade.position.reverse";

/// The order vocabulary the trade calls state, from its owner.
pub use quantick_trading::{ActionEntryKind, ActionSide};

/// The refusal every action gives when there is no chart to trade on.
///
/// A window with no tab is not a state this application reaches, and the
/// point of answering rather than indexing is that the control plane must
/// not be the thing that discovers otherwise by taking a live session down.
pub fn no_chart_open() -> ControlError {
    known_error(
        codes::CAPABILITY_UNAVAILABLE,
        "this window has no chart open",
        true,
    )
}

/// The shared shape of every descriptor here — one place for the risk
/// posture, so a fourth action cannot quietly ship a gentler one.
pub fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    risk_reducing: bool,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(TRADE_MODULE_ID).expect("static module ID is valid"),
        input_schema,
        output_schema: generated_schema::<TradeResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(TRADE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        // Placing the same order twice places two orders. There is no key
        // that could make that safe to retry, so the policy says so rather
        // than offering one that does not hold.
        idempotency: IdempotencyPolicy::Forbidden,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(stale_input_safety.to_owned()),
        dry_run_supported: false,
        // An order dies with the session, like every simulated position.
        persistence: EffectPersistence::Transient,
        reversible: true,
        // Cancelling is not `destructive` in this registry's sense, and the
        // first attempt at it here said so wrongly. Destructive means the
        // trader's *work* is gone — a drawing, a layout, an annotation —
        // and the registry answers that by demanding an expected revision,
        // which this tier's envelopes forbid outright
        // (`ObserverContract::prepare`), so the guard could never have run.
        // A working order is an instruction, re-issuable in one call, and
        // removing one only ever reduces exposure: that is exactly what
        // `risk_reducing` is for, and it is the honest flag.
        destructive: false,
        risk_reducing,
        required_permissions: [TRADE_PERMISSION_ID]
            .into_iter()
            .map(|id| quantick_control::id::PermissionId::new(id).expect("static permission"))
            .collect(),
        preconditions: Vec::new(),
        confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

/// Which named exit ladder the ticket arms next.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectStrategyInput {
    /// The strategy's name, exactly as `observe.session.paper` reports it.
    /// Omitted or null selects none, which is the bare order the trader
    /// brackets by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// How far the ruler walks the projected bracket from an aimed entry.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRulerInput {
    /// Distance in ticks, the same on both sides. Zero puts the ruler away
    /// and leaves the next order bare.
    pub ticks: u32,
}

/// What one trade may lose, and whether an entry over it is refused.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRiskInput {
    /// `off` leaves the size to whoever types it; `amount` reads `amount`;
    /// `percent` reads `percent` against the capital declared for the
    /// instrument's own currency.
    pub basis: String,
    /// The fixed amount one trade may lose, as a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    /// The share of declared capital one trade may lose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<String>,
    /// Declare the capital for `currency`. Both or neither: a capital with
    /// no currency has nothing to be keyed by, and nothing here converts
    /// between currencies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capital: Option<String>,
    /// The currency `capital` is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Whether an entry over the risk per trade is refused. Left out, the
    /// lock stays as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<bool>,
}

/// What one point of an instrument is worth, and its smallest tradable size.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetInstrumentMoneyInput {
    /// The symbol to declare for. Left out, the chart's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// What one point of price is worth per unit held, as a decimal string.
    /// Given with either of the other two missing, the declaration is
    /// cleared instead — which returns the instrument to "nothing here knows
    /// what a point is worth", the same contract `set_ruler_step` uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_value: Option<String>,
    /// The smallest tradable increment of quantity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_step: Option<String>,
    /// The currency the point value is in. Never converted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

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
    fn account_route(&mut self) -> Option<crate::trade_account::AccountRouteResult> {
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

/// The journal payload: what was asked and what the venue answered, from the
/// encoded [`TradeResult`].
///
/// Every ticket and order call records it, accepted or refused by the venue,
/// and the actor rides in the event rather than beside it. An order placed by
/// an operator that looked exactly like one the trader placed would be the
/// authorship half of the honesty contract quietly dropped — and a refusal is
/// worth recording too, since "the agent tried to buy here and was told no"
/// is precisely the line a trader reviewing a session wants to find.
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
        crate::wire::recovery::RECONNECT_CAPABILITY_ID
    )];
    error
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

/// A ticket call's posture: the family's, with the two fields that are
/// simply not true of it corrected.
///
/// Placing an order is transient, forbidden to retry and answers with the
/// order it made. Choosing a ladder or moving the ruler is none of those: it
/// rewrites the paper-state sidecar and fans out to every tab, so it is
/// durable; setting the same value twice leaves the same value, so it is
/// idempotent; and it places nothing.
fn ticket_posture(spec: &CallSpec, stale_input_safety: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        persistence: EffectPersistence::Durable,
        idempotency: IdempotencyPolicy::Optional,
        ..posture(spec, false, stale_input_safety)
    }
}

/// Edit the active desk's ticket with `edit`, persist `change` when it
/// names one, and answer what the desk now shows.
fn ticket<H: PaperHost + ?Sized>(
    host: &mut H,
    change: Option<SharedChange>,
    edit: impl FnOnce(&mut dyn TradeDesk) -> Result<(), ControlError>,
) -> Result<TradeResult, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    edit(paper.active_desk_mut().ok_or_else(no_chart_open)?)?;
    if let Some(change) = change {
        paper.persist(change);
    }
    answer(paper, &[])
}

impl<H: PaperHost + ?Sized> Call<H> for SelectStrategyInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: SELECT_STRATEGY_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Choose the ticket's exit ladder",
        description: "Sets which named exit strategy the next order rests with, or none for a bare order. Changes no order that already exists and never touches an open position; the same call the ticket's own selector makes.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(TICKET_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            ticket_posture(
                spec,
                "The call names the strategy it selected and answers with the one that is now set, so a caller working from a stale read can see that it chose something else. It changes nothing that is already working.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        ticket(host, Some(SharedChange::OrderStrategies), |desk| {
            let strategies = desk.account().order_strategies().to_vec();
            if let Some(name) = self.name.as_deref()
                && !strategies.iter().any(|strategy| strategy.name == name)
            {
                return Err(ControlError::invalid_request(format!(
                    "no exit strategy is named `{name}`"
                )));
            }
            desk.account_mut()
                .set_order_strategies(strategies, self.name.as_deref());
            Ok(())
        })
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for SetRulerInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: SET_RULER_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Set the aim's ruler distance",
        description: "Walks the projected stop and target out from an aimed entry, the same distance on both sides, in ticks of the instrument. Zero puts the ruler away. Shapes what the next order would carry rather than changing one that exists - the read a trader takes before committing.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(TICKET_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            ticket_posture(
                spec,
                "The call answers with where the ruler actually landed, clamped to what the wheel itself can reach, so a caller that asked for more can see what it got. It changes nothing that is already working.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        ticket(host, None, |desk| {
            desk.set_ruler_ticks(self.ticks);
            Ok(())
        })
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for SetRiskInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: SET_RISK_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Set the risk per trade",
        description: "Says what one trade may lose - a fixed amount, or a share of the capital declared for the instrument's currency - and whether an entry whose stop risks more than that is refused. Shapes the size the next order would carry; changes no order that exists and never touches an open position.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(TICKET_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            ticket_posture(
                spec,
                "The call answers with the risk that is now set and with what it makes of the entry the ticket is holding, including the reason when it can name no size, so a caller sees the same sentence the trader is reading. It changes nothing that is already working.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        let edit = crate::ticket_money::risk_edit(&self)?;
        ticket(host, Some(SharedChange::RiskSettings), |desk| {
            desk.account_mut().apply_risk_edit(edit);
            Ok(())
        })
    }

    fn entry(asked: Value, answer: &Value) -> Value {
        entry(asked, answer)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for SetInstrumentMoneyInput {
    type Answer = TradeResult;

    const SPEC: CallSpec = CallSpec {
        id: SET_INSTRUMENT_MONEY_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Declare what an instrument's point is worth",
        description: "Records what one point of price is worth per unit held, the smallest tradable size, and the currency - the three facts no feed reports and nothing here derives, without which risk sizing has nothing to size against. Leaving any of them out clears the declaration rather than half-setting it.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(TICKET_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            ticket_posture(
                spec,
                "The call answers with what the ticket now makes of the entry it is holding, so a caller can see the instrument turn from unknown into sized. It changes nothing that is already working, and it never converts between currencies.",
            )
        },
    };

    fn run<A: JournalAccess>(
        self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<TradeResult, ControlError> {
        ticket(host, Some(SharedChange::RiskSettings), |desk| {
            crate::ticket_money::declare_money(desk.account_mut(), &self)
        })
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
    dock_call::<SelectStrategyInput, _, _, _>(registry)?;
    dock_call::<SetRulerInput, _, _, _>(registry)?;
    dock_call::<SetRiskInput, _, _, _>(registry)?;
    dock_call::<SetInstrumentMoneyInput, _, _, _>(registry)?;
    dock_call::<PlaceInput, _, _, _>(registry)?;
    dock_call::<BracketInput, _, _, _>(registry)?;
    dock_call::<CancelInput, _, _, _>(registry)?;
    dock_call::<FlattenInput, _, _, _>(registry)?;
    dock_call::<ReverseInput, _, _, _>(registry)?;
    crate::trade_account::register(registry)
}

pub const TRADE_SHAPING_TEST: &str =
    "a_dropped_trade_shaping_answer_is_replayed_and_the_ticket_changes_once";

/// How a client reconciles an interrupted call on the paper ticket.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[
    journal(
        "trade.instrument.set_money",
        Optional,
        TICKET_EVENT_KIND,
        "payload.asked",
        "an event after the pre-call cursor carries the caller's input as `asked`",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.order.bracket",
        Forbidden,
        PAPER_SCOPE_ID,
        "tabs[].working_orders[].stop_loss",
        "the named order carries the stop and target asked for",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.order.cancel",
        Forbidden,
        PAPER_SCOPE_ID,
        "tabs[].working_orders[].order_id",
        "the named order is no longer working",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.order.place",
        Forbidden,
        PAPER_SCOPE_ID,
        "tabs[].working_orders[].order_id",
        "an order the pre-call reading lacked is working, or a position opened",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.position.flatten",
        Forbidden,
        PAPER_SCOPE_ID,
        "tabs[].position",
        "no position is open and `working_order_count` is zero",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.position.reverse",
        Forbidden,
        PAPER_SCOPE_ID,
        "tabs[].position.side",
        "the position's side is the opposite of the pre-call reading",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.risk.set",
        Optional,
        PAPER_SCOPE_ID,
        "tabs[].risk_state",
        "the ticket's risk state follows the basis asked for",
        &[TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.ruler.set",
        Optional,
        PAPER_SCOPE_ID,
        "tabs[].ruler_ticks",
        "the ruler stands at the distance the call answered with",
        &[TRADE_SHAPING_TEST, TRADE_UNREACHABLE_TEST],
    ),
    snapshot(
        "trade.strategy.select",
        Optional,
        PAPER_SCOPE_ID,
        "tabs[].armed_strategy",
        "the ticket is armed with the strategy asked for",
        &[TRADE_UNREACHABLE_TEST],
    ),
];

#[cfg(test)]
#[path = "trade_tests.rs"]
mod trade_tests;
