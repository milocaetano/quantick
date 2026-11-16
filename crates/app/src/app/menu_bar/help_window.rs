//! The Help pages the `⋮` menu opens, and which window Settings and Help are on.

use eframe::egui;
use quantick_control_schema::window_menu::{ABOUT_WORKSPACES, HelpPage};

use super::{
    CLOSE_TAB_SHORTCUT, DOCK_SHORTCUT, HOME_SHORTCUT, LEGEND_SHORTCUT, NEW_TAB_SHORTCUT,
    NEXT_TAB_SHORTCUT, PREVIOUS_TAB_SHORTCUT, REPLAY_SHORTCUT, SAVE_WORKSPACE_SHORTCUT,
    SETTINGS_SHORTCUT, WINDOW_MENU_SHORTCUT, WindowMenuState,
};

const WINDOW_WIDTH_PX: f32 = 420.0;

/// Whether Settings and Help belong on this window. The window they opened on
/// may have closed since; the root takes them over, or a call would report a
/// section nobody can see.
pub(super) fn owned_here(menu: &mut WindowMenuState, ctx: &egui::Context) -> bool {
    let owner = menu.windows_viewport;
    let owner_live =
        owner.is_some_and(|id| ctx.input(|input| input.raw.viewports.contains_key(&id)));
    if !owner_live && owner.is_some() && ctx.viewport_id() == egui::ViewportId::ROOT {
        menu.windows_viewport = Some(egui::ViewportId::ROOT);
    }
    menu.windows_viewport == Some(ctx.viewport_id())
}

pub(super) fn show_help(menu: &mut WindowMenuState, ctx: &egui::Context) {
    let Some(page) = menu.help else {
        return;
    };
    let mut open = true;
    egui::Window::new(page.title())
        .id(egui::Id::new("window_help"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(WINDOW_WIDTH_PX)
        .show(ctx, |ui| match page {
            HelpPage::KeyboardShortcuts => shortcuts_body(ui),
            HelpPage::AboutWorkspaces => {
                for paragraph in ABOUT_WORKSPACES {
                    ui.label(*paragraph);
                }
            }
            HelpPage::AboutQuantick => {
                ui.label(format!("quantick {}", env!("CARGO_PKG_VERSION")));
            }
        });
    if !open {
        menu.help = None;
    }
}

/// Rendered from the constants, never typed by hand, so the page cannot disagree
/// with the keys.
fn shortcuts_body(ui: &mut egui::Ui) {
    let rows = [
        ("Window menu", WINDOW_MENU_SHORTCUT),
        ("Settings", SETTINGS_SHORTCUT),
        ("Open another market", NEW_TAB_SHORTCUT),
        ("Close tab", CLOSE_TAB_SHORTCUT),
        ("Next tab", NEXT_TAB_SHORTCUT),
        ("Previous tab", PREVIOUS_TAB_SHORTCUT),
        ("Home", HOME_SHORTCUT),
        ("Market Replay", REPLAY_SHORTCUT),
        ("Hide panels", DOCK_SHORTCUT),
        ("Indicator legend", LEGEND_SHORTCUT),
        ("Save desk", SAVE_WORKSPACE_SHORTCUT),
    ];
    egui::Grid::new("window_help_shortcuts").show(ui, |ui| {
        for (label, shortcut) in rows {
            ui.label(label);
            ui.label(crate::theme::supporting_text(
                ui,
                super::shortcut_text(ui.ctx(), &shortcut),
            ));
            ui.end_row();
        }
    });
}
