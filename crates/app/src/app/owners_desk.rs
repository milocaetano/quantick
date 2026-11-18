//! The window's owners, lent apart: the one door the launch phase and the
//! harness hooks take to the state they stage.
//!
//! A hook borrows the owners it needs and calls their own methods; nothing
//! here decides anything. A hook that runs a registered call names the
//! control families it uses (`GatewayPort`, `TabsPort`) beside this trait,
//! so what reaches the window stays visible at the signature.

use super::QuantickApp;
use super::indicator_wiring::IndicatorWiring;
use super::layout_wiring::LayoutAdapter;
use super::workspace_runtime::WorkspaceRuntime;

/// Lends the window's owners, and nothing else of it.
pub(crate) trait OwnersDesk {
    /// The owners, the runtime bound to this window's view.
    fn owners(&mut self) -> WindowOwners<'_>;
    /// Export and import by path, past the dialog no scripted run can drive:
    /// no registered call takes a path, by design.
    #[cfg(any(feature = "scenario-harness", test))]
    fn workspace_file(&mut self) -> super::workspace_bundle_adapter::WorkspaceBundleAdapter<'_>;
    /// The layout book, as the strip's own clicks reach it.
    fn layouts(&mut self) -> LayoutAdapter<'_> {
        self.owners().into_wiring().into_layout()
    }
}

/// The window's owners, lent apart so one hook can hold two at once.
pub(crate) struct WindowOwners<'a> {
    pub(super) runtime: &'a mut WorkspaceRuntime,
    pub(super) surfaces: &'a mut crate::surfaces::Surfaces,
    pub(super) chrome: &'a mut super::chrome::ChromeState,
    #[cfg(any(feature = "control-harness", test))]
    pub(super) control: &'a mut super::control_host::ControlState,
    pub(super) drawings: &'a mut super::drawing_controller::DrawingController,
    pub(super) audio: &'a mut super::replay_and_history::AlertState,
    pub(super) config: &'a crate::config::AppConfig,
}

impl<'a> WindowOwners<'a> {
    /// The indicator and layout ports these owners hold.
    pub(super) fn into_wiring(self) -> IndicatorWiring<'a> {
        IndicatorWiring {
            runtime: self.runtime,
            surfaces: self.surfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: &mut self.chrome.harness,
        }
    }
}

impl OwnersDesk for QuantickApp {
    fn owners(&mut self) -> WindowOwners<'_> {
        WindowOwners {
            runtime: self.runtime.bound_mut(&self.presentation.binding),
            surfaces: &mut self.surfaces,
            chrome: &mut self.chrome,
            #[cfg(any(feature = "control-harness", test))]
            control: &mut self.control,
            drawings: &mut self.drawings,
            audio: &mut self.audio,
            config: &self.config,
        }
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn workspace_file(&mut self) -> super::workspace_bundle_adapter::WorkspaceBundleAdapter<'_> {
        self.workspace_bundle_adapter()
    }
}
