//! A headless host executes the same persistence decisions as the desktop shell.
use quantick_workspace::layout_commit::{LayoutCommitPolicy, LayoutSave};
use quantick_workspace::workspace_commit::{
    DocumentEdit, FrameFacts, FrameSave, WorkspaceCommitSession, WritePurpose,
};
use quantick_workspace::workspace_document::Workspace;
use std::time::{Duration, Instant};

#[test]
fn a_failed_write_is_consumed_and_a_blocked_change_never_reaches_the_sink() {
    let now = Instant::now();
    let mut policy = LayoutCommitPolicy::new(false);
    let mut writes = 0;
    policy.mark_changed(now);
    assert_eq!(
        policy.take_save(now + Duration::from_millis(999)),
        LayoutSave::Wait
    );
    if policy.take_save(now + Duration::from_millis(1000)) == LayoutSave::Write {
        writes += 1;
    }
    // The sink failed; no implicit completion or retry input exists.
    assert_eq!(policy.take_flush(), LayoutSave::Wait);
    policy.set_blocked(true);
    policy.mark_changed(now);
    assert_eq!(policy.take_flush(), LayoutSave::Blocked);
    assert_eq!(policy.take_flush(), LayoutSave::Wait);
    assert_eq!(writes, 1);
}

#[test]
fn a_second_host_preserves_standing_edits_and_operation_specific_admission() {
    let mut session = WorkspaceCommitSession::default();
    session.set_save_on_exit(false);
    assert!(
        session
            .prepare_edit(None, DocumentEdit::ReplayDayBefore(true))
            .is_none()
    );
    let mut standing = Workspace::default();
    standing.window = Some([640.0, 480.0]);
    standing.replay_folder = Some("kept".into());
    let edited = session
        .prepare_edit(Some(standing), DocumentEdit::ReplayDayBefore(true))
        .unwrap();
    assert_eq!(edited.window, Some([640.0, 480.0]));
    assert_eq!(edited.replay_folder.as_deref(), Some("kept"));
    assert_eq!(edited.replay_day_before, Some(true));
    assert!(!edited.save_on_exit);
    session.note_write(WritePurpose::Favorites, true);
    assert!(!session.saved());
    session.note_write(WritePurpose::ReplayPreference, true);
    session.note_write(WritePurpose::Full, false);
    assert!(session.saved());
    assert!(!session.reset_keeps(false, false));
    session.finish_reset(false, true);
    assert!(!session.saved());
}

#[test]
fn idle_and_close_commands_have_consumed_dirty_and_repeated_close_semantics() {
    let mut session = WorkspaceCommitSession::default();
    session.set_save_on_exit(false);
    session.inspector_moved();
    assert_eq!(
        session.frame(FrameFacts {
            size: Some([0.0, 20.0]),
            closing: false
        }),
        FrameSave::Wait
    );
    assert_eq!(session.window_size(), None);
    session.set_save_on_exit(true);
    assert_eq!(
        session.frame(FrameFacts {
            size: Some([640.0, 480.0]),
            closing: false
        }),
        FrameSave::Wait
    );
    session.inspector_moved();
    for _ in 0..2 {
        assert_eq!(
            session.frame(FrameFacts {
                size: None,
                closing: true
            }),
            FrameSave::Full
        );
    }
    assert_eq!(
        session.frame(FrameFacts {
            size: None,
            closing: false
        }),
        FrameSave::Wait
    );
    assert_eq!(session.window_size(), Some([640.0, 480.0]));
}
