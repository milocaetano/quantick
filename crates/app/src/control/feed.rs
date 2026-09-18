//! Feed, market-data capability, status, and provenance snapshot.

pub(crate) use quantick_control_schema::feed::*;

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};

use crate::{app::QuantickApp, feed::FeedNotice};

use super::registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError};

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
            .iter_with_ids()
            .map(|(tab_id, tab)| {
                let capabilities = tab.capabilities(config);
                let replay = tab.replay.is_some();
                FeedTabSnapshot {
                    tab_id: WireU64::new(tab_id),
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
                        deal_counter: capabilities.deal_counter,
                        venue_ohlcv_history: capabilities.ohlcv_history,
                        venue_ohlcv_generation: WireU64::new(capabilities.ohlcv_generation),
                    },
                    provenance: market_data_provenance(tab, config),
                    history_trade_count: WireU64::new(
                        u64::try_from(tab.history_trades).unwrap_or(u64::MAX),
                    ),
                    history_reach_running: tab.history_reach_running(),
                    feed_generation: WireU64::new(tab.feed_generation()),
                    opening_slices_remaining: tab.opening_slices_remaining().map(WireU64::new),
                    history_reach_note: tab.history_note().map(str::to_owned),
                    live_trade_count: WireU64::new(tab.live_trades),
                    latest_trade_unix_ms: tab.latest_trade_ms,
                    latest_arrival_latency_ms: tab.trade_arrival_ms(),
                    tape_age_ms: now_ms.and_then(|now| tab.tape_age_at(now)),
                    stall: now_ms
                        .and_then(|now| tab.stall_at(config, now))
                        .map(|stall| FeedStallSnapshot {
                            primary_recovery: stall.primary.wire_name().to_owned(),
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
                    deal_recording: tab
                        .deal_recording_view()
                        .as_ref()
                        .map(super::deal_recording::snapshot),
                }
            })
            .collect(),
    }
}

fn notice(notice: &FeedNotice) -> FeedNoticeSnapshot {
    let (kind, headline_present, next_step_present) = notice.summary();
    FeedNoticeSnapshot {
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
