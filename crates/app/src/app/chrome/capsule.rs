//! The window's side of the toolbar's capsule, layout popover and list door:
//! what they show this frame, the menus they hang, the controls they publish
//! to the observer scene, and what their presses do.

use eframe::egui;
use quantick_control_schema::canvas::{OPEN_WINDOW_MENU_CAPABILITY_ID, PICK_LAYOUT_CAPABILITY_ID};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_workspace::canvas::LayoutMarks;

use crate::app::QuantickApp;
use crate::app::menu_bar::MenuDoor;
use crate::toolbar::capsule::CapsuleRects;
use crate::toolbar::layout_popover::{self, PopoverModel};
use crate::toolbar::{DrawnControl, ToolbarAction};
use quantick_ui_model::chrome_toolbar::Capsule as CapsuleState;

pub(in crate::app) fn snapshot(app: &QuantickApp, ctx: &egui::Context) -> CapsuleState {
    let tab = app.runtime.tabs.active_id();
    let book = &app.runtime.workspace.book;
    let (workspace, unsaved) = book.tab_row(tab);
    let live = &app.active_tab().canvas.state;
    let saved = book
        .book()
        .recipe(&workspace)
        .map(crate::app::canvas_wiring::recipe_state);
    CapsuleState {
        tree: live.tree().clone(),
        unsaved,
        marks: LayoutMarks::of(live, saved.as_ref(), unsaved),
        menu_open: app.chrome.window_menu.tab_workspace.chip_menu_on(tab),
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
    if app.chrome.harness.take_layout_picker_autostart() {
        app.surfaces.canvas_popovers.update(
            quantick_ui_model::canvas_popovers::Intent::SetLayoutOpen(true),
        );
    }
    #[cfg(any(feature = "scenario-harness", test))]
    if app.chrome.harness.take_window_menu_toolbar() {
        crate::app::canvas_wiring::open_window_menu(&mut app.chrome.window_menu);
    }
    let mut workspace_clicked = false;
    let mut rest = Vec::new();
    for action in actions {
        match action {
            ToolbarAction::ToggleWindowMenu => {
                let menu = &mut app.chrome.window_menu;
                menu.toggle_from(viewport, MenuDoor::Toolbar, false);
            }
            ToolbarAction::ToggleWorkspaceMenu => workspace_clicked = true,
            ToolbarAction::SaveWorkspace => {
                crate::app::tab_workspaces::apply_workspace_command(app, WorkspaceCommand::SaveTo);
            }
            action => rest.push(action),
        }
    }
    let model = PopoverModel {
        workspace: &state.workspace,
        marks: state.marks,
    };
    let cells = app
        .surfaces
        .canvas_popovers
        .with_pass(
            quantick_ui_model::Slot::Toolbar,
            |_| {},
            |view, session| view.layout(session, ctx, rects.miniature, &model, &mut rest),
        )
        .unwrap_or_default();
    crate::app::menu_bar::workspace_cell(app, ctx, Some(rects.name), workspace_clicked);
    app.chrome.add_menu.door = rects.add;
    crate::app::canvas_wiring::add_from_menu(app, ctx);
    publish(app, viewport, state, rects, door, &cells);
    rest
}

/// The list door, the capsule's parts and the popover's cells, as drawn.
fn publish(
    app: &mut QuantickApp,
    viewport: egui::ViewportId,
    state: &CapsuleState,
    rects: CapsuleRects,
    door: Option<egui::Rect>,
    cells: &[layout_popover::DrawnCell],
) {
    let control = |id: &str, label: &str, rect, selected, capability| DrawnControl {
        id: format!("toolbar.{id}"),
        label: label.to_owned(),
        rect,
        selected,
        capability,
    };
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
    controls.push(control(
        "capsule.workspace",
        &state.workspace,
        rects.name,
        workspace_open,
        Some(quantick_control_handlers::workspace_book::cell_menu::MENU_CAPABILITY_ID),
    ));
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
    for (preset, rect) in cells {
        let id = format!("layout_popover.{}", preset.id);
        let selected = state.marks.current == Some(*preset);
        controls.push(control(
            &id,
            preset.label,
            *rect,
            selected,
            Some(PICK_LAYOUT_CAPABILITY_ID),
        ));
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
