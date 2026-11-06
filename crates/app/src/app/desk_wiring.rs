//! `settings.open` and the `desk.*` calls land on the same acts as the `⋮` rows.

use eframe::egui;
use quantick_control_handlers::desk::DeskAct;
use quantick_control_schema::settings_sections::SettingsSection;

use super::workspace_save_adapter::WorkspaceSaveAdapter;
use super::{QuantickApp, menu_bar::WindowMenuState, workspace_runtime::WorkspaceRuntime};

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
            open_export(&mut app.runtime);
            free
        }
        DeskAct::PickImport => {
            let picker = app.runtime.workspace.picker_mut();
            let free = !picker.is_open();
            picker.open_import();
            free
        }
        DeskAct::ReadReopen => app.runtime.workspace.session().save_on_exit(),
        DeskAct::SetReopen(reopen) => write_reopen(app.workspace_save_adapter(), reopen),
    }
}

/// The status-bar clock's door: Settings, on Display, on the clicked window.
pub(super) fn open_settings_on(menu: &mut WindowMenuState, ctx: &egui::Context) {
    menu.open_settings(ctx.viewport_id(), SettingsSection::Display);
    ctx.request_repaint();
}

/// Raise the export dialog, suggesting the active chart's name.
pub(super) fn open_export(runtime: &mut WorkspaceRuntime) {
    let tab = &runtime.tabs[runtime.tabs.active_index()];
    let (symbol, spec) = (&tab.symbol, tab.flow_pane.state.spec());
    runtime.workspace.picker_mut().open_export(symbol, spec);
}

/// Change the choice and write only it, leaving the saved desk alone; `true`
/// once it reached the disk. A failed write takes the choice back, so memory and
/// the file never disagree and a retry writes again.
pub(super) fn write_reopen(mut save: WorkspaceSaveAdapter<'_>, reopen: bool) -> bool {
    let before = save.session.save_on_exit();
    save.session.set_save_on_exit(reopen);
    let written = save.write_standing_choices();
    if !written {
        save.session.set_save_on_exit(before);
    }
    written
}
