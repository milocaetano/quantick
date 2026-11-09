//! The windows the `⋮` menu opens: Settings, in the sections its table names, and
//! the Help pages.

use eframe::egui;
use quantick_control_schema::settings_sections::{SettingRow, SettingsSection};
use quantick_control_schema::window_menu::{ABOUT_WORKSPACES, HelpPage};

use crate::control::ControlAccess;
use crate::theme;
use crate::timezone::TzOffset;

use super::super::{QuantickApp, deal_recording_wiring as deals};
use super::commands::MenuCommand;
use super::{
    CLOSE_TAB_SHORTCUT, DOCK_SHORTCUT, HOME_SHORTCUT, LEGEND_SHORTCUT, NEW_TAB_SHORTCUT,
    NEXT_TAB_SHORTCUT, PREVIOUS_TAB_SHORTCUT, REPLAY_SHORTCUT, SAVE_WORKSPACE_SHORTCUT,
    SETTINGS_SHORTCUT, WINDOW_MENU_SHORTCUT, WindowMenuState,
};

const WINDOW_WIDTH_PX: f32 = 420.0;
const SETTINGS_SIZE_PX: [f32; 2] = [640.0, 340.0];
const SECTION_LIST_WIDTH_PX: f32 = 120.0;
const ROW_TEXT_WIDTH_PX: f32 = 380.0;
const ROW_GAP_PX: f32 = 8.0;
const ROW_HINT_PX: f32 = 12.0;
const DISABLED_DIM: f32 = 0.4;
const SWITCH_SIZE_PX: egui::Vec2 = egui::vec2(34.0, 18.0);
const SWITCH_INSET_PX: f32 = 2.0;

/// Draw whichever of the two windows is up.
pub(super) fn show(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    // The window they opened on may have closed since; the root takes them over,
    // or a call would report a section nobody can see.
    let owner = app.chrome.window_menu.windows_viewport;
    let owner_live =
        owner.is_some_and(|id| ctx.input(|input| input.raw.viewports.contains_key(&id)));
    if !owner_live && owner.is_some() && ctx.viewport_id() == egui::ViewportId::ROOT {
        app.chrome.window_menu.windows_viewport = Some(egui::ViewportId::ROOT);
    }
    if app.chrome.window_menu.windows_viewport != Some(ctx.viewport_id()) {
        return;
    }
    show_settings(app, ctx, commands);
    show_help(&mut app.chrome.window_menu, ctx);
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
        .fixed_size(SETTINGS_SIZE_PX)
        .default_pos([16.0, super::super::title_bar::HEIGHT + 16.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                section_list(&mut app.chrome.window_menu, ui);
                ui.separator();
                ui.vertical(|ui| section_body(app, ui, commands));
            });
        });
    app.chrome.window_menu.settings_open = open;
}

/// The list on the left: the one on screen is filled, as in the prototype.
fn section_list(menu: &mut WindowMenuState, ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.set_width(SECTION_LIST_WIDTH_PX);
        for section in SettingsSection::ALL {
            let shown = menu.settings_section == section;
            if ui.selectable_label(shown, section.title()).clicked() {
                menu.settings_section = section;
            }
        }
    });
}

fn section_body(app: &mut QuantickApp, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
    let section = app.chrome.window_menu.settings_section;
    ui.strong(section.title());
    ui.add_space(ROW_GAP_PX);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in section.rows() {
            draw_row(app, ui, *row, commands);
            ui.separator();
        }
    });
}

/// A row's name and line on the left, what it does on the right.
fn draw_row(
    app: &mut QuantickApp,
    ui: &mut egui::Ui,
    row: SettingRow,
    commands: &mut Vec<MenuCommand>,
) {
    let palette = theme::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(ROW_TEXT_WIDTH_PX);
            ui.label(row.label());
            ui.label(
                egui::RichText::new(row.description())
                    .size(ROW_HINT_PX)
                    .color(palette.text_muted()),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            row_control(app, ui, row, commands);
        });
    });
    if row == SettingRow::AgentSwitch
        && let Some(access) = app.control.control_access.as_ref()
    {
        if let Some(error) = access.start_error() {
            ui.colored_label(palette.warn(), error);
        } else if let Some(note) = access.status_note() {
            ui.colored_label(palette.text_muted(), note);
        }
    }
}

fn row_control(
    app: &mut QuantickApp,
    ui: &mut egui::Ui,
    row: SettingRow,
    commands: &mut Vec<MenuCommand>,
) {
    match row {
        SettingRow::Appearance => {
            if ui.button(row.label()).clicked() {
                commands.push(MenuCommand::OpenAppearance);
            }
            crate::theme::choice::show(ui, app);
        }
        SettingRow::Timezone => {
            let mut tz = app.runtime.tz;
            egui::ComboBox::from_id_salt("settings_timezone")
                .selected_text(tz.label())
                .show_ui(ui, |ui| {
                    for offset in TzOffset::ALL {
                        ui.selectable_value(&mut tz, offset, offset.label());
                    }
                });
            if tz != app.runtime.tz {
                commands.push(MenuCommand::SetTimezone(tz));
            }
        }
        SettingRow::PerfReadings => {
            let mut on = app.health.show_perf;
            if switch(ui, &mut on, true, row).changed() {
                commands.push(MenuCommand::SetShowPerf(on));
            }
        }
        SettingRow::ProgressiveHistory => {
            let mut on = app.runtime.history.progressive_history;
            if switch(ui, &mut on, true, row).changed() {
                commands.push(MenuCommand::SetProgressiveHistory(on));
            }
        }
        SettingRow::VenueLeadIn => {
            let mut on = app.runtime.history.venue_lead_in;
            if switch(ui, &mut on, true, row).changed() {
                commands.push(MenuCommand::SetVenueLeadIn(on));
            }
        }
        SettingRow::RecordDeals => {
            let mut on = deals::default_for(app);
            if switch(ui, &mut on, true, row).changed() {
                deals::set_default(&mut app.chrome.record_deals, &mut app.runtime.tabs, on);
            }
        }
        SettingRow::AgentSwitch => {
            let access = app.control.control_access.as_ref();
            let (mut on, can_flip) = access.map_or((false, false), ControlAccess::switch_state);
            let hover = access.map_or("Local agent access…", ControlAccess::menu_label);
            if switch(ui, &mut on, can_flip, row)
                .on_hover_text(hover)
                .changed()
                && let Some(access) = app.control.control_access.as_mut()
            {
                access.set_switch(ui.ctx(), on);
            }
        }
        SettingRow::ManageClients => {
            if ui.button(row.label()).clicked() {
                commands.push(MenuCommand::OpenAgentAccess);
            }
        }
    }
}

/// The prototype's switch, painted because egui has none.
fn switch(ui: &mut egui::Ui, on: &mut bool, enabled: bool, row: SettingRow) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(SWITCH_SIZE_PX, sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let value = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, value, row.label())
    });
    // An off track in `border` is 1.5:1 on Light, under 3:1 for a control.
    let track = if value {
        palette.accent()
    } else {
        palette.text_faint()
    };
    let knob = palette.inset();
    let (track, knob) = if enabled {
        (track, knob)
    } else {
        (
            track.gamma_multiply(DISABLED_DIM),
            knob.gamma_multiply(DISABLED_DIM),
        )
    };
    let radius = rect.height() / 2.0;
    let centre = if value {
        rect.right_center() - egui::vec2(radius, 0.0)
    } else {
        rect.left_center() + egui::vec2(radius, 0.0)
    };
    ui.painter().rect_filled(rect, radius, track);
    ui.painter()
        .circle_filled(centre, radius - SWITCH_INSET_PX, knob);
    response
}

fn show_help(menu: &mut WindowMenuState, ctx: &egui::Context) {
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
