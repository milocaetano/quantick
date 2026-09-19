//! Recorded-session playback and paper-trading projections.
//!
//! Two scopes, one owner module: what the trader is *replaying*, and what the
//! simulator holds while they do. Both read state the application already
//! maintains; neither recomputes anything inside a capture.

use quantick_control_host::wire::{AvailabilitySnapshot, canonical_decimal};

use quantick_control::{
    limits::{CONTROL_SNAPSHOT_MAX_CLOSED_TRADES, CONTROL_SNAPSHOT_MAX_WORKING_ORDERS},
    wire::{CanonicalDecimal, WireU64},
};

use quantick_sim::{ClosedTrade, Order};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const REPLAY_SCOPE_ID: &str = "session.replay";

pub const PAPER_SCOPE_ID: &str = "session.paper";

pub const MODULE_ID: &str = "session";

pub const SCHEMA_VERSION: u32 = 1;

/// Playback speed and progress are UI floats; six places is the same
/// resolution `health.rs` publishes its metrics at.
pub const RATIO_DECIMAL_PLACES: u32 = 6;

/// The one provenance string the paper ledger has always carried, shared with
/// the selection scope so a client reading both sees one name for one source.
pub const PAPER_PROVENANCE: &str = "paper_trading_session_ledger";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplaySnapshot {
    pub tabs: Vec<TabReplaySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabReplaySnapshot {
    pub tab_id: WireU64,
    /// A tab playing no recording is live; `session` is then absent rather
    /// than a zeroed row, so "not replaying" cannot be mistaken for "at the
    /// start of a recording".
    pub replaying: bool,
    pub session: Option<ReplaySessionSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplaySessionSnapshot {
    pub symbol: String,
    /// Session day as `YYYY-MM-DD`, when the file name followed the
    /// convention. Absent is honest: the recording did not say.
    pub date: Option<String>,
    /// The recording's file name only. The absolute path is deliberately not
    /// published: it identifies nothing a client needs and would leak the
    /// trader's directory layout to every observer.
    pub file_name: String,
    pub playing: bool,
    pub finished: bool,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub position_unix_ms: i64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub start_unix_ms: i64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub end_unix_ms: i64,
    /// Wall time spent playing, which a seek resets — not the distance
    /// travelled through the recording, which is `position_unix_ms`.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub elapsed_ms: i64,
    /// Prints of *this session's own day* released so far, and how many it
    /// holds — the two numbers the transport bar draws.
    ///
    /// A day joined in front of the recording is context the chart was handed,
    /// not part of the session being rehearsed, so it is counted out of both.
    /// Publishing the raw cursor here instead would have an operator read
    /// `played_trades = 1_504_020` beside `progress = 0` on a replay that has
    /// not started, and disagree with the screen about the same session.
    pub played_trades: WireU64,
    pub total_trades: WireU64,
    /// How many prints in front of this session came from the day joined
    /// before it — `0` when none was, and what turns the two counts above back
    /// into the whole stream.
    ///
    /// Additive within v1 (contract §4).
    #[serde(default = "no_day_before_prints")]
    pub day_before_prints: WireU64,
    /// The session day joined in front of this one, `YYYY-MM-DD`, when one was.
    ///
    /// Two days on one chart is never left to be inferred from bar counts: an
    /// operator asked to read the open needs to know whose order flow sits
    /// behind it. Additive within v1.
    #[serde(default)]
    pub day_before: Option<String>,
    /// Why no day was joined, when there was a file for one and it could not
    /// be used. `None` covers both "none was asked for" and "it joined".
    ///
    /// The same reason the interface shows beside the transport, so the two
    /// surfaces cannot disagree about why yesterday is missing. Additive.
    #[serde(default)]
    pub day_before_problem: Option<String>,
    #[schemars(extend("x-unit" = "ratio"))]
    pub progress: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "playback_multiple"))]
    pub speed: Option<CanonicalDecimal>,
    pub rewinds: WireU64,
    pub trace: ReplayTraceSnapshot,
}

/// What the capture can honestly say about the control trace beside the
/// recording (contract §11): nothing, and where to ask instead.
///
/// Both halves were tried here and both are wrong on this thread. Reading the
/// sidecar to decide completeness is unbounded work. Even a bare existence
/// check is a `stat`, whose cost is the filesystem's and not this process's —
/// a recording on a network share or a cold path can take milliseconds, and a
/// capture of every scope already measures a p99 near
/// `CONTROL_UI_BUDGET_US`. So the capture does no file I/O at all and names
/// the gateway, which loads each recording's trace once and holds it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayTraceSnapshot {
    /// Whether a sidecar exists, and whether every recorded intent carries a
    /// terminal result — the property that decides if the run is a
    /// deterministic fixture. Both are served by the gateway; see the type
    /// doc for why a capture answers neither.
    pub state: AvailabilitySnapshot,
    /// The name the sidecar would carry beside the recording, so a client can
    /// ask the gateway about the right file without deriving the convention
    /// itself.
    pub file_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperSnapshot {
    pub tabs: Vec<TabPaperSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabPaperSnapshot {
    pub tab_id: WireU64,
    pub symbol: String,
    /// Where these rows come from, named rather than implied.
    pub provenance: String,
    pub flat: bool,
    /// The named exit ladder the ticket is armed with, if any.
    ///
    /// What the *next* order will carry, which the order rows below cannot
    /// say: they are what already exists. An operator asked "why is nothing
    /// projecting" has to be able to read this, and until it was here the
    /// answer lived only in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed_strategy: Option<String>,
    /// Why the armed ladder cannot be used, when it cannot. Absent when it
    /// is usable, and absent when nothing is armed — a strategy that draws
    /// nothing and says nothing is the bug this field exists to prevent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed_strategy_refusal: Option<String>,
    /// How far the aim's ruler stands from an entry, in ticks; zero when it
    /// is not in use. Sticky across aims, so a reader needs it to explain
    /// what the next order would carry.
    pub ruler_ticks: u32,
    /// One tick of this tab's instrument, so `ruler_ticks` can be read as a
    /// price rather than a count.
    pub tick_size: CanonicalDecimal,
    /// What the risk per trade makes of the entry the ticket is holding:
    /// `off`, `sized`, `short_of_budget`, `capped_at_max`,
    /// `clamped_at_minimum`, `instrument_unknown`, `no_budget` or `refused`.
    ///
    /// The state, not the pixels. An operator asked "why is the size 1" has
    /// to be able to read the answer rather than infer it from a number.
    pub risk_state: String,
    /// The size the risk per trade derived, absent when it derived none —
    /// the mode is off, or nothing here could be sized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_quantity: Option<CanonicalDecimal>,
    /// What that size stands to lose at the entry's own stop. Absent for the
    /// same reasons as `risk_quantity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_amount: Option<CanonicalDecimal>,
    /// The currency `risk_amount` is in. Nothing here converts between
    /// currencies, so a reader comparing two tabs must compare these too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_currency: Option<String>,
    /// Whether the trader's lock is refusing this entry right now.
    pub risk_blocks_entry: bool,
    /// The sentence the ticket is showing, verbatim.
    ///
    /// Produced by the same function that renders it on screen rather than a
    /// second wording kept in step by hand: an operator reading this and a
    /// trader reading the ticket are looking at one sentence. Absent only
    /// when the mode is off and there is nothing to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_sentence: Option<String>,
    pub position: Option<PaperPositionSnapshot>,
    #[schemars(length(max = CONTROL_SNAPSHOT_MAX_WORKING_ORDERS))]
    pub working_orders: Vec<PaperOrderSnapshot>,
    pub working_order_count: WireU64,
    /// True when `working_orders` was cut at its page limit; the count above
    /// still reports every order the simulator holds.
    pub working_orders_truncated: bool,
    #[schemars(length(max = CONTROL_SNAPSHOT_MAX_CLOSED_TRADES))]
    pub closed_trades: Vec<PaperClosedTradeSnapshot>,
    pub closed_trade_count: WireU64,
    /// True when `closed_trades` was cut at its page limit. The newest rows
    /// are kept: a ledger is read from its end.
    pub closed_trades_truncated: bool,
    /// Row of the whole ledger that `closed_trades[0]` stands for. Zero unless
    /// the page was cut; published because the page is the ledger's *tail*, so
    /// without it a row number here cannot be placed in the ledger the count
    /// above describes.
    pub closed_trades_page_start: WireU64,
    /// Row of the selected closed trade in the whole ledger — the same
    /// numbering as `closed_trade_count`, not a position in the page above.
    /// Subtract `closed_trades_page_start` to index `closed_trades`; a row
    /// below that start was cut and is not on this page.
    pub selected_trade_row: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperPositionSnapshot {
    pub side: String,
    pub quantity: CanonicalDecimal,
    pub average_entry_price: CanonicalDecimal,
    /// Open profit in points — price units times quantity, signed. Points and
    /// not currency: the workspace knows no per-instrument tick value, and a
    /// number that cannot be computed honestly is not published. Absent
    /// before any mark exists.
    #[schemars(extend("x-unit" = "points"))]
    pub open_points: Option<CanonicalDecimal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperOrderSnapshot {
    pub order_id: WireU64,
    pub side: String,
    pub kind: String,
    /// Limit price for a limit, trigger price for a stop, absent for a market
    /// order — which has no price of its own by definition.
    pub price: Option<CanonicalDecimal>,
    pub quantity: CanonicalDecimal,
    pub stop_loss: Option<CanonicalDecimal>,
    pub take_profit: Option<CanonicalDecimal>,
    pub cancel_at: Option<CanonicalDecimal>,
    pub flat_only: bool,
    /// Venue time of the last print seen when the order was placed. The
    /// simulator has no clock of its own.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub placed_unix_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperClosedTradeSnapshot {
    pub side: String,
    pub quantity: CanonicalDecimal,
    pub entry_price: CanonicalDecimal,
    pub exit_price: CanonicalDecimal,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub opened_unix_ms: i64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub closed_unix_ms: i64,
    #[schemars(extend("x-unit" = "points"))]
    pub pnl_points: CanonicalDecimal,
    pub exit_reason: String,
    /// Aggregate ids of the prints that opened and closed the position — the
    /// audit trail back to the tape. Absent only on rows loaded from a
    /// version-1 history file, which did not record them.
    pub entry_aggregate_id: Option<WireU64>,
    pub exit_aggregate_id: Option<WireU64>,
    /// Worst excursion against the average entry while the position was open,
    /// in points and never negative. Absent for version-1 rows: unknown is
    /// not zero.
    #[schemars(extend("x-unit" = "points"))]
    pub maximum_adverse_excursion_points: Option<CanonicalDecimal>,
}

/// What `day_before_prints` reads as in a summary written before the field
/// existed: no day was joined, which is what every such instance did.
///
/// A named function rather than `#[serde(default)]`, because the wire integer
/// has no `Default` on purpose — a zero has to be *chosen* and said out loud.
pub fn no_day_before_prints() -> WireU64 {
    WireU64::new(0)
}

pub fn order_snapshot(order: &Order) -> PaperOrderSnapshot {
    PaperOrderSnapshot {
        order_id: WireU64::new(order.id.0),
        side: order.side.as_str().to_owned(),
        kind: order.kind.as_str().to_owned(),
        price: order.price.map(canonical_decimal),
        quantity: canonical_decimal(order.quantity),
        stop_loss: order.bracket.stop_loss().map(canonical_decimal),
        take_profit: order.bracket.take_profit().map(canonical_decimal),
        cancel_at: order.cancel_at.map(canonical_decimal),
        flat_only: order.flat_only,
        placed_unix_ms: order.placed_ms,
    }
}

pub fn closed_trade_snapshot(trade: &ClosedTrade) -> PaperClosedTradeSnapshot {
    PaperClosedTradeSnapshot {
        side: trade.side.as_str().to_owned(),
        quantity: canonical_decimal(trade.quantity),
        entry_price: canonical_decimal(trade.entry_price),
        exit_price: canonical_decimal(trade.exit_price),
        opened_unix_ms: trade.opened_ms,
        closed_unix_ms: trade.closed_ms,
        pnl_points: canonical_decimal(trade.pnl_points),
        exit_reason: trade.exit_reason.as_str().to_owned(),
        entry_aggregate_id: trade.entry_agg_id.map(WireU64::new),
        exit_aggregate_id: trade.exit_agg_id.map(WireU64::new),
        maximum_adverse_excursion_points: trade.mae_points.map(canonical_decimal),
    }
}

/// The recording's file name, or an empty string for a path that has none.
/// Used by both the projection and the revision key, so the identity the
/// journal compares is the identity the wire publishes.
pub fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
