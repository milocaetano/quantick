//! Fixed dependencies for the synchronous end of an application frame.
//!
//! Recovery may journal a close, so every account settles before the active
//! report is painted. Feature outcomes remain with the caller.

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameTailStage {
    ApplyNoticeAction,
    SettlePaperPanels,
    DrawPaperReport,
    PublishFeedPopup,
}

impl FrameTailStage {
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy)]
struct StageDescriptor {
    stage: FrameTailStage,
    after: u8,
}

const STAGES: [StageDescriptor; 4] = [
    StageDescriptor {
        stage: FrameTailStage::ApplyNoticeAction,
        after: 0,
    },
    StageDescriptor {
        stage: FrameTailStage::SettlePaperPanels,
        after: FrameTailStage::ApplyNoticeAction.bit(),
    },
    StageDescriptor {
        stage: FrameTailStage::DrawPaperReport,
        after: FrameTailStage::SettlePaperPanels.bit(),
    },
    StageDescriptor {
        stage: FrameTailStage::PublishFeedPopup,
        after: FrameTailStage::DrawPaperReport.bit(),
    },
];

// Requiring every prerequisite to have been visited rejects forward edges,
// self edges, cycles and unknown prerequisite bits as well as bad coverage.
const fn valid(stages: &[StageDescriptor]) -> bool {
    let mut seen = 0;
    let mut index = 0;
    while index < stages.len() {
        let descriptor = stages[index];
        if seen & descriptor.stage.bit() != 0 || descriptor.after & seen != descriptor.after {
            return false;
        }
        seen |= descriptor.stage.bit();
        index += 1;
    }
    seen == 15
}

const _: () = assert!(valid(&STAGES));

/// The canonical synchronous traversal; no allocation, sort or payload copy.
/// Consuming a stage is not proof that an effect succeeded: the caller keeps
/// the real recovery outcomes and tests its actual effects independently.
pub struct FrameTailPlan;

impl FrameTailPlan {
    pub fn stages() -> impl ExactSizeIterator<Item = FrameTailStage> {
        STAGES.iter().map(|descriptor| descriptor.stage)
    }
}
