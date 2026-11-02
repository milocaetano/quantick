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
/// The tab holds no paper position or working order: neither can follow the
/// tab across contracts, and a roll never closes or cancels one on its own.
pub const PAPER_FLAT_PRECONDITION: &str = "feed.contract.paper_flat";
/// The tab shows the contract the caller rolled from, or the one that follows it.
pub const FROM_SYMBOL_PRECONDITION: &str = "feed.contract.from_symbol";
/// No drawing gesture is in flight on a pane of either contract: the carry
/// would copy the mark from before the drag, or lose to the drag's write.
pub const NO_GESTURE_PRECONDITION: &str = "feed.contract.no_gesture";
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
    /// False when the tab already showed the target: the feed did not move.
    /// The drawings are still copied, so a tab switched there by hand gets them.
    pub changed: bool,
    /// Drawings added to the new contract, across every layout. Those the new
    /// contract already held are not counted.
    pub drawings_carried: u64,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        CONTRACT_ROLL_CAPABILITY_ID,
        "Roll the tab to its next contract",
        "Switches the tab to the contract feed.status.sources[].contract_rolls names after from_symbol, on the same feed. The workspace, pane layout and bar specs stay, and the tab's drawings are copied to the new contract in every layout; the old contract keeps its copy. Every pane timeline is rebuilt from the new contract and armed strategies are disarmed by name. Refuses while replay owns the tab, while a paper position or working order is open, while a drawing gesture is in flight on either contract, when no roll is configured, or when the feed does not offer the target. Requires the same cockpit.recover permission as feed.source.set. Repeating a roll whose target the tab already shows does not move the feed and adds no drawing twice. A roll through this capability journals `feed.contract.rolled` under the caller's name; the toolbar's does not, like the toolbar's own source switch. Read active_symbol and feed_generation in feed.status.",
        true,
        generated_schema::<ContractRollInput>(),
    );
    descriptor.output_schema = generated_schema::<ContractRollResult>();
    descriptor.stale_input_safety = Some(
        "from_symbol is checked against the tab before anything changes: a stale caller is refused, or answered changed:false when the tab already shows the target. A roll rebuilds the pane timelines and disarms strategies; it never touches a paper position, which refuses it."
            .to_owned(),
    );
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
            "The tab holds no paper position or working order.",
        ),
        crate::bubble_scale::precondition(
            NO_GESTURE_PRECONDITION,
            "No drawing gesture is in flight on a pane of either contract.",
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
