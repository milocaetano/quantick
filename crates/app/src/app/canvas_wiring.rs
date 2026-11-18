//! Paint and input for the canvas; decisions are made by its lent owners.
use super::QuantickApp;
use eframe::egui;
use quantick_control_handlers::canvas::{self, CanvasAct, CanvasHost};
use quantick_workspace::canvas::{WidgetSpec, add_menu, slot_seat};
use quantick_workspace::canvas_layout::recipe_canvas;

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
    let area = app.active_tab().canvas.area();
    let at = |at: Option<egui::Pos2>| at.map(|at| (at.x, at.y));
    let pointer = ctx.input(|input| input.pointer.clone());
    let (press, now) = (at(pointer.press_origin()), at(pointer.latest_pos()));
    let canvas = &mut app.active_tab_mut().canvas;
    let floats = &mut canvas.floats;
    (floats.origin, floats.scale) = (origin, ctx.pixels_per_point());
    let (dropped, windows) = (floats.dropped.take(), std::mem::take(&mut floats.windows));
    let lift = canvas::lifted(&canvas.state, area, canvas.menus.lift.take(), press, now);
    floats.held = floats.held.or(lift);
    let monitor = |rect| crate::floating_panes::monitor_of(rect, ctx.pixels_per_point());
    #[cfg(any(feature = "scenario-harness", test))]
    let hooked = {
        let var = crate::hooks::captured::var;
        let (open, acts) = canvas::owner::canvas_hooks(
            app.active_tab().canvas.last_canvas.width() > 0.0,
            &mut app.chrome.add_menu.hooked,
            || {
                (
                    var("QUANTICK_ADD_MENU").unwrap_or_default(),
                    var("QUANTICK_CANVAS_ACTS").unwrap_or_default(),
                )
            },
        );
        if open {
            app.surfaces
                .canvas_popovers
                .update(quantick_ui_model::canvas_popovers::Intent::SetAddOpen(true));
        }
        acts
    };
    #[cfg(not(any(feature = "scenario-harness", test)))]
    let hooked = Vec::new();
    let mut acts: Vec<CanvasAct> = (windows.into_iter())
        .filter_map(|(slot, events)| canvas::window_act(slot, (events.moved, events.closed)))
        .collect();
    acts.extend(dropped.map(|(slot, outcome)| canvas::drop_act(slot, outcome, origin, monitor)));
    for act in acts.into_iter().chain(hooked) {
        if let Err(error) = app.act_on_canvas(act) {
            tracing::warn!(error = %error.message, "the floating chart's act was refused");
        }
    }
    #[cfg(any(feature = "scenario-harness", test))]
    app.active_tab_mut().canvas.hold_from_hook();
    if !app.surfaces.remove_chart.scoped(scope).0.has_pick() && pending.is_none() && !escape {
        return;
    }
    let saved = app
        .runtime
        .workspace
        .book
        .saved_recipe(app.runtime.tabs.active_id())
        .map(recipe_canvas);
    let book = &app.runtime.workspace.book;
    let (name, autosave) = (
        book.tab_row(app.runtime.tabs.active_id()).0,
        book.autosave(),
    );
    let surface = &mut app.surfaces.remove_chart;
    let runtime = app.runtime.bound_mut(&app.presentation.binding);
    let canvas = &mut runtime.active_tab_mut().canvas;
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
    if let Some(error) = act.and_then(|act| app.act_on_canvas(act).err()) {
        tracing::warn!(error = %error.message, "the chart menu's act was refused");
    }
}

/// The + menu: hang it from the + with the rows the canvas can take now,
/// and add what was picked through `canvas.widget.add`.
pub(super) fn add_from_menu(app: &mut QuantickApp, ctx: &egui::Context) {
    let capabilities = app.active_tab().capabilities(&app.config);
    let serves = |widget: &WidgetSpec| crate::widget_surfaces::serves(widget.id, &capabilities);
    let rows = add_menu(&app.active_tab().canvas.state, serves);
    let mut picked = None;
    app.surfaces.canvas_popovers.with_pass(
        quantick_ui_model::Slot::Toolbar,
        |response| {
            for effect in response.model_effects {
                if let quantick_ui_model::HostEffect::CanvasPopovers(effect) = effect {
                    picked = effect.0.or(picked.take());
                }
            }
        },
        |_, session| crate::add_widget::hang(ctx, &rows, &mut app.chrome.add_menu, session),
    );
    if let Some(Err(error)) = picked.map(|act| app.act_on_canvas(act)) {
        tracing::warn!(error = %error.message, "the add menu's widget was refused");
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_ADD_MENU",
    "QUANTICK_CANVAS_ACTS",
    "QUANTICK_CANVAS_HOLD"
];
