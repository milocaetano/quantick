//! A configured feed and symbol selection, with the reload permission.
use quantick_control::{registry::CapabilityDescriptor, schema::generated_schema, wire::WireU64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const FEED_SOURCE_CAPABILITY_ID: &str = "feed.source.set";

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
    pub changed: bool,
    pub timeline_kept: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        FEED_SOURCE_CAPABILITY_ID,
        "Select a configured feed and symbol",
        "Selects a pair listed in feed.status.sources through the chart's source selectors. A changed source rebuilds all pane timelines, closes and journals any paper position, disarms strategies and applies the destination asset's saved settings. Refuses while replay owns the tab. Requires the same cockpit.recover permission as feed.reload. Repeating the active pair is a no-op. Read active_feed_id, active_symbol and feed_generation in feed.status.",
        true,
        generated_schema::<FeedSourceInput>(),
    );
    descriptor.output_schema = generated_schema::<FeedSourceResult>();
    descriptor.stale_input_safety = Some("Stable tab IDs and the configured feed/symbol pair are checked before mutation. A changed selection resets that tab's timeline with the same consequences as feed.reload; an already active pair changes nothing. Replay refuses the call.".to_owned());
    descriptor
}
