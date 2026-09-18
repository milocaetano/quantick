//! Workspace picker event routing and remaining independent window/toast hooks.
use super::QuantickApp;
use crate::workspace_picker::PickerOutcome;
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

    /// Post a Workspace-menu answer through the window's one acknowledgement
    /// channel ([`Toast`]).
    ///
    /// No Undo: the file it replaced is gone, and `Reset startup layout` is
    /// the honest way back rather than a button that pretends otherwise.
    pub(super) fn note_workspace(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }
}
