//! Open a market: the palette, the open itself and taking a market the
//! trader added back out of its venue's list. The palette and these calls
//! decide through one headless list (`quantick_workspace::market_list`).
use quantick_control::id::PermissionId;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{
    CATALOG_MODULE_ID, COCKPIT_DESK_PERMISSION_ID, COCKPIT_PERMISSION_ID, MARKET_MODULE_ID,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::transient_descriptor;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal, snapshot};

const RESULT_MAX_BYTES: usize = 1024;

pub const MARKET_OPEN_CAPABILITY_ID: &str = "market.open";
pub const MARKET_PALETTE_OPEN_CAPABILITY_ID: &str = "market.palette.open";
pub const CATALOG_REMOVE_CAPABILITY_ID: &str = "catalog.remove";
/// The palette came up, with the query its field holds.
pub const PALETTE_OPENED_EVENT_KIND: &str = "market.palette.opened";

/// Open a configured market, or focus the tab that already shows it so.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarketOpenInput {
    /// A feed id from `feed.status.sources`.
    pub feed: String,
    pub symbol: String,
    /// The flow pane's bar as `kind:size` (`tick:50`, `time:1m`). Left out,
    /// the market's own: an open tab's, else the one it last closed on, else
    /// the feed's `default_bars`, else the active tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar: Option<String>,
    /// A name from `workspace.list`. Left out, `Default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Open without leaving the tab on screen.
    #[serde(default)]
    pub background: bool,
}

/// The tab that shows the market now.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct MarketOpenResult {
    pub tab_id: WireU64,
    /// True when a tab already showed this market on this bar and workspace
    /// and was focused; nothing new opened.
    pub focused: bool,
    pub feed: String,
    pub symbol: String,
    pub bar: String,
    pub workspace: String,
    pub background: bool,
}

/// Put the palette on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarketPaletteOpenInput {
    /// What the field starts with; `>` lists commands.
    #[serde(default)]
    pub query: String,
    /// Switch mode: Enter puts the active tab on the chosen market (through
    /// `tab.market.set`) instead of opening a tab, as the toolbar's market
    /// button does.
    #[serde(default)]
    pub switch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct MarketPaletteOpenResult {
    pub query: String,
    pub switch: bool,
}

/// Take a market the trader added out of its venue's list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogRemoveInput {
    pub feed: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct CatalogRemoveResult {
    pub feed: String,
    pub symbol: String,
}

fn bounded(mut descriptor: CapabilityDescriptor) -> CapabilityDescriptor {
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

pub fn market_open_descriptor() -> CapabilityDescriptor {
    bounded(transient_descriptor::<MarketOpenInput, MarketOpenResult>(
        MARKET_OPEN_CAPABILITY_ID,
        MARKET_MODULE_ID,
        "Open a market in a tab",
        "Opens a configured feed and symbol in a new tab on a bar and a workspace, as the Open a market palette's Enter does. A tab already showing that market on the same bar and workspace is focused instead and the answer says focused true; the other tab is never changed. With background true the tab on screen stays. An unknown feed or symbol, a bar that does not parse or that the feed cannot build, and an unknown workspace are refused before anything opens.",
        "A keyed repeat replays the first answer. An unkeyed repeat finds the tab the first call opened and focuses it.",
    ))
}

pub fn market_palette_open_descriptor() -> CapabilityDescriptor {
    bounded(transient_descriptor::<
        MarketPaletteOpenInput,
        MarketPaletteOpenResult,
    >(
        MARKET_PALETTE_OPEN_CAPABILITY_ID,
        MARKET_MODULE_ID,
        "Show the Open a market palette",
        "Puts the Open a market palette over the chart, as + and Ctrl+T do, with the field holding query, and journals market.palette.opened. Nothing opens until Enter or market.open. With switch true it opens in switch mode, as the toolbar's market button does: Enter switches the active tab (tab.market.set) or rolls it to its next contract (feed.contract.roll); refused when the active tab is a recording.",
        "Showing the palette twice is the same state, with the field reset to query.",
    ))
}

pub fn catalog_remove_descriptor() -> CapabilityDescriptor {
    let mut descriptor = bounded(transient_descriptor::<
        CatalogRemoveInput,
        CatalogRemoveResult,
    >(
        CATALOG_REMOVE_CAPABILITY_ID,
        CATALOG_MODULE_ID,
        "Remove a market you added",
        "Takes a symbol the trader added back out of its feed's list and out of the added-symbols file, as Delete on its palette row does. Refused with the reason for a symbol the config file declares and for one a tab is showing; close the tab first.",
        "A repeat finds the symbol gone and is refused; nothing changes.",
    ));
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.required_permissions = [COCKPIT_PERMISSION_ID, COCKPIT_DESK_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect();
    descriptor
}

pub const READBACKS: &[Readback] = &[
    snapshot(
        MARKET_OPEN_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::workspace::SCOPE_ID,
        "tabs[].tab_id",
        "a tab with the answer's tab_id shows the market, active unless background",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        MARKET_PALETTE_OPEN_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        PALETTE_OPENED_EVENT_KIND,
        "payload.query",
        "an event after the pre-call cursor names the query and the mode",
        &[EVERY_OPTIONAL_TEST],
    ),
    snapshot(
        CATALOG_REMOVE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::feed::SCOPE_ID,
        "sources[].symbols",
        "the feed's symbols no longer list the symbol",
        &[EVERY_OPTIONAL_TEST],
    ),
];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "market-open-input-v1.schema.json",
            schema::<MarketOpenInput>(),
        ),
        (
            "market-open-result-v1.schema.json",
            schema::<MarketOpenResult>(),
        ),
        (
            "market-palette-open-input-v1.schema.json",
            schema::<MarketPaletteOpenInput>(),
        ),
        (
            "market-palette-open-result-v1.schema.json",
            schema::<MarketPaletteOpenResult>(),
        ),
        (
            "catalog-remove-input-v1.schema.json",
            schema::<CatalogRemoveInput>(),
        ),
        (
            "catalog-remove-result-v1.schema.json",
            schema::<CatalogRemoveResult>(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inputs_default_what_they_may_and_refuse_extra_fields() {
        let open: MarketOpenInput =
            serde_json::from_value(json!({"feed": "binance", "symbol": "BTCUSDT"})).unwrap();
        assert_eq!(
            (open.bar, open.workspace, open.background),
            (None, None, false)
        );
        assert!(serde_json::from_value::<MarketOpenInput>(json!({"feed": "binance"})).is_err());
        assert!(
            serde_json::from_value::<MarketOpenInput>(
                json!({"feed": "binance", "symbol": "BTCUSDT", "tab": 1})
            )
            .is_err()
        );
        let palette: MarketPaletteOpenInput = serde_json::from_value(json!({})).unwrap();
        assert_eq!(palette.query, "");
        assert!(serde_json::from_value::<CatalogRemoveInput>(json!({"feed": "binance"})).is_err());
    }

    #[test]
    fn removing_writes_the_desk_and_opening_does_not() {
        let desk = PermissionId::new(COCKPIT_DESK_PERMISSION_ID).unwrap();
        assert!(
            catalog_remove_descriptor()
                .required_permissions
                .contains(&desk)
        );
        assert!(
            !market_open_descriptor()
                .required_permissions
                .contains(&desk)
        );
        assert_eq!(
            catalog_remove_descriptor().persistence,
            EffectPersistence::Durable
        );
    }
}
