use super::*;

declare_stages! {
    /// A diamond: two independent stages between a source and a sink.
    enum Diamond {
        Source after [],
        Left after [Source],
        Right after [Source],
        Sink after [Left, Right],
    }
}

#[test]
fn a_registered_stage_carries_its_name_and_declared_dependencies() {
    assert_eq!(Diamond::COUNT, 4);
    assert_eq!(Diamond::Sink.name(), "Sink");
    assert_eq!(Diamond::NODES[3].name, "Sink");
    assert_eq!(
        Diamond::Sink.after(),
        Diamond::Left.bit() | Diamond::Right.bit()
    );
    assert_eq!(Diamond::Source.after(), 0);
    assert_eq!(
        Diamond::canonical().collect::<Vec<_>>(),
        [
            Diamond::Source,
            Diamond::Left,
            Diamond::Right,
            Diamond::Sink
        ]
    );
}

#[test]
fn independent_stages_may_swap_and_dependent_ones_may_not() {
    use Diamond::*;
    assert!(Diamond::is_valid_order(&[Source, Right, Left, Sink]));
    assert!(!Diamond::is_valid_order(&[Left, Source, Right, Sink]));
    assert!(!Diamond::is_valid_order(&[Source, Left, Sink, Right]));
}

#[test]
fn every_declared_dependency_is_refused_when_hoisted() {
    let pairs: Vec<_> = dependency_pairs(&Diamond::NODES).collect();
    assert_eq!(pairs, [(0, 1), (0, 2), (1, 3), (2, 3)]);
    for (earlier, later) in pairs {
        let order = hoisted(Diamond::ORDER, earlier, later);
        assert!(!Diamond::is_valid_order(&order), "{order:?}");
        let nodes = hoisted(Diamond::NODES, earlier, later);
        assert!(!nodes_in_valid_order(&nodes, Diamond::COUNT));
    }
}

#[test]
fn hoisting_moves_one_stage_in_front_of_another() {
    assert_eq!(hoisted([0, 1, 2, 3, 4], 1, 3), [0, 3, 1, 2, 4]);
}

#[test]
fn validation_refuses_missing_duplicate_cycles_self_edges_and_unknown_bits() {
    let nodes = Diamond::NODES;
    assert!(nodes_in_valid_order(&nodes, Diamond::COUNT));
    assert!(!nodes_in_valid_order(&nodes[..3], Diamond::COUNT));
    assert!(!nodes_in_valid_order(
        &[] as &[StageNode<Diamond>],
        Diamond::COUNT
    ));
    assert!(!nodes_in_valid_order(&nodes, 0));
    assert!(!nodes_in_valid_order(&nodes, MAX_STAGES + 1));
    let mut duplicate = nodes;
    duplicate[2] = duplicate[1];
    assert!(!nodes_in_valid_order(&duplicate, Diamond::COUNT));
    let mut cyclic = nodes;
    cyclic[0].after = Diamond::Sink.bit();
    assert!(!nodes_in_valid_order(&cyclic, Diamond::COUNT));
    let mut self_edge = nodes;
    self_edge[1].after |= Diamond::Left.bit();
    assert!(!nodes_in_valid_order(&self_edge, Diamond::COUNT));
    let mut unknown = nodes;
    unknown[1].after |= 1 << 9;
    assert!(!nodes_in_valid_order(&unknown, Diamond::COUNT));
    let mut out_of_range = nodes;
    out_of_range[3].bit = 1 << 9;
    assert!(!nodes_in_valid_order(&out_of_range, Diamond::COUNT));
}

#[test]
fn coverage_follows_the_declared_variants_not_a_hand_mask() {
    // Dropping the last stage is refused because the macro counted four.
    assert!(Diamond::is_valid_order(&Diamond::ORDER));
    assert!(!Diamond::is_valid_order(&Diamond::ORDER[..3]));
    assert!(!Diamond::is_valid_order(&[]));
}
