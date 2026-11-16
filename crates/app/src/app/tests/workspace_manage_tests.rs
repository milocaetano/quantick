//! Manage workspaces and the create dialogs at the window, one path each:
//! the call opens each dialog, a chip click opens its palette, Enter in the
//! name field reaches the rename, Delete then Undo reach their handlers, and
//! the create dialog's Enter saves in the colour it holds. What each decides
//! is pinned in `quantick_control_handlers::workspace_book::manage`.
use super::*;

const FULL_HD: egui::Vec2 = egui::vec2(1920.0, 1000.0);

/// Tab 1 on a new `Scalp` (Cobalt) beside `Default` (Slate), autosave off.
fn scalp(name: &str) -> (QuantickApp, egui::Context, u64, crate::scratch::ScratchFile) {
    let layouts = crate::scratch::ScratchFile::new(name, "layouts.toml");
    let (mut app, first, _) = super::tab_workspace_tests::two_tabs(&layouts);
    app.runtime.tabs.select(0);
    let book = app.runtime.workspace.book.book_mut();
    book.save_as(first, "Scalp").unwrap();
    (app, egui::Context::default(), first, layouts)
}

/// Frames until the dialog's drawn row `id` shows; its rect.
fn drawn(app: &mut QuantickApp, ctx: &egui::Context, id: &str) -> egui::Rect {
    for _ in 0..6 {
        run_frame_at(app, ctx, FULL_HD);
        let rows = &app.chrome.window_menu.dialog_rows;
        if let Some((_, rect)) = rows.iter().find(|(row, _)| row.scene_id() == id) {
            return *rect;
        }
    }
    panic!("{id} was never drawn");
}

fn call(app: &mut QuantickApp, id: &str, input: serde_json::Value) -> serde_json::Value {
    let human = crate::control::ActionOrigin::Human;
    app.control_action(id, 1, human, input).expect(id)
}

/// `workspace.dialog.open` puts each dialog on screen, and the scene lists
/// its parts.
#[test]
fn the_dialog_call_shows_each_dialog() {
    let (mut app, ctx, _, _layouts) = scalp("dialog-open");
    let saved = call(
        &mut app,
        "workspace.dialog.open",
        serde_json::json!({"mode": "save_as"}),
    );
    assert_eq!(saved["name"], "Scalp 2");
    drawn(&mut app, &ctx, "name_dialog.colour.slate");
    app.chrome.window_menu.tab_workspace.close_dialog();
    call(
        &mut app,
        "workspace.dialog.open",
        serde_json::json!({"mode": "new"}),
    );
    let title = drawn(&mut app, &ctx, "name_dialog");
    assert!(title.width() > 300.0);
    app.chrome.window_menu.tab_workspace.close_dialog();
    call(
        &mut app,
        "workspace.dialog.open",
        serde_json::json!({"mode": "manage"}),
    );
    drawn(&mut app, &ctx, "manage_workspaces.Scalp.chip");
    let scene = crate::control::scene_snapshot(&app);
    let id = "toolbar.manage_workspaces.Default.rename";
    let rename = scene
        .controls
        .iter()
        .find(|control| control.control_id == id);
    assert!(
        rename.is_some_and(|control| !control.availability.available),
        "{id}"
    );
}

/// A chip click opens that row's palette under it.
#[test]
fn a_chip_click_opens_the_row_s_palette() {
    let (mut app, ctx, first, _layouts) = scalp("manage-chip");
    app.chrome.window_menu.tab_workspace.open_manage(first);
    let chip = drawn(&mut app, &ctx, "manage_workspaces.Scalp.chip");
    click_sized(&mut app, &ctx, FULL_HD, chip.center());
    let dialog = app
        .chrome
        .window_menu
        .tab_workspace
        .manage
        .as_ref()
        .unwrap();
    assert_eq!(dialog.palette_of(), Some("Scalp"));
    drawn(&mut app, &ctx, "manage_workspaces.Scalp.colour.petrol");
}

/// A click outside a row's palette closes that palette, and only it.
#[test]
fn a_click_outside_a_row_s_palette_closes_only_it() {
    let (mut app, ctx, first, _layouts) = scalp("manage-palette-out");
    assert!(
        app.chrome
            .window_menu
            .tab_workspace
            .stage(first, "manage:colour=Scalp")
    );
    drawn(&mut app, &ctx, "manage_workspaces.Scalp.colour.petrol");
    let title = drawn(&mut app, &ctx, "manage_workspaces");
    click_sized(&mut app, &ctx, FULL_HD, title.center());
    let dialog = app.chrome.window_menu.tab_workspace.manage.as_ref();
    let dialog = dialog.expect("Manage stays open");
    assert_eq!(dialog.palette_of(), None, "the palette closed");
}

/// After a swatch click the field has lost the keyboard; Enter still saves.
#[test]
fn enter_after_a_swatch_click_saves() {
    let (mut app, ctx, first, _layouts) = scalp("create-click-enter");
    assert!(app.chrome.window_menu.tab_workspace.stage(first, "save_as"));
    let lavender = drawn(&mut app, &ctx, "name_dialog.colour.lavender");
    click_sized(&mut app, &ctx, FULL_HD, lavender.center());
    run_sized_frame(&mut app, &ctx, FULL_HD, vec![key_press(egui::Key::Enter)]);
    run_frame_at(&mut app, &ctx, FULL_HD);
    let lavender = quantick_stores::workspaces::WorkspaceColour::preset(7);
    assert_eq!(
        app.runtime.workspace.book.book().colour("Scalp 2"),
        lavender
    );
}

/// Enter in the inline field reaches `workspace.rename`.
#[test]
fn enter_in_the_name_field_reaches_the_rename() {
    let (mut app, ctx, first, _layouts) = scalp("manage-rename");
    assert!(
        app.chrome
            .window_menu
            .tab_workspace
            .stage(first, "manage:rename=Scalp")
    );
    drawn(&mut app, &ctx, "manage_workspaces.Scalp.name");
    let dialog = app
        .chrome
        .window_menu
        .tab_workspace
        .manage
        .as_mut()
        .unwrap();
    dialog.rename.as_mut().unwrap().name = "Scalper".to_owned();
    run_sized_frame(&mut app, &ctx, FULL_HD, vec![key_press(egui::Key::Enter)]);
    run_frame_at(&mut app, &ctx, FULL_HD);
    assert!(
        app.runtime
            .workspace
            .book
            .book()
            .recipe("Scalper")
            .is_some()
    );
}

/// Delete in the armed row, then Undo, reach their handlers: the workspace
/// goes, then comes back with its colour.
#[test]
fn delete_then_undo_reach_their_handlers() {
    let (mut app, ctx, first, _layouts) = scalp("manage-delete");
    let colour = app.runtime.workspace.book.book().colour("Scalp");
    assert!(
        app.chrome
            .window_menu
            .tab_workspace
            .stage(first, "manage:delete=Scalp")
    );
    let confirm = drawn(&mut app, &ctx, "manage_workspaces.Scalp.confirm");
    click_sized(&mut app, &ctx, FULL_HD, confirm.center());
    assert!(app.runtime.workspace.book.book().recipe("Scalp").is_none());
    let undo = drawn(&mut app, &ctx, "manage_workspaces.Scalp.undo");
    click_sized(&mut app, &ctx, FULL_HD, undo.center());
    let book = app.runtime.workspace.book.book();
    assert!(book.recipe("Scalp").is_some());
    assert_eq!(book.colour("Scalp"), colour);
}

/// The create dialog's Enter saves under its name in the colour it holds.
#[test]
fn the_create_dialog_s_enter_saves_in_its_colour() {
    let (mut app, ctx, first, _layouts) = scalp("create-enter");
    assert!(app.chrome.window_menu.tab_workspace.stage(first, "save_as"));
    let lavender = drawn(&mut app, &ctx, "name_dialog.colour.lavender");
    click_sized(&mut app, &ctx, FULL_HD, lavender.center());
    let dialog = app
        .chrome
        .window_menu
        .tab_workspace
        .dialog
        .as_ref()
        .unwrap();
    assert_eq!(dialog.colour.as_deref(), Some("lavender"));
    let save = drawn(&mut app, &ctx, "name_dialog.save");
    click_sized(&mut app, &ctx, FULL_HD, save.center());
    let book = app.runtime.workspace.book.book();
    let lavender = quantick_stores::workspaces::WorkspaceColour::preset(7);
    assert_eq!(book.colour("Scalp 2"), lavender);
}
