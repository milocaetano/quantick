//! The facts a dispatch is judged against: what the window's tab holds right
//! now, read rather than owned.

use quantick_engine::Bar;
use quantick_engine::trade_tape::TradeTape;
use quantick_sources::config::{FeedCapabilities, ProviderKind};
use quantick_sources::history_reach::{Outage, ReachBounds};

/// One pane, as reaching back sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneView<'a> {
    /// The interval the pane cuts by time, `None` for a pane cut by trades.
    pub interval_ms: Option<i64>,
    /// When the pane's first print opened its bar, if it has one.
    pub first_print_open_ms: Option<i64>,
    /// The pane's first engine bar.
    pub first_bar: Option<&'a Bar>,
    /// The bar still forming.
    pub partial: Option<&'a Bar>,
    /// Whether a rebuild of older pages is still pending.
    pub rebuild_pending: bool,
    /// Whether that rebuild failed and waits for a retry.
    pub rebuild_failed: bool,
}

/// The tab, as one dispatch sees it.
pub struct TabView<'a> {
    /// The market, for the log.
    pub symbol: &'a str,
    /// What the running feed can do.
    pub capabilities: FeedCapabilities,
    /// Every pane holding this market's bars: flow first, then the context
    /// stack, in the order [`crate::Effect::InstallPrefix`] addresses them.
    pub panes: &'a [PaneView<'a>],
    /// The flow pane's tape, which a history run measures its reach against.
    pub tape: &'a TradeTape,
    /// Whether the opening session is still filling in behind the chart.
    pub opening_fill: bool,
    /// Whether a page of trade history is still owed a reply.
    pub history_wait: bool,
    /// Silences in the tape the feed marked as lost, oldest first.
    pub outages: Vec<Outage>,
    /// Which backend streams this tab's feed, when the config names one.
    pub provider: Option<ProviderKind>,
    /// The configured prints per page.
    pub page_step: usize,
    /// How far one run may reach.
    pub reach_bounds: ReachBounds,
}

impl TabView<'_> {
    /// The chart holds what it is going to hold: no opening fill arriving and
    /// no rebuild pending or failed, so a run judges what the trader sees. A
    /// failed rebuild waits for the press that retries it.
    ///
    /// A reconnect's resume floor does not count: on a quiet market no print
    /// comes to spend it, and the run's reply is told from the session's
    /// recovery window where it lands.
    #[must_use]
    pub fn history_idle(&self) -> bool {
        !self.opening_fill
            && !self.history_wait
            && !self
                .panes
                .iter()
                .any(|pane| pane.rebuild_pending || pane.rebuild_failed)
    }

    /// The prints one page asks for: the configured step, bounded by what the
    /// transport serves in one loop.
    #[must_use]
    pub fn page_size(&self) -> usize {
        self.provider.map_or(self.page_step.max(1), |provider| {
            provider.campaign_page_size(self.page_step)
        })
    }
}
