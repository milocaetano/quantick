//! `session.journal.set` and `session.journal.delete`: the Trades ledger's
//! session acts — list through the ledger's filter, rename, tag, set what
//! the ledger shows, delete — over [`JournalSessionsPort`].
//!
//! Every write goes through the port's `edit_journal`, which the window
//! answers with the very call its ledger gestures make, so a refusal here and
//! a refusal on screen are one refusal. Listing reads the folder directly:
//! it changes nothing the window holds. A view call sets the active tab's
//! ledger through the same state its filter row and fold carets change.

use quantick_civil::{DateRange, TzOffset, parse_iso_date};
use quantick_control::{
    error::{ControlError, codes},
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_host::{admission::known_error, journal::NewEvent, wire::canonical_decimal};
use quantick_control_schema::journal_sessions::{
    JOURNAL_SESSIONS_EVENT_KIND, JournalDeleteInput, JournalLedgerView, JournalSessionSnapshot,
    JournalSessionsInput, JournalSessionsResult, JournalSourceFilter, JournalTaggedTrade,
    MAX_LISTED_SESSIONS, MAX_LISTED_TAGGED_TRADES, delete_descriptor, descriptor,
};
use quantick_control_schema::session::MODULE_ID;
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::report::sessions::{
    LedgerFilter, SessionInfo, SessionKey, TradeRef, live_keys, source_word, summarize_sessions,
};
use quantick_paper::report::{SourceFilter, load_history};

use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::notify::NotifyAccess;

/// What the journal's session acts need of the window.
pub trait JournalSessionsPort {
    /// The trades folder and every file a tab is still journaling to.
    fn journal_scope(&mut self) -> JournalScope;
    /// The display timezone the ledger stamps civil days and labels with.
    fn journal_timezone(&mut self) -> TzOffset;
    /// Apply one edit exactly as the ledger's gesture does, and have every
    /// open ledger and report re-read the folder.
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError>;
    /// Apply `change` to the active tab's ledger — an empty one only reads —
    /// and answer what it shows and whether that moved; `None` when no tab
    /// holds a ledger.
    fn ledger_view(&mut self, change: &LedgerViewChange) -> Option<(LedgerView, bool)>;
}

/// Dock both capabilities. An edit journals through the access the notify
/// handlers journal through, on its clock.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: JournalSessionsPort + ?Sized,
    A: NotifyAccess,
{
    registry.register(descriptor(), journal_sessions)?;
    registry.register(delete_descriptor(), journal_delete)
}

fn journal_sessions<H: JournalSessionsPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: JournalSessionsInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let edit = match input {
        JournalSessionsInput::List {
            source,
            from,
            to,
            search,
            symbol,
        } => {
            let filter = LedgerFilter {
                source: source_filter(source.unwrap_or_default()),
                range: date_range(from.as_deref(), to.as_deref())?,
                search: search.unwrap_or_default(),
            };
            return to_value(&list(app, &filter, symbol.as_deref()));
        }
        JournalSessionsInput::View {
            clear,
            source,
            from,
            to,
            search,
            fold,
        } => {
            let change = LedgerViewChange {
                clear,
                source: source.map(source_filter),
                range: date_range(from.as_deref(), to.as_deref())?.map(Some),
                search,
                fold: fold
                    .iter()
                    .map(|fold| Ok((key(&fold.session)?, fold.folded)))
                    .collect::<Result<_, ControlError>>()?,
            };
            return view(app, access, actor, &change);
        }
        JournalSessionsInput::Rename { session, name } => JournalEdit::Rename {
            session: key(&session)?,
            name,
        },
        JournalSessionsInput::Tag {
            session,
            opened_unix_ms,
            closed_unix_ms,
            nth,
            tags,
        } => JournalEdit::Tag {
            session: key(&session)?,
            trade: TradeRef {
                opened_ms: opened_unix_ms,
                closed_ms: closed_unix_ms,
                nth,
            },
            tags,
        },
    };
    run_edit(app, access, actor, &edit)
}

fn journal_delete<H: JournalSessionsPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: JournalDeleteInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let edit = JournalEdit::Delete {
        session: key(&input.session)?,
    };
    run_edit(app, access, actor, &edit)
}

/// One edit through the window, read back and journaled.
fn run_edit<H: JournalSessionsPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    edit: &JournalEdit,
) -> Result<Value, ControlError> {
    let outcome = app.edit_journal(edit).map_err(refusal)?;
    let scope = app.journal_scope();
    let tz = app.journal_timezone();
    let live = live_keys(&scope.dir, &scope.live);
    let result = match &outcome {
        JournalEditOutcome::Deleted { moved_to, .. } => JournalSessionsResult {
            moved_to: Some(
                moved_to
                    .strip_prefix(&scope.dir)
                    .unwrap_or(moved_to)
                    .to_string_lossy()
                    .replace('\\', "/"),
            ),
            ..answer("delete", true)
        },
        JournalEditOutcome::Renamed { session, .. }
        | JournalEditOutcome::Tagged { session, .. } => {
            let action = match outcome {
                JournalEditOutcome::Renamed { .. } => "rename",
                _ => "tag",
            };
            JournalSessionsResult {
                session: SessionInfo::read(&scope.dir, session, live.contains(session))
                    .map(|info| snapshot(&info, info.label(tz), info.trades, info.net)),
                ..answer(action, true)
            }
        }
    };
    journal_event(access, actor, &result)?;
    to_value(&result)
}

/// Set or read the active ledger's view; a change that moved it journals.
fn view<H: JournalSessionsPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    change: &LedgerViewChange,
) -> Result<Value, ControlError> {
    let (view, changed) = app.ledger_view(change).ok_or_else(|| {
        let mut error = known_error(
            codes::CAPABILITY_UNAVAILABLE,
            "no tab holds a Trades ledger",
            false,
        );
        error.context.details = Some(json!({ "code": "no_ledger" }));
        error.context.next_steps = vec!["open a chart tab and call again".to_owned()];
        error
    })?;
    let result = JournalSessionsResult {
        view: Some(ledger_view(&view)),
        ..answer("view", changed)
    };
    if changed {
        journal_event(access, actor, &result)?;
    }
    to_value(&result)
}

fn journal_event<A: NotifyAccess>(
    access: &mut A,
    actor: &ActorContext,
    result: &JournalSessionsResult,
) -> Result<(), ControlError> {
    let payload = json!({ "journal_session": to_value(result)? });
    access.record_event(NewEvent::by(
        MODULE_ID,
        JOURNAL_SESSIONS_EVENT_KIND,
        actor,
        payload,
    ));
    Ok(())
}

/// A result carrying nothing but its action.
fn answer(action: &str, changed: bool) -> JournalSessionsResult {
    JournalSessionsResult {
        action: action.to_owned(),
        changed,
        session: None,
        moved_to: None,
        view: None,
        sessions: Vec::new(),
        session_count: WireU64::new(0),
        sessions_truncated: false,
        hidden_trades: WireU64::new(0),
    }
}

/// The listing: the folder read with nothing excluded, so a live session's
/// trades count from its file like every other session's.
fn list<H: JournalSessionsPort + ?Sized>(
    app: &mut H,
    filter: &LedgerFilter,
    symbol: Option<&str>,
) -> JournalSessionsResult {
    let scope = app.journal_scope();
    let tz = app.journal_timezone();
    let history = load_history(&scope.dir, symbol, &[]);
    let live = live_keys(&scope.dir, &scope.live);
    let (summaries, hidden) = summarize_sessions(&history, filter, tz, &live);
    let count = summaries.len();
    let sessions = summaries
        .iter()
        .take(MAX_LISTED_SESSIONS)
        .map(|summary| {
            let mut info = summary.info.clone();
            info.live = summary.live;
            snapshot(&info, summary.label.clone(), summary.trades, summary.net)
        })
        .collect();
    JournalSessionsResult {
        sessions,
        session_count: wire_count(count),
        sessions_truncated: count > MAX_LISTED_SESSIONS,
        hidden_trades: wire_count(hidden),
        ..answer("list", false)
    }
}

fn snapshot(
    info: &SessionInfo,
    label: String,
    trades: usize,
    net: rust_decimal::Decimal,
) -> JournalSessionSnapshot {
    let tagged_trades = info
        .tags
        .iter()
        .take(MAX_LISTED_TAGGED_TRADES)
        .map(|(trade, tags)| JournalTaggedTrade {
            opened_unix_ms: trade.opened_ms,
            closed_unix_ms: trade.closed_ms,
            nth: trade.nth,
            tags: tags.clone(),
        })
        .collect();
    JournalSessionSnapshot {
        session: info.key.to_string(),
        symbol: info.symbol.clone(),
        source: source_word(info.source).to_lowercase(),
        name: info.name.clone(),
        label,
        live: info.live,
        trade_count: wire_count(trades),
        net_points: canonical_decimal(net),
        first_closed_unix_ms: info.first_closed_ms,
        tagged_trades,
        tagged_trades_truncated: info.tags.len() > MAX_LISTED_TAGGED_TRADES,
    }
}

fn ledger_view(view: &LedgerView) -> JournalLedgerView {
    JournalLedgerView {
        source: match view.filter.source {
            SourceFilter::Real => JournalSourceFilter::Real,
            SourceFilter::Replay => JournalSourceFilter::Replay,
            SourceFilter::All => JournalSourceFilter::Both,
        },
        from: view.filter.range.map(|range| range.start.iso()),
        to: view.filter.range.map(|range| range.end.iso()),
        search: view.filter.search.clone(),
        folded_sessions: view.folded.iter().map(SessionKey::to_string).collect(),
    }
}

fn wire_count(count: usize) -> WireU64 {
    WireU64::new(u64::try_from(count).unwrap_or(u64::MAX))
}

fn source_filter(source: JournalSourceFilter) -> SourceFilter {
    match source {
        JournalSourceFilter::Real => SourceFilter::Real,
        JournalSourceFilter::Replay => SourceFilter::Replay,
        JournalSourceFilter::Both => SourceFilter::All,
    }
}

/// `from`..=`to`; either alone keeps that one day.
fn date_range(from: Option<&str>, to: Option<&str>) -> Result<Option<DateRange>, ControlError> {
    let day = |field: &str, text: &str| {
        parse_iso_date(text).ok_or_else(|| {
            let mut error = ControlError::invalid_request(format!(
                "{field} must be YYYY-MM-DD, found `{text}`"
            ));
            error.context.details = Some(json!({ "field": field, "sent": text }));
            error
        })
    };
    Ok(match (from, to) {
        (None, None) => None,
        (Some(from), None) => Some(DateRange::new(day("from", from)?, day("from", from)?)),
        (None, Some(to)) => Some(DateRange::new(day("to", to)?, day("to", to)?)),
        (Some(from), Some(to)) => Some(DateRange::new(day("from", from)?, day("to", to)?)),
    })
}

fn key(text: &str) -> Result<SessionKey, ControlError> {
    SessionKey::parse(text).ok_or_else(|| refusal(SessionEditError::InvalidKey(text.to_owned())))
}

/// An edit's refusal, its stable `details.code` beside the control code.
fn refusal(error: SessionEditError) -> ControlError {
    let code = match &error {
        SessionEditError::NotFound(_) => codes::RESOURCE_GONE,
        SessionEditError::LiveSession(_)
        | SessionEditError::WritesRefused(_)
        | SessionEditError::Io(_) => codes::CAPABILITY_UNAVAILABLE,
        SessionEditError::InvalidKey(_)
        | SessionEditError::TradeNotFound(..)
        | SessionEditError::InvalidTag(_)
        | SessionEditError::Unreadable(..) => codes::INVALID_REQUEST,
    };
    let next_step = match &error {
        SessionEditError::LiveSession(_) => {
            "a tab is still journaling to this session; it can be deleted once that tab moves to another feed, replay or symbol"
        }
        SessionEditError::NotFound(_) | SessionEditError::InvalidKey(_) => {
            "call session.journal.set with action list and send a session exactly as listed"
        }
        SessionEditError::TradeNotFound(..) => {
            "send opened_unix_ms, closed_unix_ms and nth of a trade the session holds, as session.paper, the ledger or a listing reports them"
        }
        SessionEditError::InvalidTag(_) => "send tags as single words, with or without a leading #",
        SessionEditError::Unreadable(..) => {
            "the file is not a quantick-trades file; it cannot be edited"
        }
        SessionEditError::WritesRefused(_) => {
            "another instance owns the stores; edit from that instance"
        }
        SessionEditError::Io(_) => "retry; the trades folder refused the write",
    };
    let mut control = known_error(code, error.to_string(), error.retryable());
    control.context.details = Some(json!({ "code": error.code() }));
    control.context.next_steps = vec![next_step.to_owned()];
    control
}

fn to_value(result: &JournalSessionsResult) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

#[cfg(test)]
#[path = "journal_sessions_tests.rs"]
mod journal_sessions_tests;
