//! Report gestures docked outside replay; the paper owner resolves their scope.
use quantick_control::{
    error::{ControlError, codes},
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_host::{admission::known_error, journal::NewEvent, wire::canonical_decimal};
use quantick_control_schema::session_reports::{EVENT_KIND, ReportInput, ReportResult, descriptor};
use quantick_paper::report::{
    scope::{self, ReportRequest},
    session_edit::SessionEditError,
    sessions::SessionKey,
};
use serde_json::{Value, json};

use crate::{dock::ActionDock, notify::NotifyAccess, trade::PaperHost};

pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: PaperHost + ?Sized,
    A: NotifyAccess,
{
    registry.register_outside_replay(descriptor(), open)
}

fn open<H: PaperHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: ReportInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let (session, day) =
        match input {
            ReportInput::Session { session } => (session, None),
            ReportInput::Day { session, day } => (
                session,
                Some(quantick_civil::parse_iso_date(&day).ok_or_else(|| {
                    ControlError::invalid_request("day must be a valid YYYY-MM-DD")
                })?),
            ),
        };
    let session = SessionKey::parse(&session)
        .ok_or_else(|| crate::journal_sessions::refusal(SessionEditError::InvalidKey(session)))?;
    let mut owner = host.paper();
    let tz = owner.journal_timezone();
    let dir = owner.journal_scope().dir;
    let desk = owner.active_desk_mut().ok_or_else(|| {
        let mut error = known_error(
            codes::CAPABILITY_UNAVAILABLE,
            "no active Trades ledger",
            false,
        );
        error.context.details = Some(json!({"code": "no_ledger"}));
        error
    })?;
    let prepared = scope::prepare(&dir, ReportRequest { session, day }, tz)
        .map_err(crate::journal_sessions::refusal)?;
    let receipt = &prepared.receipt;
    let result = ReportResult {
        action: if day.is_some() { "day" } else { "session" }.to_owned(),
        session: receipt.request.session.to_string(),
        origin: if receipt.request.session.is_broker() {
            "broker"
        } else {
            "simulator"
        }
        .to_owned(),
        day: day.map(|day| day.iso()),
        title: receipt.title.clone(),
        trade_count: WireU64::new(receipt.trades as u64),
        net_points: canonical_decimal(receipt.net_points),
    };
    desk.show_report(prepared);
    let result = serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    access.record_event(NewEvent::by(
        quantick_control_schema::session::MODULE_ID,
        EVENT_KIND,
        actor,
        json!({"report": result}),
    ));
    Ok(result)
}

#[cfg(test)]
#[path = "session_reports_tests.rs"]
mod tests;
