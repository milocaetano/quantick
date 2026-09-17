//! Synchronous source-drain dependencies. Workers may still be pending afterward.
//! Event payloads, FIFO reception and intrinsic publication belong to the caller.

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceDrainStage {
    PrepareSymbol,
    ReceiveAvailable,
    PublishLatestPartial,
    LandGap,
    SettleReanchors,
    TickDealRecording,
}

impl SourceDrainStage {
    const fn bit(self) -> u8 {
        1 << self as u8
    }

    // These are actual synchronous prerequisites, not worker readiness claims.
    const fn prerequisite(self) -> Option<Self> {
        match self {
            Self::PrepareSymbol => None,
            Self::ReceiveAvailable => Some(Self::PrepareSymbol),
            Self::PublishLatestPartial => Some(Self::ReceiveAvailable),
            Self::LandGap => Some(Self::PublishLatestPartial),
            Self::SettleReanchors => Some(Self::LandGap),
            Self::TickDealRecording => Some(Self::SettleReanchors),
        }
    }
}

const STAGES: [SourceDrainStage; 6] = [
    SourceDrainStage::PrepareSymbol,
    SourceDrainStage::ReceiveAvailable,
    SourceDrainStage::PublishLatestPartial,
    SourceDrainStage::LandGap,
    SourceDrainStage::SettleReanchors,
    SourceDrainStage::TickDealRecording,
];

const fn valid(stages: &[SourceDrainStage]) -> bool {
    let mut seen = 0;
    let mut index = 0;
    while index < stages.len() {
        let stage = stages[index];
        if seen & stage.bit() != 0 {
            return false;
        }
        if let Some(prerequisite) = stage.prerequisite()
            && seen & prerequisite.bit() == 0
        {
            return false;
        }
        seen |= stage.bit();
        index += 1;
    }
    seen == 63
}
const _: () = assert!(valid(&STAGES));

/// Canonical traversal; no allocation, event queue, clock or success receipt.
pub struct SourceDrainPlan;
impl SourceDrainPlan {
    pub fn stages() -> impl ExactSizeIterator<Item = SourceDrainStage> {
        STAGES.iter().copied()
    }
}
