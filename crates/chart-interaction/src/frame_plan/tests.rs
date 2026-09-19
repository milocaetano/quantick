use crate::stage_registry::{dependency_pairs, hoisted};

/// Every harness family compiled in: the capture build's plan.
mod harness {
    crate::frame_stages!(scenario: all(), control: all(), scripted: all());
}

/// No harness family: the plan a release build runs.
mod release {
    crate::frame_stages!(scenario: any(), control: any(), scripted: any());
}

#[test]
fn the_frame_executes_its_registered_order() {
    use harness::{FramePlan, FrameStage};
    assert_eq!(FramePlan::stages().collect::<Vec<_>>(), FrameStage::ORDER);
    assert_eq!(FramePlan::stages().len(), FrameStage::COUNT);
    assert_eq!(FrameStage::NODES[0].name, "RecordFrameTime");
}

#[test]
fn the_frame_refuses_every_hoisted_dependency_in_either_build() {
    let pairs: Vec<_> = dependency_pairs(&harness::FrameStage::NODES).collect();
    assert_eq!(pairs.len(), 31, "{pairs:?}");
    for (earlier, later) in pairs {
        let order = hoisted(harness::FrameStage::ORDER, earlier, later);
        assert!(!harness::FrameStage::is_valid_order(&order), "{order:?}");
    }
    let pairs: Vec<_> = dependency_pairs(&release::FrameStage::NODES).collect();
    assert_eq!(pairs.len(), 26, "{pairs:?}");
    for (earlier, later) in pairs {
        let order = hoisted(release::FrameStage::ORDER, earlier, later);
        assert!(!release::FrameStage::is_valid_order(&order), "{order:?}");
    }
}

/// A release build registers no harness stage: not a no-op arm, no stage.
#[test]
fn a_release_frame_has_no_harness_stage() {
    let release: Vec<_> = release::FrameStage::NODES
        .iter()
        .map(|node| node.name)
        .collect();
    let harness: Vec<_> = harness::FrameStage::NODES
        .iter()
        .map(|node| node.name)
        .filter(|name| !release.contains(name))
        .collect();
    assert_eq!(
        harness,
        [
            "HistoryNoteHook",
            "EnableControlAccess",
            "TakeMark",
            "AnnotateHooks",
            "EvidenceHook",
            "ScenarioHooks",
            "WindowStartupHook",
        ]
    );
    assert_eq!(release::FrameStage::COUNT, 24);
    assert_eq!(harness::FrameStage::COUNT, 31);
}

#[test]
fn independent_frame_stages_stay_free_to_move() {
    // The drain and the frame clock declare nothing between them.
    let mut order: Vec<_> = release::FramePlan::stages().collect();
    order.swap(0, 1);
    assert!(release::FrameStage::is_valid_order(&order));
}
