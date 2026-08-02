//! Public Hyperliquid perpetual market data for quantick.
//!
//! This crate translates two unauthenticated mainnet feeds into the same
//! domain types used by every other quantick source:
//!
//! - `trades` WebSocket messages become [`quantick_engine::Trade`] values with
//!   the venue-reported aggressor side;
//! - `l2Book` full images become provider-neutral
//!   [`quantick_orderbook::DepthEvent`] snapshots and absolute deltas.
//!
//! Hyperliquid's `tid` is a 50-bit hash and is not monotonic. [`TradeMapper`]
//! therefore deduplicates by the documented globally unique tuple
//! `(block_time, coin, tid)` and assigns a monotonic, session-local `agg_id`.
//! The original venue identity is used for deduplication, never presented as a
//! sequence number it is not.
//!
//! The venue publishes at most 20 L2 levels per side and sends a complete image
//! roughly every half second. [`depth::BookMapper`] labels that limited coverage
//! and uses the deterministic order-book [`quantick_orderbook::SnapshotDiffer`]
//! to publish only actual changes after the first image. A venue timestamp that
//! moves behind the last published event rejects the complete image and ends
//! that connection generation; it is never clamped to an invented time.

pub mod backfill;
pub mod depth;
pub mod reconnect;
pub mod stream;
pub mod wire;

pub use backfill::{
    HYPERLIQUID_INFO_URL, HyperliquidHttp, RecentTradesError, RecentTradesSource,
    fetch_recent_trades,
};
pub use reconnect::Backoff;
pub use stream::{
    HYPERLIQUID_WS_URL, TradeSessionError, run_trade_session, run_trades_with_reconnect,
};
pub use wire::{
    DEFAULT_SEEN_TRADE_CAPACITY, MappedBatch, RawTrade, TradeMapError, TradeMapper, TradeMessage,
    parse_rest_trades, parse_trade_message,
};
