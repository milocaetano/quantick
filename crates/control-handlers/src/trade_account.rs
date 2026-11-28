//! The `trade.account.*` family: which account orders go to, as named calls.
//!
//! The Trading panel's `Account` selector, said without a hand: read the
//! route and the accounts the terminal reports, go back to the simulator,
//! or ask for a broker route. Every decision is the route's
//! (`quantick_paper::route`), and the two that matter most are enforced
//! there, not here: no caller but the trader's own hand arms a broker
//! account, and REAL arms only by holding its row. This module reads the
//! caller's input, names the actor the route judges, turns a refusal into a
//! coded error — its route code in `details.route_code` — and journals who
//! asked for what.

use quantick_control::error::{ControlError, codes};
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy, RegistryError};
use quantick_control::wire::{ActorContext, ActorKind};
use quantick_control_host::actions::ReplayTrace;
use quantick_control_host::admission::known_error;
use quantick_control_host::authority::{ReadSpec, TRADE_MODULE_ID, read_descriptor};
use quantick_control_host::journal::NewEvent;
use quantick_paper::route::{
    Actor, Leave, Refusal, Route, RouteRead, RouteRequest, RowGesture, RowState,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::JournalAccess;
use crate::call::{Call, CallCx, CallSpec, dock_call};
use crate::dock::ActionDock;
use crate::trade::{PaperHost, PaperOwner, TRADE_PERMISSION_ID, descriptor};
use crate::wire::readback::{Readback, TRADE_UNREACHABLE_TEST, journal};
use IdempotencyPolicy::Optional;

pub const READ_CAPABILITY_ID: &str = "trade.account.read";

pub const SELECT_CAPABILITY_ID: &str = "trade.account.select";

pub const DISARM_CAPABILITY_ID: &str = "trade.account.disarm";

/// What a route change journals under, accepted or refused.
pub const ROUTE_EVENT_KIND: &str = "trade.account.routed";

/// Reads take nothing.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountReadInput {}

/// How to leave a broker route that still holds a position or orders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaveChoice {
    /// Close the position and cancel the orders at the broker first.
    FlattenFirst,
    /// Leave them at the broker; their plates stay on the chart.
    LeaveAtBroker,
}

/// Where orders go.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteChoice {
    /// The paper simulator, filling from the tape.
    Sim,
    /// The broker account the terminal reports as demo.
    Demo,
    /// The broker account the terminal reports as real money; arms only by
    /// holding its row in the Trading panel.
    Real,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountSelectInput {
    pub route: RouteChoice,
    /// Required to leave a broker route with exposure open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave: Option<LeaveChoice>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountDisarmInput {
    /// Required to leave a broker route with exposure open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave: Option<LeaveChoice>,
}

/// One row of the account list, as the panel shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountRowView {
    pub title: String,
    /// `in use`, `one click`, `hold`, or empty when the row is off.
    pub hint: String,
    /// `selected`, `enabled` or `off`.
    pub state: String,
    /// `click`, `hold` or `none`: what the panel's press does.
    pub gesture: String,
    pub details: Vec<String>,
}

/// The route and everything the panel says about it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountRouteResult {
    /// `sim`, `demo` or `real`.
    pub route: String,
    /// Where orders go, in the toolbar notice's words.
    pub orders_to: String,
    /// The Trading panel's title.
    pub title: String,
    /// Whether broker routes are offered at all (`[broker.metatrader]`).
    pub broker_enabled: bool,
    /// The chart's source cannot route: the selector stays on the simulator.
    pub locked: bool,
    pub accounts: Vec<AccountRowView>,
    /// Why the broker row is off, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    /// A leave with exposure waits for `leave`.
    pub leave_pending: bool,
}

/// The route's judge of a caller: the trader's own hand, or anything else.
#[must_use]
pub fn actor_of(actor: &ActorContext) -> Actor {
    match actor.actor_kind {
        ActorKind::HumanUi => Actor::Trader,
        ActorKind::Agent | ActorKind::Automation => Actor::Agent,
    }
}

/// A route refusal as a control error: the sentence, and the route's own
/// code in `details.route_code`.
#[must_use]
pub fn refusal_error(refusal: Refusal) -> ControlError {
    let code = match refusal {
        Refusal::AgentCannotArm
        | Refusal::RealNeedsHold
        | Refusal::AgentOrdersOff
        | Refusal::ParkedAtBroker => codes::PERMISSION_DENIED,
        Refusal::ExposureNeedsChoice => codes::INVALID_REQUEST,
        _ => codes::CAPABILITY_UNAVAILABLE,
    };
    let mut error = known_error(code, refusal.sentence(), false);
    error.context.details = Some(json!({ "route_code": refusal.code() }));
    error
}

/// The panel's sheet, as a caller reads it.
#[must_use]
pub fn route_result(read: &RouteRead) -> AccountRouteResult {
    let sheet = &read.sheet;
    AccountRouteResult {
        route: read.route.id().to_owned(),
        orders_to: sheet.notice.text.clone(),
        title: sheet.title.clone(),
        broker_enabled: read.broker_enabled,
        locked: sheet.locked,
        accounts: sheet
            .rows
            .iter()
            .map(|row| AccountRowView {
                title: row.title.clone(),
                hint: row.hint.clone(),
                state: match row.state {
                    RowState::Selected => "selected",
                    RowState::Enabled => "enabled",
                    RowState::Off => "off",
                }
                .to_owned(),
                gesture: match row.gesture {
                    RowGesture::Click(_) => "click",
                    RowGesture::Hold => "hold",
                    RowGesture::None => "none",
                }
                .to_owned(),
                details: row.subs.clone(),
            })
            .collect(),
        footer: sheet.footer.clone(),
        leave_pending: sheet.leave_prompt,
    }
}

fn route_of(choice: RouteChoice) -> Route {
    match choice {
        RouteChoice::Sim => Route::Sim,
        RouteChoice::Demo => Route::Demo,
        RouteChoice::Real => Route::Real,
    }
}

fn leave_of(choice: Option<LeaveChoice>) -> Option<Leave> {
    choice.map(|choice| match choice {
        LeaveChoice::FlattenFirst => Leave::FlattenFirst,
        LeaveChoice::LeaveAtBroker => Leave::LeaveAtBroker,
    })
}

fn current(paper: &mut dyn PaperOwner) -> Result<AccountRouteResult, ControlError> {
    paper.account_route().ok_or_else(|| {
        known_error(
            codes::CAPABILITY_UNAVAILABLE,
            "this window has no account route",
            false,
        )
    })
}

/// Ask the route for `request`, journal the answer, accepted or refused, and
/// reply with the route now in force. `asked` is the decoded input encoded
/// again: the adapter journals only successes, so a refusal is journaled
/// here, where the caller's raw JSON is no longer at hand.
fn route_to<H: PaperHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    cx: &mut CallCx<'_, A>,
    request: RouteRequest,
    asked: &impl Serialize,
) -> Result<AccountRouteResult, ControlError> {
    let mut owner = host.paper();
    let paper = &mut *owner;
    let answer = paper.request_route(request);
    cx.access.record_event(NewEvent::by(
        TRADE_MODULE_ID,
        ROUTE_EVENT_KIND,
        cx.actor,
        json!({
            "asked": asked,
            "accepted": answer.is_ok(),
            "route": paper.account_route().map(|now| now.route),
            "refused_because": answer.err().map(Refusal::code),
        }),
    ));
    answer.map_err(refusal_error)?;
    current(paper)
}

/// A route change's posture: the trade family's, retryable because asking
/// for the route in force changes nothing.
fn route_posture(
    spec: &CallSpec,
    risk_reducing: bool,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        idempotency: IdempotencyPolicy::Optional,
        ..descriptor(
            spec.id,
            spec.title,
            spec.description,
            risk_reducing,
            stale_input_safety,
        )
    }
}

impl<H: PaperHost + ?Sized> Call<H> for AccountReadInput {
    type Input = Self;
    type Answer = AccountRouteResult;

    const SPEC: CallSpec = CallSpec {
        id: READ_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Read where orders go",
        description: "Reads the account route - the simulator, or the broker account the MetaTrader terminal is logged into, demo or real - with the account list the Trading panel shows, each row's facts and why a row is off. Changes nothing.",
        permissions: &["observe", "observe.paper"],
        event: None,
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            read_descriptor::<AccountReadInput, AccountRouteResult>(&ReadSpec {
                id: spec.id,
                module: spec.module,
                title: spec.title,
                description: spec.description,
                permissions: spec.permissions,
                pagination: None,
            })
        },
    };

    fn run<A: JournalAccess>(
        _input: Self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<AccountRouteResult, ControlError> {
        current(&mut *host.paper())
    }
}

impl<H: PaperHost + ?Sized> Call<H> for AccountSelectInput {
    type Input = Self;
    type Answer = AccountRouteResult;

    const SPEC: CallSpec = CallSpec {
        id: SELECT_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Choose where orders go",
        description: "Routes orders to the simulator, or asks for the broker's DEMO account. Only the trader's own hand arms a broker account: any other caller is refused (`route.agent_cannot_arm`), and a REAL account arms only by holding its row in the Trading panel (`route.real_needs_hold`). Leaving a broker route with a position or orders open needs `leave`.",
        permissions: &[TRADE_PERMISSION_ID],
        // Journaled by `route_to`, refusals included.
        event: None,
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            route_posture(
                spec,
                false,
                "The call answers with the route now in force. Asking for the route already in force changes nothing, so a retried call is harmless.",
            )
        },
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<AccountRouteResult, ControlError> {
        let request = RouteRequest::new(
            route_of(input.route),
            actor_of(cx.actor),
            leave_of(input.leave),
        );
        route_to(host, cx, request, &input)
    }
}

impl<H: PaperHost + ?Sized> Call<H> for AccountDisarmInput {
    type Input = Self;
    type Answer = AccountRouteResult;

    const SPEC: CallSpec = CallSpec {
        id: DISARM_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Go back to the simulator",
        description: "Disarms any broker route: orders go to the simulator from now on. Risk-reducing. With a position or orders open at the broker, `leave` says whether to flatten them first or leave them there, their plates staying on the chart.",
        permissions: &[TRADE_PERMISSION_ID],
        // Journaled by `route_to`, refusals included.
        event: None,
        replay: ReplayTrace::Recorded,
        posture: |spec| {
            route_posture(
                spec,
                true,
                "Disarming an unarmed account changes nothing, so a retried call is harmless.",
            )
        },
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<AccountRouteResult, ControlError> {
        let request = RouteRequest::new(Route::Sim, actor_of(cx.actor), leave_of(input.leave));
        route_to(host, cx, request, &input)
    }
}

/// Dock the family beside the trade calls.
///
/// # Errors
///
/// A capability is already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: PaperHost + ?Sized,
    A: JournalAccess,
{
    dock_call::<AccountReadInput, _, _, _>(registry)?;
    dock_call::<AccountSelectInput, _, _, _>(registry)?;
    dock_call::<AccountDisarmInput, _, _, _>(registry)
}

/// How a client reconciles an interrupted route change.
pub const READBACKS: &[Readback] = &[
    journal(
        DISARM_CAPABILITY_ID,
        Optional,
        ROUTE_EVENT_KIND,
        "payload.route",
        "an event after the pre-call cursor names `sim` as the route",
        &[TRADE_UNREACHABLE_TEST],
    ),
    journal(
        SELECT_CAPABILITY_ID,
        Optional,
        ROUTE_EVENT_KIND,
        "payload.route",
        "an event after the pre-call cursor names the route asked for",
        &[TRADE_UNREACHABLE_TEST],
    ),
];

#[cfg(test)]
#[path = "trade_account_tests.rs"]
mod trade_account_tests;
