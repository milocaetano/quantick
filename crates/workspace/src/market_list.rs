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
//!
//! Each layer reads only the ones above it: `copy` (the fixed words),
//! `world` (what the list is told), `rows` (what it shows), `search` (finding
//! markets), `actions` (what Enter does) and `palette` (the palette on
//! screen and its keys).

pub use crate::workspace_book::DEFAULT_WORKSPACE;

mod actions;
mod copy;
mod palette;
mod rows;
mod search;
mod world;

pub use actions::{
    OpenChoice, PrimaryAction, RollOffer, SwitchChoice, SwitchTarget, focus_toast, open_toast,
    primary_action, switch_action,
};
pub use copy::{
    BAR_KINDS, COMMAND_PREFIX, COMMANDS_GROUP, MARKETS_GROUP, PLACEHOLDER, RECENT_CLOSED_LIMIT,
    RECENT_GROUP, REPLAY_CHIP, REPLAY_VENUE, ROLL_GROUP, ROLL_KEEPS, START_HERE_GROUP,
    no_match_heading, roll_toast, switch_toast,
};
pub use palette::{MarketPalette, PaletteFocus, PaletteIntent, PaletteKey, delete_removes_row};
pub use rows::{MarketGroup, MarketListing, MarketRow, MarketStatus, StatusTone};
pub use search::{focus_target, list_markets, market_bar, market_context_bars, removal_refusal};
pub use world::{
    CatalogMarket, ClosedMarket, MarketSnapshot, MarketWorld, OpenMarket, RecentMarkets, Venue,
    VenueKind,
};

#[cfg(test)]
mod market_list_tests;

#[cfg(test)]
mod market_switch_tests;
