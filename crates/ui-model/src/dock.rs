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
/// Host outputs of the complete borrowed paper-panel pass.
#[derive(Default, Debug, PartialEq)]
pub struct PaperPanelEffects {
    /// The rail's bell: open the alert manager.
    pub open_alerts: bool,
    pub close_replay: bool,
    pub open_replay_browser: bool,
    pub navigate_to_trade: Option<(i64, i64)>,
    pub journal_edit: Option<quantick_paper::report::session_edit::JournalEdit>,
    pub pick_trades_dir: bool,
    pub cmd_trading_changed: bool,
    pub order_strategies_changed: bool,
    pub risk_settings_changed: bool,
    /// A press on the account route block.
    pub route: Option<quantick_paper::route::RoutePress>,
}
impl PaperPanelEffects {
    /// Fold a later pass in: a set flag stays set, the first value wins.
    pub fn merge(&mut self, other: Self) {
        self.open_alerts |= other.open_alerts;
        self.close_replay |= other.close_replay;
        self.open_replay_browser |= other.open_replay_browser;
        self.navigate_to_trade = self.navigate_to_trade.or(other.navigate_to_trade);
        self.journal_edit = self.journal_edit.take().or(other.journal_edit);
        self.pick_trades_dir |= other.pick_trades_dir;
        self.cmd_trading_changed |= other.cmd_trading_changed;
        self.order_strategies_changed |= other.order_strategies_changed;
        self.risk_settings_changed |= other.risk_settings_changed;
        self.route = self.route.or(other.route);
    }
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
    /// Show the dock with its panel collapsed, leaving the edge to a manager.
    pub fn reveal_collapsed(&mut self) {
        if !self.visible() {
            self.toggle_visible();
        }
        self.update(Intent::Collapse);
    }
    pub fn restore(&mut self, visible: bool, tab: Option<DockTab>) {
        self.update(Intent::Restore { visible, tab });
    }
    pub fn open_tab(&mut self, tab: DockTab) {
        self.update(Intent::OpenTab(tab));
    }
}
