//! Opening a Trades report by stable session identity, a civil day or a tag.
use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence},
    schema::generated_schema,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const REPORT_OPEN_CAPABILITY_ID: &str = "session.report.open";
pub const EVENT_KIND: &str = "session.report.opened";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportInput {
    Ordinary {},
    Inspect {},
    /// Set the picked days outright: YYYY-MM-DD civil days in the display
    /// timezone, any order, contiguous or not. An empty list clears them.
    /// Duplicates collapse; more than 92 distinct days, or an entry that is
    /// not a YYYY-MM-DD date, refuses the call with `invalid_dates`.
    Days {
        days: Vec<String>,
    },
    ClearDates {},
    Period {
        #[schemars(length(min = 1, max = 16))]
        period: String,
    },
    /// Set the union of sources, or omit sources to inspect the current report.
    Sources {
        #[serde(default)]
        #[schemars(length(max = 5))]
        sources: Option<Vec<crate::journal_sessions::JournalSourceFilter>>,
    },
    Session {
        #[schemars(length(max = 512))]
        session: String,
    },
    Day {
        #[schemars(length(max = 512))]
        session: String,
        #[schemars(length(min = 10, max = 10))]
        day: String,
    },
    /// Every trade of one origin closed on the civil day, in any session.
    WholeDay {
        origin: ReportOrigin,
        #[schemars(length(min = 10, max = 10))]
        day: String,
    },
    /// Every trade of one origin carrying the tag, over all history.
    Tag {
        origin: ReportOrigin,
        #[schemars(length(min = 1, max = 33))]
        tag: String,
    },
    /// Set the symbols the report reads outright, as its multi-select does;
    /// an empty list reads every symbol. A symbol with no journal folder
    /// refuses the call with `unknown_symbol`.
    Symbols {
        #[schemars(length(max = 64))]
        symbols: Vec<String>,
    },
    /// Merge every curve into one money curve in `currency`, or read per
    /// asset with null. The currency must be one the report offers (the
    /// selection's currencies and USD), else `invalid_currency`.
    Merge {
        #[schemars(length(min = 1, max = 8))]
        currency: Option<String>,
    },
}

/// One rate a merged report converted with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RateUsedWire {
    /// YYYY-MM-DD, the civil day of the closes it converted.
    pub day: String,
    pub from: String,
    pub to: String,
    pub rate: quantick_control::wire::CanonicalDecimal,
}

/// Trades of one symbol on one day a merge left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnconvertedWire {
    pub symbol: String,
    pub day: String,
    /// `point_value_unset` or `no_rate`.
    pub reason: String,
    /// The currency without a rate, for `no_rate`.
    pub from: Option<String>,
    pub trades: quantick_control::wire::WireU64,
}

/// What a merge converted, with which rates, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MergedWire {
    pub currency: String,
    /// The merged symbols, each with its own currency.
    pub assets: Vec<[String; 2]>,
    /// The rates used per day, oldest first; past 16, only the newest 16.
    pub rates: Vec<RateUsedWire>,
    pub rate_count: quantick_control::wire::WireU64,
    /// What was not converted, never guessed; past 16, only the first 16.
    pub unconverted: Vec<UnconvertedWire>,
    pub unconverted_count: quantick_control::wire::WireU64,
    /// The merged net in `currency`.
    pub net: quantick_control::wire::CanonicalDecimal,
    /// The caption under the merged curve.
    pub caption: String,
}

/// The book a whole-day or tag report reads; the two are never summed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportOrigin {
    Simulator,
    Broker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReportResult {
    pub action: String,
    /// The session of a session or day action; null for whole_day and tag.
    pub session: Option<String>,
    pub origin: String,
    pub day: Option<String>,
    /// The normalized tag of a tag action, without `#`.
    pub tag: Option<String>,
    pub title: String,
    pub trade_count: quantick_control::wire::WireU64,
    /// Files unreadable in the history scanned for this report.
    pub unreadable_files: quantick_control::wire::WireU64,
    /// Malformed rows skipped in that history.
    pub skipped_rows: quantick_control::wire::WireU64,
    pub net_points: quantick_control::wire::CanonicalDecimal,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ReportOutput {
    Scoped(ReportResult),
    Ordinary {
        action: String,
        title: String,
        symbol: Option<String>,
    },
    Sources {
        action: String,
        sources: Vec<crate::journal_sessions::JournalSourceFilter>,
        open: bool,
        loaded: bool,
        symbol: Option<String>,
        window: String,
        /// The picked days in force, oldest first; past 92, only the
        /// newest 92. Empty while the period is in force.
        applied_days: Vec<String>,
        /// How many days are picked, including any past the listed 92.
        applied_day_count: quantick_control::wire::WireU64,
        timezone: String,
        /// The symbols chosen; empty reads every symbol.
        symbols: Vec<String>,
        /// The currencies a merge can read in.
        currencies: Vec<String>,
        /// The merge in force; null reads per asset.
        merge: Option<String>,
        /// What the merge converted; null per asset or in points.
        merged: Option<MergedWire>,
        /// Why a merge asked of a points report kept one curve per instrument.
        merge_note: Option<String>,
        scope: Option<String>,
        trade_count: quantick_control::wire::WireU64,
        unreadable_files: quantick_control::wire::WireU64,
        skipped_rows: quantick_control::wire::WireU64,
        net_points: quantick_control::wire::CanonicalDecimal,
    },
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        REPORT_OPEN_CAPABILITY_ID,
        "Open or filter a performance report",
        "Open the same performance report as the Trades session context menu. action=session selects only that session; action=day requires YYYY-MM-DD in the display timezone and selects every instrument and session of that origin on the clicked day. The session must hold a trade on that day. action=whole_day takes origin (simulator or broker) and day instead of a session and selects every trade of that origin closed that day, as the day header's context menu does. action=tag takes origin and a tag (with or without #, case ignored) and selects every trade of that origin carrying it over all history, as a trade's Report #tag menu does. Simulator and broker are never summed. action=ordinary restores the ordinary Sim live report for the chart symbol over all time; pass no session or day. action=sources with sources sets their union, exits session/day context scope and preserves visible symbol/date filters: broker, real (Sim live including legacy), replay, simulator (real + replay), both (all). An empty array selects none. Omitting sources reads the current report without opening it or emitting session.report.opened. action=inspect also reads without opening or emitting. action=days sets the picked days outright, as the report's Dates popover does: days is a list of up to 92 distinct YYYY-MM-DD civil days in the display timezone, any order, contiguous or not; a trade is in when the day it closed on is listed. Repeating the call sets the same days; an empty list clears them. A day that is not a real YYYY-MM-DD date refuses the whole call. action=symbols sets the symbols the report reads outright, as its multi-select does (empty reads every symbol); a symbol with no journal folder refuses with unknown_symbol. action=merge with a currency merges every curve into one money curve in it: each trade converted at the stored rate of the civil day it closed on, USDT and USDC read as USD, a pair converting both ways; trades with no point value or no rate that day are left out and listed in merged.unconverted, never guessed. The currency must be one listed in currencies (the selection's currencies plus USD), else invalid_currency; it becomes the default Merge all reads in. currency null reads per asset. Points never merge across instruments: a points report keeps one curve per instrument and says so in merge_note. These actions return symbols, currencies, merge and merged (currency, assets, rates per day, unconverted, net, caption). action=clear_dates clears the picked days, restoring the selected period. action=period sets Today, 7d, 30d, 90d, All or a custom duration such as 12h anchored to the newest close, and clears picked days. Date and period edits retain symbol, sources and tag restrictions; picked days other than a day context's own day take a whole-session or day context out of force and keep its origin. These actions return applied_days (oldest first; past 92 days, only the newest 92), applied_day_count and timezone. Refusals carry details.code invalid_dates or invalid_period. Returns normalized sources, window, context, open/loaded status, trade count and net points. Retries set the same selection, never toggle. Session keys are <symbol>/<file>.csv for the simulator and broker/<symbol>/<file>.csv for real-trades, independent of custom names. Session, day and ordinary actions replace prior symbol, period, source and session filters. Returns the opened scope, trade count, net points and counts of unreadable files and skipped rows in the scanned history; events.read reconciles a lost open response through session.report.opened. Refusals: invalid_currency, unknown_symbol, invalid_session, session_not_found, session_unreadable, day_not_in_session, invalid_tag, no_trades_in_scope, no_ledger. Never places orders or writes history.",
        false,
        generated_schema::<ReportInput>(),
    );
    descriptor.module =
        quantick_control::id::ModuleId::new(crate::session::MODULE_ID).expect("static module ID");
    descriptor.output_schema = generated_schema::<ReportOutput>();
    descriptor.persistence = EffectPersistence::Transient;
    descriptor.stale_input_safety = Some("Repeating a request opens the same scope from current history; deleted sessions are refused before changing the report.".to_owned());
    descriptor.expected_cost.max_response_bytes = Some(8192);
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::journal(
    REPORT_OPEN_CAPABILITY_ID,
    quantick_control::registry::IdempotencyPolicy::Optional,
    EVENT_KIND,
    "payload.report",
    "After a successful report open",
    &[crate::readback::EVERY_OPTIONAL_TEST],
)];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "session-report-input-v1.schema.json",
            generated_schema::<ReportInput>(),
        ),
        (
            "session-report-result-v1.schema.json",
            generated_schema::<ReportOutput>(),
        ),
    ]
}
