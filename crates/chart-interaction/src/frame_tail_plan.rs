//! Fixed dependencies for the synchronous end of an application frame.
//!
//! Recovery may journal a close, so every account settles before the active
//! report is painted. Feature outcomes remain with the caller.

use crate::stage_plan::Step;

#[cfg(test)]
mod tests;

crate::stage_plan::stage_enum! {
    pub enum FrameTailStage {
        ApplyNoticeAction,
        SettlePaperPanels,
        DrawPaperReport,
        PublishFeedPopup,
    }
}

const STAGES: [Step<FrameTailStage>; 4] = [
    Step {
        stage: FrameTailStage::ApplyNoticeAction,
        after: 0,
    },
    Step {
        stage: FrameTailStage::SettlePaperPanels,
        after: FrameTailStage::ApplyNoticeAction.bit(),
    },
    Step {
        stage: FrameTailStage::DrawPaperReport,
        after: FrameTailStage::SettlePaperPanels.bit(),
    },
    Step {
        stage: FrameTailStage::PublishFeedPopup,
        after: FrameTailStage::DrawPaperReport.bit(),
    },
];

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
