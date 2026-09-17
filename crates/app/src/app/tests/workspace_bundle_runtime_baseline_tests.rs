//! Existing runtime and picker behavior, characterized before L4 extraction.
use super::*;
use crate::workspace_store::WorkspacePick;

#[test]
fn bundle_runtime_baseline_idle_and_pending_picker_do_no_allocating_work() {
    let (mut app, _evt, _cmd, _book) = test_app();
    for pending in [false, true] {
        let (_sender, receiver) = std::sync::mpsc::channel();
        if pending {
            app.workspace
                .picker_mut()
                .set_pending_for_test(WorkspacePick::Import, receiver);
        }
        let before = crate::work_meter::tally();
        for _ in 0..1000 {
            app.poll_workspace_picker();
            if pending {
                app.workspace.picker_mut().open_import();
                let tab = &app.tabs[app.tabs.active_index()];
                app.workspace
                    .picker_mut()
                    .open_export(&tab.symbol, tab.flow_pane.state.spec());
            }
        }
        let work = crate::work_meter::tally().since(before);
        assert_eq!(work.allocs, 0);
        assert_eq!(work.alloc_bytes, 0);
        assert_eq!(work.reallocs, 0);
        assert_eq!(work.realloc_copy_bytes, 0);
        assert_eq!(app.workspace.picker_open(), pending);
        assert_eq!(app.surfaces.toast.message(), None);
        assert!(app.workspace.session().recent().is_empty());
        assert!(!app.workspace.ui_state_path().exists());
    }
}

#[test]
fn bundle_runtime_baseline_cancel_clears_once_and_stays_silent() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.workspace
        .picker_mut()
        .set_pending_for_test(WorkspacePick::Export, receiver);
    sender.send(None).unwrap();
    app.poll_workspace_picker();
    assert!(!app.workspace.picker_open());
    assert_eq!(app.surfaces.toast.message(), None);
    app.poll_workspace_picker();
    assert_eq!(app.surfaces.toast.message(), None);
    assert!(!app.workspace.ui_state_path().exists());
}

#[test]
fn bundle_runtime_baseline_lost_picker_clears_and_reports_once() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.workspace
        .picker_mut()
        .set_pending_for_test(WorkspacePick::Import, receiver);
    drop(sender);
    app.poll_workspace_picker();
    assert!(!app.workspace.picker_open());
    assert_eq!(
        app.surfaces.toast.message(),
        Some("The file chooser could not open — see the log. Try again.")
    );
    app.surfaces.toast.clear();
    app.poll_workspace_picker();
    assert_eq!(app.surfaces.toast.message(), None);
}

#[test]
fn bundle_runtime_baseline_chosen_export_writes_and_visits() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let output = crate::scratch::ScratchFile::new("bundle-picker-export", "chosen.qws.toml");
    let (sender, receiver) = std::sync::mpsc::channel();
    app.workspace
        .picker_mut()
        .set_pending_for_test(WorkspacePick::Export, receiver);
    sender.send(Some(output.to_path_buf())).unwrap();
    app.poll_workspace_picker();
    assert!(!app.workspace.picker_open());
    assert!(output.is_file());
    assert_eq!(
        app.workspace.session().recent(),
        [output.to_string_lossy().into_owned()]
    );
    assert_eq!(app.workspace.recent_on_disk(), [output.to_path_buf()]);
    assert!(
        app.surfaces
            .toast
            .message()
            .unwrap()
            .starts_with("Workspace exported to ")
    );
}

#[test]
fn bundle_runtime_baseline_chosen_refusal_preserves_runtime_and_recent() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let input = crate::scratch::ScratchFile::new("bundle-picker-refused", "future.qws.toml");
    std::fs::write(&input, "version = 99\n").unwrap();
    app.toolrail.set_favorites(&["measure".to_owned()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    app.workspace
        .picker_mut()
        .set_pending_for_test(WorkspacePick::Import, receiver);
    sender.send(Some(input.to_path_buf())).unwrap();
    app.poll_workspace_picker();
    assert!(!app.workspace.picker_open());
    assert!(app.workspace.session().recent().is_empty());
    assert_eq!(app.workspace_state().starred_tool_ids(), ["measure"]);
    assert!(
        app.surfaces
            .toast
            .message()
            .unwrap()
            .starts_with("Workspace not opened — ")
    );
}

#[test]
fn bundle_runtime_baseline_failed_export_does_not_visit() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let output = crate::scratch::ScratchDir::new("bundle-export-directory");
    app.workspace_bundle_adapter().export_workspace_to(&output);
    assert!(app.workspace.session().recent().is_empty());
    assert!(app.workspace.recent_on_disk().is_empty());
    assert!(output.is_dir());
    assert!(
        app.surfaces
            .toast
            .message()
            .unwrap()
            .starts_with("Workspace not exported — ")
    );
}

#[test]
fn bundle_runtime_baseline_import_reloads_store_absent_from_bundle() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.added_symbols.add("binance", "KEPTUSDT");
    symbols_file::save(app.workspace.symbols_path(), &app.added_symbols).unwrap();
    app.added_symbols.remove("binance", "KEPTUSDT");
    let input = crate::scratch::ScratchFile::new("bundle-absent-store", "empty.qws.toml");
    std::fs::write(&input, "version = 1\nname = 'empty'\n[sections]\n").unwrap();
    app.workspace_bundle_adapter().import_workspace_from(&input);
    assert!(app.added_symbols.contains("binance", "KEPTUSDT"));
    assert_eq!(
        app.workspace.session().recent(),
        [input.to_string_lossy().into_owned()]
    );
    assert!(
        app.surfaces
            .toast
            .message()
            .unwrap()
            .contains("0 settings groups restored")
    );
}
