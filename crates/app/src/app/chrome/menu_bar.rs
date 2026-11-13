//! Adapt domain snapshots and execute registered window-menu effects.
use crate::app::{
    LayoutPort, QuantickApp,
    menu_bar::{MenuCommand, MenuDoor},
};
use eframe::egui;
use quantick_control_schema::window_menu::{self as menu, RowId};
use quantick_ui_model::chrome_menu_bar::{Action, Snapshot, Workspace};

use crate::toolrail::ToolRailExt as _;
pub(in crate::app) fn draw(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    commands: &mut Vec<MenuCommand>,
) {
    let door = match app.chrome.window_menu.door() {
        MenuDoor::Kebab => app.chrome.title_bar.control_rect(menu::KEBAB_CONTROL),
        MenuDoor::Toolbar => app.chrome.window_menu.toolbar_door,
    };
    let scope = app.runtime.tabs.windows().presented().0;
    app.surfaces
        .retain_slot_scopes(quantick_ui_model::Slot::MenuBar, |scope| {
            app.runtime
                .tabs
                .windows()
                .window(quantick_workspace::arrangement::windows::WindowId(scope))
                .is_some()
        });
    let snapshot = snapshot(app, ctx);
    app.surfaces.update_scoped(
        scope,
        crate::surfaces::HostIntent::ChromeMenuBar(Box::new(
            quantick_ui_model::chrome_menu_bar::Intent::Refresh(Box::new(snapshot)),
        )),
    );
    let view = app.surfaces.chrome_menu_bar.scoped(scope).1;
    view.door = door;
    view.shortcuts = crate::app::menu_bar::menu_shortcuts(ctx);
    let response = app.surfaces.draw_slot_scoped(
        scope,
        quantick_ui_model::Slot::MenuBar,
        &mut crate::surfaces::ViewContext { ctx, ui: None },
    );
    for effect in response.model_effects {
        if let quantick_ui_model::HostEffect::ChromeMenuBar(effect) = effect {
            if effect.closed {
                app.chrome.window_menu.close();
            }
            for action in effect.actions {
                settle(action, commands);
            }
        }
    }
}
fn snapshot(app: &QuantickApp, ctx: &egui::Context) -> Snapshot {
    if !app.chrome.window_menu.is_open_on(ctx.viewport_id()) {
        return Snapshot::default();
    }
    let tab = app.active_tab();
    let book = app.runtime.workspace.book.book();
    let tab_id = app.runtime.tabs.active_id();
    let (name, modified) = app.runtime.workspace.book.tab_row(tab_id);
    let followers = book.save_to_preview(tab_id).unwrap_or_default();
    let workspace = Workspace {
        name,
        modified,
        symbol: tab.symbol.clone(),
        autosave: book.autosave(),
        followers: crate::app::tab_workspaces::symbols_of(&app.runtime.tabs, &followers),
        choices: book
            .list()
            .into_iter()
            .map(|row| {
                (
                    row.name,
                    crate::app::tab_workspaces::symbols_of(&app.runtime.tabs, &row.tabs).join(", "),
                )
            })
            .collect(),
    };
    let mut selected_rows = Vec::new();
    for id in [RowId::DockL2, RowId::DockBubbles] {
        if let Some(section) = menu::panel_section_of(id)
            && menu::panel_row_selected(
                &tab.tape().settings_panel,
                app.runtime.tabs.panel_folds(),
                &tab.symbol,
                section,
            )
        {
            selected_rows.push(id);
        }
    }
    let layout = app.layout_state();
    Snapshot {
        open: app.chrome.window_menu.is_open_on(ctx.viewport_id()),
        door: app.chrome.window_menu.door(),
        keyboard: app.chrome.window_menu.keyboard,
        chart_shown: !app.chrome.title_bar.surface.is_home(),
        replaying: tab.link.replay.is_some(),
        startup_saved: app.runtime.workspace.session().saved(),
        save_on_exit: app.runtime.workspace.session().save_on_exit(),
        dock_visible: app.surfaces.dock.visible(),
        selected_rows,
        flow_pane_shown: crate::orderflow_panel::flow_pane_shown(tab),
        toolbox_dock: app
            .surfaces
            .toolrail
            .visible()
            .then(|| app.surfaces.toolrail.dock()),
        dock_tab: app.surfaces.dock.tab(),
        workspace,
        layouts: layout
            .layouts()
            .layouts()
            .iter()
            .map(|layout| (layout.id, layout.name.clone()))
            .collect(),
        active_layout: Some(layout.focused_pane_layout()),
        preset: tab.canvas.layout.preset().id.into(),
        context_column: tab.canvas.layout.shows_time() && tab.canvas.layout.shows_flow(),
        context_collapsed: tab.canvas.context_collapsed,
        context_panes: tab.pane_count().saturating_sub(1),
        legend_collapsed: tab.focused_pane().legend_collapsed,
        legend_side: tab
            .shows_context_charts()
            .then(|| tab.focused_side().title()),
        has_legend: !tab.focused_pane().indicators.all().is_empty(),
        recent: app
            .runtime
            .workspace
            .recent_on_disk()
            .iter()
            .map(|path| (path.clone(), crate::workspace_bundle::recent_label(path)))
            .collect(),
    }
}
fn settle(action: Action, commands: &mut Vec<MenuCommand>) {
    use crate::layout_strip::StripAction;
    let command = match action {
        Action::Row(id) => {
            settle_row(id, commands);
            return;
        }
        Action::Workspace(command) => MenuCommand::Workspace(command),
        Action::SaveOnExit(on) => MenuCommand::SetSaveOnExit(on),
        Action::SwitchLayout(id) => MenuCommand::Strip(StripAction::Switch(id)),
        Action::CreateLayout => MenuCommand::Strip(StripAction::Create),
        Action::RenameLayout(id) => MenuCommand::Strip(StripAction::BeginRename(id)),
        Action::DeleteLayout(id) => MenuCommand::Strip(StripAction::Delete(id)),
        Action::Preset(id) => {
            if let Some(preset) = quantick_workspace::canvas::preset(id) {
                commands.push(MenuCommand::ApplyLayoutPreset(preset));
            }
            return;
        }
        Action::ContextCharts => MenuCommand::ToggleContextCharts,
        Action::MoveChart { from, to } => MenuCommand::MoveContextChart { from, to },
        Action::Legend => MenuCommand::ToggleLegend,
        Action::Recent(path) => MenuCommand::OpenRecentDesk(path),
    };
    commands.push(command);
}
fn settle_row(id: RowId, commands: &mut Vec<MenuCommand>) {
    use quantick_control_schema::{settings_sections::SettingsSection, window_menu::HelpPage};
    if let Some(section) = menu::panel_section_of(id) {
        commands.push(MenuCommand::OpenOrderFlowPanel(section));
        return;
    }
    let command = match id {
        RowId::DockSession => MenuCommand::OpenDockTab(crate::dock::DockTab::Session),
        RowId::DockPaperTrading => MenuCommand::OpenDockTab(crate::dock::DockTab::Trading),
        RowId::DockTrades => MenuCommand::OpenDockTab(crate::dock::DockTab::Trades),
        RowId::ToolbarLeft => MenuCommand::SetToolboxDock(crate::toolrail::ToolboxDock::Left),
        RowId::ToolbarTop => MenuCommand::SetToolboxDock(crate::toolrail::ToolboxDock::Top),
        RowId::ToolbarBottom => MenuCommand::SetToolboxDock(crate::toolrail::ToolboxDock::Bottom),
        RowId::ToolbarHidden => MenuCommand::ToggleToolbox,
        RowId::SaveDesk => MenuCommand::SaveWorkspace("menu"),
        RowId::TogglePanels => MenuCommand::ToggleDock,
        RowId::MarketReplay => MenuCommand::OpenReplayBrowser,
        RowId::CloseReplay => MenuCommand::CloseReplay,
        RowId::Settings => MenuCommand::OpenSettings(SettingsSection::Display),
        RowId::ExportDesk => MenuCommand::ExportDesk,
        RowId::ImportDesk => MenuCommand::ImportDesk,
        RowId::RevealDesk => MenuCommand::RevealDesk,
        RowId::ResetStartupDesk => MenuCommand::ForgetWorkspace,
        RowId::KeyboardShortcuts => MenuCommand::OpenHelp(HelpPage::KeyboardShortcuts),
        RowId::ReplayFileFormat => MenuCommand::OpenReplayFormatHelp,
        RowId::AboutWorkspaces => MenuCommand::OpenHelp(HelpPage::AboutWorkspaces),
        RowId::AboutQuantick => MenuCommand::OpenHelp(HelpPage::AboutQuantick),
        _ => return,
    };
    commands.push(command);
}
