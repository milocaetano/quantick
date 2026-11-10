//! The window's side of the toolbar's capsule, layout popover and list door:
//! what they show this frame, the menus they hang, the controls they publish
//! to the observer scene, and what their presses do.

use eframe::egui;
use quantick_control_schema::canvas::{OPEN_WINDOW_MENU_CAPABILITY_ID, PICK_LAYOUT_CAPABILITY_ID};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_workspace::canvas::{CanvasState, LayoutPreset, PaneTree};

use super::QuantickApp;
use super::menu_bar::MenuDoor;
use crate::toolbar::capsule::{CapsuleModel, CapsuleRects};
use crate::toolbar::layout_popover::{self, PopoverModel};
use crate::toolbar::{DrawnControl, ToolbarAction};

/// What the capsule shows, read before the toolbar borrows the tab.
pub(super) struct CapsuleState {
    tree: PaneTree,
    workspace: String,
    unsaved: bool,
    layout_unsaved: bool,
    menu_open: bool,
    popover_open: bool,
    current: Option<&'static LayoutPreset>,
    saved: Option<&'static LayoutPreset>,
    door_open: bool,
    door_hover: String,
}

impl CapsuleState {
    pub(super) fn of(app: &QuantickApp, ctx: &egui::Context) -> Self {
        let tab = app.runtime.tabs.active_id();
        let book = &app.runtime.workspace.book;
        let (workspace, unsaved) = book.tab_row(tab);
        let live = &app.active_tab().canvas.state;
        let saved = book
            .book()
            .recipe(&workspace)
            .map(super::canvas_wiring::recipe_state);
        let layout_unsaved = unsaved
            && saved
                .as_ref()
                .is_some_and(|saved| !same_layout(saved, live));
        Self {
            tree: live.tree().clone(),
            unsaved,
            layout_unsaved,
            menu_open: app.chrome.window_menu.tab_workspace.chip_menu_on(tab),
            popover_open: app.chrome.layout_picker_open,
            current: live.preset(),
            saved: saved.as_ref().and_then(CanvasState::preset),
            workspace,
            door_open: (app.chrome.window_menu).opened_from(ctx.viewport_id(), MenuDoor::Toolbar),
            door_hover: format!(
                "Window menu ({})",
                super::menu_bar::shortcut_text(ctx, &super::menu_bar::WINDOW_MENU_SHORTCUT)
            ),
        }
    }

    /// The toolbar's `≡`, writing where it was drawn into `rect`.
    pub(super) fn door<'a>(
        &'a self,
        rect: &'a mut Option<egui::Rect>,
    ) -> crate::toolbar::WindowMenuDoor<'a> {
        crate::toolbar::WindowMenuDoor {
            open: self.door_open,
            hover: self.door_hover.clone(),
            rect,
        }
    }

    /// The name, the floppy's dot and the miniature's dot.
    #[cfg(test)]
    pub(super) fn marks(&self) -> (&str, bool, bool) {
        (&self.workspace, self.unsaved, self.layout_unsaved)
    }

    pub(super) fn model(&self) -> CapsuleModel<'_> {
        CapsuleModel {
            tree: &self.tree,
            workspace: &self.workspace,
            unsaved: self.unsaved,
            layout_unsaved: self.layout_unsaved,
            menu_open: self.menu_open,
            popover_open: self.popover_open,
            compact: false,
        }
    }
}

/// Whether two canvases show the same layout: the preset, the folds, the
/// maximized and the floating panes; slot numbers aside.
fn same_layout(saved: &CanvasState, live: &CanvasState) -> bool {
    saved.preset() == live.preset()
        && saved.tree().len() == live.tree().len()
        && saved.minimized().len() == live.minimized().len()
        && saved.maximized().is_some() == live.maximized().is_some()
        && saved.floating().len() == live.floating().len()
}

/// After the toolbar drew: hang the popover and the workspace menu, publish
/// the controls, and carry out the presses only the window can.
pub(super) fn settle(
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
        app.chrome.layout_picker_open = true;
    }
    #[cfg(any(feature = "scenario-harness", test))]
    if app.chrome.harness.take_window_menu_toolbar() {
        super::canvas_wiring::open_window_menu(&mut app.chrome.window_menu);
    }
    let mut workspace_clicked = false;
    let mut rest = Vec::new();
    for action in actions {
        match action {
            ToolbarAction::ToggleWindowMenu => {
                let menu = &mut app.chrome.window_menu;
                menu.toggle_from(viewport, MenuDoor::Toolbar, false);
            }
            ToolbarAction::ToggleLayoutPopover => {
                app.chrome.layout_picker_open = !app.chrome.layout_picker_open;
            }
            ToolbarAction::ToggleWorkspaceMenu => workspace_clicked = true,
            ToolbarAction::SaveWorkspace => {
                super::tab_workspaces::apply_workspace_command(app, WorkspaceCommand::SaveTo);
            }
            action => rest.push(action),
        }
    }
    let mut cells = Vec::new();
    if app.chrome.layout_picker_open {
        let model = PopoverModel {
            workspace: &state.workspace,
            current: state.current,
            saved: state.saved,
            layout_unsaved: state.layout_unsaved,
        };
        let (drawn, outside) = layout_popover::draw(ctx, rects.miniature, &model, &mut rest);
        let picked = rest
            .iter()
            .any(|action| matches!(action, ToolbarAction::SetLayout(_)));
        app.chrome.layout_picker_open = !(outside || picked);
        cells = drawn;
    }
    super::menu_bar::workspace_cell(app, ctx, Some(rects.name), workspace_clicked);
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
    let layout = state.current.map_or("Custom layout", |preset| preset.label);
    controls.push(control(
        "capsule.layout",
        layout,
        rects.miniature,
        app.chrome.layout_picker_open,
        None,
    ));
    let workspace_open = menu.tab_workspace.chip_menu_on(tab);
    controls.push(control(
        "capsule.workspace",
        &state.workspace,
        rects.name,
        workspace_open,
        None,
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
        let selected = state.current == Some(*preset);
        controls.push(control(
            &id,
            preset.label,
            *rect,
            selected,
            Some(PICK_LAYOUT_CAPABILITY_ID),
        ));
    }
    app.chrome.toolbar_controls = controls;
}
