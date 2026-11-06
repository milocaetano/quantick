//! The forming bar's progress on a market's last-price tag, as a named call.
//!
//! The price axis menu's first row and `chart.price_tag.progress.set` switch
//! the same setting, kept per market (venue and asset) in the market
//! settings document: every pane of that market follows it, and it is never
//! a workspace or layout edit.

use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chart::{MODULE_ID, SCOPE_ID};
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const PRICE_TAG_PROGRESS_CAPABILITY_ID: &str = "chart.price_tag.progress.set";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceTagProgressInput {
    /// The tab, as `chart.summary` lists it.
    pub tab_id: WireU64,
    /// A pane on that tab, as `chart.summary` lists it: the setting belongs to
    /// the market the pane shows, so every pane of that market follows.
    pub pane_id: WireU64,
    /// `true` prints the forming bar's progress under the last price.
    pub on: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PriceTagProgressResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The market's setting as it stands after the call.
    pub on: bool,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

impl PriceTagProgressInput {
    /// What the call answers once the market's setting stands as asked.
    #[must_use]
    pub fn result(&self, changed: bool) -> PriceTagProgressResult {
        PriceTagProgressResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            on: self.on,
            changed,
        }
    }
}

#[must_use]
pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<
        PriceTagProgressInput,
        PriceTagProgressResult,
    >(
        PRICE_TAG_PROGRESS_CAPABILITY_ID,
        MODULE_ID,
        "Show bar progress on the price tag",
        "Switches the second line of the last-price tag: how far the forming bar is from closing (done/target for a count rule, the time left on the market clock for a time rule, the running total for an imbalance rule). Kept for the market the pane shows (venue and asset) and saved with the market settings; every pane of that market follows. The price axis menu's first row makes the same change. On by default.",
        "Stable tab and pane IDs name the market before anything changes; setting the value it already has changes nothing, and chart.summary reports it as price_tag_progress.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[Readback] = &[snapshot(
    PRICE_TAG_PROGRESS_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    SCOPE_ID,
    "panes[].price_tag_progress",
    "every pane of the addressed pane's market reports the requested price_tag_progress",
    &[
        EVERY_OPTIONAL_TEST,
        "the_price_tag_progress_call_switches_the_market_and_reads_it_back",
    ],
)];
