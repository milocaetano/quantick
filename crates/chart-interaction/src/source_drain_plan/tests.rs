use super::*;

#[test]
fn canonical_source_traversal_preserves_each_actual_prerequisite() {
    assert!(valid(&STAGES));
    assert_eq!(
        SourceDrainPlan::stages().collect::<Vec<_>>(),
        [
            SourceDrainStage::PrepareSymbol,
            SourceDrainStage::ReceiveAvailable,
            SourceDrainStage::PublishLatestPartial,
            SourceDrainStage::LandGap,
            SourceDrainStage::SettleReanchors,
            SourceDrainStage::TickDealRecording,
        ]
    );
}

#[test]
fn source_traversal_rejects_missing_duplicate_and_every_adjacent_inversion() {
    assert!(!valid(&[]));
    for index in 0..STAGES.len() {
        let missing: Vec<_> = STAGES
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != index)
            .map(|(_, stage)| *stage)
            .collect();
        assert!(!valid(&missing));
        let mut duplicate = STAGES;
        duplicate[index] = STAGES[(index + 1) % STAGES.len()];
        assert!(!valid(&duplicate));
    }
    for index in 1..STAGES.len() {
        let mut inverted = STAGES;
        inverted.swap(index - 1, index);
        assert!(!valid(&inverted));
    }
}
