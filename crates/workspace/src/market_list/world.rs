//! What the list is told: the venues and markets the config declares, the
//! tabs the window holds and the markets it closed.

use super::copy::RECENT_CLOSED_LIMIT;

/// What a venue can do today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueKind {
    Live,
    Replay,
    /// Listed so a trader knows it is coming; never chosen.
    ComingSoon,
}

/// One configured feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Venue {
    pub feed_id: String,
    pub name: String,
    /// The letter on each row: B, H, 5.
    pub chip: char,
    pub kind: VenueKind,
    /// The bar the feed declares its tabs open on (`default_bars`), as
    /// `kind:size`; `None` when it declares none.
    pub bar: Option<String>,
    /// Whether a market here waits on a terminal bridge before it streams.
    pub bridged: bool,
}

/// One market the config offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogMarket {
    pub feed_id: String,
    pub symbol: String,
    /// The explicit asset key; the only thing rows are grouped by.
    pub asset: String,
    /// Other names the asset is searched by.
    pub aliases: Vec<String>,
    /// Added by the trader, so the trader may take it out again.
    pub user_added: bool,
}

/// One tab the window holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMarket {
    pub tab_id: u64,
    /// One-based place among the market tabs.
    pub position: usize,
    pub feed_id: String,
    pub symbol: String,
    pub asset: String,
    pub workspace: String,
    /// The flow pane's bar, as `kind:size`.
    pub bar: String,
    pub replay: bool,
    /// Whether a trade has arrived.
    pub streaming: bool,
    pub price: Option<String>,
    pub trades_per_s: Option<u32>,
}

/// A market whose tab was closed, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedMarket {
    pub feed_id: String,
    pub symbol: String,
    /// `HH:MM`, as the window's clock reads it.
    pub at: String,
    /// The last price the tab showed.
    pub price: Option<String>,
    /// The bar the tab showed when it closed, as `kind:size`.
    pub bar: String,
}

/// The markets closed lately, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentMarkets {
    closed: Vec<ClosedMarket>,
}

impl RecentMarkets {
    /// Remember a close; a market closed again moves to the front.
    pub fn closed(&mut self, market: ClosedMarket) {
        self.closed
            .retain(|seen| (&seen.feed_id, &seen.symbol) != (&market.feed_id, &market.symbol));
        self.closed.insert(0, market);
        self.closed.truncate(RECENT_CLOSED_LIMIT);
    }

    #[must_use]
    pub fn entries(&self) -> &[ClosedMarket] {
        &self.closed
    }
}

impl Extend<ClosedMarket> for RecentMarkets {
    fn extend<I: IntoIterator<Item = ClosedMarket>>(&mut self, closes: I) {
        closes.into_iter().for_each(|market| self.closed(market));
    }
}

/// Everything the list is built from.
#[derive(Debug, Clone, Copy)]
pub struct MarketWorld<'a> {
    pub venues: &'a [Venue],
    pub markets: &'a [CatalogMarket],
    pub open: &'a [OpenMarket],
    pub recent: &'a RecentMarkets,
    /// The feed whose markets are shown under Start here.
    pub starter_feed: &'a str,
    /// The bar a new tab takes when its market has none of its own: the
    /// active tab's.
    pub inherited_bar: Option<&'a str>,
}

/// The same, owned: what the window hands the palette while it is open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketSnapshot {
    pub venues: Vec<Venue>,
    pub markets: Vec<CatalogMarket>,
    pub open: Vec<OpenMarket>,
    pub recent: RecentMarkets,
    pub starter_feed: String,
    /// The Workspace selector's names, Default first.
    pub workspaces: Vec<String>,
    /// See [`MarketWorld::inherited_bar`].
    pub inherited_bar: Option<String>,
}

impl MarketSnapshot {
    #[must_use]
    pub fn world(&self) -> MarketWorld<'_> {
        MarketWorld {
            venues: &self.venues,
            markets: &self.markets,
            open: &self.open,
            recent: &self.recent,
            starter_feed: &self.starter_feed,
            inherited_bar: self.inherited_bar.as_deref(),
        }
    }

    /// Refuses a workspace with no such name.
    pub fn check_workspace(&self, name: &str) -> Result<(), String> {
        if self.workspaces.iter().any(|known| known == name) {
            Ok(())
        } else {
            Err(format!("No workspace is named {name}"))
        }
    }
}

pub(super) fn find_market<'a>(
    world: &MarketWorld<'a>,
    feed_id: &str,
    symbol: &str,
) -> Option<&'a CatalogMarket> {
    world
        .markets
        .iter()
        .find(|market| market.feed_id == feed_id && market.symbol == symbol)
}
