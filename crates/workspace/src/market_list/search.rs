//! Finding markets: the list for a query, the browse an empty query shows,
//! the filter every row answers, the tab Enter focuses and why a market
//! cannot leave its venue's list.

use super::copy::{
    COMMAND_PREFIX, COMMANDS_GROUP, MARKETS_GROUP, RECENT_GROUP, START_HERE_GROUP, no_match_heading,
};
use super::rows::{MarketGroup, MarketListing, MarketRow, add_row, catalog_row, group, replay_row};
use super::world::{MarketWorld, OpenMarket, VenueKind, find_market};

/// The bar a market remembers: its first open tab's, else the one its tab
/// last closed on, else its venue's `default_bars`. `None` when it has none
/// (a new tab then takes the active tab's) or the config does not offer it.
#[must_use]
pub fn market_bar(world: &MarketWorld<'_>, feed_id: &str, symbol: &str) -> Option<String> {
    let market = find_market(world, feed_id, symbol)?;
    catalog_row(world, market)?.bar
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

/// Whether a row answers `needle` (lower-cased): its symbol, venue or asset,
/// or one of its asset's aliases. Every row the query filters asks this.
pub(super) fn row_matches(world: &MarketWorld<'_>, row: &MarketRow, needle: &str) -> bool {
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

fn replay_rows<'a>(world: &'a MarketWorld<'a>) -> impl Iterator<Item = MarketRow> + 'a {
    world.open.iter().filter(|tab| tab.replay).map(replay_row)
}
