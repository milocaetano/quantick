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

pub use crate::workspace_book::DEFAULT_WORKSPACE;

pub const PLACEHOLDER: &str = "Search markets, or > for commands";
pub const RECENT_GROUP: &str = "Recent";
pub const START_HERE_GROUP: &str = "Start here";
pub const MARKETS_GROUP: &str = "Markets";
pub const COMMANDS_GROUP: &str = "Commands";
/// The switch-mode group holding the contract roll's row.
pub const ROLL_GROUP: &str = "Contract roll";
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
    /// The context charts' bars, top first: the pane timeframes.
    pub context_bars: Vec<String>,
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
    /// The context charts' bars it closed on, top first.
    pub context_bars: Vec<String>,
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

    /// What `market.open` and Enter open: the market's own bar (see
    /// [`market_bar`]) and Default when left out. Refuses a market the config
    /// does not offer and a workspace with no such name.
    pub fn choice(
        &self,
        market: (&str, &str),
        bar: Option<String>,
        workspace: Option<String>,
        background: bool,
    ) -> Result<OpenChoice, String> {
        let (feed_id, symbol) = market;
        let world = self.world();
        let row = find_market(&world, feed_id, symbol)
            .and_then(|market| catalog_row(&world, market))
            .ok_or_else(|| format!("{symbol} is not in the {feed_id} list"))?;
        let workspace = workspace.unwrap_or_else(|| DEFAULT_WORKSPACE.to_owned());
        self.check_workspace(&workspace)?;
        let inherit_bar = bar.is_none() && row.bar.is_none();
        let bar = bar.or(row.bar).or_else(|| self.inherited_bar.clone());
        Ok(OpenChoice {
            feed_id: feed_id.to_owned(),
            symbol: symbol.to_owned(),
            bar: bar.unwrap_or_default(),
            context_bars: row.context_bars,
            inherit_bar,
            workspace,
            background,
        })
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

/// The bar a market remembers: its first open tab's, else the one its tab
/// last closed on, else its venue's `default_bars`. `None` when it has none
/// (a new tab then takes the active tab's) or the config does not offer it.
#[must_use]
pub fn market_bar(world: &MarketWorld<'_>, feed_id: &str, symbol: &str) -> Option<String> {
    let market = find_market(world, feed_id, symbol)?;
    catalog_row(world, market)?.bar
}

/// The pane timeframes a market remembers, as [`market_bar`] remembers its
/// flow bar: its first open tab's context charts, else the ones its tab last
/// closed on. Empty when it has none: a new tab then opens as any new tab does.
#[must_use]
pub fn market_context_bars(world: &MarketWorld<'_>, feed_id: &str, symbol: &str) -> Vec<String> {
    let market = find_market(world, feed_id, symbol);
    let row = market.and_then(|market| catalog_row(world, market));
    row.map(|row| row.context_bars).unwrap_or_default()
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
    /// The bar the market remembers, as [`market_bar`] answers; `None` when
    /// a tab opened from here takes the active tab's.
    pub bar: Option<String>,
    /// The context charts the market remembers, as [`market_context_bars`]
    /// answers; empty when a tab opened from here takes a new tab's.
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
    MarketListing {
        groups: list_groups(world, query.trim()),
        inherited_bar: world.inherited_bar.map(str::to_owned),
    }
}

fn list_groups(world: &MarketWorld<'_>, query: &str) -> Vec<MarketGroup> {
    if query.starts_with(COMMAND_PREFIX) {
        return vec![group(COMMANDS_GROUP, Vec::new())];
    }
    if query.is_empty() {
        return browse(world);
    }
    let needle = query.to_lowercase();
    let mut found: Vec<MarketRow> = world
        .markets
        .iter()
        .filter_map(|market| catalog_row(world, market))
        .chain(replay_rows(world))
        .filter(|row| row_matches(world, row, &needle))
        .collect();
    if found.is_empty() {
        let adds = world
            .venues
            .iter()
            .filter(|venue| venue.kind == VenueKind::Live)
            .map(|venue| add_row(venue, query))
            .collect();
        return vec![group(&no_match_heading(query), adds)];
    }
    let mut assets: Vec<String> = Vec::new();
    for row in &found {
        if !assets.contains(&row.asset) {
            assets.push(row.asset.clone());
        }
    }
    found.sort_by_key(|row| assets.iter().position(|asset| asset == &row.asset));
    vec![group(MARKETS_GROUP, found)]
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
    Focus {
        tab_id: u64,
        position: usize,
    },
    OpenSecond,
    OpenReplay,
    AddAndOpen {
        symbol: String,
    },
    Run,
    /// Switch mode: put the tab on this market.
    Switch {
        symbol: String,
        venue: String,
    },
    /// Switch mode: roll the tab onto this contract.
    Roll {
        target: String,
    },
    /// Switch mode: add the symbol to the venue's list, then switch to it.
    AddAndSwitch {
        symbol: String,
    },
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
            Self::Switch { symbol, venue } => format!("Switch to {symbol} \u{b7} {venue}"),
            Self::Roll { target } => format!("Roll to {target}"),
            Self::AddAndSwitch { symbol } => format!("Add {symbol} and switch"),
        }
    }
}

/// What Enter does on `row` in switch mode; `None` on a row that cannot be
/// chosen there: the tab's own market, a coming-soon venue, a recording.
#[must_use]
pub fn switch_action(row: &MarketRow) -> Option<PrimaryAction> {
    if !row.selectable() || row.kind == VenueKind::Replay {
        return None;
    }
    Some(match &row.status {
        MarketStatus::Roll { .. } => PrimaryAction::Roll {
            target: row.symbol.clone(),
        },
        MarketStatus::Add => PrimaryAction::AddAndSwitch {
            symbol: row.symbol.clone(),
        },
        _ => PrimaryAction::Switch {
            symbol: row.symbol.clone(),
            venue: row.venue.clone(),
        },
    })
}

/// The contract a tab could roll to, and why the roll is greyed when it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollOffer {
    pub target: String,
    pub refusal: Option<String>,
}

/// The tab a switch-mode palette switches, as it stood when the palette
/// opened: its market, and the bar and workspace the footer starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTarget {
    pub tab_id: u64,
    pub feed_id: String,
    pub symbol: String,
    pub venue: String,
    pub chip: char,
    /// The flow pane's bar, as `kind:size`.
    pub bar: String,
    pub workspace: String,
    /// The next contract, when the feed config names one.
    pub roll: Option<RollOffer>,
}

impl SwitchTarget {
    /// Tab `tab_id` as `snapshot` reads it; `None` for a tab it does not hold
    /// and for a recording, which has no market to switch.
    #[must_use]
    pub fn of(snapshot: &MarketSnapshot, tab_id: u64, roll: Option<RollOffer>) -> Option<Self> {
        let tab = (snapshot.open.iter()).find(|tab| tab.tab_id == tab_id && !tab.replay)?;
        let venue = (snapshot.venues.iter()).find(|venue| venue.feed_id == tab.feed_id);
        Some(Self {
            tab_id,
            feed_id: tab.feed_id.clone(),
            symbol: tab.symbol.clone(),
            venue: venue.map_or_else(|| tab.feed_id.clone(), |venue| venue.name.clone()),
            chip: venue.map_or('?', |venue| venue.chip),
            bar: tab.bar.clone(),
            workspace: tab.workspace.clone(),
            roll,
        })
    }

    /// The line under the field.
    #[must_use]
    pub fn caption(&self) -> String {
        format!(
            "Switching {} \u{b7} {} on this tab \u{b7} drawings stay with each market",
            self.symbol, self.venue
        )
    }

    /// The roll's row, when there is a roll and `needle` (lower-cased) finds
    /// it as it finds any row (symbol, venue, asset, aliases) or names the
    /// contract it rolls from. A refused roll stays on show, greyed.
    fn roll_row(&self, world: &MarketWorld<'_>, needle: &str) -> Option<MarketRow> {
        let roll = self.roll.as_ref()?;
        let asset = [&roll.target, &self.symbol]
            .into_iter()
            .find_map(|symbol| find_market(world, &self.feed_id, symbol))
            .map_or_else(|| roll.target.clone(), |market| market.asset.clone());
        let row = MarketRow {
            feed_id: self.feed_id.clone(),
            venue: self.venue.clone(),
            chip: self.chip,
            kind: VenueKind::Live,
            symbol: roll.target.clone(),
            asset,
            user_added: false,
            bar: None,
            context_bars: Vec::new(),
            status: MarketStatus::Roll {
                from: self.symbol.clone(),
                refused: roll.refusal.is_some(),
            },
            price: None,
            price_dim: false,
            trades_per_s: None,
            tabs: Vec::new(),
            hover: Some(
                roll.refusal
                    .clone()
                    .unwrap_or_else(|| ROLL_KEEPS.to_owned()),
            ),
        };
        let named = row_matches(world, &row, needle) || self.symbol.to_lowercase().contains(needle);
        named.then_some(row)
    }

    /// The list as switch mode shows it: no recordings, the tab's own market
    /// marked "this tab", and the roll first when there is one, in place of
    /// the plain row for the same contract.
    fn listing(
        &self,
        world: &MarketWorld<'_>,
        mut listing: MarketListing,
        query: &str,
    ) -> MarketListing {
        let query = query.trim();
        let roll = (!query.starts_with(COMMAND_PREFIX))
            .then(|| self.roll_row(world, &query.to_lowercase()))
            .flatten();
        for group in &mut listing.groups {
            group.rows.retain(|row| {
                let rolled = (roll.as_ref()).is_some_and(|roll| {
                    (&row.feed_id, &row.symbol) == (&roll.feed_id, &roll.symbol)
                });
                row.kind != VenueKind::Replay && !rolled
            });
            for row in &mut group.rows {
                let own = (&row.feed_id, &row.symbol) == (&self.feed_id, &self.symbol);
                if own && row.status != MarketStatus::Add {
                    row.status = MarketStatus::ThisTab;
                }
            }
        }
        listing
            .groups
            .retain(|group| !group.rows.is_empty() || query.starts_with(COMMAND_PREFIX));
        if let Some(row) = roll {
            listing.groups.insert(0, group(ROLL_GROUP, vec![row]));
        }
        listing
    }
}

/// What a roll row says on hover when nothing refuses it.
pub const ROLL_KEEPS: &str = "Keeps this tab's workspace and drawings";

/// A market to put an open tab on, and the footer's changes, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchChoice {
    pub tab_id: u64,
    pub feed_id: String,
    pub symbol: String,
    /// The bar the trader picked, as `kind:size`; `None` keeps the tab's.
    pub bar: Option<String>,
    /// The workspace the trader picked; `None` keeps the tab's.
    pub workspace: Option<String>,
    /// Add the symbol to the venue's list first.
    pub add: bool,
}

/// The toast after a switch.
#[must_use]
pub fn switch_toast(symbol: &str) -> String {
    format!("Switched this tab to {symbol} \u{b7} drawings stay with each market")
}

/// The toast after a roll.
#[must_use]
pub fn roll_toast(target: &str) -> String {
    format!("Rolled to {target} \u{b7} workspace and drawings kept")
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

/// Whether a row answers `needle` (lower-cased): its symbol, venue or asset,
/// or one of its asset's aliases. Every row the query filters asks this.
fn row_matches(world: &MarketWorld<'_>, row: &MarketRow, needle: &str) -> bool {
    [&row.symbol, &row.venue, &row.asset]
        .into_iter()
        .any(|text| text.to_lowercase().contains(needle))
        || alias_hit(world, &row.asset, needle)
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
fn browse(world: &MarketWorld<'_>) -> Vec<MarketGroup> {
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
    groups
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
        context_bars: Vec::new(),
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
    /// The bar the tab opens on, as `kind:size`.
    pub bar: String,
    /// The context charts the tab opens on, top first: the market's own (see
    /// [`market_context_bars`]); empty takes what any new tab takes.
    pub context_bars: Vec<String>,
    /// The market has no bar of its own and nobody named one: `bar` is the
    /// active tab's, shown, and the open names none so the new tab takes it
    /// the way every new tab does.
    pub inherit_bar: bool,
    pub workspace: String,
    pub background: bool,
}

impl OpenChoice {
    /// The workspace a new tab is to join, when it is not Default (which a
    /// new tab joins by itself). The window's book adopts a tab only once
    /// its panes stand, so the open leaves this as the tab's arrival rather
    /// than setting it on a tab the book does not hold yet.
    #[must_use]
    pub fn arrival(&self) -> Option<String> {
        (self.workspace != DEFAULT_WORKSPACE).then(|| self.workspace.clone())
    }
}

/// The toast after a focus: `Focused WINZ26 · tab 1`.
#[must_use]
pub fn focus_toast(symbol: &str, position: usize) -> String {
    format!("Focused {symbol} \u{b7} tab {position}")
}

/// The toast after an open: `Opened BTCUSDT · tick 50 · Swing`, or
/// [`focus_toast`] when `focused` names the tab.
#[must_use]
pub fn open_toast(choice: &OpenChoice, focused: Option<usize>) -> String {
    if let Some(position) = focused {
        return focus_toast(&choice.symbol, position);
    }
    let lead = if choice.background {
        "Opened in the background \u{b7}"
    } else {
        "Opened"
    };
    format!(
        "{lead} {} \u{b7} {} \u{b7} {}",
        choice.symbol,
        choice.bar.replacen(':', " ", 1),
        choice.workspace
    )
}

/// What a key in the palette asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteIntent {
    /// Open, or focus the tab that already shows exactly this.
    Open(OpenChoice),
    /// Bring this tab forward; a replay row's Enter, so it never opens a
    /// live tab.
    Focus {
        tab_id: u64,
        symbol: String,
    },
    /// Add the symbol to the venue's list, then open it. May be refused.
    Add(OpenChoice),
    Remove {
        feed_id: String,
        symbol: String,
    },
    /// Switch mode: put the tab on another market.
    Switch(SwitchChoice),
    /// Switch mode: roll the tab onto the contract its feed names next.
    Roll {
        tab_id: u64,
        from_symbol: String,
    },
}

/// The palette while it is on screen: the query, the selection and the
/// footer's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketPalette {
    query: String,
    /// Index among the selectable rows; `None` until the trader moves it,
    /// which reads as the first selectable row that is not a roll.
    selected: Option<usize>,
    focus: PaletteFocus,
    /// The stop whose widget should take the keyboard, once: set when the
    /// palette opens and on Tab, so a click elsewhere is not pulled back.
    focus_request: Option<PaletteFocus>,
    /// The trader's bar, as `kind:size`; `None` follows the selected row.
    bar: Option<String>,
    workspace: String,
    refusal: Option<String>,
    /// Switch mode: the tab Enter switches, rather than opening a tab.
    switching: Option<SwitchTarget>,
}

impl Default for MarketPalette {
    fn default() -> Self {
        Self {
            query: String::new(),
            selected: None,
            focus: PaletteFocus::Field,
            focus_request: Some(PaletteFocus::Field),
            bar: None,
            workspace: DEFAULT_WORKSPACE.to_owned(),
            refusal: None,
            switching: None,
        }
    }
}

impl MarketPalette {
    /// The palette in switch mode, its footer on the tab's bar and workspace.
    #[must_use]
    pub fn switching(target: SwitchTarget) -> Self {
        Self {
            workspace: target.workspace.clone(),
            switching: Some(target),
            ..Self::default()
        }
    }

    /// The palette as it comes up: on `query`, switching `switching`'s tab
    /// when it names one.
    #[must_use]
    pub fn opened(query: &str, switching: Option<SwitchTarget>) -> Self {
        let mut palette = switching.map_or_else(Self::default, Self::switching);
        palette.set_query(query);
        palette
    }

    #[must_use]
    pub fn switch_target(&self) -> Option<&SwitchTarget> {
        self.switching.as_ref()
    }

    /// The list for the query in this palette's mode.
    #[must_use]
    pub fn listing(&self, world: &MarketWorld<'_>) -> MarketListing {
        let listing = list_markets(world, &self.query);
        match &self.switching {
            Some(target) => target.listing(world, listing, &self.query),
            None => listing,
        }
    }

    /// What Enter does on `row` in this mode.
    fn action(&self, row: &MarketRow, bar: &str) -> Option<PrimaryAction> {
        match self.switching {
            Some(_) => switch_action(row),
            None => primary_action(row, bar, &self.workspace),
        }
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replace the query; the selection goes back to the first row, and a
    /// refusal about the old query goes.
    pub fn set_query(&mut self, query: &str) {
        if self.query != query {
            query.clone_into(&mut self.query);
            (self.selected, self.refusal) = (None, None);
        }
    }

    /// The editable query, for the text field; call [`Self::settle_query`]
    /// with what it held before.
    pub fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    /// After the field edited the query: a changed query selects the first
    /// row and drops the refusal about the old one.
    pub fn settle_query(&mut self, before: &str) {
        if self.query != before {
            (self.selected, self.refusal) = (None, None);
        }
    }

    /// The row Enter acts on: the one the trader moved to, else the first
    /// selectable row that is not a roll. A reflex Enter never rolls.
    #[must_use]
    pub fn selected_row<'l>(&self, listing: &'l MarketListing) -> Option<&'l MarketRow> {
        (self.selected)
            .and_then(|index| listing.selectable().nth(index))
            .or_else(|| listing.selectable().find(|row| !row.is_roll()))
    }

    /// The selected row's index among the selectable rows.
    fn selected_index(&self, listing: &MarketListing) -> Option<usize> {
        let row = self.selected_row(listing)?;
        (listing.selectable()).position(|candidate| std::ptr::eq(candidate, row))
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
        // Nothing selected (only a roll is left): Down lands on the first
        // row, Up on the last.
        let from =
            (self.selected_index(listing)).map_or(isize::from(delta < 0) - 1, |at| at as isize);
        let index = (from + delta).rem_euclid(count as isize) as usize;
        self.choose(index, listing);
    }

    /// Select the selectable row at `index`; a refusal about another row goes.
    fn choose(&mut self, index: usize, listing: &MarketListing) {
        if self.selected_index(listing) != Some(index) {
            self.refusal = None;
        }
        self.selected = Some(index);
    }

    /// Select `row`, as a hover or click does.
    pub fn select(&mut self, listing: &MarketListing, row: &MarketRow) {
        if let Some(index) = listing
            .selectable()
            .position(|candidate| std::ptr::eq(candidate, row))
        {
            self.choose(index, listing);
        }
    }

    /// The bar Enter opens on: the trader's, else (switching) the tab's, else
    /// the selected row's, else the one a new tab inherits.
    #[must_use]
    pub fn bar(&self, listing: &MarketListing) -> String {
        (self.bar.clone())
            .or_else(|| self.switching.as_ref().map(|target| target.bar.clone()))
            .or_else(|| self.selected_row(listing).and_then(|row| row.bar.clone()))
            .or_else(|| listing.inherited_bar.clone())
            .unwrap_or_default()
    }

    /// The trader picked a bar other than the one the footer started on.
    #[must_use]
    pub fn bar_changed(&self) -> bool {
        let start = self.switching.as_ref().map(|target| &target.bar);
        self.bar.is_some() && self.bar.as_ref() != start
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
        let start = self.switching.as_ref();
        self.workspace != start.map_or(DEFAULT_WORKSPACE, |target| target.workspace.as_str())
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
        self.focus_request = Some(self.focus);
    }

    /// Keep a widget's keyboard focus and `stop` agreeing, as each footer
    /// stop's widget draws: a click on it (`clicked`) moves the stop there.
    /// Answers whether the widget should ask for the keyboard now: once when
    /// its stop changes, after a click, or while it is the stop and nothing
    /// holds the keyboard (`idle`, as after a press on a row) -- never every
    /// frame, so a click elsewhere is not pulled back.
    pub fn settle_focus(&mut self, stop: PaletteFocus, clicked: bool, idle: bool) -> bool {
        if clicked {
            self.adopt_focus(stop);
        }
        self.take_focus_request(stop) || (self.focus == stop && (idle || clicked))
    }

    /// A click gave `stop` the keyboard: follow it, and ask nothing back.
    pub fn adopt_focus(&mut self, stop: PaletteFocus) {
        if self.focus != stop {
            (self.focus, self.focus_request) = (stop, None);
        }
    }

    /// Whether `stop`'s widget should request the keyboard now; answers
    /// true once per open or Tab.
    pub fn take_focus_request(&mut self, stop: PaletteFocus) -> bool {
        let asked = self.focus_request == Some(stop);
        if asked {
            self.focus_request = None;
        }
        asked
    }

    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Show why the last ask was refused, under the field.
    pub fn refuse(&mut self, reason: String) {
        self.refusal = Some(reason);
    }

    /// Enter (or Ctrl+Enter with `background`) on the selected row: what
    /// its [`primary_action`] says. A replay row with no tab has nothing to
    /// open from here.
    pub fn submit(&mut self, listing: &MarketListing, background: bool) -> Option<PaletteIntent> {
        if self.switching.is_some() {
            // Ctrl+Enter is inert: a switch has no background.
            return (!background).then(|| self.submit_switch(listing)).flatten();
        }
        self.refusal = None;
        let row = self.selected_row(listing)?;
        let bar = self.bar(listing);
        let action = primary_action(row, &bar, &self.workspace)?;
        let choice = OpenChoice {
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
            bar,
            context_bars: row.context_bars.clone(),
            inherit_bar: self.bar.is_none() && row.bar.is_none(),
            workspace: self.workspace.clone(),
            background,
        };
        match action {
            PrimaryAction::Open | PrimaryAction::OpenSecond => Some(PaletteIntent::Open(choice)),
            PrimaryAction::AddAndOpen { .. } => Some(PaletteIntent::Add(choice)),
            PrimaryAction::Focus { tab_id, .. } => Some(PaletteIntent::Focus {
                tab_id,
                symbol: choice.symbol,
            }),
            _ => None,
        }
    }

    /// Enter in switch mode: switch the tab to the selected market with the
    /// footer's changes, or roll it.
    fn submit_switch(&mut self, listing: &MarketListing) -> Option<PaletteIntent> {
        self.refusal = None;
        let target = self.switching.as_ref()?;
        let row = self.selected_row(listing)?;
        let action = switch_action(row)?;
        if let PrimaryAction::Roll { .. } = action {
            return Some(PaletteIntent::Roll {
                tab_id: target.tab_id,
                from_symbol: target.symbol.clone(),
            });
        }
        Some(PaletteIntent::Switch(SwitchChoice {
            tab_id: target.tab_id,
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
            bar: self.bar_changed().then(|| self.bar(listing)),
            workspace: self.workspace_changed().then(|| self.workspace.clone()),
            add: matches!(action, PrimaryAction::AddAndSwitch { .. }),
        }))
    }

    /// Delete on the selected row: take that market out of its venue's list.
    /// Asked for any listed market, so one the config declares (or a tab
    /// shows) is refused where the trader can read why, not ignored.
    #[must_use]
    pub fn remove_selected(&self, listing: &MarketListing) -> Option<PaletteIntent> {
        let row = self.selected_row(listing)?;
        let listed = row.kind == VenueKind::Live
            && !matches!(row.status, MarketStatus::Add | MarketStatus::Roll { .. });
        listed.then(|| PaletteIntent::Remove {
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
        })
    }

    /// The query starts with `>`: commands, not markets.
    #[must_use]
    pub fn commands_mode(&self) -> bool {
        self.query.trim_start().starts_with(COMMAND_PREFIX)
    }

    /// The Bars selector's kind and size.
    #[must_use]
    pub fn bar_parts(&self, listing: &MarketListing) -> (String, String) {
        let bar = self.bar(listing);
        let (kind, size) = bar.split_once(':').unwrap_or((&bar, ""));
        (kind.to_owned(), size.to_owned())
    }

    /// What the primary label says Enter does.
    #[must_use]
    pub fn primary_label(&self, listing: &MarketListing) -> String {
        if self.commands_mode() {
            return PrimaryAction::Run.label();
        }
        let bar = self.bar(listing);
        let chosen = self
            .selected_row(listing)
            .and_then(|row| self.action(row, &bar));
        match (chosen, &self.switching) {
            (Some(action), _) => action.label(),
            (None, Some(_)) => "Switch".to_owned(),
            (None, None) => PrimaryAction::Open.label(),
        }
    }

    /// Answer a key. Up and Down move what has focus: the selection, the bar
    /// kind (opening on `default_size`'s size for it) or the workspace.
    pub fn press(
        &mut self,
        key: PaletteKey,
        listing: &MarketListing,
        workspaces: &[String],
        default_size: impl Fn(&str) -> String,
    ) -> Option<PaletteIntent> {
        let step = match key {
            PaletteKey::Tab => {
                self.cycle_focus();
                return None;
            }
            PaletteKey::Enter { background } => return self.submit(listing, background),
            PaletteKey::Delete => return self.remove_selected(listing),
            PaletteKey::Up => -1,
            PaletteKey::Down => 1,
        };
        match self.focus {
            PaletteFocus::Field => self.move_selection(step, listing),
            PaletteFocus::Bars => {
                let (kind, _) = self.bar_parts(listing);
                let next = stepped(&BAR_KINDS, &kind, step);
                self.set_bar_kind(next, &default_size(next));
            }
            PaletteFocus::Workspace => {
                let names: Vec<&str> = workspaces.iter().map(String::as_str).collect();
                let next = stepped(&names, &self.workspace, step).to_owned();
                self.set_workspace(&next);
            }
        }
        None
    }
}

/// Whether Delete in the field takes the selected row out rather than
/// editing the query: only with the cursor at the end of the text and
/// nothing selected, where Delete has nothing to delete. `cursor` is the
/// field's (primary, secondary) character offsets; `None`, no cursor yet,
/// is the end, where the field puts it.
#[must_use]
pub fn delete_removes_row(query: &str, cursor: Option<(usize, usize)>) -> bool {
    let end = query.chars().count();
    cursor.is_none_or(|(primary, secondary)| primary == end && secondary == end)
}

/// A key the palette answers; Escape is the window's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKey {
    Up,
    Down,
    Tab,
    Enter { background: bool },
    Delete,
}

fn stepped<'a>(options: &[&'a str], current: &str, step: isize) -> &'a str {
    let Some(last) = options.len().checked_sub(1) else {
        return "";
    };
    let at = options.iter().position(|option| *option == current);
    let next = at.map_or(0, |at| (at as isize + step).rem_euclid(last as isize + 1));
    options[next as usize]
}

#[cfg(test)]
#[path = "market_list_tests.rs"]
mod market_list_tests;

#[cfg(test)]
#[path = "market_switch_tests.rs"]
mod market_switch_tests;
