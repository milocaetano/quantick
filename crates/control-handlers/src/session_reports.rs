//! Report gestures docked outside replay; the paper owner resolves their scope.
use quantick_control::{
    error::{ControlError, codes},
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_host::{admission::known_error, journal::NewEvent, wire::canonical_decimal};
use quantick_control_schema::session_reports::{
    EVENT_KIND, MergedWire, RateUsedWire, ReportInput, ReportOrigin as WireOrigin, ReportOutput,
    ReportResult, UnconvertedWire, descriptor,
};
use quantick_paper::pnl::PnlPick;
use quantick_paper::rates::Currency;
use quantick_paper::report::{
    ReportPeriod,
    calendar::{DaySet, MAX_LISTED_DAYS},
    journals,
    merge::{MAX_LISTED, MergeReadout, Unconvertible},
    parse_period,
    scope::{self, ReportOrigin, ReportRequest},
    session_edit::SessionEditError,
    sessions::SessionKey,
    snapshot::{ReportChange, ReportReadout},
};
use quantick_session::paper::SharedChange;
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
    let inspect_only = matches!(
        input,
        ReportInput::Sources { sources: None } | ReportInput::Inspect {}
    );
    let mut picked = None;
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
            report(desk, "sources", sources.map(ReportChange::Sources), tz)?
        }
        ReportInput::Inspect {} => report(desk, "inspect", None, tz)?,
        ReportInput::Symbols { symbols } => {
            let known = journals::symbols(&dir);
            if let Some(unknown) = symbols.iter().find(|symbol| !known.contains(symbol)) {
                let mut error = invalid("unknown_symbol", &format!("no journal holds {unknown}"));
                error.context.details = Some(json!({"code": "unknown_symbol", "symbols": known}));
                return Err(error);
            }
            let change = ReportChange::Symbols(symbols.into_iter().collect());
            report(desk, "symbols", Some(change), tz)?
        }
        ReportInput::Merge { currency: None } => {
            report(desk, "merge", Some(ReportChange::Merge(None)), tz)?
        }
        ReportInput::Merge {
            currency: Some(code),
        } => {
            let before = desk.report_change(None, tz).merge.request;
            let currency = Currency::new(&code);
            let view = desk.report_change(Some(ReportChange::Merge(currency.clone())), tz);
            let Some(currency) =
                currency.filter(|currency| view.merge.currencies.contains(currency))
            else {
                desk.report_change(Some(ReportChange::Merge(before)), tz);
                let offered: Vec<&str> = view.merge.currencies.iter().map(Currency::code).collect();
                let mut error = invalid("invalid_currency", &format!("{code} is not on offer"));
                error.context.details =
                    Some(json!({"code": "invalid_currency", "currencies": offered}));
                return Err(error);
            };
            desk.account_mut()
                .pick_pnl(PnlPick::MergeCurrency(currency.clone()));
            picked = Some(currency);
            readout(view, "merge", tz)
        }
        ReportInput::Days { days } => {
            let days = DaySet::parse_list(&days)
                .map_err(|error| invalid("invalid_dates", &error.to_string()))?;
            report(desk, "days", Some(ReportChange::Days(days)), tz)?
        }
        ReportInput::ClearDates {} => report(
            desk,
            "clear_dates",
            Some(ReportChange::Days(DaySet::default())),
            tz,
        )?,
        ReportInput::Period { period } => {
            let period = (period.len() <= 16)
                .then(|| {
                    ReportPeriod::PILLS
                        .into_iter()
                        .find(|preset| preset.label().eq_ignore_ascii_case(&period))
                        .or_else(|| parse_period(&period).map(ReportPeriod::Custom))
                })
                .flatten()
                .ok_or_else(|| {
                    invalid(
                        "invalid_period",
                        "Choose a report preset or a positive duration.",
                    )
                })?;
            report(desk, "period", Some(ReportChange::Period(period)), tz)?
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
        ReportInput::Session { session } => {
            let session = key(session)?;
            scoped(
                desk,
                &dir,
                tz,
                ReportRequest::Session { session, day: None },
            )?
        }
        ReportInput::Day { session, day } => {
            let request = ReportRequest::Session {
                session: key(session)?,
                day: Some(civil_day(&day)?),
            };
            scoped(desk, &dir, tz, request)?
        }
        ReportInput::WholeDay { origin, day } => {
            let request = ReportRequest::Day {
                origin: report_origin(origin),
                day: civil_day(&day)?,
            };
            scoped(desk, &dir, tz, request)?
        }
        ReportInput::Tag { origin, tag } => {
            let origin = report_origin(origin);
            scoped(desk, &dir, tz, ReportRequest::Tag { origin, tag })?
        }
    };
    if picked.is_some() {
        // Merge all's next default, saved and fanned out like the ticket's switch.
        owner.persist(SharedChange::RiskSettings);
    }
    let result = serde_json::to_value(output)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    if !inspect_only {
        access.record_event(NewEvent::by(
            quantick_control_schema::session::MODULE_ID,
            EVENT_KIND,
            actor,
            json!({"report": result}),
        ));
    }
    Ok(result)
}

fn invalid(code: &str, message: &str) -> ControlError {
    let mut error = ControlError::invalid_request(message);
    error.context.details = Some(json!({ "code": code }));
    error
}

fn report(
    desk: &mut dyn crate::trade::TradeDesk,
    action: &str,
    change: Option<ReportChange>,
    tz: quantick_civil::TzOffset,
) -> Result<ReportOutput, ControlError> {
    Ok(readout(desk.report_change(change, tz), action, tz))
}

fn readout(view: ReportReadout, action: &str, tz: quantick_civil::TzOffset) -> ReportOutput {
    use quantick_control_schema::journal_sessions::JournalSourceFilter as WireSource;
    let sources = [WireSource::Broker, WireSource::Real, WireSource::Replay]
        .into_iter()
        .filter(|source| {
            view.source
                .contains(crate::journal_sessions::source_filter(*source))
        })
        .collect();
    ReportOutput::Sources {
        action: action.to_owned(),
        // The newest days, still oldest first: the edge of a long pick is
        // the one a caller acts on, and the count says how many came before.
        applied_days: view
            .days
            .iter()
            .skip(view.days.len().saturating_sub(MAX_LISTED_DAYS))
            .map(|day| day.iso())
            .collect(),
        applied_day_count: WireU64::new(view.days.len() as u64),
        timezone: tz.label(),
        symbols: view.symbols,
        currencies: (view.merge.currencies.iter())
            .map(|currency| currency.code().to_owned())
            .collect(),
        merge: view
            .merge
            .request
            .as_ref()
            .map(|currency| currency.code().to_owned()),
        merged: view
            .merge
            .readout
            .as_ref()
            .map(|readout| Box::new(merged(readout))),
        merge_note: view.merge.note.map(str::to_owned),
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

/// A merge's readout on the wire: rates per day, newest kept past the cap.
fn merged(readout: &MergeReadout) -> MergedWire {
    let skip = readout.rates.len().saturating_sub(MAX_LISTED);
    MergedWire {
        currency: readout.currency.code().to_owned(),
        assets: (readout.assets.iter())
            .map(|(symbol, currency)| [symbol.clone(), currency.code().to_owned()])
            .collect(),
        rates: (readout.rates.iter().skip(skip))
            .map(|rate| RateUsedWire {
                day: rate.day.iso(),
                from: rate.from.code().to_owned(),
                to: rate.to.code().to_owned(),
                rate: canonical_decimal(rate.rate),
            })
            .collect(),
        rate_count: WireU64::new(readout.rates.len() as u64),
        unconverted: (readout.unconverted.iter().take(MAX_LISTED))
            .map(|entry| UnconvertedWire {
                symbol: entry.symbol.clone(),
                day: entry.day.iso(),
                reason: match entry.why {
                    Unconvertible::PointValueUnset => "point_value_unset",
                    Unconvertible::NoRate(_) => "no_rate",
                }
                .to_owned(),
                from: match &entry.why {
                    Unconvertible::NoRate(from) => Some(from.code().to_owned()),
                    Unconvertible::PointValueUnset => None,
                },
                trades: WireU64::new(entry.trades as u64),
            })
            .collect(),
        unconverted_count: WireU64::new(readout.unconverted.len() as u64),
        net: canonical_decimal(readout.net),
        caption: readout.caption(),
    }
}

fn key(session: String) -> Result<SessionKey, ControlError> {
    SessionKey::parse(&session)
        .ok_or_else(|| crate::journal_sessions::refusal(SessionEditError::InvalidKey(session)))
}

fn civil_day(day: &str) -> Result<quantick_civil::CivilDate, ControlError> {
    quantick_civil::parse_iso_date(day)
        .ok_or_else(|| ControlError::invalid_request("day must be a valid YYYY-MM-DD"))
}

fn report_origin(origin: WireOrigin) -> ReportOrigin {
    match origin {
        WireOrigin::Simulator => ReportOrigin::Simulator,
        WireOrigin::Broker => ReportOrigin::Broker,
    }
}

fn scoped(
    desk: &mut dyn crate::trade::TradeDesk,
    dir: &std::path::Path,
    tz: quantick_civil::TzOffset,
    request: ReportRequest,
) -> Result<ReportOutput, ControlError> {
    let prepared = scope::prepare(dir, request, tz).map_err(crate::journal_sessions::refusal)?;
    let receipt = &prepared.receipt;
    let (action, session, tag) = match &receipt.request {
        ReportRequest::Session { session, day } => (
            if day.is_some() { "day" } else { "session" },
            Some(session.to_string()),
            None,
        ),
        ReportRequest::Day { .. } => ("whole_day", None, None),
        ReportRequest::Tag { tag, .. } => ("tag", None, Some(tag.clone())),
    };
    let result = ReportResult {
        action: action.to_owned(),
        session,
        origin: match receipt.request.origin() {
            ReportOrigin::Simulator => "simulator",
            ReportOrigin::Broker => "broker",
        }
        .to_owned(),
        day: receipt.request.day().map(|day| day.iso()),
        tag,
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
