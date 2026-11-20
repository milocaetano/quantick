//! Report gestures docked outside replay; the paper owner resolves their scope.
use quantick_control::{
    error::{ControlError, codes},
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_host::{admission::known_error, journal::NewEvent, wire::canonical_decimal};
use quantick_control_schema::session_reports::{
    EVENT_KIND, ReportInput, ReportOutput, ReportResult, descriptor,
};
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
    let output = match input {
        ReportInput::Sources { sources } => {
            if sources.as_ref().is_some_and(|sources| sources.len() > 5) {
                return Err(ControlError::invalid_request(
                    "at most five source selectors are allowed",
                ));
            }
            let sources = sources.map(|sources| {
                sources.into_iter().fold(
                    quantick_paper::report::SourceSelection::default(),
                    |selection, source| {
                        selection.union(crate::journal_sessions::source_filter(source))
                    },
                )
            });
            let view = desk.report_sources(sources, tz);
            use quantick_control_schema::journal_sessions::JournalSourceFilter as WireSource;
            let sources = [WireSource::Broker, WireSource::Real, WireSource::Replay]
                .into_iter()
                .filter(|source| {
                    view.source
                        .contains(crate::journal_sessions::source_filter(*source))
                })
                .collect();
            ReportOutput::Sources {
                action: "sources".to_owned(),
                sources,
                open: view.open,
                loaded: view.loaded,
                symbol: view.symbol,
                window: view.window,
                scope: view.scope,
                trade_count: WireU64::new(view.trade_count as u64),
                unreadable_files: WireU64::new(view.unreadable_files as u64),
                skipped_rows: WireU64::new(view.skipped_rows as u64),
                net_points: canonical_decimal(view.net_points),
            }
        }
        ReportInput::Ordinary {} => {
            desk.reset_report();
            let symbol = desk.account().symbol();
            ReportOutput::Ordinary {
                action: "ordinary".to_owned(),
                title: "Sim live, all time".to_owned(),
                symbol: (!symbol.is_empty()).then(|| symbol.to_owned()),
            }
        }
        ReportInput::Session { session } => scoped(desk, &dir, tz, session, None)?,
        ReportInput::Day { session, day } => {
            let day = quantick_civil::parse_iso_date(&day)
                .ok_or_else(|| ControlError::invalid_request("day must be a valid YYYY-MM-DD"))?;
            scoped(desk, &dir, tz, session, Some(day))?
        }
    };
    let result = serde_json::to_value(output)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    access.record_event(NewEvent::by(
        quantick_control_schema::session::MODULE_ID,
        EVENT_KIND,
        actor,
        json!({"report": result}),
    ));
    Ok(result)
}

fn scoped(
    desk: &mut dyn crate::trade::TradeDesk,
    dir: &std::path::Path,
    tz: quantick_civil::TzOffset,
    session: String,
    day: Option<quantick_civil::CivilDate>,
) -> Result<ReportOutput, ControlError> {
    let session = SessionKey::parse(&session)
        .ok_or_else(|| crate::journal_sessions::refusal(SessionEditError::InvalidKey(session)))?;
    let prepared = scope::prepare(dir, ReportRequest { session, day }, tz)
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
        unreadable_files: WireU64::new(prepared.history.unreadable_files as u64),
        skipped_rows: WireU64::new(prepared.history.problem_rows as u64),
        net_points: canonical_decimal(receipt.net_points),
    };
    desk.show_report(prepared);
    Ok(ReportOutput::Scoped(result))
}

#[cfg(test)]
#[path = "session_reports_tests.rs"]
mod tests;
