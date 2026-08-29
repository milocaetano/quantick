//! Feed, market-data capability, status, and provenance snapshot.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    app::QuantickApp,
    feed::{FeedConnectionState, FeedNotice, stall::Recovery},
};

use super::registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError};

pub(crate) const SCOPE_ID: &str = "feed.status";
const MODULE_ID: &str = "feed";
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FeedSnapshot {
    pub tabs: Vec<FeedTabSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FeedTabSnapshot {
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
    /// Stretches of market time this tab's tape does not cover, left by a
    /// reconnect that kept the timeline. Bounded, oldest first.
    ///
    /// Omitted when there are none, so a reader written against the scope
    /// before this field existed keeps parsing every payload from a tab that
    /// never reconnected — which is nearly all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tape_gaps: Vec<FeedGapSnapshot>,
}

/// A feed the chart has decided is stalled.
///
/// Carries no free text, for the same reason [`FeedNoticeSnapshot`] carries
/// none: the words name the venue and the terminal behind it, and the observe
/// tier does not publish them. What it does carry is the part a client can act
/// on — which of the two recovery capabilities the application is offering
/// first, and how long the feed has been like this.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FeedStallSnapshot {
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
pub(crate) struct FeedGapSnapshot {
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
pub(crate) struct FeedCapabilitiesSnapshot {
    pub book_capture: bool,
    pub history_paging: bool,
    pub traded_volume: bool,
    pub venue_ohlcv_history: bool,
    pub venue_ohlcv_generation: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct MarketDataProvenance {
    pub price: String,
    pub volume: String,
    pub aggressor_side: String,
    pub replay: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FeedNoticeSnapshot {
    pub kind: String,
    pub headline_present: bool,
    pub next_step_present: bool,
    pub text_availability: String,
}

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Feed".to_owned(),
            description: "Feed connection, capabilities, and market-data provenance.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Feed status",
        "Reports the selected and active markets, source health, capabilities, and provenance.",
        &["observe", "observe.market"],
        project,
    )
}

fn revision(app: &QuantickApp) -> FeedSnapshot {
    snapshot(app, None)
}

fn project(app: &QuantickApp, context: CaptureContext) -> FeedSnapshot {
    snapshot(app, Some(context.captured_at_unix_ms))
}

/// Where a tab's prices, volumes and aggressor sides come from, in the one
/// vocabulary every scope uses. The feed scope reports it per tab and the
/// chart scope stamps it on every bar; both call this, so the two can never
/// disagree about the same tab.
pub(crate) fn market_data_provenance(
    tab: &crate::tab::Tab,
    config: &crate::config::AppConfig,
) -> MarketDataProvenance {
    let capabilities = tab.capabilities(config);
    let replay = tab.replay.is_some();
    MarketDataProvenance {
        price: if replay {
            "recorded_trade".to_owned()
        } else {
            "venue_or_broker_trade".to_owned()
        },
        volume: if capabilities.traded_volume {
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
        } else if tab.side_note(config).is_some() {
            "inferred_or_derived".to_owned()
        } else {
            "venue_reported".to_owned()
        },
        replay,
    }
}

fn snapshot(app: &QuantickApp, now_ms: Option<i64>) -> FeedSnapshot {
    let config = app.control_config();
    FeedSnapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| {
                let capabilities = tab.capabilities(config);
                let replay = tab.replay.is_some();
                FeedTabSnapshot {
                    tab_id: WireU64::new(tab.id),
                    requested_feed_id: tab.feed_id.clone(),
                    requested_symbol: tab.symbol.clone(),
                    active_feed_id: tab.active.0.clone(),
                    active_symbol: tab.active.1.clone(),
                    feed_display_name: tab.feed_display_name(config).to_owned(),
                    source_mode: if replay { "replay" } else { "live" }.to_owned(),
                    connection_state: connection_state(tab.feed_connection).to_owned(),
                    notice: notice(&tab.notice),
                    capabilities: FeedCapabilitiesSnapshot {
                        book_capture: capabilities.book_capture,
                        history_paging: capabilities.history_paging,
                        traded_volume: capabilities.traded_volume,
                        venue_ohlcv_history: capabilities.ohlcv_history,
                        venue_ohlcv_generation: WireU64::new(capabilities.ohlcv_generation),
                    },
                    provenance: market_data_provenance(tab, config),
                    history_trade_count: WireU64::new(
                        u64::try_from(tab.history_trades).unwrap_or(u64::MAX),
                    ),
                    history_reach_running: tab.history_reach_running(),
                    history_reach_note: tab.history_note().map(str::to_owned),
                    live_trade_count: WireU64::new(tab.live_trades),
                    latest_trade_unix_ms: tab.latest_trade_ms,
                    latest_arrival_latency_ms: tab.trade_arrival_ms(),
                    tape_age_ms: now_ms.and_then(|now| tab.tape_age_at(now)),
                    stall: now_ms
                        .and_then(|now| tab.stall_at(config, now))
                        .map(|stall| FeedStallSnapshot {
                            primary_recovery: recovery(stall.primary).to_owned(),
                            needs_attention: stall.needs_attention,
                            text_availability: "redacted_pending_attention_scope".to_owned(),
                        }),
                    tape_gaps: tab
                        .feed_gaps
                        .iter()
                        .map(|gap| FeedGapSnapshot {
                            from_unix_ms: gap.from_ms,
                            to_unix_ms: gap.to_ms,
                            duration_ms: gap.duration_ms(),
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

/// The wire name of a recovery control, so the snapshot and the capability
/// registry call the same act the same thing.
fn recovery(recovery: Recovery) -> &'static str {
    match recovery {
        Recovery::Reconnect => "reconnect",
        Recovery::Reload => "reload",
    }
}

pub(crate) fn connection_state(state: FeedConnectionState) -> &'static str {
    match state {
        FeedConnectionState::Connecting => "connecting",
        FeedConnectionState::Reconnecting => "reconnecting",
        FeedConnectionState::Connected => "connected",
    }
}

fn notice(notice: &FeedNotice) -> FeedNoticeSnapshot {
    match notice {
        FeedNotice::Connected => FeedNoticeSnapshot {
            kind: "connected".to_owned(),
            headline_present: false,
            next_step_present: false,
            text_availability: "not_applicable".to_owned(),
        },
        FeedNotice::Reconnecting { .. } => FeedNoticeSnapshot {
            kind: "reconnecting".to_owned(),
            headline_present: true,
            next_step_present: false,
            text_availability: "redacted_pending_attention_scope".to_owned(),
        },
        FeedNotice::Clear => FeedNoticeSnapshot {
            kind: "clear".to_owned(),
            headline_present: false,
            next_step_present: false,
            text_availability: "not_applicable".to_owned(),
        },
        FeedNotice::Working { .. } => FeedNoticeSnapshot {
            kind: "working".to_owned(),
            headline_present: true,
            next_step_present: false,
            text_availability: "redacted_pending_attention_scope".to_owned(),
        },
        FeedNotice::Attention { .. } => FeedNoticeSnapshot {
            kind: "attention".to_owned(),
            headline_present: true,
            next_step_present: true,
            text_availability: "redacted_pending_attention_scope".to_owned(),
        },
    }
}
