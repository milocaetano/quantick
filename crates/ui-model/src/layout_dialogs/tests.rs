use super::*;
use crate::Model as _;

fn begin(state: &mut State, tab: u64, pane: PaneSide, layout: LayoutId) {
    assert_eq!(
        Model::update(
            state,
            Intent::BeginRename {
                tab,
                pane,
                layout,
                name: "Original".into(),
            },
        ),
        Effects::None
    );
}

fn finish(escaped: bool, enter: bool, lost_focus: bool) -> Intent {
    Intent::Finish {
        escaped,
        enter,
        lost_focus,
    }
}

#[test]
fn layout_rename_keeps_its_address_and_replaces_only_on_begin() {
    let mut state = State::default();
    begin(&mut state, 7, PaneSide::Time(1), LayoutId(3));
    Model::update(&mut state, Intent::Edit("Changed".into()));
    Model::update(&mut state, finish(false, false, false));
    Model::update(
        &mut state,
        Intent::RequestDelete {
            layout: LayoutId(9),
            exists: true,
        },
    );
    assert_eq!(
        state.rename(),
        Some(&Rename {
            tab: 7,
            pane: PaneSide::Time(1),
            layout: LayoutId(3),
            draft: "Changed".into(),
        })
    );
    begin(&mut state, 8, PaneSide::Flow, LayoutId(4));
    let renamed = state.rename().unwrap();
    assert_eq!(
        (renamed.tab, renamed.pane, renamed.layout),
        (8, PaneSide::Flow, LayoutId(4))
    );
    assert_eq!(renamed.draft, "Original");
    assert_eq!(state.pending_delete(), Some(LayoutId(9)));
}

#[test]
fn layout_rename_escape_wins_and_commit_closes_before_host_result() {
    let mut state = State::default();
    begin(&mut state, 7, PaneSide::Flow, LayoutId(3));
    assert_eq!(
        Model::update(&mut state, finish(true, true, true)),
        Effects::Focus {
            tab: 7,
            pane: PaneSide::Flow
        }
    );
    assert!(state.rename().is_none());
    for (enter, lost_focus, text) in [(true, false, "  "), (false, true, "New name")] {
        begin(&mut state, 7, PaneSide::Flow, LayoutId(3));
        Model::update(&mut state, Intent::Edit(text.into()));
        let effect = Model::update(&mut state, finish(false, enter, lost_focus));
        assert!(
            state.rename().is_none(),
            "host validation or failure cannot leave an editor"
        );
        assert_eq!(
            effect,
            Effects::Rename {
                layout: LayoutId(3),
                name: text.into(),
                target: Some((7, PaneSide::Flow)),
            }
        );
        assert_eq!(
            Model::update(&mut state, finish(false, true, true)),
            Effects::None
        );
    }
}

#[test]
fn layout_delete_consumes_once_and_missing_layout_clears() {
    let mut state = State::default();
    for confirm in [false, true] {
        Model::update(
            &mut state,
            Intent::RequestDelete {
                layout: LayoutId(3),
                exists: true,
            },
        );
        let effect = Model::update(&mut state, Intent::AnswerDelete(confirm));
        assert!(
            state.pending_delete().is_none(),
            "consumed before host deletion, even on failure"
        );
        assert_eq!(
            effect,
            if confirm {
                Effects::Delete(LayoutId(3))
            } else {
                Effects::None
            }
        );
        assert_eq!(
            Model::update(&mut state, Intent::AnswerDelete(true)),
            Effects::None
        );
    }
    Model::update(
        &mut state,
        Intent::RequestDelete {
            layout: LayoutId(3),
            exists: true,
        },
    );
    Model::update(
        &mut state,
        Intent::RequestDelete {
            layout: LayoutId(9),
            exists: false,
        },
    );
    assert_eq!(state.pending_delete(), None);
    assert_eq!(
        Model::update(&mut state, Intent::AnswerDelete(true)),
        Effects::None
    );
}

#[test]
fn layout_reload_clears_both_transient_editors() {
    let mut state = State::default();
    begin(&mut state, 7, PaneSide::Time(0), LayoutId(3));
    Model::update(
        &mut state,
        Intent::RequestDelete {
            layout: LayoutId(9),
            exists: true,
        },
    );
    assert_eq!(Model::update(&mut state, Intent::Reload), Effects::None);
    assert!(state.rename().is_none());
    assert!(state.pending_delete().is_none());
    assert_eq!(
        Model::update(&mut state, finish(false, true, true)),
        Effects::None
    );
    assert_eq!(
        Model::update(&mut state, Intent::AnswerDelete(true)),
        Effects::None
    );
}

#[test]
fn external_commit_needs_no_matching_draft_and_cancel_ignores_delete() {
    let mut state = State::default();
    for active_editor in [false, true] {
        if active_editor {
            begin(&mut state, 7, PaneSide::Flow, LayoutId(3));
        }
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Commit {
                    layout: LayoutId(9),
                    name: "External".into()
                }
            ),
            Effects::Rename {
                layout: LayoutId(9),
                name: "External".into(),
                target: None,
            }
        );
        assert!(state.rename().is_none());
    }
    Model::update(&mut state, Intent::Edit("No editor".into()));
    assert!(state.rename().is_none());
    begin(&mut state, 7, PaneSide::Flow, LayoutId(3));
    Model::update(
        &mut state,
        Intent::RequestDelete {
            layout: LayoutId(9),
            exists: true,
        },
    );
    assert_eq!(
        Model::update(&mut state, Intent::CancelRename),
        Effects::None
    );
    assert!(state.rename().is_none());
    assert_eq!(state.pending_delete(), Some(LayoutId(9)));
}
