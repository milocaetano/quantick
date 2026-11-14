//! Catalog ownership of dock chrome; workspace retains every dock policy.
use quantick_workspace::dock::{Dock, DockTab};

#[derive(Debug, Default)]
pub struct State {
    dock: Dock,
}
impl State {
    pub const fn visible(&self) -> bool {
        self.dock.visible()
    }
    pub const fn tab(&self) -> Option<DockTab> {
        self.dock.tab()
    }
    pub const fn width(&self, tab: DockTab) -> f32 {
        self.dock.width(tab)
    }
}
#[derive(Debug)]
pub enum Intent {
    ToggleVisible,
    Restore { visible: bool, tab: Option<DockTab> },
    OpenTab(DockTab),
    Collapse,
    Select(DockTab),
    RememberWidth { tab: DockTab, width: f32 },
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = ();
    fn update(state: &mut State, intent: Intent) {
        match intent {
            Intent::ToggleVisible => state.dock.toggle_visible(),
            Intent::Restore { visible, tab } => state.dock.restore(visible, tab),
            Intent::OpenTab(tab) => state.dock.open_tab(tab),
            Intent::Collapse => state.dock.collapse(),
            Intent::Select(tab) => state.dock.select(tab),
            Intent::RememberWidth { tab, width } => state.dock.remember_width(tab, width),
        }
    }
}
#[cfg(test)]
#[path = "dock/tests.rs"]
mod tests;

/// Catalog-owner operations use the same intents as the native view.
impl<V, E> crate::Adapter<Model, V, E> {
    pub fn visible(&self) -> bool {
        self.model_state.visible()
    }
    pub fn tab(&self) -> Option<DockTab> {
        self.model_state.tab()
    }
    pub fn toggle_visible(&mut self) {
        self.update(Intent::ToggleVisible);
    }
    pub fn restore(&mut self, visible: bool, tab: Option<DockTab>) {
        self.update(Intent::Restore { visible, tab });
    }
    pub fn open_tab(&mut self, tab: DockTab) {
        self.update(Intent::OpenTab(tab));
    }
}
