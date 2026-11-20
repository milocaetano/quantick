//! The tab-scoped feed and market calls' one port: the window resolves the
//! addressed tab and lends its config, its drawings and its History default;
//! the tab answers the rest, and the config answers which pairs and rolls
//! exist.
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot, sets_window_default};
use quantick_sources::history_reach::HistoryReach;
use quantick_sources::recovery::{AutoRoll, Recovery};
use quantick_stores::config::AppConfig;

use crate::tabs::OpenTabs;

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

/// One tab, as its feed and market calls read and change it. Its names
/// differ from the tab's own methods, so an implementation that forwards to
/// one cannot fall back on itself.
pub trait MarketTab {
    /// The pair the tab shows, and whether it plays a recording.
    fn market(&self) -> FeedSelection;
    /// Put the tab on the validated pair, through its own source switch.
    fn switch_market(&mut self, config: &AppConfig, feed_id: &str, symbol: &str);
    /// A paper position or a working order on the tab.
    fn paper_open(&self) -> bool;
    /// The contract the tab's feed reported it does not list, as the feed's
    /// structured report names it.
    fn unlisted(&self) -> Option<String>;
    /// The tab's roll memory: armed, decided, pressed and on offer.
    fn roll_memory(&mut self) -> &mut AutoRoll;
    /// Reconnect or reload the tab's feed, as the buttons in the chart's
    /// offline corner do.
    fn recover(&mut self, config: &AppConfig, recovery: Recovery) -> RecoveredFeed;
    /// Whether the tab's feed can page older trades.
    fn pages_history(&self, config: &AppConfig) -> bool;
    /// The History button's press or cancel on the tab.
    fn history(&mut self, config: &AppConfig, act: HistoryAct) -> HistoryOutcome;
    /// The tab's session as it stands.
    fn session(&self) -> SessionSnapshot;
    /// The tab's script strategies and charts, lent for one
    /// `strategy.script.*` call; `None` where the tab keeps none.
    fn script_tab(&mut self) -> Option<crate::script_strategy::ScriptTab<'_>> {
        None
    }
}

/// The tab a mutating call reaches, beside what the window lends it.
pub struct LentTab<'a> {
    pub tab: &'a mut dyn MarketTab,
    pub config: &'a AppConfig,
    /// The window's main-click History reach.
    pub history_default: &'a mut HistoryReach,
}

/// The window's market job: its open tabs read as [`OpenTabs`], each lent
/// for a change with the config and the History default, and the drawings a
/// roll carries.
pub trait MarketTabHost: OpenTabs<Tab: MarketTab> {
    /// The tab at `index`, lent for a change.
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_>>;
    /// A drawing gesture in flight on any pane holding `from`'s or `to`'s marks.
    fn drawing_gesture_on(&self, feed_id: &str, from: &str, to: &str) -> bool;
    /// Copy the drawings on `from` to `to`; how many objects were added.
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize;
}

/// A History act on a lent tab, the one the toolbar's press and
/// `feed.history.*` make: a taken press moves the window's main-click
/// default ([`sets_window_default`]).
pub fn run_history(lent: LentTab<'_>, act: HistoryAct) -> HistoryOutcome {
    let outcome = lent.tab.history(lent.config, act);
    if let (HistoryAct::Press(reach), HistoryOutcome::Pressed(press)) = (act, outcome)
        && sets_window_default(press)
    {
        *lent.history_default = reach;
    }
    outcome
}

/// The contract feed `feed_id`'s config names after `symbol`.
pub(crate) fn next_contract(config: &AppConfig, feed_id: &str, symbol: &str) -> Option<String> {
    config
        .feed(feed_id)?
        .next_contract(symbol)
        .map(str::to_owned)
}

/// Whether feed `feed_id` is configured and lists `symbol` (`None`: any).
pub(crate) fn feed_offers(config: &AppConfig, feed_id: &str, symbol: Option<&str>) -> bool {
    config.feed(feed_id).is_some_and(|feed| {
        symbol.is_none_or(|symbol| feed.symbols.iter().any(|offered| offered == symbol))
    })
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
    host.open_tab(index).map(|tab| tab.market())
}

#[cfg(test)]
#[path = "market_tab_tests.rs"]
mod tests;
