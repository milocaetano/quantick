//! Fixed dependencies for one ordinary live print, before the next is received.
//!
//! The caller performs the effects against its paper and pane owners. Resumed
//! history must not use this plan: it seeds the mark without executing orders.

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTradeStage {
    PaperTrade,
    PaneTrades,
    StrategyEvaluation,
}

impl LiveTradeStage {
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy)]
struct StageDescriptor {
    stage: LiveTradeStage,
    after: u8,
}

const STAGES: [StageDescriptor; 3] = [
    StageDescriptor {
        stage: LiveTradeStage::PaperTrade,
        after: 0,
    },
    StageDescriptor {
        stage: LiveTradeStage::PaneTrades,
        after: LiveTradeStage::PaperTrade.bit(),
    },
    StageDescriptor {
        stage: LiveTradeStage::StrategyEvaluation,
        after: LiveTradeStage::PaneTrades.bit(),
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
    seen == 7
}

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
