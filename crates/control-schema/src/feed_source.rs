//! A configured feed and symbol selection, with the reload permission.
use quantick_control::{registry::CapabilityDescriptor, schema::generated_schema, wire::WireU64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const FEED_SOURCE_CAPABILITY_ID: &str = "feed.source.set";

/// The journal event a changed source writes: who switched the tab, and the
/// pair it left for the pair it chose. Its payload is the call's result under
/// `feed_source`.
pub const FEED_SOURCE_EVENT_KIND: &str = "feed.source.changed";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedSourceChoice {
    pub feed_id: String,
    pub display_name: String,
    pub symbols: Vec<String>,
}

#[must_use]
pub fn choices(config: &quantick_stores::config::AppConfig) -> Vec<FeedSourceChoice> {
    config
        .feeds
        .iter()
        .map(|feed| FeedSourceChoice {
            feed_id: feed.id.clone(),
            display_name: feed.name.clone(),
            symbols: feed.symbols.clone(),
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedSourceInput {
    pub tab_id: WireU64,
    pub feed_id: String,
    pub symbol: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedSourceResult {
    pub tab_id: WireU64,
    pub feed_id: String,
    pub symbol: String,
    /// The pair the tab showed before the call; the same pair when nothing
    /// changed.
    pub from_feed_id: String,
    pub from_symbol: String,
    pub changed: bool,
    pub timeline_kept: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        FEED_SOURCE_CAPABILITY_ID,
        "Select a configured feed and symbol",
        "Selects a pair listed in feed.status.sources through the chart's source selectors. A changed source rebuilds all pane timelines, closes and journals any paper position, disarms strategies and applies the destination asset's saved settings. Refuses while replay owns the tab. Requires the same cockpit.recover permission as feed.reload. The tab id and the configured pair are checked before anything changes; repeating the active pair is a no-op. A changed source journals `feed.source.changed` naming who switched it, the pair it left and the pair it chose. Read active_feed_id, active_symbol and feed_generation in feed.status.",
        true,
        generated_schema::<FeedSourceInput>(),
    );
    descriptor.output_schema = generated_schema::<FeedSourceResult>();
    descriptor
}
