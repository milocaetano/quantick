//! Fixed dependencies for one ordinary live print, before the next is received.
//!
//! The caller performs the effects against its paper and pane owners. Resumed
//! history must not use this plan: it seeds the mark without executing orders.

use crate::stage_plan::Step;

#[cfg(test)]
mod tests;

crate::stage_plan::stage_enum! {
    pub enum LiveTradeStage {
        PaperTrade,
        PaneTrades,
        StrategyEvaluation,
    }
}

const STAGES: [Step<LiveTradeStage>; 3] = [
    Step {
        stage: LiveTradeStage::PaperTrade,
        after: 0,
    },
    Step {
        stage: LiveTradeStage::PaneTrades,
        after: LiveTradeStage::PaperTrade.bit(),
    },
    Step {
        stage: LiveTradeStage::StrategyEvaluation,
        after: LiveTradeStage::PaneTrades.bit(),
    },
];

const _: () = assert!(valid(&STAGES));

/// The canonical synchronous traversal; no allocation, sort or payload copy.
/// Consuming a stage is not proof that an effect succeeded: the caller keeps
/// the real simulator outcomes and tests its actual effects independently.
pub struct LiveTradePlan;

impl LiveTradePlan {
    pub fn stages() -> impl ExactSizeIterator<Item = LiveTradeStage> {
        STAGES.iter().map(|descriptor| descriptor.stage)
    }
}
