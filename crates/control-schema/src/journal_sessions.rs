//! The trades journal's sessions as named calls: list them through the
//! ledger's own filter, name one, tag one of its trades, set or read what the
//! ledger on screen shows — and, as a capability of its own, delete one.
//!
//! A session is one journal file. The ledger's gestures and these
//! capabilities reach the same edit in `quantick-paper`
//! (`report::session_edit`), so the two refuse the same things in the same
//! words. `session.journal.set` sits in the `session` module beside
//! `session.paper`, under the plain cockpit permission the deal recorder
//! uses: it writes words the trader keeps into the session's own file and
//! changes what the ledger shows, and removes nothing. A delete does remove
//! something, so it is a second capability — the split `feed.reconnect` and
//! `feed.reload` made, for the reason they made it: what a client must read
//! before calling cannot depend on an input field. `session.journal.delete`
//! needs `cockpit.journal.delete`, a sensitive permission off until the
//! trader ticks it, declares the `journal_session_removed` risk flag and is
//! not reversible. The file moves into `.deleted/` under the trades folder
//! rather than being erased, and the session a tab is still journaling to is
//! refused with `live_session`.

use std::collections::BTreeSet;

use quantick_control::{
    id::{EffectId, PermissionId, RiskFlagId},
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    schema::generated_schema,
    wire::{CanonicalDecimal, WireU64},
};
use quantick_control_host::authority::{
    COCKPIT_JOURNAL_DELETE_PERMISSION_ID, COCKPIT_PERMISSION_ID, JOURNAL_DELETE_EFFECT_ID,
    SESSION_REMOVED_RISK_FLAG,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};

pub const JOURNAL_SESSIONS_CAPABILITY_ID: &str = "session.journal.set";

pub const JOURNAL_DELETE_CAPABILITY_ID: &str = "session.journal.delete";

/// What every rename, tag, view change and delete journals.
pub const JOURNAL_SESSIONS_EVENT_KIND: &str = "session.journal.changed";

/// Most sessions one listing returns; the count says how many matched.
pub const MAX_LISTED_SESSIONS: usize = 200;

/// Most tagged trades one session row lists.
pub const MAX_LISTED_TAGGED_TRADES: usize = 64;

/// Most sessions one view call folds or opens.
pub const MAX_VIEW_FOLDS: usize = 64;

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
        /// Which of the session's trades with these two times: 0, the
        /// default, is the first. Only partial closes of one position or
        /// fills in one millisecond share both; a listing reports it.
        #[serde(default)]
        nth: u32,
        #[schemars(length(max = 16))]
        tags: Vec<String>,
    },
    /// Set, or with no field read, what the active tab's Trades ledger
    /// shows: its filter row and the sessions folded shut. Fields left out
    /// keep their value.
    View {
        /// Drop every filter first — both sources, any date, no search — so
        /// the rest of the call states the whole filter.
        #[serde(default)]
        clear: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<JournalSourceFilter>,
        /// First civil day shown, `YYYY-MM-DD` in the display timezone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        /// Last civil day shown, inclusive; alone, it shows that one day.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(max = 256))]
        search: Option<String>,
        /// Sessions to fold shut or open inside their day.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[schemars(length(max = MAX_VIEW_FOLDS))]
        fold: Vec<JournalSessionFold>,
    },
}

/// One session folded or opened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalSessionFold {
    pub session: String,
    pub folded: bool,
}

/// `session.journal.delete`'s input.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalDeleteInput {
    /// `<symbol folder>/<file>.csv`, as a listing names it.
    pub session: String,
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
    /// How many earlier trades of the session share both times.
    pub nth: u32,
    pub tags: Vec<String>,
}

/// What the active tab's Trades ledger shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JournalLedgerView {
    pub source: JournalSourceFilter,
    /// The first civil day shown; absent shows every day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub search: String,
    /// Sessions folded shut inside their day.
    pub folded_sessions: Vec<String>,
}

/// What the call did, and the sessions it reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JournalSessionsResult {
    /// The action performed.
    pub action: String,
    /// Whether a file or the ledger's view changed.
    pub changed: bool,
    /// The edited session after the edit; absent for a listing, a view and
    /// a delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<JournalSessionSnapshot>,
    /// A delete: where the file went, relative to the trades folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<String>,
    /// A view call: what the ledger shows after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<JournalLedgerView>,
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
        "List, name or tag trade journal sessions, or set the Trades ledger's view",
        "The Trades ledger's session acts that remove nothing. A session is one journal file under the trades folder, named `<symbol folder>/<file>.csv`; three replays of one tape are three sessions. `list` returns the sessions holding a trade the ledger's filter keeps (source pills, a civil date span, and a search where `#tag` tokens must all be on the trade and other words must appear in the session's name or default label), newest first and bounded, and changes nothing. `rename` and `tag` append a line to the session's own file (the last line wins, the trade columns never change), the live session included. `view` sets the active tab's ledger filter and folds sessions shut or open, and answers what the ledger then shows; with no field it only reads. Deleting a session is `session.journal.delete`. Refusals carry `details.code`: invalid_session, session_not_found, trade_not_found, invalid_tag, session_unreadable, writes_refused, write_failed, no_ledger. Every edit and view change journals `session.journal.changed`, and every open ledger and report re-reads the folder after an edit.",
        false,
        generated_schema::<JournalSessionsInput>(),
    );
    descriptor.module = quantick_control::id::ModuleId::new(crate::session::MODULE_ID)
        .expect("static module ID is valid");
    descriptor.output_schema = generated_schema::<JournalSessionsResult>();
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.stale_input_safety = Some(
        "Every action names its session by file. A repeated rename, tag or view writes the same name, tags or view again and leaves the same state; a stale listing can only name a session that has since moved, which the next edit refuses."
            .to_owned(),
    );
    descriptor
}

/// `session.journal.delete`: the one journal act that removes the trader's
/// work, declared that way in every field a client reads before calling.
pub fn delete_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        JOURNAL_DELETE_CAPABILITY_ID,
        "Delete a trade journal session",
        "Moves one session file out of the trades journal into `.deleted/` under the trades folder: the ledger, the report and every listing stop reading it, and only moving the file back by hand restores it — nothing in this contract does. A session a tab is still journaling to is refused with `live_session`, however the key is spelled. Needs `cockpit.journal.delete`, which is off until the trader ticks it. Refusals carry `details.code`: invalid_session, session_not_found, live_session, writes_refused, write_failed. A delete journals `session.journal.changed`, and every open ledger and report re-reads the folder.",
        false,
        generated_schema::<JournalDeleteInput>(),
    );
    descriptor.module = quantick_control::id::ModuleId::new(crate::session::MODULE_ID)
        .expect("static module ID is valid");
    descriptor.output_schema = generated_schema::<JournalSessionsResult>();
    descriptor.effect = EffectId::new(JOURNAL_DELETE_EFFECT_ID).expect("static effect ID is valid");
    descriptor.required_permissions = [COCKPIT_PERMISSION_ID, COCKPIT_JOURNAL_DELETE_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect();
    descriptor.risk_flags = BTreeSet::from([
        RiskFlagId::new(SESSION_REMOVED_RISK_FLAG).expect("static risk flag is valid")
    ]);
    descriptor.persistence = EffectPersistence::Durable;
    // Nothing in the contract moves the file back. `destructive` stays false
    // for the reason `feed.reload`'s does (see `crate::recovery`): the
    // registry couples it to an expected-revision check this host refuses,
    // so the loss is declared by the permission, the risk flag and this.
    descriptor.reversible = false;
    descriptor.stale_input_safety = Some(
        "The call names its session by file. A repeated delete finds nothing and is refused with session_not_found, removing nothing more; a stale listing can only name a session that has since moved, which the call refuses the same way."
            .to_owned(),
    );
    descriptor
}

/// How a client reconciles an interrupted edit or delete.
pub const READBACKS: &[Readback] = &[
    journal(
        JOURNAL_SESSIONS_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        JOURNAL_SESSIONS_EVENT_KIND,
        "payload.journal_session",
        "an event after the pre-call cursor names the session and the name or tags, or the ledger view, the call asked for",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        JOURNAL_DELETE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        JOURNAL_SESSIONS_EVENT_KIND,
        "payload.journal_session",
        "an event after the pre-call cursor names the deleted session and where its file moved",
        &[EVERY_OPTIONAL_TEST],
    ),
];
