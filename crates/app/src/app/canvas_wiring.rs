//! The window's side of the `canvas.*` capabilities: each validated act runs
//! against the active tab's [`CanvasState`], and the answer is the canvas as
//! it stands after. The transitions are `quantick_workspace::canvas`'s; a
//! layout pick goes through the one path every picker takes.

use quantick_control::error::ControlError;
use quantick_control_handlers::canvas::{CanvasAct, CanvasHost};
use quantick_control_schema::canvas::{
    CanvasCorner, CanvasEdge, CanvasRectDto, CanvasResult, CanvasView,
};
use quantick_stores::workspaces::WorkspaceRecipe;
use quantick_workspace::canvas::{
    CanvasRect, CanvasRefusal, CanvasState, Corner, Edge, FloatHost, remove_needs_confirm,
};

use crate::tab::CanvasLayout;

use super::QuantickApp;
use super::menu_bar::MenuDoor;

/// Where a chart popped out without a monitor named goes.
const PRIMARY_MONITOR: &str = "primary";

fn refused(refusal: &CanvasRefusal) -> ControlError {
    let mut error = ControlError::invalid_request(refusal.to_string());
    error.context.details = Some(serde_json::json!({ "refusal": refusal.code() }));
    error
}

fn rect_of(dto: CanvasRectDto) -> CanvasRect {
    CanvasRect::new(dto.x, dto.y, dto.w, dto.h)
}

const fn edge_of(edge: CanvasEdge) -> Edge {
    match edge {
        CanvasEdge::Left => Edge::Left,
        CanvasEdge::Right => Edge::Right,
        CanvasEdge::Top => Edge::Top,
        CanvasEdge::Bottom => Edge::Bottom,
    }
}

const fn corner_of(corner: CanvasCorner) -> Corner {
    match corner {
        CanvasCorner::TopLeft => Corner::TopLeft,
        CanvasCorner::TopRight => Corner::TopRight,
        CanvasCorner::BottomLeft => Corner::BottomLeft,
        CanvasCorner::BottomRight => Corner::BottomRight,
    }
}

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

fn act_on(app: &mut QuantickApp, act: CanvasAct) -> Result<(bool, Option<u32>), CanvasRefusal> {
    let area = canvas_area(app);
    if let CanvasAct::PickLayout { preset_id } = &act {
        let preset = quantick_workspace::canvas::preset(preset_id)
            .ok_or_else(|| CanvasRefusal::UnknownPreset(preset_id.clone()))?;
        let before = app.active_tab().canvas.state.clone();
        app.apply_layout_preset(preset);
        return Ok((app.active_tab().canvas.state != before, None));
    }
    let state = &mut app.active_tab_mut().canvas.state;
    Ok(match act {
        CanvasAct::PickLayout { .. } => unreachable!("picked above"),
        CanvasAct::Minimize { slot } => (state.minimize(slot)?, Some(slot)),
        CanvasAct::Maximize { slot } => (state.maximize(slot)?, Some(slot)),
        CanvasAct::Restore { slot } => (state.restore(slot)?, Some(slot)),
        CanvasAct::Remove { slot, .. } => (state.remove(slot)?, Some(slot)),
        CanvasAct::AddWidget { widget_id } => {
            let widget = quantick_workspace::canvas::widget(&widget_id)
                .ok_or(CanvasRefusal::UnknownWidget(widget_id))?;
            (true, Some(state.add(widget.kind, area)?))
        }
        CanvasAct::Float { slot, rect } => {
            let half = CanvasRect::new(0.0, 0.0, area.w / 2.0, area.h / 2.0);
            let rect = rect.map_or_else(|| half.centred_in(area), rect_of);
            (state.float(slot, rect)?, Some(slot))
        }
        CanvasAct::Dock { slot, edge } => (state.dock(slot, edge_of(edge))?, Some(slot)),
        CanvasAct::Park { slot, corner } => {
            (state.park(slot, corner_of(corner), area)?, Some(slot))
        }
        CanvasAct::PopOut {
            slot,
            monitor,
            rect,
        } => {
            let kept = state.floating().iter().find(|pane| pane.slot == slot);
            let kept = kept.map(|pane| match &pane.host {
                FloatHost::Canvas { rect } | FloatHost::OsWindow { rect, .. } => *rect,
            });
            let rect = rect.map(rect_of).or(kept).unwrap_or(area);
            let monitor = monitor.as_deref().unwrap_or(PRIMARY_MONITOR);
            (state.pop_out(slot, monitor, rect)?, Some(slot))
        }
        CanvasAct::Return { slot } => (state.return_to_canvas(slot, area)?, Some(slot)),
    })
}

impl CanvasHost for QuantickApp {
    fn open_window_menu(&mut self) -> Result<bool, ControlError> {
        Ok(open_window_menu(&mut self.chrome.window_menu))
    }

    fn act_on_canvas(&mut self, act: CanvasAct) -> Result<CanvasResult, ControlError> {
        answer(self, act)
    }
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

/// Refuse an unconfirmed removal of a pane the save holds, act, and read the
/// canvas back.
fn answer(app: &mut QuantickApp, act: CanvasAct) -> Result<CanvasResult, ControlError> {
    if let CanvasAct::Remove {
        slot,
        confirm: false,
    } = act
        && saved_recipe(app).is_some_and(|saved| remove_needs_confirm(&saved.canvas_state(), slot))
        && app.active_tab().canvas.state.holds(slot)
    {
        let mut error = ControlError::invalid_request(format!(
            "the saved workspace holds chart {slot}: remove it with confirm true, as the trader's dialog asks first"
        ));
        error.context.details = Some(serde_json::json!({ "refusal": "canvas.confirm_required" }));
        return Err(error);
    }
    let (changed, slot) = act_on(app, act).map_err(|refusal| refused(&refusal))?;
    let view = view(&app.active_tab().canvas.state);
    Ok(CanvasResult {
        changed,
        slot,
        preset_id: view.preset_id,
        unsaved: unsaved(app),
        panes: view.panes,
    })
}

/// One tab's canvas as the wire reads it.
#[must_use]
pub(crate) fn view(state: &CanvasState) -> CanvasView {
    use quantick_control_schema::canvas::{
        CanvasPaneDto, CanvasPaneKind as Kind, CanvasPaneMode as Wire,
    };
    use quantick_workspace::canvas::PaneMode as Mode;
    let panes = (state.panes().into_iter())
        .map(|(slot, kind, mode)| CanvasPaneDto {
            slot,
            kind: match kind {
                quantick_workspace::canvas::PaneKind::Time => Kind::Time,
                _ => Kind::Flow,
            },
            mode: match mode {
                Mode::Docked => Wire::Docked,
                Mode::Minimized => Wire::Minimized,
                Mode::Maximized => Wire::Maximized,
                Mode::Floating => Wire::Floating,
                Mode::OsWindow => Wire::OsWindow,
            },
        })
        .collect();
    let floating = (state.floating().iter())
        .map(|pane| {
            let (monitor, rect) = match &pane.host {
                FloatHost::Canvas { rect } => (None, rect),
                FloatHost::OsWindow { monitor, rect } => (Some(monitor.clone()), rect),
            };
            let point = |value: f32| value.round() as i32;
            quantick_control_schema::canvas::CanvasPlaceDto {
                slot: pane.slot,
                monitor,
                x: point(rect.x),
                y: point(rect.y),
                w: point(rect.w),
                h: point(rect.h),
            }
        })
        .collect();
    CanvasView {
        preset_id: state.preset().map(|preset| preset.id.to_owned()),
        panes,
        floating,
    }
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
