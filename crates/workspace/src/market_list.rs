//! The "Open a market" list: one row per configured market of every feed,
//! the filter, the groups, the status each row reads and what Enter does.
//!
//! Told everything: the venues and markets the config declares, the tabs the
//! window holds and the markets it closed. The window and the control plane
//! read this one list, so the palette, `market.open` and `catalog.remove`
//! cannot disagree about a market.
//!
//! Rows are grouped only through the explicit `asset` key the config gives a
//! symbol; nothing is merged by guessing at a symbol's text.

use crate::workspace_book::DEFAULT_WORKSPACE;

pub const PLACEHOLDER: &str = "Search markets, or > for commands";
pub const RECENT_GROUP: &str = "Recent";
pub const START_HERE_GROUP: &str = "Start here";
pub const MARKETS_GROUP: &str = "Markets";
pub const COMMANDS_GROUP: &str = "Commands";
/// What a recording's row says where a venue's name would be.
pub const REPLAY_VENUE: &str = "Replay";
pub const REPLAY_CHIP: char = 'R';
/// A query starting with this lists commands, never markets.
pub const COMMAND_PREFIX: char = '>';
/// How many closed markets Recent remembers.
pub const RECENT_CLOSED_LIMIT: usize = 8;
/// The kinds the Bars selector offers, in its order.
pub const BAR_KINDS: [&str; 5] = ["tick", "volume", "dollar", "imbalance", "time"];

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
    /// The bar a new tab on this venue opens on, as `kind:size`.
    pub bar: String,
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

/// Everything the list is built from.
#[derive(Debug, Clone, Copy)]
pub struct MarketWorld<'a> {
    pub venues: &'a [Venue],
    pub markets: &'a [CatalogMarket],
    pub open: &'a [OpenMarket],
    pub recent: &'a RecentMarkets,
    /// The feed whose markets are shown under Start here.
    pub starter_feed: &'a str,
}

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
        }
    }

    #[must_use]
    pub fn tone(&self) -> StatusTone {
        match self {
            Self::Live | Self::Closed(_) | Self::ComingSoon => StatusTone::Muted,
            Self::OpenTab(_) => StatusTone::Open,
            Self::WaitingForBridge => StatusTone::Warn,
            Self::Replay => StatusTone::Replay,
            Self::Add => StatusTone::Add,
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
    /// The bar a tab opened from here starts on: the open tab's, else the
    /// venue's.
    pub bar: String,
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
        self.kind != VenueKind::ComingSoon
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
}

impl MarketListing {
    pub fn rows(&self) -> impl Iterator<Item = &MarketRow> {
        self.groups.iter().flat_map(|group| group.rows.iter())
    }

    fn selectable(&self) -> impl Iterator<Item = &MarketRow> {
        self.rows().filter(|row| row.selectable())
    }
}

/// The no-match heading.
#[must_use]
pub fn no_match_heading(query: &str) -> String {
    format!("No market matches \u{201c}{query}\u{201d}")
}

/// Build the list for `query`.
#[must_use]
pub fn list_markets(world: &MarketWorld<'_>, query: &str) -> MarketListing {
    let query = query.trim();
    if query.starts_with(COMMAND_PREFIX) {
        return MarketListing {
            groups: vec![group(COMMANDS_GROUP, Vec::new())],
        };
    }
    if query.is_empty() {
        return browse(world);
    }
    let needle = query.to_lowercase();
    let matches = |row: &MarketRow| {
        [&row.symbol, &row.venue, &row.asset]
            .into_iter()
            .any(|text| text.to_lowercase().contains(&needle))
            || alias_hit(world, &row.asset, &needle)
    };
    let mut found: Vec<MarketRow> = world
        .markets
        .iter()
        .filter_map(|market| catalog_row(world, market))
        .chain(replay_rows(world))
        .filter(|row| matches(row))
        .collect();
    if found.is_empty() {
        let adds = world
            .venues
            .iter()
            .filter(|venue| venue.kind == VenueKind::Live)
            .map(|venue| add_row(venue, &query.to_uppercase()))
            .collect();
        return MarketListing {
            groups: vec![group(&no_match_heading(query), adds)],
        };
    }
    let mut assets: Vec<String> = Vec::new();
    for row in &found {
        if !assets.contains(&row.asset) {
            assets.push(row.asset.clone());
        }
    }
    found.sort_by_key(|row| assets.iter().position(|asset| asset == &row.asset));
    MarketListing {
        groups: vec![group(MARKETS_GROUP, found)],
    }
}

/// The tab Enter focuses for this market at this bar and workspace, if one
/// already shows exactly that. The palette's label and `market.open` both
/// ask this.
#[must_use]
pub fn focus_target<'a>(
    open: &'a [OpenMarket],
    feed_id: &str,
    symbol: &str,
    bar: &str,
    workspace: &str,
) -> Option<&'a OpenMarket> {
    open.iter().find(|tab| {
        !tab.replay
            && tab.feed_id == feed_id
            && tab.symbol == symbol
            && tab.workspace == workspace
            && tab.bar == bar
    })
}

/// Why `symbol` cannot leave feed `feed_id`'s list, or `None` when it can.
#[must_use]
pub fn removal_refusal(world: &MarketWorld<'_>, feed_id: &str, symbol: &str) -> Option<String> {
    let Some(market) = world
        .markets
        .iter()
        .find(|market| market.feed_id == feed_id && market.symbol == symbol)
    else {
        return Some(format!("{symbol} is not in this venue's list"));
    };
    if !market.user_added {
        return Some(format!(
            "{symbol} comes from the config file; only a market you added can be removed"
        ));
    }
    let shown = world
        .open
        .iter()
        .any(|tab| !tab.replay && tab.feed_id == feed_id && tab.symbol == symbol);
    shown.then(|| format!("A tab is showing {symbol} \u{2014} close it first"))
}

/// What Enter does on a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryAction {
    Open,
    Focus { tab_id: u64, position: usize },
    OpenSecond,
    OpenReplay,
    AddAndOpen { symbol: String },
    Run,
}

impl PrimaryAction {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Open => "Open".to_owned(),
            Self::Focus { position, .. } => format!("Focus tab {position}"),
            Self::OpenSecond => "Open second tab".to_owned(),
            Self::OpenReplay => "Open replay".to_owned(),
            Self::AddAndOpen { symbol } => format!("Add {symbol} and open"),
            Self::Run => "Run".to_owned(),
        }
    }
}

/// What Enter does on `row` with the footer at `bar` and `workspace`;
/// `None` on a row that cannot be chosen.
#[must_use]
pub fn primary_action(row: &MarketRow, bar: &str, workspace: &str) -> Option<PrimaryAction> {
    if !row.selectable() {
        return None;
    }
    let focus = |tab: &OpenMarket| PrimaryAction::Focus {
        tab_id: tab.tab_id,
        position: tab.position,
    };
    Some(match (&row.status, row.kind) {
        (MarketStatus::Add, _) => PrimaryAction::AddAndOpen {
            symbol: row.symbol.clone(),
        },
        (_, VenueKind::Replay) => row.tabs.first().map_or(PrimaryAction::OpenReplay, focus),
        _ => match focus_target(&row.tabs, &row.feed_id, &row.symbol, bar, workspace) {
            Some(tab) => focus(tab),
            None if row.tabs.is_empty() => PrimaryAction::Open,
            None => PrimaryAction::OpenSecond,
        },
    })
}

fn group(heading: &str, rows: Vec<MarketRow>) -> MarketGroup {
    MarketGroup {
        heading: heading.to_owned(),
        rows,
    }
}

fn alias_hit(world: &MarketWorld<'_>, asset: &str, needle: &str) -> bool {
    world.markets.iter().any(|market| {
        market.asset == asset
            && market
                .aliases
                .iter()
                .any(|alias| alias.to_lowercase().contains(needle))
    })
}

/// The empty query: Recent (or Start here), then one group per venue of
/// what is left.
fn browse(world: &MarketWorld<'_>) -> MarketListing {
    let mut groups = Vec::new();
    let mut recent: Vec<MarketRow> = Vec::new();
    let push = |rows: &mut Vec<MarketRow>, row: MarketRow| {
        if !rows.iter().any(|seen| same_row(seen, &row)) {
            rows.push(row);
        }
    };
    let mut open: Vec<&OpenMarket> = world.open.iter().collect();
    open.sort_by_key(|tab| tab.position);
    for tab in open {
        let row = if tab.replay {
            Some(replay_row(tab))
        } else {
            find_market(world, &tab.feed_id, &tab.symbol)
                .and_then(|market| catalog_row(world, market))
        };
        if let Some(row) = row {
            push(&mut recent, row);
        }
    }
    for closed in world.recent.entries() {
        if let Some(row) = find_market(world, &closed.feed_id, &closed.symbol)
            .and_then(|market| catalog_row(world, market))
        {
            push(&mut recent, row);
        }
    }
    let mut shown = recent.clone();
    if recent.is_empty() {
        let starters: Vec<MarketRow> = world
            .markets
            .iter()
            .filter(|market| market.feed_id == world.starter_feed)
            .filter_map(|market| catalog_row(world, market))
            .collect();
        shown.extend(starters.iter().cloned());
        if !starters.is_empty() {
            groups.push(group(START_HERE_GROUP, starters));
        }
    } else {
        groups.push(group(RECENT_GROUP, recent));
    }
    for venue in world.venues {
        let rows: Vec<MarketRow> = world
            .markets
            .iter()
            .filter(|market| market.feed_id == venue.feed_id)
            .filter_map(|market| catalog_row(world, market))
            .filter(|row| !shown.iter().any(|seen| same_row(seen, row)))
            .collect();
        if !rows.is_empty() {
            groups.push(group(&venue.name, rows));
        }
    }
    MarketListing { groups }
}

fn same_row(one: &MarketRow, other: &MarketRow) -> bool {
    (&one.feed_id, &one.symbol, one.kind) == (&other.feed_id, &other.symbol, other.kind)
}

fn find_market<'a>(
    world: &MarketWorld<'a>,
    feed_id: &str,
    symbol: &str,
) -> Option<&'a CatalogMarket> {
    world
        .markets
        .iter()
        .find(|market| market.feed_id == feed_id && market.symbol == symbol)
}

fn catalog_row(world: &MarketWorld<'_>, market: &CatalogMarket) -> Option<MarketRow> {
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
        bar: venue.bar.clone(),
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
        row.bar.clone_from(&tab.bar);
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

fn replay_rows<'a>(world: &'a MarketWorld<'a>) -> impl Iterator<Item = MarketRow> + 'a {
    world.open.iter().filter(|tab| tab.replay).map(replay_row)
}

fn replay_row(tab: &OpenMarket) -> MarketRow {
    MarketRow {
        feed_id: tab.feed_id.clone(),
        venue: REPLAY_VENUE.to_owned(),
        chip: REPLAY_CHIP,
        kind: VenueKind::Replay,
        symbol: tab.symbol.clone(),
        asset: tab.asset.clone(),
        user_added: false,
        bar: tab.bar.clone(),
        status: MarketStatus::Replay,
        price: tab.price.clone(),
        price_dim: false,
        trades_per_s: None,
        tabs: vec![tab.clone()],
        hover: None,
    }
}

fn add_row(venue: &Venue, symbol: &str) -> MarketRow {
    MarketRow {
        feed_id: venue.feed_id.clone(),
        venue: venue.name.clone(),
        chip: venue.chip,
        kind: venue.kind,
        symbol: symbol.to_owned(),
        asset: symbol.to_owned(),
        user_added: false,
        bar: venue.bar.clone(),
        status: MarketStatus::Add,
        price: None,
        price_dim: false,
        trades_per_s: None,
        tabs: Vec::new(),
        hover: None,
    }
}

/// Which footer stop has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteFocus {
    #[default]
    Field,
    Bars,
    Workspace,
}

/// A market, a bar and a workspace to open it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenChoice {
    pub feed_id: String,
    pub symbol: String,
    pub bar: String,
    pub workspace: String,
    pub background: bool,
}

/// What a key in the palette asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteIntent {
    /// Open, or focus the tab that already shows exactly this.
    Open(OpenChoice),
    /// Add the symbol to the venue's list, then open it. May be refused.
    Add(OpenChoice),
    Remove {
        feed_id: String,
        symbol: String,
    },
}

/// The palette while it is on screen: the query, the selection and the
/// footer's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketPalette {
    query: String,
    /// Index among the selectable rows.
    selected: usize,
    focus: PaletteFocus,
    /// The trader's bar, as `kind:size`; `None` follows the selected row.
    bar: Option<String>,
    workspace: String,
    refusal: Option<String>,
}

impl Default for MarketPalette {
    fn default() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            focus: PaletteFocus::Field,
            bar: None,
            workspace: DEFAULT_WORKSPACE.to_owned(),
            refusal: None,
        }
    }
}

impl MarketPalette {
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replace the query; the selection goes back to the first row.
    pub fn set_query(&mut self, query: &str) {
        if self.query != query {
            query.clone_into(&mut self.query);
            self.selected = 0;
        }
    }

    /// The editable query, for the text field; call [`Self::settle_query`]
    /// with what it held before.
    pub fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    /// After the field edited the query: a changed query selects the first row.
    pub fn settle_query(&mut self, before: &str) {
        if self.query != before {
            self.selected = 0;
        }
    }

    #[must_use]
    pub fn selected_row<'l>(&self, listing: &'l MarketListing) -> Option<&'l MarketRow> {
        listing
            .selectable()
            .nth(self.selected)
            .or_else(|| listing.selectable().next())
    }

    /// Whether `row` is the selected one.
    #[must_use]
    pub fn is_selected(&self, listing: &MarketListing, row: &MarketRow) -> bool {
        self.selected_row(listing)
            .is_some_and(|selected| std::ptr::eq(selected, row))
    }

    /// Move over the selectable rows, wrapping.
    pub fn move_selection(&mut self, delta: isize, listing: &MarketListing) {
        let count = listing.selectable().count();
        if count == 0 {
            return;
        }
        let current = self.selected.min(count - 1);
        let step = delta.rem_euclid(count as isize) as usize;
        self.selected = (current + step) % count;
    }

    /// Select `row`, as a hover or click does.
    pub fn select(&mut self, listing: &MarketListing, row: &MarketRow) {
        if let Some(index) = listing
            .selectable()
            .position(|candidate| std::ptr::eq(candidate, row))
        {
            self.selected = index;
        }
    }

    /// The bar Enter opens on: the trader's, else the selected row's.
    #[must_use]
    pub fn bar(&self, listing: &MarketListing) -> String {
        self.bar.clone().unwrap_or_else(|| {
            self.selected_row(listing)
                .map(|row| row.bar.clone())
                .unwrap_or_default()
        })
    }

    #[must_use]
    pub fn bar_changed(&self) -> bool {
        self.bar.is_some()
    }

    pub fn set_bar_kind(&mut self, kind: &str, size: &str) {
        self.bar = Some(format!("{kind}:{size}"));
    }

    /// A new size on the kind shown.
    pub fn set_bar_size(&mut self, listing: &MarketListing, size: &str) {
        let bar = self.bar(listing);
        let kind = bar.split_once(':').map_or(bar.as_str(), |(kind, _)| kind);
        self.bar = Some(format!("{kind}:{size}"));
    }

    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub fn workspace_changed(&self) -> bool {
        self.workspace != DEFAULT_WORKSPACE
    }

    pub fn set_workspace(&mut self, name: &str) {
        name.clone_into(&mut self.workspace);
    }

    #[must_use]
    pub fn focus(&self) -> PaletteFocus {
        self.focus
    }

    /// Tab: the field, then Bars, then Workspace, then the field.
    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            PaletteFocus::Field => PaletteFocus::Bars,
            PaletteFocus::Bars => PaletteFocus::Workspace,
            PaletteFocus::Workspace => PaletteFocus::Field,
        };
    }

    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Show why the last ask was refused, under the field.
    pub fn refuse(&mut self, reason: String) {
        self.refusal = Some(reason);
    }

    /// Enter (or Ctrl+Enter with `background`) on the selected row.
    pub fn submit(&mut self, listing: &MarketListing, background: bool) -> Option<PaletteIntent> {
        self.refusal = None;
        let row = self.selected_row(listing)?;
        let choice = OpenChoice {
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
            bar: self.bar(listing),
            workspace: self.workspace.clone(),
            background,
        };
        Some(if row.status == MarketStatus::Add {
            PaletteIntent::Add(choice)
        } else {
            PaletteIntent::Open(choice)
        })
    }

    /// Delete on the selected row: take that market out of its venue's list.
    #[must_use]
    pub fn remove_selected(&self, listing: &MarketListing) -> Option<PaletteIntent> {
        let row = self.selected_row(listing)?;
        (row.kind == VenueKind::Live && row.status != MarketStatus::Add).then(|| {
            PaletteIntent::Remove {
                feed_id: row.feed_id.clone(),
                symbol: row.symbol.clone(),
            }
        })
    }
}

#[cfg(test)]
#[path = "market_list_tests.rs"]
mod market_list_tests;
