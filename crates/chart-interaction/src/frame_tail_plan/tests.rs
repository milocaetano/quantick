use super::*;

#[test]
fn canonical_frame_tail_dependencies_are_the_executable_traversal() {
    assert!(valid(&STAGES));
    assert_eq!(
        FrameTailPlan::stages().collect::<Vec<_>>(),
        [
            FrameTailStage::ApplyNoticeAction,
            FrameTailStage::SettlePaperPanels,
            FrameTailStage::DrawPaperReport,
            FrameTailStage::PublishFeedPopup,
        ]
    );
}

#[test]
fn frame_tail_plan_refuses_missing_duplicate_and_forward_dependencies() {
    assert!(!valid(&STAGES[..2]));
    assert!(!valid(&[]));
    let mut duplicate = STAGES;
    duplicate[1] = duplicate[0];
    assert!(!valid(&duplicate));
    let reordered = [STAGES[1], STAGES[2], STAGES[0], STAGES[3]];
    assert!(!valid(&reordered));
}

#[test]
fn frame_tail_plan_refuses_cycles_self_edges_and_unknown_dependencies() {
    let mut cyclic = STAGES;
    cyclic[0].after = FrameTailStage::DrawPaperReport.bit();
    assert!(!valid(&cyclic));
    let mut self_edge = STAGES;
    self_edge[0].after = FrameTailStage::ApplyNoticeAction.bit();
    assert!(!valid(&self_edge));
    let mut unknown = STAGES;
    unknown[0].after = 1 << 7;
    assert!(!valid(&unknown));
}
