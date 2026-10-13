//! What a market-data source is, below the host that runs one.
//!
//! [`config`] is the feed-shaped half of the cockpit's configuration: which
//! backend streams a feed, what it can report, how the MetaTrader listener is
//! addressed. [`history_reach`] is how far one press of *load older* reaches,
//! as a campaign told what the chart holds. [`recovery`] names the two ways
//! an operator revives a stalled feed. [`ohlcv`], [`candles`] and
//! [`candle_base`] are the venue-candle vocabulary: the bases a chart folds
//! from, the folds themselves, and which base a chart is after. All are headless — no runtime,
//! no thread, no clock — so the config document in `quantick-stores` and the
//! feed host in `quantick-feed` share them without the first linking the
//! second's network stack.

pub mod candle_base;
pub mod candles;
pub mod config;
pub mod history_reach;
pub mod history_run;
pub mod ohlcv;
pub mod recovery;
pub mod trade_admission;
