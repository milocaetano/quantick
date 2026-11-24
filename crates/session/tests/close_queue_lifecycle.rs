use quantick_session::close_queue::{
    CloseAnswer, CloseQuestion, CloseQueue, CloseScope, CloseStep,
};

fn pending(tab: u64) -> Option<CloseQuestion> {
    Some(CloseQuestion {
        tab,
        symbol: format!("market-{tab}"),
        workspace: "desk".to_owned(),
        changes: vec!["Drawing moved".to_owned()],
        followers: vec!["linked market".to_owned()],
    })
}

#[test]
fn repeated_frames_keep_the_same_question_without_rechecking_pending_state() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::Tabs, [7, 9]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    let question = queue.question().unwrap().clone();
    for _ in 0..3 {
        assert_eq!(
            queue.next(|_| panic!("pending was already evaluated")),
            CloseStep::Ask
        );
        assert_eq!(queue.question(), Some(&question));
        assert_eq!(queue.scope(), Some(CloseScope::Tabs));
    }
    queue.answer(CloseAnswer::Save);
    assert!(queue.question().is_none());
    assert_eq!(
        queue.next(pending),
        CloseStep::Settle {
            tab: 7,
            save: Some(true),
            scope: CloseScope::Tabs,
        }
    );
    assert_eq!(queue.next(pending), CloseStep::Ask);
    assert_eq!(queue.question().unwrap().tab, 9);
}

#[test]
fn an_answer_before_a_question_cannot_skip_the_first_dirty_tab() {
    let mut queue = CloseQueue::default();
    queue.answer(CloseAnswer::Save);
    assert_eq!(queue.next(pending), CloseStep::Idle);
    queue.request(CloseScope::Tabs, [1]);
    queue.answer(CloseAnswer::CloseWithoutSaving);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    assert_eq!(queue.question().unwrap().tab, 1);
    assert!(!queue.take_exit());
}

#[test]
fn a_duplicate_answer_cannot_overwrite_an_already_accepted_save() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::Tabs, [1]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::Save);
    queue.answer(CloseAnswer::CloseWithoutSaving);
    queue.answer(CloseAnswer::Cancel);
    assert_eq!(
        queue.next(pending),
        CloseStep::Settle {
            tab: 1,
            save: Some(true),
            scope: CloseScope::Tabs,
        }
    );
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::Tabs));
    assert_eq!(queue.next(pending), CloseStep::Idle);
}

#[test]
fn aborting_an_unanswered_exit_drops_its_question_and_releases_the_queue() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::App, [1, 2]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.abort();
    assert!(queue.question().is_none());
    assert!(queue.scope().is_none());
    assert_eq!(queue.next(pending), CloseStep::Idle);
    assert!(!queue.take_exit());
    assert!(queue.request(CloseScope::Tabs, [3]));
    assert_eq!(queue.next(pending), CloseStep::Ask);
    assert_eq!(queue.question().unwrap().tab, 3);
}

#[test]
fn a_failed_exit_save_aborts_before_any_discard_or_exit_signal() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::App, [1, 2, 3]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::CloseWithoutSaving);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::Save);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::CloseWithoutSaving);
    assert_eq!(
        queue.next(pending),
        CloseStep::Settle {
            tab: 2,
            save: Some(true),
            scope: CloseScope::App,
        }
    );
    // The caller reports the save failure by aborting rather than advancing.
    queue.abort();
    assert_eq!(queue.next(pending), CloseStep::Idle);
    assert!(!queue.take_exit());
    assert!(queue.request(CloseScope::App, [1, 2, 3]));
    assert_eq!(queue.next(pending), CloseStep::Ask);
    assert_eq!(queue.question().unwrap().tab, 1);
}

#[test]
fn exit_settlement_does_not_requery_tabs_after_all_answers_are_collected() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::App, [1, 2]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::Save);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::Save);
    for tab in [1, 2] {
        assert_eq!(
            queue.next(|_| panic!("settlement must use the collected decisions")),
            CloseStep::Settle {
                tab,
                save: Some(true),
                scope: CloseScope::App
            }
        );
        assert!(
            !queue.take_exit(),
            "exit is not signalled before settlement completes"
        );
    }
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::App));
    assert!(queue.take_exit());
    assert!(
        !queue.take_exit(),
        "the window consumes the signal exactly once"
    );
    assert_eq!(queue.next(pending), CloseStep::Idle);
}

#[test]
fn an_empty_tab_close_completes_without_requesting_an_application_exit() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::Tabs, []);
    assert_eq!(
        queue.next(|_| panic!("there are no tabs")),
        CloseStep::Done(CloseScope::Tabs)
    );
    assert!(!queue.take_exit());
    assert!(queue.scope().is_none());
    assert_eq!(queue.next(pending), CloseStep::Idle);
}

#[test]
fn an_empty_application_exit_still_signals_the_window_once() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::App, []);
    assert_eq!(
        queue.next(|_| panic!("there are no tabs")),
        CloseStep::Done(CloseScope::App)
    );
    assert!(queue.take_exit());
    assert!(!queue.take_exit());
    assert!(queue.scope().is_none());
}

#[test]
fn a_tab_becoming_clean_before_its_turn_is_settled_without_a_stale_prompt() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::Tabs, [1, 2]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::Save);
    assert_eq!(
        queue.next(pending),
        CloseStep::Settle {
            tab: 1,
            save: Some(true),
            scope: CloseScope::Tabs,
        }
    );
    // Saving the first workspace can clean its follower before its turn.
    let mut inspected = Vec::new();
    assert_eq!(
        queue.next(|tab| {
            inspected.push(tab);
            None
        }),
        CloseStep::Settle {
            tab: 2,
            save: None,
            scope: CloseScope::Tabs,
        }
    );
    assert_eq!(inspected, [2]);
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::Tabs));
}

#[test]
fn aborting_after_an_answer_does_not_leak_that_answer_into_the_next_request() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::Tabs, [1]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    queue.answer(CloseAnswer::Save);
    queue.abort();
    queue.request(CloseScope::Tabs, [2]);
    assert_eq!(queue.next(pending), CloseStep::Ask);
    assert_eq!(queue.question().unwrap().tab, 2);
    queue.answer(CloseAnswer::CloseWithoutSaving);
    assert_eq!(
        queue.next(pending),
        CloseStep::Settle {
            tab: 2,
            save: Some(false),
            scope: CloseScope::Tabs,
        }
    );
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::Tabs));
}

#[test]
fn a_pending_exit_signal_survives_a_later_tab_close_until_consumed() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::App, []);
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::App));
    queue.request(CloseScope::Tabs, [1]);
    assert_eq!(
        queue.next(|_| None),
        CloseStep::Settle {
            tab: 1,
            save: None,
            scope: CloseScope::Tabs,
        }
    );
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::Tabs));
    assert!(queue.take_exit());
    assert!(!queue.take_exit());
}

#[test]
fn new_requests_are_rejected_during_exit_settlement_without_consuming_their_tabs() {
    let mut queue = CloseQueue::default();
    queue.request(CloseScope::App, [1, 2]);
    assert_eq!(
        queue.next(|_| None),
        CloseStep::Settle {
            tab: 1,
            save: None,
            scope: CloseScope::App,
        }
    );
    let incoming = std::iter::once_with(|| panic!("rejected request must not consume input"));
    assert!(!queue.request(CloseScope::Tabs, incoming));
    assert_eq!(
        queue.next(pending),
        CloseStep::Settle {
            tab: 2,
            save: None,
            scope: CloseScope::App,
        }
    );
    assert_eq!(queue.next(pending), CloseStep::Done(CloseScope::App));
    assert!(queue.take_exit());
}
