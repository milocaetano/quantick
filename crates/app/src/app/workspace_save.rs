//! Workspace picker event routing and remaining independent window/toast hooks.
use super::QuantickApp;
use crate::workspace_picker::PickerOutcome;
use eframe::egui;
use std::time::Instant;

impl QuantickApp {
    pub(super) fn poll_workspace_picker(&mut self) {
        match self.workspace.picker_mut().poll() {
            PickerOutcome::Idle | PickerOutcome::Cancelled => {}
            PickerOutcome::Lost => {
                super::workspace_bundle_adapter::report_picker_lost(&mut self.surfaces.toast)
            }
            PickerOutcome::Chosen { intent, path } => {
                self.workspace_bundle_adapter().dispatch(intent, &path)
            }
        }
    }

    /// Take the window manager's own maximise, once, on the first frame.
    ///
    /// Through [`egui::ViewportCommand::Maximized`] rather than the viewport
    /// builder's `with_maximized`: eframe 0.29 does not honour that flag beside
    /// an `inner_size`, and a hook that silently opens a 1100×650 window while
    /// reporting success is worse than no hook — a validation run would
    /// photograph the wrong state and call it a pass. The command is the one
    /// the platform runs when a hand hits the title bar, which is the state
    /// this hook exists to reach.
    pub(super) fn apply_maximize_hook(&mut self, ctx: &egui::Context) {
        if !self.harness.take_maximize() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "WINDOW_MAXIMIZE_AUTOSTART",
            action = "maximize",
            "QUANTICK_WINDOW_MAXIMIZED asked for the maximised layout"
        );
    }

    /// Post a Workspace-menu answer through the window's one acknowledgement
    /// channel ([`Toast`]).
    ///
    /// No Undo: the file it replaced is gone, and `Reset startup layout` is
    /// the honest way back rather than a button that pretends otherwise.
    pub(super) fn note_workspace(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }
}
