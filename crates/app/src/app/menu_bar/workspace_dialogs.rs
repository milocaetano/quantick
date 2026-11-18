//! Manage workspaces and the name dialogs: `manage_workspaces` sheets on the
//! one painter the menu uses; their state, keys and answers are headless.
use eframe::egui::{self, Key, Modifiers as Mods, Order};
use quantick_control_handlers::workspace_book::manage::fill_create;
use quantick_session::colour_popover::Key as PopKey;
use quantick_session::manage_workspaces::{ManageKey, ManageKeys};
use quantick_session::workspace_menu::{WorkspaceCommand as Command, WorkspaceMenuState as Menu};
use quantick_ui_model::manage_workspaces as marks;

use super::MenuCommand;
use super::workspace_rows::{Drawn, Pen, sheet};
use crate::app::{GatewayPort as _, QuantickApp};

type Measure<'a> = &'a dyn Fn(&str, f32) -> f32;

/// The dialogs on the tab on screen, the name dialog over Manage and taking
/// the keys first; answers the rows drawn, for the scene.
pub(super) fn dialogs(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    out: &mut Vec<MenuCommand>,
) -> Drawn {
    let (tab, mut drawn) = (app.runtime.tabs.active_id(), Vec::new());
    let mut menu = std::mem::take(&mut app.chrome.window_menu.tab_workspace);
    menu.follow_staged(tab, !app.chrome.title_bar.surface.is_home());
    fill_create(&app.workspace_calls(), &mut menu);
    let theme = crate::theme::get(ctx);
    let ink = |t: Option<_>| t.map_or(egui::Color32::TRANSPARENT, |t| *theme.get(t));
    let dark = theme.resolved.dark;
    let named = name(app, ctx, (&mut menu, tab), (&ink, &mut drawn, dark));
    let managed = manage(app, ctx, (&mut menu, tab), (&ink, &mut drawn, dark));
    app.chrome.window_menu.tab_workspace = menu;
    out.extend(named.or(managed).map(MenuCommand::Workspace));
    drawn
}

/// An area centred over the window.
fn centred(order: Order) -> egui::Area {
    let area = egui::Area::new(egui::Id::new(("workspace_sheet", order))).order(order);
    area.anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
}

/// The name dialog: its field, its palette, Enter and Esc.
fn name(
    app: &QuantickApp,
    ctx: &egui::Context,
    (menu, tab): (&mut Menu, u64),
    pen: Pen<'_, '_>,
) -> Option<Command> {
    let dialog = menu.dialog.as_mut().filter(|dialog| dialog.tab == tab)?;
    let (book, held, dark) = (app.runtime.workspace.book.book(), dialog.clone(), pen.2);
    let esc = ctx.input_mut(|i| i.consume_key(Mods::NONE, Key::Escape));
    // Its top stays where it opens at rest; Custom grows it downward.
    let screen = ctx.screen_rect();
    let (top, room) = marks::name_place(book, &held, [screen.top(), screen.bottom()]);
    let view =
        |measure: Measure<'_>| marks::capped(marks::name_dialog(book, &held, dark, measure), room);
    let area = centred(Order::Tooltip).pivot(egui::Align2::CENTER_TOP);
    let area = area.fixed_pos(egui::pos2(screen.center().x, top));
    let field = Some((&mut dialog.name, &mut dialog.wants_focus));
    let parts = (field, Some((&mut dialog.palette, [false; 8])), None);
    let painted = sheet(ctx, area, &view, parts, pen);
    let enter = [painted.enter, ctx.input(|i| i.key_pressed(Key::Enter))];
    let answer = (enter, painted.saves, esc);
    marks::name_answer(dialog, &held.name, painted.asked.flatten(), answer)
}

/// Manage: its keys while no name dialog is over it, its Undo rows timed on
/// the wall clock, and its sheet.
fn manage(
    app: &QuantickApp,
    ctx: &egui::Context,
    (menu, tab): (&mut Menu, u64),
    pen: Pen<'_, '_>,
) -> Option<Command> {
    let keyed = menu.dialog.is_none();
    let dialog = menu.manage.as_mut().filter(|dialog| dialog.tab == tab)?;
    let now_ms = crate::metrics::wall_clock_ms();
    if dialog.tick(now_ms) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let consume = |name: &str, command: bool| {
        let mods = [Mods::NONE, Mods::COMMAND][usize::from(command)];
        let key = Key::from_name(name).filter(|_| keyed);
        key.is_some_and(|key| ctx.input_mut(|i| i.consume_key(mods, key)))
    };
    let open = dialog.palette.as_ref().map(|open| open.popover.clone());
    let takes = |k: PopKey| open.as_ref().is_some_and(|open| open.takes(k));
    let pop_keys = PopKey::ALL.map(|k| takes(k) && consume(k.name(), false));
    let book = app.runtime.workspace.book.book();
    let names = marks::names(book);
    let ours = ManageKeys::of(
        ManageKey::ALL.map(|k| dialog.takes(k, now_ms, &names) && consume(k.name(), k.command())),
    );
    let keyed = dialog.keys(ours, &names, now_ms);
    let current = book.tab_workspace(tab).to_owned();
    let moved = std::mem::take(&mut dialog.reveal);
    let held = dialog.clone();
    let symbols = |tabs: &[u64]| crate::app::tab_workspaces::symbols_of(&app.runtime.tabs, tabs);
    let at = (current.as_str(), now_ms, pen.2);
    let view =
        |measure: Measure<'_>| marks::manage(book, &held, at, &symbols, measure).revealed(moved);
    let (field, pop) = dialog.typed();
    let parts = (field, pop.map(|pop| (pop, pop_keys)), None);
    let painted = sheet(ctx, centred(Order::Foreground), &view, parts, pen);
    marks::manage_answer(keyed, painted.asked.flatten(), painted.enter)
}
