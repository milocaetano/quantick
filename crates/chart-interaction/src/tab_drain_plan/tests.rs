use super::*;
use crate::stage_registry::{dependency_pairs, hoisted};

#[test]
fn the_tab_drain_executes_its_registered_order() {
    assert_eq!(
        TabDrainPlan::stages().collect::<Vec<_>>(),
        TabDrainStage::ORDER
    );
    assert!(TabDrainStage::is_valid_order(&TabDrainStage::ORDER));
}

#[test]
fn the_tab_drain_refuses_every_hoisted_dependency() {
    let pairs: Vec<_> = dependency_pairs(&TabDrainStage::NODES).collect();
    assert_eq!(pairs.len(), 2);
    for (earlier, later) in pairs {
        let order = hoisted(TabDrainStage::ORDER, earlier, later);
        assert!(!TabDrainStage::is_valid_order(&order), "{order:?}");
    }
}
