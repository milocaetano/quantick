//! The window's side of the `canvas.*` capabilities: the active tab's
//! [`CanvasState`] and its save, lent to `quantick_control_handlers::canvas`,
//! which acts and answers. A layout pick goes through the one path every
//! picker takes.

use eframe::egui;
use quantick_control::error::ControlError;
use quantick_control_handlers::canvas::{self, CanvasAct, CanvasHost};
use quantick_control_schema::canvas::CanvasResult;
use quantick_stores::workspaces::WorkspaceRecipe;
use quantick_workspace::canvas::{CanvasRect, CanvasState, WidgetSpec, add_menu, slot_seat};

use crate::tab::CanvasLayout;

use super::QuantickApp;
use super::menu_bar::MenuDoor;

/// The canvas the active tab last drew, in model points; before its first
/// frame a nominal one stands in.
fn canvas_area(app: &QuantickApp) -> CanvasRect {
    tab_area(&app.active_tab().canvas)
}

/// `canvas`'s area as last drawn, or a nominal one before its first frame.
pub(super) fn tab_area(canvas: &crate::tab::layout::TabCanvas) -> CanvasRect {
    let drawn = canvas.last_canvas;
    if drawn.width() > 0.0 && drawn.height() > 0.0 {
        CanvasRect::new(drawn.left(), drawn.top(), drawn.width(), drawn.height())
    } else {
        CanvasRect::new(0.0, 0.0, 1280.0, 720.0)
    }
}

/// `tab` as the window draws it: its area and whether the column painter
/// draws it.
pub(crate) fn drawn(tab: &crate::tab::layout::TabCanvas) -> canvas::Drawn {
    let (area, columns) = (tab_area(tab), tab.column_painted());
    canvas::Drawn { area, columns }
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
    let (canvas, monitor_of) = (&app.active_tab().canvas, crate::floating_panes::monitor_of);
    let (origin, scale) = (canvas.floats.origin, canvas.floats.scale);
    let place = (&canvas.state, area);
    let act = canvas::placed_pop_out(act, place, origin, |rect| monitor_of(rect, scale));
    let (changed, slot) = if let CanvasAct::PickLayout { preset_id } = &act {
        let before = app.active_tab().canvas.state.clone();
        app.apply_layout_preset(canvas::picked(preset_id)?);
        (app.active_tab().canvas.state != before, None)
    } else if let Some(id) = canvas::surface_widget(&act) {
        let reads = (&app.config, app.runtime.tz);
        let tabs = (&mut app.surfaces.widgets, &mut app.runtime.tabs);
        crate::widget_surfaces::answer(tabs, reads, (id, Some(true)))
            .map_err(ControlError::invalid_request)?;
        (true, None)
    } else {
        let drawn = drawn(&app.active_tab().canvas);
        let state = &mut app.active_tab_mut().canvas.state;
        canvas::act(state, saved.as_ref(), drawn, act)?
    };
    let (tab, unsaved) = (&app.active_tab().canvas, unsaved(app));
    let drawn = drawn(tab);
    Ok(canvas::result(&tab.state, changed, slot, unsaved, drawn))
}

/// After the canvas drew: open the indicator library a header asked for, and
/// carry out a ⋯ row or a strip's restore, Esc on a maximized chart, and the
/// remove dialog's answer, each as the act its capability takes.
pub(super) fn settle_canvas(app: &mut QuantickApp, ctx: &egui::Context) {
    if let Some(side) = app.active_tab_mut().take_indicator_picker_request() {
        app.indicator_wiring().open_indicator_picker_on(side);
    }
    use quantick_ui_model::remove_chart::Effects;
    let runtime = app.runtime.bound(&app.presentation.binding);
    let scope = runtime.tabs.active_id();
    let surface = &mut app.surfaces.remove_chart;
    surface.retain_scopes(|id| runtime.tabs.position(id).is_some());
    let pending = surface.scoped(scope).0.pending();
    // Esc restores a maximized chart only when nothing else wants the key.
    let canvas = &app.active_tab().canvas;
    let escape = canvas.state.maximized().is_some()
        && pending.is_none()
        && !ctx.wants_keyboard_input()
        && !ctx.memory(|memory| memory.any_popup_open())
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let at = ctx.input(|input| input.viewport().inner_rect);
    let origin = at.map_or((0.0, 0.0), |at| (at.min.x, at.min.y));
    let floats = &mut app.active_tab_mut().canvas.floats;
    (floats.origin, floats.scale) = (origin, ctx.pixels_per_point());
    let (dropped, windows) = (floats.dropped.take(), std::mem::take(&mut floats.windows));
    let monitor = |rect| crate::floating_panes::monitor_of(rect, ctx.pixels_per_point());
    #[cfg(any(feature = "scenario-harness", test))]
    let hooked = canvas_hooks(app);
    #[cfg(not(any(feature = "scenario-harness", test)))]
    let hooked = Vec::new();
    let mut acts: Vec<CanvasAct> = (windows.into_iter())
        .filter_map(|(slot, events)| canvas::window_act(slot, (events.moved, events.closed)))
        .collect();
    acts.extend(dropped.map(|(slot, outcome)| canvas::drop_act(slot, outcome, origin, monitor)));
    for act in acts.into_iter().chain(hooked) {
        if let Err(error) = answer(app, act) {
            tracing::warn!(error = %error.message, "the floating chart's act was refused");
        }
    }
    #[cfg(any(feature = "scenario-harness", test))]
    hold_from_hook(app);
    let canvas = &app.active_tab().canvas;
    if canvas.menus.picked.is_none() && pending.is_none() && !escape {
        return;
    }
    let saved = saved_recipe(app).map(recipe_state);
    let book = &app.runtime.workspace.book;
    let (name, autosave) = (
        book.tab_row(app.runtime.tabs.active_id()).0,
        book.autosave(),
    );
    let surface = &mut app.surfaces.remove_chart;
    let runtime = app.runtime.bound_mut(&app.presentation.binding);
    let canvas = &mut runtime.active_tab_mut().canvas;
    let picked = canvas.menus.picked.take();
    let mut act = None;
    if surface.supports_pass(quantick_ui_model::Slot::Overlay) {
        surface.with_scoped_updates(
            scope,
            |response| {
                for effect in response.model_effects {
                    if let crate::surfaces::HostEffect::RemoveChart(Effects::Act(value)) = effect {
                        act = value;
                    }
                }
            },
            |_, session| {
                if let Some(intent) = session.state().observe(
                    picked,
                    escape,
                    &canvas.state,
                    saved.as_ref(),
                    (&name, autosave),
                ) {
                    session.update(intent);
                }
                let Some((slot, prompt)) = session.state().open() else {
                    return;
                };
                let side = match slot_seat(&canvas.state, &mut canvas.seats, slot) {
                    Some(Some(seat)) => crate::pane::PaneSide::Time(seat),
                    _ => crate::pane::PaneSide::Flow,
                };
                let doomed = canvas.drawn_bands.iter().find(|(drawn, _)| *drawn == side);
                let doomed = doomed.map_or(egui::Rect::NOTHING, |(_, rect)| *rect);
                if let Some(intent) =
                    crate::surfaces::remove_chart::draw(ctx, prompt, doomed, &mut canvas.menus)
                {
                    session.update(intent);
                }
            },
        );
    }
    if let Some(error) = act.and_then(|act| answer(app, act).err()) {
        tracing::warn!(error = %error.message, "the chart menu's act was refused");
    }
}

/// The + menu: hang it from the + with the rows the canvas can take now,
/// and add what was picked through `canvas.widget.add`.
pub(super) fn add_from_menu(app: &mut QuantickApp, ctx: &egui::Context) {
    let capabilities = app.active_tab().capabilities(&app.config);
    let serves = |widget: &WidgetSpec| crate::widget_surfaces::serves(widget.id, &capabilities);
    let rows = add_menu(&app.active_tab().canvas.state, serves);
    crate::add_widget::hang(ctx, &rows, &mut app.chrome.add_menu);
    let picked = app.chrome.add_menu.picked.take();
    if let Some(Err(error)) = picked.map(|widget| answer(app, canvas::add_act(widget).1)) {
        tracing::warn!(error = %error.message, "the add menu's widget was refused");
    }
}

/// `QUANTICK_CANVAS_ACTS` and `QUANTICK_ADD_MENU`, once, after the canvas
/// first drew: the acts as their capabilities take them, the + menu open.
#[cfg(any(feature = "scenario-harness", test))]
fn canvas_hooks(app: &mut QuantickApp) -> Vec<CanvasAct> {
    let drawn = app.active_tab().canvas.last_canvas.width() > 0.0;
    if !drawn || std::mem::replace(&mut app.chrome.add_menu.hooked, true) {
        return Vec::new();
    }
    let var = crate::hooks::captured::var;
    app.chrome.add_menu.open |= var("QUANTICK_ADD_MENU").as_deref() == Some("1");
    canvas::hook_acts(&var("QUANTICK_CANVAS_ACTS").unwrap_or_default())
}

/// `QUANTICK_CANVAS_HOLD`: once its chart floats, hold it by the grip with
/// the pointer pinned at that share of the canvas, for the dock preview.
#[cfg(any(feature = "scenario-harness", test))]
fn hold_from_hook(app: &mut QuantickApp) {
    let value = crate::hooks::captured::var("QUANTICK_CANVAS_HOLD").unwrap_or_default();
    let canvas = &mut app.active_tab_mut().canvas;
    let held = canvas::held_by_hook(&canvas.state, &value);
    if let (Some((held, (x, y))), None) = (held, canvas.floats.pinned) {
        let area = canvas.last_canvas;
        canvas.floats.held = Some(held);
        canvas.floats.pinned = Some(area.min + egui::vec2(x, y) * area.size());
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_ADD_MENU",
    "QUANTICK_CANVAS_ACTS",
    "QUANTICK_CANVAS_HOLD"
];

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
    let plain = canvas.column_painted() && canvas.state.numbered_as(canvas.layout.preset());
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
