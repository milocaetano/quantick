//! `settings.*` and the `desk.*` calls land on the same acts as the `⋮` rows,
//! through the window's [`DeskAdapter`].

use eframe::egui;
use quantick_control_handlers::desk::{DeskAct, DeskOwner};

use super::workspace_runtime::{WindowPresentation, WorkspaceRuntime};
use super::workspace_save_adapter::WorkspaceSaveAdapter;

/// The window's Settings, desk and REC owner: the fields its acts touch, lent
/// one by one by `desk_adapter!`.
pub(super) struct DeskAdapter<'a> {
    pub(super) runtime: &'a mut WorkspaceRuntime,
    pub(super) presentation: &'a WindowPresentation,
    pub(super) chrome: &'a mut super::chrome::ChromeState,
    pub(super) pane_ids: &'a mut crate::canvas_layout::PaneIdAllocator,
    pub(super) config: &'a crate::config::AppConfig,
    pub(super) control: &'a mut super::control_host::ControlState,
    pub(super) surfaces: &'a mut crate::surfaces::Surfaces,
    pub(super) health: &'a mut super::health::HealthCounters,
    pub(super) drawings: &'a mut super::drawing_controller::DrawingController,
}

impl DeskAdapter<'_> {
    fn save(&mut self) -> WorkspaceSaveAdapter<'_> {
        arrangement_adapter!(self).into_save(&self.surfaces.replay_browser)
    }
}

impl DeskOwner for DeskAdapter<'_> {
    fn act(&mut self, act: DeskAct) -> bool {
        match act {
            DeskAct::OpenSettings(section) => {
                let viewport =
                    (self.chrome.window_menu.windows_viewport).unwrap_or(egui::ViewportId::ROOT);
                super::chrome::settings::open(self, viewport, section);
                true
            }
            DeskAct::Save => self.save().save_workspace("control"),
            // A file holding only standing choices has no desk to forget: the call
            // says nothing changed rather than claiming a write.
            DeskAct::Forget => {
                let saved = crate::ui_state::load(self.runtime.workspace.ui_state_path());
                !saved.tabs.is_empty() && self.save().forget_workspace()
            }
            DeskAct::PickExport => {
                let free = !self.runtime.workspace.picker_mut().is_open();
                open_export(self.runtime);
                free
            }
            DeskAct::PickImport => {
                let picker = self.runtime.workspace.picker_mut();
                let free = !picker.is_open();
                picker.open_import();
                free
            }
            DeskAct::ReadReopen => self.runtime.workspace.session().save_on_exit(),
            DeskAct::SetReopen(reopen) => write_reopen(self.save(), reopen),
        }
    }
}

/// Raise the export dialog, suggesting the active chart's name.
pub(super) fn open_export(runtime: &mut WorkspaceRuntime) {
    let tab = &runtime.tabs[runtime.tabs.active_index()];
    let (symbol, spec) = (&tab.symbol, tab.flow_pane.series.state.spec());
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
