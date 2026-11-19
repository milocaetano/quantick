//! The tab as its feed and market calls read and change it.
use super::Tab;
use crate::config::AppConfig;
use quantick_control_handlers::contract_roll::AutoRoll;
use quantick_control_handlers::market_tab::{FeedSelection, MarketTab, RecoveredFeed};
use quantick_control_handlers::script_strategy::{ScriptPane, ScriptTab};
use quantick_feed::stall::Recovery;
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot};

impl MarketTab for Tab {
    fn market(&self) -> FeedSelection {
        FeedSelection {
            feed_id: self.active.0.clone(),
            symbol: self.active.1.clone(),
            replay: self.link.replay.is_some(),
        }
    }
    fn switch_market(&mut self, config: &AppConfig, feed_id: &str, symbol: &str) {
        (self.feed_id, self.symbol) = (feed_id.to_owned(), symbol.to_owned());
        self.maybe_switch_feed(config);
    }
    fn paper_open(&self) -> bool {
        !self.paper.is_flat()
    }
    fn unlisted(&self) -> Option<String> {
        self.link.notice.unlisted_symbol().map(str::to_owned)
    }
    fn roll_memory(&mut self) -> &mut AutoRoll {
        &mut self.link.auto_roll
    }
    fn recover(&mut self, config: &AppConfig, recovery: Recovery) -> RecoveredFeed {
        let respawned = match recovery {
            Recovery::Reconnect => self.reconnect_feed(config),
            Recovery::Reload => self.reload_feed(config),
        };
        RecoveredFeed {
            symbol: self.symbol.clone(),
            respawned,
        }
    }
    fn pages_history(&self, config: &AppConfig) -> bool {
        self.capabilities(config).history_paging
    }
    fn history(&mut self, config: &AppConfig, act: HistoryAct) -> HistoryOutcome {
        match act {
            HistoryAct::Press(reach) => HistoryOutcome::Pressed(self.load_history(config, reach)),
            HistoryAct::Cancel => HistoryOutcome::Cancelled(self.cancel_history()),
        }
    }
    fn session(&self) -> SessionSnapshot {
        self.session.snapshot()
    }
    fn script_tab(&mut self) -> Option<ScriptTab<'_>> {
        let panes = std::iter::once(&self.flow_pane).chain(&self.time_panes);
        Some(ScriptTab {
            library: crate::launch::operator_paths().strategies_dir.as_deref(),
            panes: (panes.map(|pane| ScriptPane {
                id: pane.id,
                series: pane.series_read(),
                drawings: &pane.drawings,
            }))
            .collect(),
            strategies: &mut self.market.strategies,
        })
    }
}
