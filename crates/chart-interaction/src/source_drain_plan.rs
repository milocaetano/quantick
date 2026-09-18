//! Synchronous source-drain dependencies. Workers may still be pending afterward.
//! Event payloads, FIFO reception and intrinsic publication belong to the caller.

use crate::stage_plan::Step;

#[cfg(test)]
mod tests;

crate::stage_plan::stage_enum! {
    pub enum SourceDrainStage {
        PrepareSymbol,
        ReceiveAvailable,
        PublishLatestPartial,
        LandGap,
        SettleReanchors,
        TickDealRecording,
    }
}

// These are actual synchronous prerequisites, not worker readiness claims.
const STAGES: [Step<SourceDrainStage>; 6] = [
    Step {
        stage: SourceDrainStage::PrepareSymbol,
        after: 0,
    },
    Step {
        stage: SourceDrainStage::ReceiveAvailable,
        after: SourceDrainStage::PrepareSymbol.bit(),
    },
    Step {
        stage: SourceDrainStage::PublishLatestPartial,
        after: SourceDrainStage::ReceiveAvailable.bit(),
    },
    Step {
        stage: SourceDrainStage::LandGap,
        after: SourceDrainStage::PublishLatestPartial.bit(),
    },
    Step {
        stage: SourceDrainStage::SettleReanchors,
        after: SourceDrainStage::LandGap.bit(),
    },
    Step {
        stage: SourceDrainStage::TickDealRecording,
        after: SourceDrainStage::SettleReanchors.bit(),
    },
];
const _: () = assert!(valid(&STAGES));

/// Canonical traversal; no allocation, event queue, clock or success receipt.
pub struct SourceDrainPlan;
impl SourceDrainPlan {
    pub fn stages() -> impl ExactSizeIterator<Item = SourceDrainStage> {
        STAGES.iter().map(|step| step.stage)
    }
}
