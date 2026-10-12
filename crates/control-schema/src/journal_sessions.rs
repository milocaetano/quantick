//! The trades journal's sessions as a named call: list them through the
//! ledger's own filter, name one, tag one of its trades, or delete it.
//!
//! A session is one journal file. The ledger's gestures and this capability
//! reach the same edit in `quantick-paper` (`report::session_edit`), so the
//! two refuse the same things in the same words. It sits in the `session`
//! module beside `session.paper`, under the plain cockpit permission the
//! deal recorder uses: it writes files the trader keeps and nothing the chart
//! or a position holds. A delete moves the file into `.deleted/` under the
//! trades folder rather than erasing it, and the session a tab is still
//! journaling to is refused with `live_session`.

use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    schema::generated_schema,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};

pub const JOURNAL_SESSIONS_CAPABILITY_ID: &str = "session.journal.set";

/// What every rename, tag and delete journals.
pub const JOURNAL_SESSIONS_EVENT_KIND: &str = "session.journal.changed";

/// Most sessions one listing returns; the count says how many matched.
pub const MAX_LISTED_SESSIONS: usize = 200;

/// Most tagged trades one session row lists.
pub const MAX_LISTED_TAGGED_TRADES: usize = 64;

/// The ledger's source pills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JournalSourceFilter {
    /// Live sessions, and files from before a source was recorded.
    Real,
    /// Replay-driven practice sessions.
    Replay,
    /// Everything.
    #[default]
    Both,
}

/// One thing to do with the journal's sessions.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalSessionsInput {
    /// List sessions holding at least one trade the filter keeps, newest
    /// first. Changes nothing.
    List {
        /// Defaults to `both`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<JournalSourceFilter>,
        /// First civil day kept, `YYYY-MM-DD` in the display timezone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        /// Last civil day kept, inclusive; alone, it keeps that one day.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        /// The ledger's search box: `#tag` tokens must all be on a trade,
        /// other words must all appear in its session's name or label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(max = 256))]
        search: Option<String>,
        /// One instrument's folder; omitted lists every instrument.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        symbol: Option<String>,
    },
    /// Name a session. An empty name clears it; past 80 characters it is cut.
    Rename {
        session: String,
        #[schemars(length(max = 256))]
        name: String,
    },
    /// Set one trade's tags, replacing what it had; none clears them.
    Tag {
        session: String,
        #[schemars(extend("x-unit" = "unix_milliseconds"))]
        opened_unix_ms: i64,
        #[schemars(extend("x-unit" = "unix_milliseconds"))]
        closed_unix_ms: i64,
        #[schemars(length(max = 16))]
        tags: Vec<String>,
    },
    /// Move a session file out of the history. The live session is refused.
    Delete { session: String },
}

/// One session, as a listing or an edit reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JournalSessionSnapshot {
    /// `<symbol folder>/<file>.csv` — what every edit takes.
    pub session: String,
    pub symbol: String,
    /// `live`, `replay` or `unrecorded`.
    pub source: String,
    /// The trader's name; absent until one is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The name, or the default label the ledger shows.
    pub label: String,
    /// A tab is still journaling to it: it cannot be deleted.
    pub live: bool,
    /// Trades kept by the filter (a listing), or every trade (an edit).
    pub trade_count: WireU64,
    #[schemars(extend("x-unit" = "points"))]
    pub net_points: CanonicalDecimal,
    /// When its first trade closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub first_closed_unix_ms: Option<i64>,
    /// Its tagged trades.
    #[schemars(length(max = MAX_LISTED_TAGGED_TRADES))]
    pub tagged_trades: Vec<JournalTaggedTrade>,
    pub tagged_trades_truncated: bool,
}

/// One tagged trade.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JournalTaggedTrade {
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub opened_unix_ms: i64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub closed_unix_ms: i64,
    pub tags: Vec<String>,
}

/// What the call did, and the sessions it reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JournalSessionsResult {
    /// The action performed.
    pub action: String,
    /// Whether a file changed.
    pub changed: bool,
    /// The edited session after the edit; absent for a listing and a delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<JournalSessionSnapshot>,
    /// A delete: where the file went, relative to the trades folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<String>,
    /// A listing: the sessions, newest first.
    #[schemars(length(max = MAX_LISTED_SESSIONS))]
    pub sessions: Vec<JournalSessionSnapshot>,
    /// A listing: every session that matched, listed or not.
    pub session_count: WireU64,
    pub sessions_truncated: bool,
    /// A listing: trades the filter kept out.
    pub hidden_trades: WireU64,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        JOURNAL_SESSIONS_CAPABILITY_ID,
        "List, name, tag or delete trade journal sessions",
        "The Trades ledger's session acts. A session is one journal file under the trades folder, named `<symbol folder>/<file>.csv`; three replays of one tape are three sessions. `list` returns the sessions holding a trade the ledger's filter keeps (source pills, a civil date span, and a search where `#tag` tokens must all be on the trade and other words must appear in the session's name or default label), newest first and bounded, and changes nothing. `rename` and `tag` append a line to the session's own file (the last line wins, the trade columns never change), the live session included. `delete` moves the file into `.deleted/` under the trades folder, where no listing reads it and from where it can be moved back by hand; a session a tab is still journaling to is refused with `live_session`. Refusals carry `details.code`: invalid_session, session_not_found, live_session, trade_not_found, invalid_tag, session_unreadable, writes_refused, write_failed. Every edit journals `session.journal.changed`, and every open ledger and report re-reads the folder.",
        false,
        generated_schema::<JournalSessionsInput>(),
    );
    descriptor.module = quantick_control::id::ModuleId::new(crate::session::MODULE_ID)
        .expect("static module ID is valid");
    descriptor.output_schema = generated_schema::<JournalSessionsResult>();
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.stale_input_safety = Some(
        "Every action names its session by file. A repeated rename or tag writes the same name or tags again and leaves the same state; a repeated delete finds nothing and is refused with session_not_found, removing nothing more; a stale listing can only name a session that has since moved, which the next edit refuses."
            .to_owned(),
    );
    descriptor
}

/// How a client reconciles an interrupted edit.
pub const READBACKS: &[Readback] = &[journal(
    JOURNAL_SESSIONS_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    JOURNAL_SESSIONS_EVENT_KIND,
    "payload.journal_session",
    "an event after the pre-call cursor names the session and the name, tags or deletion the call asked for",
    &[EVERY_OPTIONAL_TEST],
)];
