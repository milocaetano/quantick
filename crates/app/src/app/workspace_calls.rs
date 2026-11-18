//! The desk, Settings, REC, workspace book and the layout, paper and route
//! owners over the window fields they span, built by `workspace_calls!` and
//! lent to the control plane through the `DeskHost` and `WorkspaceBookHost` doors.

use std::time::Instant;

use eframe::egui;
use quantick_control_handlers::desk::{DeskAct, DeskOwner};
use quantick_control_handlers::workspace_book::{self as ops, WorkspaceOwner};
use quantick_session::workspace_menu::{WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::{market_settings::MarketSettingsStore, workspaces::WorkspaceBookStore};
use quantick_workspace::book_upkeep;

use super::indicator_wiring::IndicatorWiring;
use super::layout_wiring::LayoutAdapter;
use super::workspace_runtime::{WindowPresentation, WorkspaceRuntime};

/// The window's fields the workspace calls reach, borrowed apart.
pub(crate) struct WorkspaceCalls<'a> {
    pub(super) runtime: &'a mut WorkspaceRuntime,
    pub(super) presentation: &'a WindowPresentation,
    pub(super) chrome: &'a mut super::chrome::ChromeState,
    pub(super) pane_ids: &'a mut crate::canvas_layout::PaneIdAllocator,
    pub(super) config: &'a crate::config::AppConfig,
    pub(super) control: &'a mut super::control_host::ControlState,
    pub(super) surfaces: &'a mut crate::surfaces::Surfaces,
    pub(super) health: &'a mut super::health::HealthCounters,
    pub(super) drawings: &'a mut super::drawing_controller::DrawingController,
}

impl<'a> WorkspaceCalls<'a> {
    pub(crate) fn layout(&mut self) -> LayoutAdapter<'_> {
        IndicatorWiring {
            runtime: self.runtime.bound_mut(&self.presentation.binding),
            surfaces: self.surfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: &mut self.chrome.harness,
        }
        .into_layout()
    }

    fn save_adapter(&mut self) -> super::workspace_save_adapter::WorkspaceSaveAdapter<'_> {
        arrangement_adapter!(self).into_save(&self.surfaces.replay_browser)
    }

    /// What the frame owes the book, once its panes are built and seeded; with
    /// autosave on, the quiet edits are written through.
    pub(super) fn maintain(&mut self) {
        self.layout().own_indicator_copies();
        let upkeep = book_upkeep::maintain(self, Instant::now());
        self.runtime.workspace.book.save_upkeep(upkeep.save);
        let due = upkeep.write_through;
        if !due.is_empty() {
            self.as_hand(|calls, to| ops::write_through(calls, to, &due));
        }
    }

    /// Run `act` journaled as the trader's hand, toasting each note it answers.
    pub(super) fn as_hand<I: IntoIterator<Item = String>>(
        &mut self,
        act: impl FnOnce(&mut Self, ops::Journal<'_, crate::control::ControlAccess>) -> I,
    ) {
        let mut access = self.control.control_access.take();
        let hand = access.as_mut().and_then(|access| access.hand_actor());
        let notes = act(self, access.as_mut().zip(hand.as_ref()));
        self.control.control_access = access;
        for message in notes {
            crate::surfaces::toast::note(&mut self.surfaces.toast, message, Instant::now());
        }
    }

    /// Carry out what a workspace row or the toolbar asked for, on what the
    /// active tab shows now, journaled as the trader's hand.
    pub(super) fn apply_workspace_command(&mut self, command: WorkspaceCommand) {
        let tab = self.runtime.tabs.active_id();
        book_upkeep::watch_tab(self, tab, Instant::now());
        let symbol = self.runtime.active_tab().symbol.clone();
        let mut menu = std::mem::take(&mut self.chrome.window_menu.tab_workspace);
        self.as_hand(|calls, to| {
            ops::apply_row_command(calls, to, &mut menu, tab, &symbol, command)
        });
        self.chrome.window_menu.tab_workspace = menu;
    }

    /// Close the market tab at `index` without asking.
    fn close_tab_at(&mut self, index: usize) -> bool {
        let close = crate::tabstrip::TabAction::Close(index);
        let asked = arrangement_adapter!(self).apply_tab_action(close);
        let closed = asked.1.is_some();
        super::menu_bar::settle_strip(&mut self.surfaces.market_palette, self.runtime.tz, asked);
        closed
    }
}

/// `settings.open` and the `desk.*` calls land on the same acts as the `⋮` rows.
impl DeskOwner for WorkspaceCalls<'_> {
    fn act(&mut self, act: DeskAct) -> bool {
        match act {
            DeskAct::OpenSettings(section) => {
                let menu = &mut self.chrome.window_menu;
                let viewport = menu.windows_viewport.unwrap_or(egui::ViewportId::ROOT);
                let settings = &mut self.surfaces.settings_window;
                super::chrome::settings::open(menu, settings, viewport, section);
                true
            }
            DeskAct::Save(reason) => self.save_adapter().save_workspace(reason),
            // A file holding only standing choices has no desk to forget: the
            // call says nothing changed rather than claiming a write.
            DeskAct::Forget => {
                let saved = crate::ui_state::load(self.runtime.workspace.ui_state_path());
                !saved.tabs.is_empty() && self.save_adapter().forget_workspace()
            }
            DeskAct::PickExport => {
                let free = !self.runtime.workspace.picker_mut().is_open();
                self.runtime.open_export();
                free
            }
            DeskAct::PickImport => {
                let picker = self.runtime.workspace.picker_mut();
                let free = !picker.is_open();
                picker.open_import();
                free
            }
            DeskAct::ReadReopen => self.runtime.workspace.session().save_on_exit(),
            DeskAct::SetReopen(reopen) => self.save_adapter().write_reopen(reopen),
        }
    }
}

impl WorkspaceOwner for WorkspaceCalls<'_> {
    fn workspace_store(&mut self) -> Option<&mut WorkspaceBookStore> {
        Some(&mut self.runtime.workspace.book)
    }
    fn workspace_store_ref(&self) -> Option<&WorkspaceBookStore> {
        Some(&self.runtime.workspace.book)
    }
    fn workspace_market(&mut self) -> Option<&mut MarketSettingsStore> {
        Some(self.runtime.workspace.layouts_mut().market_mut())
    }
    fn show_tab_recipe(&mut self, tab: u64, recipe: &quantick_stores::workspaces::WorkspaceRecipe) {
        book_upkeep::show_recipe(self, tab, recipe);
    }
    fn persist_workspace_autosave(&mut self, on: bool) -> Result<(), String> {
        self.save_adapter().write_auto_save(on)
    }
    fn close_market_tab(&mut self, tab: u64, close: bool) -> bool {
        let index = self.runtime.tabs.position(tab);
        // Without asking; with `close` false it only answers whether it would.
        let Some(index) = index else { return false };
        if !close {
            return self.runtime.tabs.plan_close(index).is_ok();
        }
        self.close_tab_at(index)
    }
    fn workspace_menu(&mut self) -> Option<(&mut WorkspaceMenuState, bool)> {
        let dark = self.runtime.appearance.resolved().dark;
        Some((&mut self.chrome.window_menu.tab_workspace, dark))
    }
    fn tab_symbols(&self, tabs: &[u64]) -> Vec<String> {
        super::tab_workspaces::symbols_of(&self.runtime.tabs, tabs)
    }
}

impl quantick_control_handlers::TabsHost for WorkspaceCalls<'_> {
    fn tab_list(&self) -> &dyn quantick_control_handlers::TabDirectory {
        &self.runtime.tabs
    }
}
