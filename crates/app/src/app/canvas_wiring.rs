//! The window's side of the `canvas.*` capabilities: the active tab's
//! [`CanvasState`] and its save, lent to `quantick_control_handlers::canvas`,
//! which acts and answers. A layout pick goes through the one path every
//! picker takes.

use quantick_control::error::ControlError;
use quantick_control_handlers::canvas::{self, CanvasAct, CanvasHost};
use quantick_control_schema::canvas::CanvasResult;
use quantick_stores::workspaces::WorkspaceRecipe;
use quantick_workspace::canvas::{CanvasRect, CanvasState};

use crate::tab::CanvasLayout;

use super::QuantickApp;
use super::menu_bar::MenuDoor;

/// The canvas the active tab last drew, in model points; before its first
/// frame a nominal one stands in.
fn canvas_area(app: &QuantickApp) -> CanvasRect {
    let drawn = app.active_tab().canvas.last_canvas;
    if drawn.width() > 0.0 && drawn.height() > 0.0 {
        CanvasRect::new(drawn.left(), drawn.top(), drawn.width(), drawn.height())
    } else {
        CanvasRect::new(0.0, 0.0, 1280.0, 720.0)
    }
}

/// The active tab's workspace as last saved, if it has a save.
fn saved_recipe(app: &QuantickApp) -> Option<&WorkspaceRecipe> {
    let book = &app.runtime.workspace.book;
    book.book()
        .recipe(&book.tab_row(app.runtime.tabs.active_id()).0)
}

/// Whether the active tab's workspace differs from its save now: never while
/// auto save is on, which writes every change through.
pub(super) fn unsaved(app: &QuantickApp) -> bool {
    let live = super::tab_workspaces::live_recipe(&app.runtime, app.runtime.tabs.active_id());
    let differs = saved_recipe(app)
        .zip(live)
        .is_some_and(|(saved, live)| !saved.holds(&live));
    differs && !app.runtime.workspace.book.autosave()
}

impl CanvasHost for QuantickApp {
    fn open_window_menu(&mut self) -> Result<bool, ControlError> {
        Ok(open_window_menu(&mut self.chrome.window_menu))
    }

    fn act_on_canvas(&mut self, act: CanvasAct) -> Result<CanvasResult, ControlError> {
        answer(self, act)
    }
}

/// Act on the active tab's canvas against its save, and read it back.
fn answer(app: &mut QuantickApp, act: CanvasAct) -> Result<CanvasResult, ControlError> {
    let (area, saved) = (canvas_area(app), saved_recipe(app).map(recipe_state));
    let (changed, slot) = if let CanvasAct::PickLayout { preset_id } = &act {
        let before = app.active_tab().canvas.state.clone();
        app.apply_layout_preset(canvas::picked(preset_id)?);
        (app.active_tab().canvas.state != before, None)
    } else {
        let state = &mut app.active_tab_mut().canvas.state;
        canvas::act(state, saved.as_ref(), area, act)?
    };
    let state = &app.active_tab().canvas.state;
    Ok(canvas::result(state, changed, slot, unsaved(app)))
}

/// Hang the window menu from the toolbar's list door, as a click on it does.
pub(super) fn open_window_menu(menu: &mut super::menu_bar::WindowMenuState) -> bool {
    let viewport = eframe::egui::ViewportId::ROOT;
    if !menu.opened_from(viewport, MenuDoor::Toolbar) {
        if menu.is_open_on(viewport) {
            menu.toggle(viewport, false);
        }
        menu.toggle_from(viewport, MenuDoor::Toolbar, true);
    }
    menu.opened_from(viewport, MenuDoor::Toolbar)
}

/// `recipe` carrying `canvas`'s model: none while the column painter's own
/// words say it all, the whole model otherwise.
#[must_use]
pub(super) fn with_canvas(
    mut recipe: WorkspaceRecipe,
    canvas: &crate::tab::layout::TabCanvas,
) -> WorkspaceRecipe {
    let plain = canvas.column_painted() && canvas.state.floating().is_empty();
    recipe.canvas = (!plain).then(|| canvas.state.clone());
    recipe
}

/// The layout a recipe opens on: its saved canvas's preset, else its
/// declared layout.
#[must_use]
pub(super) fn recipe_layout(recipe: &WorkspaceRecipe) -> CanvasLayout {
    (recipe.canvas.as_ref())
        .and_then(CanvasState::preset)
        .map(CanvasLayout::from_preset)
        .unwrap_or_else(|| CanvasLayout::from(recipe.layout))
}

/// The canvas model a recipe restores: its saved one, else its layout's preset.
#[must_use]
pub(super) fn recipe_state(recipe: &WorkspaceRecipe) -> CanvasState {
    let layout = recipe_layout(recipe);
    (recipe.canvas.clone()).unwrap_or_else(|| CanvasState::from_preset(layout.preset()))
}
