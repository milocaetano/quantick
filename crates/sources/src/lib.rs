//! What a market-data source is, below the host that runs one.
//!
//! [`config`] is the feed-shaped half of the cockpit's configuration: which
//! backend streams a feed, what it can report, how the MetaTrader listener is
//! addressed. [`history_reach`] is how far one press of *load older* reaches,
//! as a campaign told what the chart holds. Both are headless — no runtime,
//! no thread, no clock — so the config document in `quantick-stores` and the
//! feed host in `quantick-feed` share them without the first linking the
//! second's network stack.

pub mod config;
pub mod history_reach;
