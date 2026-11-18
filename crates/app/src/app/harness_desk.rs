//! The one door the launch phase and the harness hooks take into the window.
//!
//! A hook stages state its owners already know how to take: it borrows them
//! here, split so it can hold two at once, and calls their own methods. What
//! a registered capability stands for runs as that call through the
//! [`ControlPort`] supertrait instead. Nothing here decides anything.

use super::QuantickApp;
use super::control_host::ControlPort;
use super::indicator_wiring::IndicatorWiring;
use super::layout_wiring::LayoutAdapter;
use super::workspace_runtime::WorkspaceRuntime;

/// What a launch or harness hook may borrow from the window.
pub(crate) trait HarnessDesk: ControlPort {
    /// The owners a hook stages, the runtime bound to this window's view.
    fn owners(&mut self) -> HarnessOwners<'_>;
    /// The workspace file, as the File menu's entries reach it.
    #[cfg(any(feature = "scenario-harness", test))]
    fn workspace_bundle(&mut self) -> super::workspace_bundle_adapter::WorkspaceBundleAdapter<'_>;
}

/// The window's owners, lent apart.
pub(crate) struct HarnessOwners<'a> {
    pub(super) runtime: &'a mut WorkspaceRuntime,
    pub(super) surfaces: &'a mut crate::surfaces::Surfaces,
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) chrome: &'a mut super::chrome::ChromeState,
    #[cfg(any(feature = "control-harness", test))]
    pub(super) control: &'a mut super::control_host::ControlState,
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) audio: &'a mut super::replay_and_history::AlertState,
}

impl HarnessOwners<'_> {
    /// The layout book, as the strip's own clicks reach it.
    pub(super) fn layouts(&mut self) -> LayoutAdapter<'_> {
        IndicatorWiring {
            runtime: &mut *self.runtime,
            surfaces: &mut *self.surfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: &mut self.chrome.harness,
        }
        .into_layout()
    }
}

impl HarnessDesk for QuantickApp {
    fn owners(&mut self) -> HarnessOwners<'_> {
        HarnessOwners {
            runtime: self.runtime.bound_mut(&self.presentation.binding),
            surfaces: &mut self.surfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            chrome: &mut self.chrome,
            #[cfg(any(feature = "control-harness", test))]
            control: &mut self.control,
            #[cfg(any(feature = "scenario-harness", test))]
            audio: &mut self.audio,
        }
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn workspace_bundle(&mut self) -> super::workspace_bundle_adapter::WorkspaceBundleAdapter<'_> {
        self.workspace_bundle_adapter()
    }
}
