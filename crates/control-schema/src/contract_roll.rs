//! Rolling a market tab onto the contract its feed's config names next.
use quantick_control::{registry::CapabilityDescriptor, schema::generated_schema, wire::WireU64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CONTRACT_ROLL_CAPABILITY_ID: &str = "feed.contract.roll";

/// The journal event a roll writes: who rolled the tab, and between which
/// contracts. Its payload is the call's result under `contract_roll`.
pub const CONTRACT_ROLL_EVENT_KIND: &str = "feed.contract.rolled";

/// The feed config names a contract after the tab's (`contract_rolls`). Fixed
/// in config, never by retrying.
pub const ROLL_CONFIGURED_PRECONDITION: &str = "feed.contract.roll_configured";
/// The configured next contract is one the feed offers (`symbols`).
pub const TARGET_OFFERED_PRECONDITION: &str = "feed.contract.target_offered";
/// The tab holds no open paper position: it cannot follow the tab across
/// contracts, and a roll never closes one on its own.
pub const PAPER_FLAT_PRECONDITION: &str = "feed.contract.paper_flat";
/// The tab shows the contract the caller rolled from, or the one that follows it.
pub const FROM_SYMBOL_PRECONDITION: &str = "feed.contract.from_symbol";
/// A recorded session is not a venue with a next contract.
pub const LIVE_SOURCE_PRECONDITION: &str = "feed.contract.live_source";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContractRollInput {
    pub tab_id: WireU64,
    /// The contract the caller read in feed.status. Naming it is what makes a
    /// retry safe: a second call cannot roll a contract further.
    pub from_symbol: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContractRollResult {
    pub tab_id: WireU64,
    pub feed_id: String,
    /// The contract the tab shows now.
    pub symbol: String,
    pub from_symbol: String,
    /// False when the tab had already rolled: nothing happened on this call.
    pub changed: bool,
    /// Drawings added to the new contract, across every layout. Those the new
    /// contract already held are not counted.
    pub drawings_carried: u64,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        CONTRACT_ROLL_CAPABILITY_ID,
        "Roll the tab to its next contract",
        "Switches the tab to the contract feed.status.sources[].contract_rolls names after from_symbol, on the same feed. The workspace, pane layout and bar specs stay, and the tab's drawings are copied to the new contract in every layout; the old contract keeps its copy. Every pane timeline is rebuilt from the new contract and armed strategies are disarmed by name. Refuses while replay owns the tab, while a paper position is open, when no roll is configured, or when the feed does not offer the target. Requires the same cockpit.recover permission as feed.source.set. Repeating a roll whose target the tab already shows is a no-op. A roll journals `feed.contract.rolled`. Read active_symbol and feed_generation in feed.status.",
        true,
        generated_schema::<ContractRollInput>(),
    );
    descriptor.output_schema = generated_schema::<ContractRollResult>();
    descriptor.preconditions = vec![
        crate::bubble_scale::precondition(
            LIVE_SOURCE_PRECONDITION,
            "The tab is on a live feed, not a recorded session.",
        ),
        crate::bubble_scale::precondition(
            FROM_SYMBOL_PRECONDITION,
            "The tab shows from_symbol, or the contract configured to follow it.",
        ),
        crate::bubble_scale::precondition(
            ROLL_CONFIGURED_PRECONDITION,
            "The feed config names a contract after from_symbol (contract_rolls).",
        ),
        crate::bubble_scale::precondition(
            TARGET_OFFERED_PRECONDITION,
            "The feed offers the configured next contract (symbols).",
        ),
        crate::bubble_scale::precondition(
            PAPER_FLAT_PRECONDITION,
            "The tab holds no open paper position.",
        ),
    ];
    descriptor
}

/// Published schemas of the roll.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "feed-contract-roll-input-v1.schema.json",
            generated_schema::<ContractRollInput>(),
        ),
        (
            "feed-contract-roll-result-v1.schema.json",
            generated_schema::<ContractRollResult>(),
        ),
    ]
}
