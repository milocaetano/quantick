//! Bind presentation decisions to the arrangement's retained runtime store.
use super::{Arrangement, Command, Entries, TabRuntime};
use crate::arrangement::{
    TabId,
    windows::{TabDrag, TabWindows, TransferResult, TransferTarget, WindowError, WindowId},
};

impl<R> Arrangement<R> {
    pub const fn windows(&self) -> &TabWindows {
        &self.windows
    }

    /// Physical runtime indices in the strip order of one native window.
    pub fn window_indices(&self, window: WindowId) -> impl Iterator<Item = usize> + '_ {
        self.windows
            .window(window)
            .into_iter()
            .flat_map(|window| window.tabs())
            .map(|tab| {
                self.position(tab.0)
                    .expect("presentation references an owned runtime")
            })
    }

    pub fn presented_index(&self) -> usize {
        let window = self
            .windows
            .window(self.windows.presented())
            .expect("live presentation");
        window
            .tabs()
            .iter()
            .position(|tab| *tab == window.selected())
            .expect("selected window tab")
    }

    pub fn focus_window(&mut self, window: WindowId) -> Result<(), WindowError> {
        self.windows.focus(window)
    }
}

impl<R: TabRuntime> Arrangement<R> {
    pub fn present_window(&mut self, window: WindowId) -> Result<(), WindowError> {
        self.windows.present(window)?;
        self.align_presented_selection();
        Ok(())
    }

    pub fn transfer_tab(
        &mut self,
        tab: TabId,
        target: TransferTarget,
        position: usize,
    ) -> Result<TransferResult, WindowError> {
        let result = self.windows.transfer(tab, target, position)?;
        self.align_presented_selection();
        Ok(result)
    }

    pub fn commit_tab_drag(
        &mut self,
        drag: TabDrag,
        target: TransferTarget,
        position: usize,
    ) -> Result<TransferResult, WindowError> {
        let result = self.windows.commit_drag(drag, target, position)?;
        self.align_presented_selection();
        Ok(result)
    }

    pub fn close_window(&mut self, window: WindowId) -> Result<Option<WindowId>, WindowError> {
        let destination = self.windows.close_window(window)?;
        self.align_presented_selection();
        Ok(destination)
    }

    pub(super) fn align_presented_selection(&mut self) {
        let tab = self
            .windows
            .present(self.windows.presented())
            .expect("live presentation");
        let index = self.position(tab.0).expect("owned window selection");
        if self.lifecycle.active_index() != index {
            let plan = self
                .lifecycle
                .plan(Command::Select(index), &Entries(&self.runtimes))
                .expect("owned selection");
            self.lifecycle
                .commit(plan, &Entries(&self.runtimes))
                .expect("selection does not change runtimes");
        }
    }
}
