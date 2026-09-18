//! Fixed dependencies for the synchronous end of an application frame.
//!
//! Recovery may journal a close, so every account settles before the active
//! report is painted. Feature outcomes remain with the caller.

use crate::stage_registry::declare_stages;
#[cfg(test)]
use crate::stage_registry::{StageNode, nodes_in_valid_order};

#[cfg(test)]
mod tests;

declare_stages! {
    pub enum FrameTailStage {
        /// The feed notice's reconnect or reload runs first.
        ApplyNoticeAction after [],
        /// A reload may journal a close; every account settles after it.
        SettlePaperPanels after [ApplyNoticeAction],
        /// The report reads the settled account.
        DrawPaperReport after [SettlePaperPanels],
        /// The popup state is published last, from this frame's answers.
        PublishFeedPopup after [DrawPaperReport],
    }
}

#[cfg(test)]
type StageDescriptor = StageNode<FrameTailStage>;

// The test-facing names the reorder proofs in `tests` are written against.
#[cfg(test)]
const STAGES: [StageDescriptor; FrameTailStage::COUNT] = FrameTailStage::NODES;

#[cfg(test)]
const fn valid(stages: &[StageDescriptor]) -> bool {
    nodes_in_valid_order(stages, FrameTailStage::COUNT)
}

/// The canonical synchronous traversal; no allocation, sort or payload copy.
/// Consuming a stage is not proof that an effect succeeded: the caller keeps
/// the real recovery outcomes and tests its actual effects independently.
pub struct FrameTailPlan;

impl FrameTailPlan {
    pub fn stages() -> impl ExactSizeIterator<Item = FrameTailStage> {
        FrameTailStage::canonical()
    }
}
