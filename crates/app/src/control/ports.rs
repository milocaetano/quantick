//! The window's adapter onto the tab-scoped and attention ports of
//! `quantick_control_handlers`: each act is the one the window's own button
//! makes, on the window as the gateway holds it.

use quantick_control_handlers::{
    TabDirectory,
    history::{HistoryPort, HistoryRun},
    notify::AttentionPort,
    recovery::{FeedRecoveryPort, RecoveredFeed},
};
use quantick_control_schema::notify::AgentPopup;
use quantick_feed::history_reach::HistoryReach;
use quantick_feed::history_run::{Cancelled, Press};

use crate::{app::control_host::ControlPort, tab::Tab};

impl TabDirectory for dyn ControlPort {
    fn active_tab_index(&self) -> usize {
        self.tab_reads().active_tab_index()
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tab_reads().tabs().position(tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tab_reads().tabs().id_at(index)
    }
}

impl FeedRecoveryPort for dyn ControlPort {
    fn recover_feed(&mut self, index: usize, keep_timeline: bool) -> Option<RecoveredFeed> {
        let (tab, config) = self.tabs_mut().tab_with_config(index)?;
        let respawned = if keep_timeline {
            tab.reconnect_feed(config)
        } else {
            tab.reload_feed(config)
        };
        Some(RecoveredFeed {
            symbol: tab.symbol.clone(),
            respawned,
        })
    }
}

impl HistoryPort for dyn ControlPort {
    fn history_paging(&self, index: usize) -> Option<bool> {
        let reads = self.tab_reads();
        let tab = reads.tab_at(index)?;
        Some(tab.capabilities(reads.config()).history_paging)
    }
    fn press_history(&mut self, index: usize, reach: HistoryReach) -> Option<(Press, HistoryRun)> {
        let (tab, press) = self.tabs_mut().press_history(index, reach)?;
        Some((press, run(tab)))
    }
    fn cancel_history(&mut self, index: usize) -> Option<(Cancelled, HistoryRun)> {
        let tab = self.tabs_mut().tab_at_mut(index)?;
        Some((tab.cancel_history(), run(tab)))
    }
}

fn run(tab: &Tab) -> HistoryRun {
    HistoryRun {
        status: tab.history_status(),
        main_reach: tab.main_history_reach(),
    }
}

impl AttentionPort for dyn ControlPort {
    fn show_popup(&mut self, popup: AgentPopup) {
        self.alerts().show_popup(popup);
    }
    fn show_toast(&mut self, message: String) {
        self.alerts().show_toast(message);
    }
    fn sound_alert(&mut self) -> Option<String> {
        self.alerts().sound_alert()
    }
}
