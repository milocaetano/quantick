//! The window's one pane job: resolve the pane a call addresses. Everything
//! else is the pane's, through each family's narrow port: the axes and the
//! native tape ([`crate::axis_pane`]), the marks ([`crate::pane_marks`]), the
//! tape's size, fold, view and opening scale.
use quantick_control::{error::ControlError, wire::WireU64};

/// The window's pane job: resolve the addressed pane. Each family bounds
/// [`Self::Pane`] by the narrow port it acts through.
pub trait AddressedPanePort {
    /// The pane, as each family's port acts on it.
    type Pane: ?Sized;
    /// The pane `tab_id` and `pane_id` name.
    ///
    /// # Errors
    /// An unknown tab or pane.
    fn addressed_pane(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut Self::Pane, ControlError>;
}
