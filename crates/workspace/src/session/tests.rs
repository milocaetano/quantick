use super::*;

#[test]
fn removal_invalidates_retained_views_and_stale_selection() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let old = session.register(10, Some(LayoutId(1)));
    let next = session.create(Some("other")).unwrap();
    let selection = session.select(10, next, PaneFacts::default()).unwrap();
    session.remove_pane(10);
    assert_eq!(old.layout(), None);
    let current = session.register(10, Some(LayoutId(1)));
    assert_eq!(
        session.commit_selection(selection),
        Err(LayoutError::Unknown)
    );
    assert_eq!(current.layout(), Some(LayoutId(1)));
    assert_eq!(old.layout(), None);
}

#[test]
fn selection_keeps_outgoing_membership_until_commit_and_default_until_finish() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let view = session.register(10, Some(LayoutId(1)));
    let second = session.create(Some("second")).unwrap();
    let selection = session.select(10, second, PaneFacts::default()).unwrap();
    assert_eq!(view.layout(), Some(LayoutId(1)));
    let committed = session.commit_selection(selection).unwrap();
    assert_eq!(view.layout(), Some(second));
    assert!(view.seeded());
    assert_eq!(session.book().active_id(), LayoutId(1));
    session.finish_selection(committed).unwrap();
    assert_eq!(session.book().active_id(), second);
}

#[test]
fn no_op_selection_bypasses_refusal_and_does_not_seed_twice() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let view = session.register(10, None);
    let selection = session
        .select(
            10,
            LayoutId(1),
            PaneFacts {
                strategy_armed: true,
                gesture_in_flight: true,
            },
        )
        .unwrap();
    assert!(!selection.changed());
    session.commit_selection(selection).unwrap();
    assert_eq!(view.layout(), Some(LayoutId(1)));
    assert!(!view.seeded());
    assert_eq!(session.seed(10, None, None), Some(LayoutId(1)));
    assert_eq!(session.seed(10, None, None), None);
}

#[test]
fn all_panes_are_preflighted_before_delete_without_partial_state() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let second = session.create(Some("second")).unwrap();
    let first_view = session.register(10, Some(second));
    let last_view = session.register(20, Some(second));
    let facts = [
        (10, PaneFacts::default()),
        (
            20,
            PaneFacts {
                strategy_armed: true,
                gesture_in_flight: true,
            },
        ),
    ];
    assert_eq!(
        session.plan_delete(second, facts.into_iter()),
        Err(LayoutError::StrategyArmed)
    );
    assert_eq!(first_view.layout(), Some(second));
    assert_eq!(last_view.layout(), Some(second));
    assert!(session.book().get(second).is_some());
}

#[test]
fn mirror_order_follows_stable_pane_identity_after_reorder() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let second = session.create(None).unwrap();
    session.register(10, Some(LayoutId(1)));
    session.register(20, Some(second));
    session.register(30, Some(LayoutId(1)));
    assert_eq!(
        session
            .targets(LayoutId(1), [30, 20, 10].into_iter())
            .collect::<Vec<_>>(),
        vec![30, 10]
    );
}

#[test]
fn background_pane_inherits_its_flow_before_foreground_focus() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let second = session.create(None).unwrap();
    session.register(10, Some(LayoutId(1)));
    session.seed(10, None, None);
    session.register(20, Some(second));
    session.seed(20, None, None);
    session.register(21, None);
    assert_eq!(session.seed(21, Some(20), Some(10)), Some(second));
}

#[test]
fn delete_cannot_omit_a_registered_member_or_finish_before_swaps() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let second = session.create(None).unwrap();
    session.register(1, Some(second));
    session.register(2, Some(second));
    assert_eq!(
        session.plan_delete(second, [(1, PaneFacts::default())].into_iter()),
        Err(LayoutError::Unknown)
    );
    assert_eq!(
        session.plan_delete(
            second,
            [(1, PaneFacts::default()), (1, PaneFacts::default())].into_iter()
        ),
        Err(LayoutError::Unknown)
    );
    assert_eq!(session.finish_delete(second), Err(LayoutError::Unknown));
}

#[test]
fn finishing_after_removal_or_replacement_cannot_move_default() {
    let mut session = LayoutSession::new(LayoutBook::default());
    session.register(1, None);
    let second = session.create(None).unwrap();
    let selection = session.select(1, second, PaneFacts::default()).unwrap();
    let committed = session.commit_selection(selection).unwrap();
    session.remove_pane(1);
    assert_eq!(
        session.finish_selection(committed),
        Err(LayoutError::Unknown)
    );
    assert_eq!(session.book().active_id(), LayoutId(1));
    session.register(1, None);
    let selection = session.select(1, second, PaneFacts::default()).unwrap();
    let committed = session.commit_selection(selection).unwrap();
    session.replace_book(LayoutBook::default());
    assert_eq!(
        session.finish_selection(committed),
        Err(LayoutError::Unknown)
    );
}

#[test]
fn intervening_round_trip_invalidates_earlier_completion() {
    let mut session = LayoutSession::new(LayoutBook::default());
    session.register(1, Some(LayoutId(1)));
    let second = session.create(None).unwrap();
    let selection = session.select(1, second, PaneFacts::default()).unwrap();
    let old = session.commit_selection(selection).unwrap();
    let back = session
        .select(1, LayoutId(1), PaneFacts::default())
        .unwrap();
    let back = session.commit_selection(back).unwrap();
    session.finish_selection(back).unwrap();
    let again = session.select(1, second, PaneFacts::default()).unwrap();
    session.commit_selection(again).unwrap();
    assert_eq!(session.finish_selection(old), Err(LayoutError::Unknown));
    assert_eq!(session.book().active_id(), LayoutId(1));
}

#[test]
fn removed_target_cannot_be_committed_and_seed_invalidates_earlier_plan() {
    let mut session = LayoutSession::new(LayoutBook::default());
    session.register(1, None);
    let second = session.create(None).unwrap();
    let selection = session.select(1, second, PaneFacts::default()).unwrap();
    session.finish_delete(second).unwrap();
    assert_eq!(
        session.commit_selection(selection),
        Err(LayoutError::Unknown)
    );
    let selection = session
        .select(1, LayoutId(1), PaneFacts::default())
        .unwrap();
    session.seed(1, None, None);
    assert_eq!(
        session.commit_selection(selection),
        Err(LayoutError::Unknown)
    );
}

#[test]
fn a_gesture_alone_refuses_selection_and_delete() {
    let gesture = PaneFacts {
        strategy_armed: false,
        gesture_in_flight: true,
    };
    let mut session = LayoutSession::new(LayoutBook::default());
    let second = session.create(Some("second")).unwrap();
    let view = session.register(10, Some(LayoutId(1)));
    session.register(20, Some(second));
    assert_eq!(
        session.select(10, second, gesture).map(|_| ()),
        Err(LayoutError::GestureInFlight)
    );
    assert_eq!(view.layout(), Some(LayoutId(1)));
    assert_eq!(
        session
            .plan_delete(
                second,
                [(10, PaneFacts::default()), (20, gesture)].into_iter()
            )
            .map(|_| ()),
        Err(LayoutError::GestureInFlight)
    );
    assert!(session.book().get(second).is_some());
}
