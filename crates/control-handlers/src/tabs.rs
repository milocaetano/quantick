//! Which tab a call names: the one port every tab-scoped family shares.

use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_host::journal::NewEvent;

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

/// The open tabs in screen order answer for themselves.
impl<R> TabDirectory for quantick_workspace::arrangement::Arrangement<R> {
    fn active_tab_index(&self) -> usize {
        self.active_index()
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.position(tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.id_at(index)
    }
}

/// The window's open tabs, each lent as the window's own tab type, beside the
/// config they read and the drawing rail's magnet: what the session,
/// analysis and mark scopes walk. Each scope names the trait it reads a tab
/// through, and the tab implements it.
pub trait OpenTabs: TabDirectory {
    type Tab;
    fn tab_count(&self) -> usize;
    /// The tab at `index`, a position the directory gave.
    fn open_tab(&self, index: usize) -> Option<&Self::Tab>;
    fn config(&self) -> &quantick_stores::config::AppConfig;
    /// Whether the tool rail's magnet is on, as the drawings scope publishes
    /// it beside the tabs' marks.
    fn rail_magnet(&self) -> bool;
}

/// One row per open tab in screen order, from the tab and its stable id.
pub(crate) fn per_open_tab<P, T>(app: &P, mut row: impl FnMut(u64, &P::Tab) -> T) -> Vec<T>
where
    P: OpenTabs + ?Sized,
{
    (0..app.tab_count())
        .filter_map(|index| Some(row(app.tab_id_at(index), app.open_tab(index)?)))
        .collect()
}

/// Which tab a call named, or the one the trader is looking at.
pub fn tab_index<P: TabDirectory + ?Sized>(
    app: &P,
    tab_id: Option<WireU64>,
) -> Result<usize, ControlError> {
    let Some(id) = tab_id else {
        return Ok(app.active_tab_index());
    };
    app.tab_position(id.get()).ok_or_else(|| no_tab(id.get()))
}

/// The refusal a call gets when it names a tab id no open tab has.
pub(crate) fn no_tab(tab_id: u64) -> ControlError {
    ControlError::invalid_request(format!("no open tab has id {tab_id}"))
}

/// The refusal a call gets when the tab it resolved is gone by the time it
/// acts — the words every tab-scoped action uses.
pub(crate) fn tab_closed() -> ControlError {
    ControlError::invalid_request("the tab closed while the call ran")
}

/// What an action needs of the control access it journals through.
pub trait JournalAccess {
    /// Journal one event, stamped by the access's own clock; answers its
    /// sequence.
    fn record_event(&mut self, event: NewEvent) -> WireU64;
    /// The access's clock, for a command that must be stamped as it runs;
    /// a handler is told the time, it never reads one.
    fn now_unix_ms(&self) -> i64;
}
