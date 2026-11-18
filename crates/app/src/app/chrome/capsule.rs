//! The window's side of the toolbar's capsule, layout popover and list door:
//! what they show this frame, the menus they hang, the controls they publish
//! to the observer scene, and what their presses do.

use eframe::egui;
use quantick_control_schema::canvas::{
    LAYOUT_PICKER_CAPABILITY_ID, OPEN_WINDOW_MENU_CAPABILITY_ID, PICK_LAYOUT_CAPABILITY_ID,
};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_ui_model::canvas_popovers::picked;
use quantick_workspace::canvas::LayoutMarks;

use crate::app::menu_bar::MenuDoor;
use crate::app::{GatewayPort as _, QuantickApp};
use crate::toolbar::capsule::CapsuleRects;
use crate::toolbar::layout_popover;
use crate::toolbar::{DrawnControl, ToolbarAction};
use quantick_ui_model::chrome_toolbar::Capsule as CapsuleState;

/// The active tab's workspace, whether it holds edits, and its layout marks.
pub(in crate::app) fn layout_of(app: &QuantickApp) -> (String, bool, LayoutMarks) {
    app.runtime
        .workspace
        .book
        .canvas_layout(app.runtime.tabs.active_id(), &app.active_tab().canvas.state)
}

pub(in crate::app) fn snapshot(app: &QuantickApp, ctx: &egui::Context) -> CapsuleState {
    let tab = app.runtime.tabs.active_id();
    let book = &app.runtime.workspace.book;
    let (workspace, unsaved, marks) = layout_of(app);
    CapsuleState {
        tree: app.active_tab().canvas.state.tree().clone(),
        unsaved,
        marks,
        menu_open: app.chrome.window_menu.tab_workspace.chip_menu_on(tab),
        colour: book.book().tab_colour(tab),
        workspace,
        door_open: (app.chrome.window_menu).opened_from(ctx.viewport_id(), MenuDoor::Toolbar),
        door_hover: format!(
            "Window menu ({})",
            crate::app::menu_bar::shortcut_text(ctx, &crate::app::menu_bar::WINDOW_MENU_SHORTCUT)
        ),
    }
}

/// After the toolbar drew: hang the popover and the workspace menu, publish
/// the controls, and carry out the presses only the window can.
pub(in crate::app) fn settle(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    state: &CapsuleState,
    rects: CapsuleRects,
    door: Option<egui::Rect>,
    actions: Vec<ToolbarAction>,
) -> Vec<ToolbarAction> {
    let viewport = ctx.viewport_id();
    app.chrome.capsule = rects;
    app.chrome.window_menu.toolbar_door = door;
    #[cfg(any(feature = "scenario-harness", test))]
    if app.chrome.harness.take_window_menu_toolbar() {
        app.chrome.window_menu.open_from_toolbar();
    }
    let mut workspace_clicked = false;
    let mut rest = Vec::new();
    #[cfg(any(feature = "scenario-harness", test))]
    if let Some(hook) = app.chrome.harness.take_layout_picker() {
        let now_ms = layout_popover::now_ms(ctx);
        let stage = quantick_ui_model::canvas_popovers::Intent::StageLayout;
        let staged = stage(hook, state.marks.current, now_ms);
        rest.extend(picked(
            app.surfaces.canvas_popovers.update(staged).model_effects,
        ));
    }
    for action in actions {
        match action {
            ToolbarAction::ToggleWindowMenu => {
                let menu = &mut app.chrome.window_menu;
                menu.toggle_from(viewport, MenuDoor::Toolbar, false);
            }
            ToolbarAction::ToggleWorkspaceMenu => workspace_clicked = true,
            ToolbarAction::SaveWorkspace => {
                (app.workspace_calls()).apply_workspace_command(WorkspaceCommand::SaveTo);
            }
            action => rest.push(action),
        }
    }
    let shown = (state.workspace.as_str(), state.colour, state.marks);
    let hang = (rects.miniature, rects.miniature.union(rects.name));
    let tab = app.runtime.tabs.active_id();
    let drawn = (app.surfaces.canvas_popovers)
        .with_pass(
            quantick_ui_model::Slot::Toolbar,
            |response| rest.extend(picked(response.model_effects)),
            |view, session| view.layout(session, (ctx, hang, tab), shown),
        )
        .unwrap_or_default();
    crate::app::menu_bar::workspace_cell(app, ctx, Some(rects.name), workspace_clicked);
    app.chrome.add_menu.door = rects.add;
    crate::app::canvas_wiring::add_from_menu(app, ctx);
    publish(app, ctx, state, rects, door, &drawn);
    rest
}

/// The list door, the capsule's parts and the popover's cells, as drawn.
fn publish(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    state: &CapsuleState,
    rects: CapsuleRects,
    door: Option<egui::Rect>,
    drawn: &layout_popover::Drawn,
) {
    let control = |id: &str, label: &str, rect, selected, capability| DrawnControl {
        id: format!("toolbar.{id}"),
        label: label.to_owned(),
        rect,
        selected,
        capability,
        description: None,
    };
    let (viewport, dark) = (ctx.viewport_id(), crate::theme::get(ctx).resolved.dark);
    let popovers = app.surfaces.canvas_popovers.model_state.projection();
    let tab = app.runtime.tabs.active_id();
    let menu = &app.chrome.window_menu;
    let mut controls = Vec::new();
    if let Some(door) = door {
        let open = menu.opened_from(viewport, MenuDoor::Toolbar);
        let capability = Some(OPEN_WINDOW_MENU_CAPABILITY_ID);
        controls.push(control(
            "window_menu",
            "Window menu",
            door,
            open,
            capability,
        ));
    }
    let layout = state
        .marks
        .current
        .map_or("Custom layout", |preset| preset.label);
    controls.push(control(
        "capsule.layout",
        layout,
        rects.miniature,
        popovers.layout_open,
        None,
    ));
    let workspace_open = menu.tab_workspace.chip_menu_on(tab);
    let mut workspace = control(
        "capsule.workspace",
        &state.workspace,
        rects.name,
        workspace_open,
        Some(quantick_control_handlers::workspace_book::cell_menu::MENU_CAPABILITY_ID),
    );
    workspace.description = state.colour.map(|colour| colour.describe(dark));
    controls.push(workspace);
    if let Some(floppy) = rects.floppy {
        let save = Some(quantick_control_schema::workspace_book::WORKSPACE_SAVE_TO_CAPABILITY_ID);
        controls.push(control(
            "capsule.save",
            "Save workspace",
            floppy,
            false,
            save,
        ));
    }
    if let Some((rect, label)) = &drawn.header {
        let read = Some(LAYOUT_PICKER_CAPABILITY_ID);
        controls.push(control("layout_popover.header", label, *rect, false, read));
    }
    let pick = Some(PICK_LAYOUT_CAPABILITY_ID);
    if let Some((rect, words)) = &drawn.undo {
        controls.push(control("layout_popover.undo", words, *rect, false, pick));
    }
    for (preset, rect) in &drawn.cells {
        let id = format!("layout_popover.{}", preset.id);
        let mut cell = control(
            &id,
            preset.label,
            *rect,
            state.marks.current == Some(*preset),
            pick,
        );
        cell.description = Some(preset.hint.to_owned());
        controls.push(cell);
    }
    let add = &app.chrome.add_menu;
    if let Some(door) = add.door {
        let capability = Some(quantick_control_schema::canvas::ADD_WIDGET_CAPABILITY_ID);
        controls.push(control(
            "add_widget",
            "Add to canvas",
            door,
            popovers.add_open,
            capability,
        ));
    }
    controls.extend(add.drawn.iter().cloned());
    app.chrome.toolbar_controls = controls;
}
