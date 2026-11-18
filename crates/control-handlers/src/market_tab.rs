//! The tab-scoped feed and market calls' one port: the window resolves the
//! addressed tab and lends its feed table, its drawings and its History
//! default; the tab answers the rest, and the feed table answers what the
//! config names.
use quantick_control_schema::feed_source::{FeedSourceChoice, choices};
use quantick_session::{HistoryAct, HistoryOutcome, HistoryReply, sets_window_default};
use quantick_sources::history_reach::HistoryReach;
use quantick_sources::recovery::AutoRoll;
use quantick_stores::config::AppConfig;

use crate::tabs::TabDirectory;

/// The pair a tab shows, and whether it plays a recording.
pub struct FeedSelection {
    pub feed_id: String,
    pub symbol: String,
    pub replay: bool,
}

/// What a tab did when asked to recover its feed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveredFeed {
    /// The market the tab shows.
    pub symbol: String,
    /// Whether a feed session was actually spawned: a recorded session owns
    /// the chart while it plays, and a feed id that left the feed table has
    /// nothing to spawn.
    pub respawned: bool,
}

/// What the config says about feeds: the pairs a tab may switch to and the
/// contract that follows another.
pub trait FeedTable {
    fn source_choices(&self) -> Vec<FeedSourceChoice>;
    /// The contract the feed's config names after `symbol`.
    fn next_contract(&self, feed_id: &str, symbol: &str) -> Option<String>;
    /// Whether feed `feed_id` lists `symbol`.
    fn feed_offers(&self, feed_id: &str, symbol: &str) -> bool;
}

impl FeedTable for AppConfig {
    fn source_choices(&self) -> Vec<FeedSourceChoice> {
        choices(self)
    }
    fn next_contract(&self, feed_id: &str, symbol: &str) -> Option<String> {
        self.feed(feed_id)?.next_contract(symbol).map(str::to_owned)
    }
    fn feed_offers(&self, feed_id: &str, symbol: &str) -> bool {
        self.feed(feed_id)
            .is_some_and(|feed| feed.symbols.iter().any(|offered| offered == symbol))
    }
}

/// One tab, as its feed and market calls read and change it, given the
/// window's feed table `C`. Its names differ from the tab's own methods, so
/// an implementation that forwards to one cannot fall back on itself.
pub trait MarketTab<C> {
    /// The pair the tab shows, and whether it plays a recording.
    fn market(&self) -> FeedSelection;
    /// Put the tab on the validated pair, through its own source switch.
    fn switch_market(&mut self, config: &C, feed_id: &str, symbol: &str);
    /// A paper position or a working order on the tab.
    fn paper_open(&self) -> bool;
    /// The contract the tab's feed reported it does not list, as the feed's
    /// structured report names it.
    fn unlisted(&self) -> Option<String>;
    /// The tab's roll memory: armed, decided, pressed and on offer.
    fn roll_memory(&mut self) -> &mut AutoRoll;
    /// Reconnect (`keep_timeline`) or reload the tab's feed, as the buttons
    /// in the chart's offline corner do.
    fn recover(&mut self, config: &C, keep_timeline: bool) -> RecoveredFeed;
    /// Whether the tab's feed can page older trades.
    fn pages_history(&self, config: &C) -> bool;
    /// The History button's press or cancel on the tab, and the session as
    /// it then stands.
    fn history(&mut self, config: &C, act: HistoryAct) -> HistoryReply;
}

/// The tab a mutating call reaches, beside what the window lends it.
pub struct LentTab<'a, C> {
    pub tab: &'a mut dyn MarketTab<C>,
    pub config: &'a C,
    /// The window's main-click History reach.
    pub history_default: &'a mut HistoryReach,
}

/// The window's market job: resolve the addressed tab and lend the feed
/// table, the drawings a roll carries and the History default.
pub trait MarketTabHost: TabDirectory {
    type Config: FeedTable;
    /// How many tabs are open.
    fn tab_count(&self) -> usize;
    fn feed_table(&self) -> &Self::Config;
    /// The tab at `index`, a position the directory gave.
    fn market_tab(&self, index: usize) -> Option<&dyn MarketTab<Self::Config>>;
    /// The tab at `index`, lent for a change.
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_, Self::Config>>;
    /// A drawing gesture in flight on any pane holding `from`'s or `to`'s marks.
    fn drawing_gesture_on(&self, feed_id: &str, from: &str, to: &str) -> bool;
    /// Copy the drawings on `from` to `to`; how many objects were added.
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize;
}

/// The History button's act on a lent tab, the toolbar's and the call's
/// both: a taken press moves the window's main-click default
/// ([`sets_window_default`]).
pub fn run_history<C>(lent: LentTab<'_, C>, act: HistoryAct) -> HistoryReply {
    let reply = lent.tab.history(lent.config, act);
    if let (HistoryAct::Press(reach), HistoryOutcome::Pressed(press)) = (act, reply.0)
        && sets_window_default(press)
    {
        *lent.history_default = reach;
    }
    reply
}

/// Put the tab at `index` on the pair and answer where it landed; `None`
/// when the tab is gone.
pub(crate) fn switch_market<H: MarketTabHost + ?Sized>(
    host: &mut H,
    index: usize,
    feed_id: &str,
    symbol: &str,
) -> Option<FeedSelection> {
    let lent = host.market_tab_mut(index)?;
    lent.tab.switch_market(lent.config, feed_id, symbol);
    Some(lent.tab.market())
}

/// The pair the tab at `index` shows; `None` when the tab is gone.
pub(crate) fn market<H: MarketTabHost + ?Sized>(host: &H, index: usize) -> Option<FeedSelection> {
    host.market_tab(index).map(|tab| tab.market())
}

#[cfg(test)]
#[path = "market_tab_tests.rs"]
mod tests;
