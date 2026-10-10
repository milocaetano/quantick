//! Which tab a call names: the one port every tab-scoped family shares.

use quantick_control::{error::ControlError, wire::WireU64};

/// The window's tabs as a call addresses them: by stable id, by position,
/// and the one the trader is looking at.
pub trait TabDirectory {
    /// The position of the tab on screen.
    fn active_tab_index(&self) -> usize;
    /// The position of the open tab with `tab_id`, if one is open.
    fn tab_position(&self, tab_id: u64) -> Option<usize>;
    /// The stable id of the tab at `index`, a position this directory gave.
    fn tab_id_at(&self, index: usize) -> u64;
}

/// Which tab a call named, or the one the trader is looking at.
pub fn tab_index<P: TabDirectory + ?Sized>(
    app: &P,
    tab_id: Option<WireU64>,
) -> Result<usize, ControlError> {
    let Some(id) = tab_id else {
        return Ok(app.active_tab_index());
    };
    app.tab_position(id.get())
        .ok_or_else(|| ControlError::invalid_request(format!("no open tab has id {}", id.get())))
}

/// The refusal a call gets when the tab it resolved is gone by the time it
/// acts — the words every tab-scoped action uses.
pub(crate) fn tab_closed() -> ControlError {
    ControlError::invalid_request("the tab closed while the call ran")
}
