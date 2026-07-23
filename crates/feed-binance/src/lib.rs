//! quantick-feed-binance — live aggTrades feed from Binance public endpoints.
//!
//! Produces the trade stream that `quantick-engine` consumes. Requires no API
//! key: Binance market-data endpoints are public.
//!
//! The [`wire`] module is the pure, deterministic translation layer from
//! Binance's aggTrade JSON to the engine's [`quantick_engine::Trade`]. The
//! [`backfill`] module fetches recent history over REST so the chart opens
//! populated. The live WebSocket stream and reconnect handling build on top in
//! later milestones.

pub mod backfill;
pub mod continuity;
pub mod reconnect;
pub mod stream;
pub mod wire;

pub use backfill::{AggTradeSource, BinanceHttp, FeedError, backfill, backfill_before};
pub use continuity::{Anomaly, ContinuityTracker};
pub use reconnect::{Backoff, run_with_reconnect};
pub use stream::{BINANCE_WS_BASE, agg_trade_url, run_agg_trade_stream};
