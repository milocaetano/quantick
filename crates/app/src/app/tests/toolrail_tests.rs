use super::*;

/// A star clicked is a star kept, on the frame it was clicked. Waiting for
/// a clean exit means one crash — or one session that ends any other way —
/// costs the trader the rail they curated.
#[test]
fn starring_a_tool_reaches_the_disk_on_the_spot() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-written"));
    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    assert_eq!(
        ui_state::load(app.runtime.workspace.ui_state_path()).favorite_tools,
        vec!["measure".to_owned()],
        "the star is on disk with nobody having saved the workspace"
    );
    let _ = std::fs::remove_file(app.runtime.workspace.ui_state_path());
}

/// Unstarring is a choice too — it must reach the disk exactly as starring
/// does, or a tool the trader took off the rail would be back tomorrow.
#[test]
fn unstarring_a_tool_reaches_the_disk_too() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-removed"));
    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);
    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    assert!(
        ui_state::load(app.runtime.workspace.ui_state_path())
            .favorite_tools
            .is_empty(),
        "the rail the trader emptied stays empty"
    );
    let _ = std::fs::remove_file(app.runtime.workspace.ui_state_path());
}

/// Autosave governs the arrangement, never the rail: a trader who switched
/// it off to stop their layout drifting has not asked to rebuild their
/// tools every session. And the write that keeps the stars must carry
/// nothing else — no tabs, and not autosave switched back on.
#[test]
fn starring_a_tool_is_written_with_autosave_off_and_drags_nothing_along() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-no-autosave"));
    app.runtime.workspace.session_mut().set_save_on_exit(false);
    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    let file = ui_state::load(app.runtime.workspace.ui_state_path());
    assert_eq!(
        file.favorite_tools,
        vec!["measure".to_owned()],
        "the star survives autosave being off"
    );
    assert!(
        file.tabs.is_empty(),
        "one standing choice was written, not the cockpit around it"
    );
    assert!(
        !file.save_on_exit,
        "and the trader's autosave switch stayed off"
    );
    let _ = std::fs::remove_file(app.runtime.workspace.ui_state_path());
}

/// Restoring a saved list is not the trader making a choice, so it must
/// not write one back — otherwise every launch rewrites the file for
/// nothing, and a startup that read a stale list would cement it.
#[test]
fn restoring_the_saved_stars_writes_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-restore"));
    app.surfaces.toolrail.set_favorites(&["measure".to_owned()]);
    run_frame(&mut app, &ctx);

    assert!(
        !app.runtime.workspace.ui_state_path().exists(),
        "a restore is not a save: nothing was written"
    );
}

/// Reset throws away the *startup arrangement*. The stars were never part
/// of it, so they survive — like the bookmarks beside them, and the file
/// stays on disk to hold them.
#[test]
fn resetting_the_startup_layout_keeps_the_starred_tools() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("reset-stars"));
    app.workspace_save_adapter().save_workspace("test");
    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    app.workspace_save_adapter().forget_workspace();

    let file = ui_state::load(app.runtime.workspace.ui_state_path());
    assert_eq!(
        file.favorite_tools,
        vec!["measure".to_owned()],
        "resetting a layout is not asking to rebuild the rail"
    );
    assert!(file.tabs.is_empty(), "and the arrangement really was reset");
    let _ = std::fs::remove_file(app.runtime.workspace.ui_state_path());
}

/// The rail a validation run wears is a costume, not a choice.
///
/// `QUANTICK_TOOL_FAVORITES` stages a pinned rail so a screenshot can
/// reach a state that would otherwise take clicks. Since a star now
/// reaches the disk the moment it is clicked, a run that toggles one would
/// write the harness's list into the trader's own workspace — the failure
/// `replay_view.stored_pick()` already guards for `QUANTICK_REPLAY_DIR`.
#[test]
fn a_staged_rail_is_never_written_down() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-staged"));
    app.runtime.workspace.session_mut().stage_favorites();

    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    assert!(
        !app.runtime.workspace.ui_state_path().exists(),
        "a staged rail leaves no trace in the trader's workspace"
    );
}

/// Starring a tool is not saving a layout.
///
/// `workspace_saved` answers "is there a startup arrangement to reset?".
/// On a fresh install the Reset entry is disabled and says "Nothing saved
/// yet"; a star must not light it up, because clicking it would then
/// promise to forget an arrangement nobody ever saved.
#[test]
fn starring_a_tool_does_not_pretend_a_layout_was_saved() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-not-a-layout"));
    app.runtime.workspace.session_mut().set_saved(false);

    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    assert!(
        !app.runtime.workspace.session().saved(),
        "a star is a standing choice, not a saved arrangement"
    );
    let _ = std::fs::remove_file(app.runtime.workspace.ui_state_path());
}

/// An empty list in a file is silence, not an order to empty the rail.
///
/// The format cannot tell "the trader starred nothing" from "this file
/// predates the field" or "this bundle came from an install that never
/// saved a cockpit" — and this same function restores an *imported*
/// workspace mid-session, where acting on that silence would throw away a
/// curated rail on the strength of a key nobody wrote.
#[test]
fn a_restored_workspace_with_no_stars_leaves_the_rail_alone() {
    let (mut app, _evt, _cmd, _book) = test_app();
    app.surfaces.toolrail.set_favorites(&["measure".to_owned()]);

    let config = app.config.clone();
    app.arrangement_adapter().restore_workspace(
        ui_state::Workspace::new(true, None, 0, Vec::new(), None).restore(&config),
    );

    assert_eq!(
        app.workspace_state().starred_tool_ids(),
        vec!["measure".to_owned()],
        "silence about the stars is not an instruction to drop them"
    );
}

/// The end-to-end promise, at the seam that used to break it: stars saved
/// by one session are on the rail of the next one, whatever the
/// arrangement in between says.
#[test]
fn the_next_session_opens_on_the_stars_the_last_one_left() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state("star-next-session"));
    app.surfaces.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);

    // A second session reading the same file, restoring as startup does.
    let (mut next, _commands) = app_with_history(50);
    next.runtime
        .workspace
        .set_ui_state_path(app.runtime.workspace.ui_state_path().to_path_buf());
    let saved =
        ui_state::load(next.runtime.workspace.ui_state_path()).restore(&next.config.clone());
    next.arrangement_adapter().restore_workspace(saved);

    assert_eq!(
        next.workspace_state().starred_tool_ids(),
        vec!["measure".to_owned()],
        "the rail opens on what the trader starred"
    );
    let _ = std::fs::remove_file(app.runtime.workspace.ui_state_path());
}

#[test]
fn a_rail_folded_away_puts_no_tools_on_a_screen_that_does_not_show_them() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    let visible = scene_control_ids(&observer_scene(&app));
    assert!(
        visible.iter().any(|id| id == "tool_rail.tool.pointer"),
        "the rail opens with the pointer armed"
    );

    app.surfaces.toolrail.toggle_visible();
    run_frame(&mut app, &ctx);
    let folded = scene_control_ids(&observer_scene(&app));
    assert!(
        !folded.iter().any(|id| id.starts_with("tool_rail.")),
        "a rail nobody can see contributes no controls"
    );
    // The scene is what is on screen, so folding the rail away removes its
    // tools rather than listing them as unavailable — but everything that
    // is still painted keeps the name it had.
    for id in folded {
        assert!(visible.contains(&id), "{id} appeared out of nowhere");
    }
}

#[test]
fn toolrail_multi_event_frame_preserves_effect_order() {
    use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
    use quantick_ui_model::tool_rail::{Effect, Intent};
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    let mut response = crate::surfaces::SurfaceResponse::default();
    app.surfaces.toolrail.with_updates(
        |effect| response.model_effects.extend(effect.model_effects),
        |_, session| {
            session.update(Intent::Arm(Tool::Crosshair));
            session.update(Intent::SetAllHidden(true));
            session.update(Intent::SetManagerOpen(true));
            session.update(Intent::SetAllHidden(false));
            session.update(Intent::SetManagerOpen(false));
            session.update(Intent::SetAllLocked(true));
            session.update(Intent::RequestMagnet);
            session.update(Intent::Arm(Tool::Pointer));
        },
    );
    assert_eq!(
        response.model_effects,
        vec![
            crate::surfaces::HostEffect::ToolRail(vec![Effect::SetAllHidden(true)]),
            crate::surfaces::HostEffect::ToolRail(vec![Effect::SetManagerOpen(true)]),
            crate::surfaces::HostEffect::ToolRail(vec![Effect::SetAllHidden(false)]),
            crate::surfaces::HostEffect::ToolRail(vec![Effect::SetManagerOpen(false)]),
            crate::surfaces::HostEffect::ToolRail(vec![Effect::SetAllLocked(true)]),
        ]
    );
    assert!(
        !app.surfaces.toolrail.magnet(),
        "a queued magnet request is unconfirmed"
    );
    let requested = app
        .surfaces
        .toolrail
        .take_magnet_request()
        .expect("one magnet request");
    crate::control::flip_rail_magnet(&mut app, requested);
    assert_eq!(
        app.surfaces.toolrail.take_magnet_request(),
        None,
        "the request settles once"
    );
    let tab = app
        .runtime
        .tabs
        .runtime_mut(app.runtime.tabs.active_index());
    let drawings = &mut tab.flow_pane.drawings;
    super::super::frame::settle_rail_effects(response, drawings, &mut app.surfaces.drawing_chrome);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |_| {
            assert!(
                !drawings.all_hidden(),
                "queued hide/show settles before canvas"
            );
            assert!(
                !drawings.all_locked(),
                "empty-store lock policy stays in Drawings"
            );
            assert!(!app.surfaces.drawing_chrome.manager_open());
            assert_eq!(app.surfaces.toolrail.tool(), Tool::Pointer);
            assert!(
                app.surfaces.toolrail.magnet(),
                "confirmed magnet is visible to the same canvas"
            );
        });
    });
    assert!(drawings.undo(), "show created an undo entry");
    assert!(drawings.all_hidden(), "undo proves hide preceded show");
    assert!(drawings.undo(), "hide created one earlier entry");
    assert!(!drawings.all_hidden());
    assert!(!drawings.undo(), "each queued effect settled only once");
}
