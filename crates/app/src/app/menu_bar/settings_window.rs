//! The windows the `⋮` menu opens: Settings, flat until its sections ship, and
//! the Help pages.

use eframe::egui;
use quantick_control_schema::window_menu::{
    ABOUT_WORKSPACES, HelpPage, PROGRESSIVE_HISTORY_HOVER, VENUE_LEAD_IN_HOVER,
};

use crate::surfaces::ChartSettingsTab::Candles;
use crate::timezone::TzOffset;

use super::super::{LayersPort, QuantickApp};
use super::commands::MenuCommand;
use super::{
    CLOSE_TAB_SHORTCUT, DOCK_SHORTCUT, HOME_SHORTCUT, LEGEND_SHORTCUT, NEW_TAB_SHORTCUT,
    NEXT_TAB_SHORTCUT, PREVIOUS_TAB_SHORTCUT, REPLAY_SHORTCUT, SAVE_WORKSPACE_SHORTCUT,
    SETTINGS_SHORTCUT, WINDOW_MENU_SHORTCUT,
};

const WINDOW_WIDTH_PX: f32 = 420.0;

/// Draw whichever of the two windows is up.
pub(super) fn show(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    if app.chrome.window_menu.windows_viewport != Some(ctx.viewport_id()) {
        return;
    }
    show_settings(app, ctx, commands);
    show_help(app, ctx);
}

fn show_settings(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    let mut open = app.chrome.window_menu.settings_open;
    if !open {
        return;
    }
    egui::Window::new("Settings")
        .id(egui::Id::new("window_settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(WINDOW_WIDTH_PX)
        .show(ctx, |ui| settings_body(app, ui, commands));
    app.chrome.window_menu.settings_open = open;
}

fn settings_body(app: &mut QuantickApp, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
    if ui.button("Appearance…").clicked() {
        commands.push(MenuCommand::OpenAppearance);
    }
    ui.menu_button("Timezone", |ui| {
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
                for tz in TzOffset::ALL {
                    if ui
                        .selectable_label(app.runtime.tz == tz, tz.label())
                        .clicked()
                    {
                        commands.push(MenuCommand::SetTimezone(tz));
                        ui.close_menu();
                    }
                }
            });
    });
    ui.separator();
    let mut show_perf = app.health.show_perf;
    if ui
        .checkbox(&mut show_perf, "Perf readings")
        .on_hover_text("fps, frame time and trade count on the status bar")
        .changed()
    {
        commands.push(MenuCommand::SetShowPerf(show_perf));
    }
    let mut progressive = app.runtime.history.progressive_history;
    if ui
        .checkbox(&mut progressive, "Progressive venue history")
        .on_hover_text(PROGRESSIVE_HISTORY_HOVER)
        .changed()
    {
        commands.push(MenuCommand::SetProgressiveHistory(progressive));
    }
    let mut lead_in = app.runtime.history.venue_lead_in;
    if ui
        .checkbox(&mut lead_in, "Venue candles on charts cut by trades")
        .on_hover_text(VENUE_LEAD_IN_HOVER)
        .changed()
    {
        commands.push(MenuCommand::SetVenueLeadIn(lead_in));
    }
    super::super::deal_recording_wiring::draw_toggle(app, ui);
    ui.separator();
    let access_label = app
        .control
        .control_access
        .as_ref()
        .map_or("Local agent access…", |access| access.menu_label());
    if ui.button(access_label).clicked() {
        commands.push(MenuCommand::OpenAgentAccess);
    }
}

fn show_help(app: &mut QuantickApp, ctx: &egui::Context) {
    let Some(page) = app.chrome.window_menu.help else {
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
        app.chrome.window_menu.help = None;
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
            ui.weak(super::shortcut_text(ui.ctx(), &shortcut));
            ui.end_row();
        }
    });
}

/// Carry out the appearance row, which belongs to the layer wiring.
pub(super) fn open_appearance(app: &mut QuantickApp) {
    app.layer_wiring().open_settings(Candles);
}
