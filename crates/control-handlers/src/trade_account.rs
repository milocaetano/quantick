//! The `trade.account.*` handlers: read the route, choose it, disarm it.
//!
//! Every decision is the route's (`quantick_paper::route`); this module reads
//! the caller's input, names the actor the route judges, turns a refusal into
//! a coded error and journals who asked for what.

use quantick_control::error::{ControlError, codes};
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, ActorKind};
use quantick_control_host::admission::known_error;
use quantick_control_host::authority::TRADE_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::trade_account::{
    AccountDisarmInput, AccountReadInput, AccountRouteResult, AccountRowView, AccountSelectInput,
    LeaveChoice, ROUTE_EVENT_KIND, disarm_descriptor, read_route_descriptor, select_descriptor,
};
use quantick_paper::route::{Actor, Leave, Refusal, Route, RowGesture, RowState, Sheet};
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::dock::ActionDock;
use crate::trade::TradePort;

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
        Refusal::AgentCannotArm | Refusal::RealNeedsHold | Refusal::AgentOrdersOff => {
            codes::PERMISSION_DENIED
        }
        Refusal::ExposureNeedsChoice => codes::INVALID_REQUEST,
        _ => codes::CAPABILITY_UNAVAILABLE,
    };
    let mut error = known_error(code, refusal.sentence(), false);
    error.context.details = Some(json!({ "route_code": refusal.code() }));
    error
}

/// The panel's sheet, as a caller reads it.
#[must_use]
pub fn route_result(sheet: &Sheet, route: Route, broker_enabled: bool) -> AccountRouteResult {
    AccountRouteResult {
        route: route.id().to_owned(),
        orders_to: sheet.notice.text.clone(),
        title: sheet.title.clone(),
        broker_enabled,
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

fn leave_of(choice: Option<LeaveChoice>) -> Option<Leave> {
    choice.map(|choice| match choice {
        LeaveChoice::FlattenFirst => Leave::FlattenFirst,
        LeaveChoice::LeaveAtBroker => Leave::LeaveAtBroker,
    })
}

fn current<P: TradePort + ?Sized>(app: &mut P) -> Result<AccountRouteResult, ControlError> {
    app.account_route().ok_or_else(|| {
        known_error(
            codes::CAPABILITY_UNAVAILABLE,
            "this window has no account route",
            false,
        )
    })
}

fn to_value(result: &AccountRouteResult) -> Result<Value, ControlError> {
    serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(format!("account route: {error}")))
}

fn read<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let _: AccountReadInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    to_value(&current(app)?)
}

/// Ask the route for `route`, journal the answer, and reply with the route
/// now in force.
fn route_to<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    (route, leave): (Route, Option<Leave>),
    asked: Value,
) -> Result<Value, ControlError> {
    let answer = app.request_route(route, actor_of(actor), leave);
    access.record_event(NewEvent::by(
        TRADE_MODULE_ID,
        ROUTE_EVENT_KIND,
        actor,
        json!({
            "asked": asked,
            "accepted": answer.is_ok(),
            "route": app.account_route().map(|now| now.route),
            "refused_because": answer.err().map(Refusal::code),
        }),
    ));
    answer.map_err(refusal_error)?;
    to_value(&current(app)?)
}

fn select<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: AccountSelectInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let route = Route::from_id(&input.route).ok_or_else(|| {
        ControlError::invalid_request(format!(
            "route must be `sim`, `demo` or `real` - got `{}`",
            input.route
        ))
    })?;
    route_to(app, access, actor, (route, leave_of(input.leave)), asked)
}

fn disarm<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: AccountDisarmInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    route_to(
        app,
        access,
        actor,
        (Route::Sim, leave_of(input.leave)),
        asked,
    )
}

/// Dock the family beside the trade calls.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: TradePort + ?Sized,
    A: JournalAccess,
{
    registry.register(read_route_descriptor(), read::<H, A>)?;
    registry.register(select_descriptor(), select::<H, A>)?;
    registry.register(disarm_descriptor(), disarm::<H, A>)?;
    Ok(())
}

#[cfg(test)]
#[path = "trade_account_tests.rs"]
mod trade_account_tests;
