//! What the list shows: a row per market, the status it reads, the groups
//! it falls in, and the rows built from a catalog market, a recording and a
//! query no market matches.

use super::copy::{REPLAY_CHIP, REPLAY_VENUE};
use super::world::{CatalogMarket, MarketWorld, OpenMarket, Venue, VenueKind};

/// What a row says about its market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketStatus {
    Live,
    /// Open in the tab at this one-based place.
    OpenTab(usize),
    WaitingForBridge,
    /// Its tab closed at this `HH:MM`.
    Closed(String),
    Replay,
    ComingSoon,
    /// A no-match row that adds the query to a venue.
    Add,
    /// Switch mode: the market the tab being switched shows now.
    ThisTab,
    /// Switch mode: the contract the feed config names after `from`;
    /// `refused` while the roll's own rule refuses it (greyed, unselectable).
    Roll {
        from: String,
        refused: bool,
    },
}

/// The ink a status is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Muted,
    Open,
    Warn,
    Replay,
    Add,
}

impl MarketStatus {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Live => "live".to_owned(),
            Self::OpenTab(position) => format!("open \u{b7} tab {position}"),
            Self::WaitingForBridge => "waiting for bridge".to_owned(),
            Self::Closed(at) => format!("closed {at}"),
            Self::Replay => "replay".to_owned(),
            Self::ComingSoon => "coming soon".to_owned(),
            Self::Add => "add to My markets and open".to_owned(),
            Self::ThisTab => "this tab".to_owned(),
            Self::Roll { from, .. } => format!("Roll from {from}"),
        }
    }

    #[must_use]
    pub fn tone(&self) -> StatusTone {
        match self {
            Self::Live | Self::Closed(_) | Self::ComingSoon => StatusTone::Muted,
            Self::OpenTab(_) | Self::ThisTab => StatusTone::Open,
            Self::WaitingForBridge => StatusTone::Warn,
            Self::Replay => StatusTone::Replay,
            Self::Add | Self::Roll { .. } => StatusTone::Add,
        }
    }
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketRow {
    pub feed_id: String,
    pub venue: String,
    pub chip: char,
    pub kind: VenueKind,
    pub symbol: String,
    pub asset: String,
    pub user_added: bool,
    /// The bar the market remembers, as [`market_bar`](super::market_bar)
    /// answers; `None` when a tab opened from here takes the active tab's.
    pub bar: Option<String>,
    /// The context charts the market remembers, as
    /// [`market_context_bars`](super::market_context_bars) answers; empty
    /// when a tab opened from here takes a new tab's.
    pub context_bars: Vec<String>,
    pub status: MarketStatus,
    pub price: Option<String>,
    /// The price is the last one a closed tab showed.
    pub price_dim: bool,
    pub trades_per_s: Option<u32>,
    /// The tabs showing this market, in strip order.
    pub tabs: Vec<OpenMarket>,
    pub hover: Option<String>,
}

impl MarketRow {
    #[must_use]
    pub fn selectable(&self) -> bool {
        let refused = matches!(self.status, MarketStatus::Roll { refused: true, .. });
        self.kind != VenueKind::ComingSoon && self.status != MarketStatus::ThisTab && !refused
    }

    /// The switch palette's roll row, which Enter never lands on by default.
    #[must_use]
    pub fn is_roll(&self) -> bool {
        matches!(self.status, MarketStatus::Roll { .. })
    }

    #[must_use]
    pub fn rate_label(&self) -> Option<String> {
        self.trades_per_s.map(|rate| format!("{rate}/s"))
    }
}

/// A heading and its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketGroup {
    pub heading: String,
    pub rows: Vec<MarketRow>,
}

/// The list as the palette shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketListing {
    pub groups: Vec<MarketGroup>,
    /// See [`MarketWorld::inherited_bar`].
    pub inherited_bar: Option<String>,
}

impl MarketListing {
    pub fn rows(&self) -> impl Iterator<Item = &MarketRow> {
        self.groups.iter().flat_map(|group| group.rows.iter())
    }

    pub(super) fn selectable(&self) -> impl Iterator<Item = &MarketRow> {
        self.rows().filter(|row| row.selectable())
    }
}

pub(super) fn group(heading: &str, rows: Vec<MarketRow>) -> MarketGroup {
    MarketGroup {
        heading: heading.to_owned(),
        rows,
    }
}

pub(super) fn catalog_row(world: &MarketWorld<'_>, market: &CatalogMarket) -> Option<MarketRow> {
    let venue = world
        .venues
        .iter()
        .find(|venue| venue.feed_id == market.feed_id)?;
    let tabs: Vec<OpenMarket> = world
        .open
        .iter()
        .filter(|tab| !tab.replay && tab.feed_id == market.feed_id && tab.symbol == market.symbol)
        .cloned()
        .collect();
    let closed = world
        .recent
        .entries()
        .iter()
        .find(|closed| closed.feed_id == market.feed_id && closed.symbol == market.symbol);
    let mut row = MarketRow {
        feed_id: market.feed_id.clone(),
        venue: venue.name.clone(),
        chip: venue.chip,
        kind: venue.kind,
        symbol: market.symbol.clone(),
        asset: market.asset.clone(),
        user_added: market.user_added,
        bar: (tabs.first().map(|tab| tab.bar.clone()))
            .or_else(|| closed.map(|closed| closed.bar.clone()))
            .or_else(|| venue.bar.clone()),
        context_bars: (tabs.first().map(|tab| tab.context_bars.clone()))
            .or_else(|| closed.map(|closed| closed.context_bars.clone()))
            .unwrap_or_default(),
        status: MarketStatus::Live,
        price: None,
        price_dim: false,
        trades_per_s: None,
        tabs: Vec::new(),
        hover: None,
    };
    if venue.kind == VenueKind::ComingSoon {
        row.status = MarketStatus::ComingSoon;
    } else if let Some(tab) = tabs.first() {
        if venue.bridged && !tab.streaming {
            row.status = MarketStatus::WaitingForBridge;
            row.hover = Some(format!(
                "Waiting for bridge \u{b7} attach the Quantick EA to {} in MetaTrader",
                market.symbol
            ));
        } else {
            row.status = MarketStatus::OpenTab(tab.position);
            row.price.clone_from(&tab.price);
            row.trades_per_s = tab.trades_per_s;
        }
    } else if let Some(closed) = closed {
        row.status = MarketStatus::Closed(closed.at.clone());
        row.price.clone_from(&closed.price);
        row.price_dim = true;
    }
    row.tabs = tabs;
    Some(row)
}

pub(super) fn replay_row(tab: &OpenMarket) -> MarketRow {
    MarketRow {
        feed_id: tab.feed_id.clone(),
        venue: REPLAY_VENUE.to_owned(),
        chip: REPLAY_CHIP,
        kind: VenueKind::Replay,
        symbol: tab.symbol.clone(),
        asset: tab.asset.clone(),
        user_added: false,
        bar: Some(tab.bar.clone()),
        context_bars: tab.context_bars.clone(),
        status: MarketStatus::Replay,
        price: tab.price.clone(),
        price_dim: false,
        trades_per_s: None,
        tabs: vec![tab.clone()],
        hover: None,
    }
}

/// A row that adds `symbol` to `venue`, spelled exactly as typed.
pub(super) fn add_row(venue: &Venue, symbol: &str) -> MarketRow {
    MarketRow {
        feed_id: venue.feed_id.clone(),
        venue: venue.name.clone(),
        chip: venue.chip,
        kind: venue.kind,
        symbol: symbol.to_owned(),
        asset: symbol.to_owned(),
        user_added: false,
        bar: venue.bar.clone(),
        context_bars: Vec::new(),
        status: MarketStatus::Add,
        price: None,
        price_dim: false,
        trades_per_s: None,
        tabs: Vec::new(),
        hover: None,
    }
}
