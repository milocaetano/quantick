//! `settings.open` and the `desk.*` calls land on the same acts as the `⋮` rows.

use eframe::egui;
use quantick_control_handlers::desk::DeskAct;
use quantick_control_schema::settings_sections::SettingsSection;

use super::QuantickApp;

/// Carry out one call; see [`quantick_control_handlers::desk::DeskHost`] for
/// what the answer means.
pub(super) fn apply(app: &mut QuantickApp, act: DeskAct) -> bool {
    match act {
        DeskAct::OpenSettings(section) => {
            let menu = &mut app.chrome.window_menu;
            let viewport = menu.windows_viewport.unwrap_or(egui::ViewportId::ROOT);
            menu.open_settings(viewport, section);
            true
        }
        DeskAct::Save => app.workspace_save_adapter().save_workspace("control"),
        // A file holding only standing choices has no desk to forget: the call
        // says nothing changed rather than claiming a write.
        DeskAct::Forget => {
            let saved = crate::ui_state::load(app.runtime.workspace.ui_state_path());
            !saved.tabs.is_empty() && app.workspace_save_adapter().forget_workspace()
        }
        DeskAct::PickExport => {
            let free = !app.runtime.workspace.picker_mut().is_open();
            open_export(app);
            free
        }
        DeskAct::PickImport => {
            let picker = app.runtime.workspace.picker_mut();
            let free = !picker.is_open();
            picker.open_import();
            free
        }
        DeskAct::ReadReopen => app.runtime.workspace.session().save_on_exit(),
        DeskAct::SetReopen(reopen) => write_reopen(app, reopen),
    }
}

/// The status-bar clock's door: Settings, on Display, on the clicked window.
pub(super) fn open_settings_on(app: &mut QuantickApp, ctx: &egui::Context) {
    let menu = &mut app.chrome.window_menu;
    menu.open_settings(ctx.viewport_id(), SettingsSection::Display);
    ctx.request_repaint();
}

/// Raise the export dialog, suggesting the active chart's name.
pub(super) fn open_export(app: &mut QuantickApp) {
    let tab = &app.runtime.tabs[app.runtime.tabs.active_index()];
    let (symbol, spec) = (&tab.symbol, tab.flow_pane.state.spec());
    app.runtime.workspace.picker_mut().open_export(symbol, spec);
}

/// Change the choice and write only it, leaving the saved desk alone; `true`
/// once it reached the disk. A failed write takes the choice back, so memory and
/// the file never disagree and a retry writes again.
pub(super) fn write_reopen(app: &mut QuantickApp, reopen: bool) -> bool {
    let session = app.runtime.workspace.session_mut();
    let before = session.save_on_exit();
    session.set_save_on_exit(reopen);
    let written = app.workspace_save_adapter().write_standing_choices();
    if !written {
        app.runtime.workspace.session_mut().set_save_on_exit(before);
    }
    written
}
