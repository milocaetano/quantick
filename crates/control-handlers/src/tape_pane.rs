//! The tape families' one window port: the window resolves the addressed
//! pane, and the pane serves its own size, fold, view and opening-scale calls
//! through each family's narrow port.
use quantick_control::{error::ControlError, wire::WireU64};

/// The window's tape job: resolve the addressed pane. Everything else is the
/// pane's.
pub trait PaneTapePort {
    /// The pane, as each tape family's port acts on it.
    type Pane: ?Sized;
    /// The pane `tab_id` and `pane_id` name.
    fn pane_tape(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut Self::Pane, ControlError>;
}
