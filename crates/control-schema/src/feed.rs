//! Feed, market-data capability, status, and provenance snapshot.

use quantick_control::wire::WireU64;

use schemars::JsonSchema;

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};
use quantick_control::registry::IdempotencyPolicy::Optional;
use serde::{Deserialize, Serialize};

pub const SCOPE_ID: &str = "feed.status";

pub const MODULE_ID: &str = "feed";

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedSnapshot {
    pub tabs: Vec<FeedTabSnapshot>,
    /// Configured source pairs available to feed.source.set and the selectors.
    #[serde(default)]
    pub sources: Vec<crate::feed_source::FeedSourceChoice>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedTabSnapshot {
    pub tab_id: WireU64,
    pub requested_feed_id: String,
    pub requested_symbol: String,
    pub active_feed_id: String,
    pub active_symbol: String,
    pub feed_display_name: String,
    pub source_mode: String,
    pub connection_state: String,
    pub notice: FeedNoticeSnapshot,
    pub capabilities: FeedCapabilitiesSnapshot,
    pub provenance: MarketDataProvenance,
    pub history_trade_count: WireU64,
    /// Whether a run of *load older* requests is in flight right now.
    ///
    /// `#[serde(default)]` keeps it out of the schema's `required` list, which
    /// is what makes adding it here a *compatible* change to a v1 payload: the
    /// control-plane contract (§5.6 of the development plan) counts a new
    /// required field as breaking, and a v1 payload recorded by an earlier
    /// build must keep deserializing. Absent reads as "no run", which is what
    /// a build with no reach campaigns was in fact reporting.
    #[serde(default)]
    pub history_reach_running: bool,
    /// How many feed sessions this tab has taken over since it opened. It
    /// advances by one on every respawn — a reconnect, a reload, a market
    /// switch, a replay opened or closed — and never otherwise.
    ///
    /// The readback for `feed.reconnect` and `feed.reload`: `connection_state`
    /// says whether the feed is healthy, not whether a call respawned it,
    /// and a reload normally lands on a connected feed. A client that lost
    /// the answer compares this with its reading from before the call.
    /// `#[serde(default)]` for the same reason as `history_reach_running`:
    /// an optional field is an additive change to the v1 payload.
    #[serde(default = "no_feed_generation")]
    pub feed_generation: WireU64,
    /// Slices of the opening session still to arrive, while a source is
    /// filling the chart in behind what it first painted.
    ///
    /// Absent when nothing is filling, which is the steady state — so an
    /// operator can tell "this chart is still arriving" from "this is all
    /// there is", which `history_trade_count` alone cannot say: it rises with
    /// no denominator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening_slices_remaining: Option<WireU64>,
    /// What the last *load older* press had to say, while it is still on
    /// screen — the exact sentence the trader reads in the loading lane.
    ///
    /// `null` is the ordinary state: no press yet, a press that landed what it
    /// promised, or one whose remark has had its time. Here because the
    /// outcome of a reach is the whole point of making one, and an operator
    /// driving the button through `QUANTICK_LOAD_OLDER` or `quantick_invoke`
    /// must be able to learn it from the same words rather than by diffing
    /// bar counts.
    pub history_reach_note: Option<String>,
    /// This tab's history run: idle, queued behind the opening fill, loading
    /// toward its target with how far back it has got, or paused while the
    /// tab is off screen. Optional so a v1 payload recorded before it keeps
    /// parsing; the application always reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_load: Option<super::history::HistoryLoadSnapshot>,
    pub live_trade_count: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub latest_trade_unix_ms: Option<i64>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    /// Signed local receipt stamp minus venue stamp; includes clock disagreement.
    pub latest_arrival_latency_ms: Option<i64>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub tape_age_ms: Option<i64>,
    /// The application's own judgement that this feed has stopped delivering,
    /// or absent while it is merely slow.
    ///
    /// Distinct from `notice`, which is what the *provider* said. A transport
    /// that reports itself connected while nothing comes down it produces no
    /// notice at all, and this is the only field that reports it — which is
    /// exactly the case a client watching for a frozen terminal has to see.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall: Option<FeedStallSnapshot>,
    /// Market-time bounds of source loss or a reconnect interruption.
    /// Equal bounds can bracket known missing messages. Bounded, oldest first.
    ///
    /// Omitted when there are none, so a reader written against the scope
    /// before this field existed keeps parsing every payload from a tab that
    /// never reconnected — which is nearly all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tape_gaps: Vec<FeedGapSnapshot>,
    /// The feed's refill of the hole its last reconnect left, while one runs
    /// and after it ends. Absent on a tab whose feed never refilled one,
    /// which is every feed but MetaTrader and every tab that never
    /// reconnected; optional, so additive to v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tape_backfill: Option<FeedBackfillSnapshot>,
    /// The deal recorder, where the feed carries a deal counter; absent on
    /// a feed without one. The same view the REC control draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deal_recording: Option<super::deal_recording::DealRecordingSnapshot>,
    /// What every surface on screen says about this feed, from the one state
    /// they all read. Optional, so additive to v1; the application always
    /// reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnectionSnapshot>,
}

/// The tab's `ConnectionState`. Carries words of its own, never the venue's:
/// like [`FeedNoticeSnapshot`] it publishes no provider text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectionSnapshot {
    /// `live`, `attempting`, `offline`, `quiet` or `no_transport`. `offline`
    /// latches until data arrives or `feed.reconnect` is called.
    pub link: String,
    /// `connecting`, `live` or `idle`.
    pub feed: String,
    /// `connecting`, `synced`, `idle`, `off` or `lost`.
    pub book: String,
    /// `connecting`, `live` or `idle`.
    pub trades: String,
    /// When the transport was last seen connected.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub last_connected_unix_ms: Option<i64>,
    /// The job the header's slot is naming, in the words it uses.
    pub current_job: Option<String>,
    /// The word the status bar shows, absent while a new session settles.
    pub status_word: Option<String>,
    /// The overlay stands over the panes with no bars.
    pub overlay_up: bool,
    /// A `feed.reconnect` press is on offer on screen: the overlay's or the
    /// status bar's.
    pub reconnect_offered: bool,
}

/// A feed the chart has decided is stalled.
///
/// Carries no free text, for the same reason [`FeedNoticeSnapshot`] carries
/// none: the words name the venue and the terminal behind it, and the observe
/// tier does not publish them. What it does carry is the part a client can act
/// on — which of the two recovery capabilities the application is offering
/// first, and how long the feed has been like this.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedStallSnapshot {
    /// `reconnect` or `reload` — the capability that addresses this stall.
    /// The other one is always available too.
    pub primary_recovery: String,
    /// Whether this is unambiguously wrong (a transport that never landed or
    /// dropped) or merely observed silence, which is also what a closed market
    /// looks like. The interface colours the two differently and so should a
    /// client deciding whether to say anything to the trader.
    pub needs_attention: bool,
    /// Why the words are not in this payload.
    pub text_availability: String,
}

/// A hole in the tape, in market time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedGapSnapshot {
    /// The last print before the silence.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub from_unix_ms: i64,
    /// The first print after it.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub to_unix_ms: i64,
    /// How long nothing was heard.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub duration_ms: i64,
}

/// A feed's refill of a reconnect hole: what it asked for and how it ended.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedBackfillSnapshot {
    /// `filling`, `filled` or `partial`.
    pub state: String,
    /// The last print the chart held before the source went away.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub requested_from_unix_ms: i64,
    /// The oldest print the new session opened with.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub requested_to_unix_ms: i64,
    /// Prints received toward the refill, before the seams were deduplicated.
    pub trades_received: WireU64,
    /// The part no print covers, on a `partial` refill; it is also one of
    /// `tape_gaps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_gap: Option<FeedGapSnapshot>,
    /// Why a `partial` refill stopped: `session_lost`, `no_paging`, `reach`,
    /// `source_exhausted`, `nothing_found` or `held_too_long`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

impl From<quantick_stores::config::TapeBackfill> for FeedBackfillSnapshot {
    fn from(fill: quantick_stores::config::TapeBackfill) -> Self {
        let (remaining_gap, stop_reason) = match fill.state {
            quantick_stores::config::BackfillState::Partial {
                unfilled_to_ms,
                stop,
            } => (
                Some(FeedGapSnapshot {
                    from_unix_ms: fill.from_ms,
                    to_unix_ms: unfilled_to_ms,
                    duration_ms: unfilled_to_ms - fill.from_ms,
                }),
                Some(stop.wire_name().to_owned()),
            ),
            _ => (None, None),
        };
        Self {
            state: fill.state.wire_name().to_owned(),
            requested_from_unix_ms: fill.from_ms,
            requested_to_unix_ms: fill.to_ms,
            trades_received: WireU64::new(fill.received),
            remaining_gap,
            stop_reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedCapabilitiesSnapshot {
    pub book_capture: bool,
    pub history_paging: bool,
    /// Why `history_paging` is off when the feed knows: `venue_retention`
    /// means the venue keeps nothing older. Absent while paging is on, and
    /// on a provider that never pages; optional, so additive to v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_boundary: Option<HistoryBoundarySnapshot>,
    pub traded_volume: bool,
    /// Defaults off for snapshots produced before deal counters existed.
    #[serde(default)]
    pub deal_counter: bool,
    pub venue_ohlcv_history: bool,
    pub venue_ohlcv_generation: WireU64,
}

/// Where a feed's pageable trade history ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryBoundarySnapshot {
    /// The venue retains no executions older than the chart holds.
    VenueRetention,
}

impl From<quantick_stores::config::HistoryBoundary> for HistoryBoundarySnapshot {
    fn from(boundary: quantick_stores::config::HistoryBoundary) -> Self {
        match boundary {
            quantick_stores::config::HistoryBoundary::VenueRetention => Self::VenueRetention,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketDataProvenance {
    pub price: String,
    pub volume: String,
    pub aggressor_side: String,
    pub replay: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedNoticeSnapshot {
    pub kind: String,
    pub headline_present: bool,
    pub next_step_present: bool,
    pub text_availability: String,
}

impl FeedNoticeSnapshot {
    /// The redacted view of a notice, from its kind and which of its two
    /// texts it carries: the words name the venue and the terminal behind
    /// it, and the observe tier does not publish them.
    #[must_use]
    pub fn from_summary((kind, headline_present, next_step_present): (&str, bool, bool)) -> Self {
        Self {
            kind: kind.to_owned(),
            headline_present,
            next_step_present,
            text_availability: if headline_present {
                "redacted_pending_attention_scope"
            } else {
                "not_applicable"
            }
            .to_owned(),
        }
    }
}

/// What a v1 payload recorded before `feed_generation` existed reads as: no
/// session counted, which is what a build without the field was reporting.
pub fn no_feed_generation() -> WireU64 {
    WireU64::new(0)
}

pub const FEED_TEST: &str = "a_dropped_feed_recovery_answer_is_replayed_and_the_recovery_runs_once";

/// The feed generation moves on every respawn.
pub const FEED_GENERATION_TEST: &str =
    "the_feed_generation_advances_on_every_respawn_and_reads_back";
/// How a client reconciles an interrupted call that respawns a feed or arms its recorder.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[
    snapshot(
        crate::feed_source::FEED_SOURCE_CAPABILITY_ID,
        Optional,
        SCOPE_ID,
        "tabs[].active_symbol",
        "the tab reports the requested active_feed_id and active_symbol; feed_generation advances by one when the pair changed and not otherwise",
        &[
            "configured_feed_selection_is_validated_retry_safe_and_reads_back",
            "a_pair_switch_advances_the_generation_once_and_flattens_the_paper_position",
        ],
    ),
    snapshot(
        "feed.reconnect",
        Optional,
        SCOPE_ID,
        "tabs[].feed_generation",
        "the tab's generation is past the pre-call reading: it took over a new feed session (a tab with nothing to respawn answers `respawned: false` and keeps its generation)",
        &[FEED_TEST, EVERY_OPTIONAL_TEST, FEED_GENERATION_TEST],
    ),
    snapshot(
        "feed.reload",
        Optional,
        SCOPE_ID,
        "tabs[].feed_generation",
        "the tab's generation is past the pre-call reading: it took over a new feed session (a tab with nothing to respawn answers `respawned: false` and keeps its generation)",
        &[FEED_TEST, EVERY_OPTIONAL_TEST, FEED_GENERATION_TEST],
    ),
    snapshot(
        "feed.deal_recording.set",
        Optional,
        SCOPE_ID,
        "tabs[].deal_recording.state",
        "the tab's recorder reads the state asked for: `recording` or `stale` after `enabled: true`, `off` after `enabled: false`, and `record_by_default` the standing choice asked for; a tab whose feed carries no counter and has no recorded day lists no `deal_recording` at all, and the call changed nothing there",
        &[EVERY_OPTIONAL_TEST],
    ),
];

/// Shared factual provenance vocabulary for feed and chart snapshots.
pub fn market_data_provenance(
    replay: bool,
    traded_volume: bool,
    inferred_side: bool,
) -> MarketDataProvenance {
    MarketDataProvenance {
        price: if replay {
            "recorded_trade".to_owned()
        } else {
            "venue_or_broker_trade".to_owned()
        },
        volume: if traded_volume {
            if replay {
                "recorded".to_owned()
            } else {
                "venue_reported".to_owned()
            }
        } else {
            "synthetic_unit_per_quote".to_owned()
        },
        aggressor_side: if replay {
            "recording_declared".to_owned()
        } else if inferred_side {
            "inferred_or_derived".to_owned()
        } else {
            "venue_reported".to_owned()
        },
        replay,
    }
}
