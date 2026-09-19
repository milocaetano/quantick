//! Fixed dependencies for one ordinary live print, before the next is received.
//!
//! The caller performs the effects against its paper and pane owners. Resumed
//! history must not use this plan: it seeds the mark without executing orders.

use crate::stage_registry::declare_stages;
#[cfg(test)]
use crate::stage_registry::{StageNode, nodes_in_valid_order};

#[cfg(test)]
mod tests;

declare_stages! {
    pub enum LiveTradeStage {
        /// The simulator fills against the print before any bar moves.
        PaperTrade after [],
        /// Every pane folds the print into its bars.
        PaneTrades after [PaperTrade],
        /// Strategies judge the bars this print just built.
        StrategyEvaluation after [PaneTrades],
    }
}

#[cfg(test)]
type StageDescriptor = StageNode<LiveTradeStage>;

// The test-facing names the reorder proofs in `tests` are written against.
#[cfg(test)]
const STAGES: [StageDescriptor; LiveTradeStage::COUNT] = LiveTradeStage::NODES;

#[cfg(test)]
const fn valid(stages: &[StageDescriptor]) -> bool {
    nodes_in_valid_order(stages, LiveTradeStage::COUNT)
}

/// The canonical synchronous traversal; no allocation, sort or payload copy.
/// Consuming a stage is not proof that an effect succeeded: the caller keeps
/// the real simulator outcomes and tests its actual effects independently.
pub struct LiveTradePlan;

impl LiveTradePlan {
    pub fn stages() -> impl ExactSizeIterator<Item = LiveTradeStage> {
        LiveTradeStage::canonical()
    }
}
