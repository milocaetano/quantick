use super::*;

#[test]
fn canonical_live_trade_dependencies_are_the_executable_traversal() {
    assert!(valid(&STAGES));
    assert_eq!(
        LiveTradePlan::stages().collect::<Vec<_>>(),
        [
            LiveTradeStage::PaperTrade,
            LiveTradeStage::PaneTrades,
            LiveTradeStage::StrategyEvaluation,
        ]
    );
}

#[test]
fn live_trade_plan_refuses_missing_duplicate_and_forward_dependencies() {
    assert!(!valid(&STAGES[..2]));
    assert!(!valid(&[]));
    let mut duplicate = STAGES;
    duplicate[1] = duplicate[0];
    assert!(!valid(&duplicate));
    let reordered = [STAGES[1], STAGES[2], STAGES[0]];
    assert!(!valid(&reordered));
}

#[test]
fn live_trade_plan_refuses_cycles_self_edges_and_unknown_dependencies() {
    let mut cyclic = STAGES;
    cyclic[0].after = LiveTradeStage::StrategyEvaluation.bit();
    assert!(!valid(&cyclic));
    let mut self_edge = STAGES;
    self_edge[0].after = LiveTradeStage::PaperTrade.bit();
    assert!(!valid(&self_edge));
    let mut unknown = STAGES;
    unknown[0].after = 1 << 7;
    assert!(!valid(&unknown));
}
