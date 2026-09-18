use super::*;
use crate::stage_registry::{dependency_pairs, hoisted};

#[test]
fn the_frame_executes_its_registered_order() {
    assert_eq!(FramePlan::stages().collect::<Vec<_>>(), FrameStage::ORDER);
    assert_eq!(FramePlan::stages().len(), FrameStage::COUNT);
    assert_eq!(FrameStage::NODES[0].name, "RecordFrameTime");
}

#[test]
fn the_frame_refuses_every_hoisted_dependency() {
    let pairs: Vec<_> = dependency_pairs(&FrameStage::NODES).collect();
    assert!(pairs.len() >= 20, "{pairs:?}");
    for (earlier, later) in pairs {
        let order = hoisted(FrameStage::ORDER, earlier, later);
        assert!(!FrameStage::is_valid_order(&order), "{order:?}");
    }
}

#[test]
fn independent_frame_stages_stay_free_to_move() {
    // The drain and the frame clock declare nothing between them.
    let mut order = FrameStage::ORDER;
    order.swap(0, 1);
    assert!(FrameStage::is_valid_order(&order));
}
