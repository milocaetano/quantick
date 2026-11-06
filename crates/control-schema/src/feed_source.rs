//! A configured feed and symbol selection, with the reload permission.
use quantick_control::{registry::CapabilityDescriptor, schema::generated_schema, wire::WireU64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Which contract follows which on this feed, for feed.contract.roll.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contract_rolls: BTreeMap<String, String>,
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
            contract_rolls: feed.contract_rolls.clone(),
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

pub const TAB_MARKET_SET_CAPABILITY_ID: &str = "tab.market.set";

/// Put a tab on another configured market: the toolbar's market button and
/// the palette's switch mode, as one call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabMarketSetInput {
    /// The tab's id, as `feed.status` reports it.
    pub tab: WireU64,
    /// A feed id from `feed.status.sources`.
    pub feed: String,
    /// A symbol that feed offers.
    pub symbol: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabMarketSetResult {
    pub tab: WireU64,
    pub feed: String,
    pub symbol: String,
    /// The market the tab showed before the call; the same when nothing changed.
    pub from_feed: String,
    pub from_symbol: String,
    pub changed: bool,
}

impl From<FeedSourceResult> for TabMarketSetResult {
    fn from(result: FeedSourceResult) -> Self {
        Self {
            tab: result.tab_id,
            feed: result.feed_id,
            symbol: result.symbol,
            from_feed: result.from_feed_id,
            from_symbol: result.from_symbol,
            changed: result.changed,
        }
    }
}

pub fn tab_market_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        TAB_MARKET_SET_CAPABILITY_ID,
        "Switch a tab to another market",
        "Puts the tab on a configured feed and symbol, as the toolbar's market button and the Open a market palette's switch mode do. It is feed.source.set under the tab's market names: the same checks, the same switch and the same `feed.source.changed` journal row naming the caller. The tab keeps its workspace and pane layout; each market keeps its own drawings, so switching back shows the first market's again. Refuses while replay owns the tab and for a pair feed.status.sources does not list. Repeating the market the tab shows is a no-op. Requires cockpit.recover like feed.source.set.",
        true,
        generated_schema::<TabMarketSetInput>(),
    );
    descriptor.output_schema = generated_schema::<TabMarketSetResult>();
    descriptor.module =
        quantick_control::id::ModuleId::new(quantick_control_host::authority::TAB_MODULE_ID)
            .expect("static module ID is valid");
    descriptor
}

/// Published schemas of the feed source and tab market.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "feed-source-input-v1.schema.json",
            schema::<FeedSourceInput>(),
        ),
        (
            "feed-source-result-v1.schema.json",
            schema::<FeedSourceResult>(),
        ),
        (
            "tab-market-set-input-v1.schema.json",
            schema::<TabMarketSetInput>(),
        ),
        (
            "tab-market-set-result-v1.schema.json",
            schema::<TabMarketSetResult>(),
        ),
    ]
}
