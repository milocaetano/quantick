//! Window membership and strip order over process-owned tab identities.
//!
//! The arrangement retains every runtime. These lists order presentations, so
//! transferring a tab never removes, clones, closes or reconstructs a runtime.
use super::TabId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

impl WindowId {
    pub const ROOT: Self = Self(0);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketWindow {
    pub id: WindowId,
    tabs: Vec<TabId>,
    selected: TabId,
}

impl MarketWindow {
    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    pub const fn selected(&self) -> TabId {
        self.selected
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferTarget {
    Window(WindowId),
    NewWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowError {
    MissingTab,
    MissingWindow,
    InvalidPosition,
    StaleDrag,
    IdentityExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferResult {
    pub tab: TabId,
    pub source: WindowId,
    pub destination: WindowId,
    pub position: usize,
    pub changed: bool,
    pub closed_window: Option<WindowId>,
}

/// A press captures ownership without changing it. Dropping this token cancels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabDrag {
    workspace: crate::lifetime::WorkspaceId,
    tab: TabId,
    source: WindowId,
    revision: u64,
}

impl TabDrag {
    pub const fn tab(self) -> TabId {
        self.tab
    }

    pub const fn source(self) -> WindowId {
        self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabWindows {
    workspace: crate::lifetime::WorkspaceId,
    windows: Vec<MarketWindow>,
    focused: WindowId,
    presented: WindowId,
    next_id: Option<u64>,
    revision: u64,
}

impl TabWindows {
    pub fn new(workspace: crate::lifetime::WorkspaceId, first: TabId) -> Self {
        Self {
            workspace,
            windows: vec![MarketWindow {
                id: WindowId::ROOT,
                tabs: vec![first],
                selected: first,
            }],
            focused: WindowId::ROOT,
            presented: WindowId::ROOT,
            next_id: Some(1),
            revision: 0,
        }
    }

    pub fn windows(&self) -> &[MarketWindow] {
        &self.windows
    }

    pub const fn focused(&self) -> WindowId {
        self.focused
    }

    pub const fn presented(&self) -> WindowId {
        self.presented
    }

    pub fn window(&self, id: WindowId) -> Option<&MarketWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    pub fn owner(&self, tab: TabId) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|window| window.tabs.contains(&tab))
            .map(|window| window.id)
    }

    /// Select the presentation to paint, without changing keyboard focus.
    pub fn present(&mut self, id: WindowId) -> Result<TabId, WindowError> {
        let selected = self.window(id).ok_or(WindowError::MissingWindow)?.selected;
        self.presented = id;
        Ok(selected)
    }

    pub fn focus(&mut self, id: WindowId) -> Result<(), WindowError> {
        self.window(id).ok_or(WindowError::MissingWindow)?;
        self.focused = id;
        Ok(())
    }

    pub fn select(&mut self, tab: TabId) -> Result<(), WindowError> {
        let window = self
            .windows
            .iter_mut()
            .find(|window| window.tabs.contains(&tab))
            .ok_or(WindowError::MissingTab)?;
        window.selected = tab;
        self.presented = window.id;
        Ok(())
    }

    pub(super) fn append(&mut self, tab: TabId) {
        assert!(
            self.owner(tab).is_none(),
            "tab identity is already presented"
        );
        let window = self
            .windows
            .iter_mut()
            .find(|window| window.id == self.presented)
            .expect("a live presentation owns newly opened tabs");
        window.tabs.push(tab);
        window.selected = tab;
        self.changed();
    }

    pub(super) fn remove(&mut self, tab: TabId) {
        for window in &mut self.windows {
            if let Some(index) = window.tabs.iter().position(|id| *id == tab) {
                let selected = window
                    .tabs
                    .iter()
                    .position(|id| *id == window.selected)
                    .unwrap();
                window.tabs.remove(index);
                if !window.tabs.is_empty() {
                    // Preserve the existing close policy: keep the position,
                    // then clamp. Transfers preserve selected identity instead.
                    window.selected = window.tabs[selected.min(window.tabs.len() - 1)];
                }
                break;
            }
        }
        self.remove_empty();
        self.changed();
    }

    pub fn begin_drag(&self, tab: TabId) -> Result<TabDrag, WindowError> {
        Ok(TabDrag {
            workspace: self.workspace,
            tab,
            source: self.owner(tab).ok_or(WindowError::MissingTab)?,
            revision: self.revision,
        })
    }

    pub fn commit_drag(
        &mut self,
        drag: TabDrag,
        target: TransferTarget,
        position: usize,
    ) -> Result<TransferResult, WindowError> {
        if drag.workspace != self.workspace
            || drag.revision != self.revision
            || self.owner(drag.tab) != Some(drag.source)
        {
            return Err(WindowError::StaleDrag);
        }
        self.transfer(drag.tab, target, position)
    }

    /// `position` is the insertion index after excluding the transferred tab.
    /// All validation precedes changes; an invalid drop leaves the model intact.
    pub fn transfer(
        &mut self,
        tab: TabId,
        target: TransferTarget,
        position: usize,
    ) -> Result<TransferResult, WindowError> {
        let source = self.owner(tab).ok_or(WindowError::MissingTab)?;
        let destination = match target {
            TransferTarget::Window(id) => {
                let window = self.window(id).ok_or(WindowError::MissingWindow)?;
                let available = window.tabs.len() - usize::from(source == id);
                if position > available {
                    return Err(WindowError::InvalidPosition);
                }
                id
            }
            TransferTarget::NewWindow => {
                if position != 0 {
                    return Err(WindowError::InvalidPosition);
                }
                WindowId(self.next_id.ok_or(WindowError::IdentityExhausted)?)
            }
        };
        let source_window = self.windows.iter_mut().find(|w| w.id == source).unwrap();
        let previous = source_window.tabs.iter().position(|id| *id == tab).unwrap();
        if source == destination && previous == position {
            return Ok(TransferResult {
                tab,
                source,
                destination,
                position,
                changed: false,
                closed_window: None,
            });
        }
        source_window.tabs.remove(previous);
        if source != destination && source_window.selected == tab && !source_window.tabs.is_empty()
        {
            source_window.selected = source_window.tabs[previous.min(source_window.tabs.len() - 1)];
        }
        if matches!(target, TransferTarget::NewWindow) {
            self.next_id = destination.0.checked_add(1);
            self.windows.push(MarketWindow {
                id: destination,
                tabs: vec![tab],
                selected: tab,
            });
        } else {
            let window = self
                .windows
                .iter_mut()
                .find(|w| w.id == destination)
                .unwrap();
            window.tabs.insert(position, tab);
            if source != destination {
                window.selected = tab;
            }
        }
        let closed_window = self
            .window(source)
            .filter(|w| w.tabs.is_empty())
            .map(|w| w.id);
        self.focused = destination;
        self.presented = destination;
        self.remove_empty();
        self.changed();
        Ok(TransferResult {
            tab,
            source,
            destination,
            position,
            changed: true,
            closed_window,
        })
    }

    /// Closing a presentation keeps its live tabs in a surviving window.
    /// `None` means this is the final window: the caller may exit and save all tabs.
    pub fn close_window(&mut self, id: WindowId) -> Result<Option<WindowId>, WindowError> {
        let index = self
            .windows
            .iter()
            .position(|w| w.id == id)
            .ok_or(WindowError::MissingWindow)?;
        let Some(destination) = self.windows.iter().find(|w| w.id != id).map(|w| w.id) else {
            return Ok(None);
        };
        let closing = self.windows.remove(index);
        let survivor = self
            .windows
            .iter_mut()
            .find(|w| w.id == destination)
            .unwrap();
        survivor.tabs.extend(closing.tabs);
        if self.focused == id {
            self.focused = destination;
        }
        if self.presented == id {
            self.presented = destination;
        }
        self.changed();
        Ok(Some(destination))
    }

    fn remove_empty(&mut self) {
        self.windows.retain(|window| !window.tabs.is_empty());
        if let Some(first) = self.windows.first().map(|window| window.id) {
            if self.window(self.focused).is_none() {
                self.focused = first;
            }
            if self.window(self.presented).is_none() {
                self.presented = self.focused;
            }
        }
    }

    fn changed(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("window revision exhausted");
    }
}

#[cfg(test)]
mod tests;
