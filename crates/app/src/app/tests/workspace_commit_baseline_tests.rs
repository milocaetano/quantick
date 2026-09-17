//! Literal persistence behavior captured before the L3 owner extraction.
use super::*;

const FUTURE: &str = "version = 99\nsaved = []\nkeep_me = true\n";

#[test]
fn idle_commit_adapter_borrows_and_decides_without_allocation() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let before = crate::work_meter::tally();
        for _ in 0..1000 {
            app.workspace_save_adapter().maintain_workspace(ctx);
        }
        let work = crate::work_meter::tally().since(before);
        assert_eq!(work.allocs, 0);
        assert_eq!(work.alloc_bytes, 0);
        assert_eq!(work.reallocs, 0);
        assert_eq!(work.realloc_copy_bytes, 0);
    });
    assert!(!app.workspace.ui_state_path().exists());
}

fn maintain(app: &mut QuantickApp, closing: bool) {
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    if closing {
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
    }
    let _ = ctx.run(input, |ctx| {
        app.workspace_save_adapter().maintain_workspace(ctx)
    });
}

#[test]
fn commit_baseline_full_and_toggle_save_overwrite_unknown_documents() {
    for reason in ["explicit", "save_on_exit_toggled"] {
        let (mut app, _evt, _cmd, _book) = test_app();
        app.workspace.session_mut().set_save_on_exit(false);
        std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
        app.workspace_save_adapter().save_workspace(reason);
        let file = ui_state::load(app.workspace.ui_state_path());
        assert_eq!(file.tabs.len(), 1);
        assert_eq!(file.tabs[0].symbol, "TESTUSDT");
        assert!(!file.save_on_exit);
        assert!(app.workspace.session().saved());
    }
}

#[test]
fn commit_baseline_close_overwrites_unknown_and_consumes_inspector_dirty_each_time() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.workspace.session_mut().set_save_on_exit(true);
    for position in [[120.0, 240.0], [220.0, 340.0]] {
        std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
        app.surfaces
            .drawing_chrome
            .restore_inspector_position(Some(position));
        app.workspace.session_mut().inspector_moved();
        maintain(&mut app, true);
        assert!(!app.workspace.session().inspector_pending());
        let file = ui_state::load(app.workspace.ui_state_path());
        assert_eq!(file.tabs.len(), 1);
        assert_eq!(file.chrome.unwrap().inspector_position, Some(position));
    }
}

#[test]
fn commit_baseline_autosave_off_consumes_inspector_without_later_retry() {
    let (mut app, _evt, _cmd, _book) = test_app();
    std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
    app.workspace.session_mut().set_save_on_exit(false);
    app.workspace.session_mut().inspector_moved();
    maintain(&mut app, false);
    assert!(!app.workspace.session().inspector_pending());
    app.workspace.session_mut().set_save_on_exit(true);
    maintain(&mut app, false);
    assert_eq!(
        std::fs::read_to_string(app.workspace.ui_state_path()).unwrap(),
        FUTURE
    );
    assert!(!app.workspace.session().saved());
}

#[test]
fn commit_baseline_inspector_refusal_is_silent_and_does_not_rearm() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.workspace.session_mut().set_save_on_exit(true);
    std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
    app.surfaces
        .drawing_chrome
        .restore_inspector_position(Some([120.0, 240.0]));
    app.workspace.session_mut().inspector_moved();
    let before = app.surfaces.toast.message().map(str::to_owned);
    maintain(&mut app, false);
    assert!(!app.workspace.session().inspector_pending());
    assert_eq!(app.surfaces.toast.message(), before.as_deref());
    assert_eq!(
        std::fs::read_to_string(app.workspace.ui_state_path()).unwrap(),
        FUTURE
    );
    // The failed attempt was consumed: admitting a file later is not a retry.
    let mut admitted = app.workspace_state().capture_workspace();
    admitted.chrome.as_mut().unwrap().inspector_position = None;
    assert!(ui_state::save(app.workspace.ui_state_path(), &admitted));
    maintain(&mut app, false);
    assert_eq!(
        ui_state::load(app.workspace.ui_state_path())
            .chrome
            .unwrap()
            .inspector_position,
        None
    );
}

#[test]
fn commit_baseline_named_memory_changes_survive_standing_edit_refusal() {
    let (mut app, _evt, _cmd, _book) = test_app();
    std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
    app.workspace_save_adapter().save_named_workspace("opening");
    assert_eq!(app.workspace.session().bookmarks().len(), 1);
    assert!(!app.workspace.session().saved());
    assert_eq!(
        std::fs::read_to_string(app.workspace.ui_state_path()).unwrap(),
        FUTURE
    );
    app.workspace_save_adapter()
        .delete_named_workspace("opening");
    assert!(app.workspace.session().bookmarks().is_empty());
    assert_eq!(
        std::fs::read_to_string(app.workspace.ui_state_path()).unwrap(),
        FUTURE
    );
    app.workspace_save_adapter().save_named_workspace("return");
    app.workspace_save_adapter().save_workspace("explicit");
    assert_eq!(
        ui_state::load(app.workspace.ui_state_path()).saved[0].name,
        "return"
    );
}

#[test]
fn commit_baseline_empty_reset_removes_unknown_but_kept_reset_refuses_it() {
    for keep_star in [false, true] {
        let (mut app, _evt, _cmd, _book) = test_app();
        let favorites = if keep_star {
            vec!["measure".to_owned()]
        } else {
            vec![]
        };
        app.toolrail.set_favorites(&favorites);
        assert!(app.workspace.session().bookmarks().is_empty());
        assert!(app.workspace.session().recent().is_empty());
        assert!(app.replay_view.stored_pick().is_none());
        std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
        app.workspace_save_adapter().forget_workspace();
        assert_eq!(app.workspace.ui_state_path().exists(), keep_star);
        assert_eq!(app.workspace.session().saved(), keep_star);
        if keep_star {
            assert_eq!(
                std::fs::read_to_string(app.workspace.ui_state_path()).unwrap(),
                FUTURE
            );
        }
    }
}

#[test]
fn commit_baseline_day_before_alone_does_not_keep_a_reset_file() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.toolrail.set_favorites(&[]);
    app.workspace_save_adapter().write_replay_day_before(true);
    assert!(app.workspace.session().saved());
    assert_eq!(
        ui_state::load(app.workspace.ui_state_path()).replay_day_before,
        Some(true)
    );
    app.workspace_save_adapter().forget_workspace();
    assert!(!app.workspace.ui_state_path().exists());
    assert!(!app.workspace.session().saved());
}

#[test]
fn commit_baseline_saved_flag_depends_on_write_purpose_and_failure_is_or() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.toolrail.set_favorites(&["measure".to_owned()]);
    app.workspace_save_adapter().write_favorites();
    assert!(app.workspace.ui_state_path().is_file());
    assert!(!app.workspace.session().saved());
    app.workspace_save_adapter()
        .write_replay_folder(Some("D:/baseline-tape"));
    assert!(app.workspace.session().saved());
    std::fs::write(app.workspace.ui_state_path(), FUTURE).unwrap();
    app.workspace_save_adapter().write_replay_day_before(false);
    assert!(app.workspace.session().saved());
    assert_eq!(
        std::fs::read_to_string(app.workspace.ui_state_path()).unwrap(),
        FUTURE
    );
}
