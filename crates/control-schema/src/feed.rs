//! Feed, market-data capability, status, and provenance snapshot.

use quantick_feed::FeedConnectionState;

use quantick_control::wire::WireU64;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const SCOPE_ID: &str = "feed.status";

pub const MODULE_ID: &str = "feed";

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedSnapshot {
    pub tabs: Vec<FeedTabSnapshot>,
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
    pub live_trade_count: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub latest_trade_unix_ms: Option<i64>,
    #[schemars(extend("x-unit" = "milliseconds"))]
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
    /// The deal recorder, where the feed carries a deal counter; absent on
    /// a feed without one. The same view the REC control draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deal_recording: Option<super::deal_recording::DealRecordingSnapshot>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedCapabilitiesSnapshot {
    pub book_capture: bool,
    pub history_paging: bool,
    pub traded_volume: bool,
    /// Defaults off for snapshots produced before deal counters existed.
    #[serde(default)]
    pub deal_counter: bool,
    pub venue_ohlcv_history: bool,
    pub venue_ohlcv_generation: WireU64,
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

/// What a v1 payload recorded before `feed_generation` existed reads as: no
/// session counted, which is what a build without the field was reporting.
pub fn no_feed_generation() -> WireU64 {
    WireU64::new(0)
}

pub fn connection_state(state: FeedConnectionState) -> &'static str {
    match state {
        FeedConnectionState::Connecting => "connecting",
        FeedConnectionState::Reconnecting => "reconnecting",
        FeedConnectionState::Connected => "connected",
    }
}
