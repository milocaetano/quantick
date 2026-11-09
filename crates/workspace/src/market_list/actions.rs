//! What Enter does on a row: open, focus, add, switch or roll, the tab a
//! switch-mode palette switches, and the choices and toasts those leave.

use super::copy::{COMMAND_PREFIX, ROLL_GROUP, ROLL_KEEPS};
use super::rows::{MarketListing, MarketRow, MarketStatus, catalog_row, group};
use super::search::{focus_target, row_matches};
use super::world::{MarketSnapshot, MarketWorld, OpenMarket, VenueKind, find_market};
use crate::workspace_book::DEFAULT_WORKSPACE;

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
    pub(super) fn listing(
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

/// A market, a bar and a workspace to open it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenChoice {
    pub feed_id: String,
    pub symbol: String,
    /// The bar the tab opens on, as `kind:size`.
    pub bar: String,
    /// The context charts the tab opens on, top first: the market's own (see
    /// [`market_context_bars`](super::market_context_bars)); empty takes what
    /// any new tab takes.
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

impl MarketSnapshot {
    /// What `market.open` and Enter open: the market's own bar (see
    /// [`market_bar`](super::market_bar)) and Default when left out. Refuses
    /// a market the config does not offer and a workspace with no such name.
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
