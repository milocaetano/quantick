//! Synchronous source-drain dependencies. Workers may still be pending afterward.
//! Event payloads, FIFO reception and intrinsic publication belong to the caller.

use crate::stage_registry::declare_stages;

#[cfg(test)]
mod tests;

declare_stages! {
    // These are actual synchronous prerequisites, not worker readiness claims.
    pub enum SourceDrainStage {
        /// Before ingress: the first new print belongs to this market.
        PrepareSymbol after [],
        ReceiveAvailable after [PrepareSymbol],
        PublishLatestPartial after [ReceiveAvailable],
        LandGap after [PublishLatestPartial],
        SettleReanchors after [LandGap],
        TickDealRecording after [SettleReanchors],
    }
}

// The test-facing names the reorder proofs in `tests` are written against.
#[cfg(test)]
const STAGES: [SourceDrainStage; SourceDrainStage::COUNT] = SourceDrainStage::ORDER;

#[cfg(test)]
const fn valid(stages: &[SourceDrainStage]) -> bool {
    SourceDrainStage::is_valid_order(stages)
}

/// Canonical traversal; no allocation, event queue, clock or success receipt.
pub struct SourceDrainPlan;
impl SourceDrainPlan {
    pub fn stages() -> impl ExactSizeIterator<Item = SourceDrainStage> {
        SourceDrainStage::canonical()
    }
}
