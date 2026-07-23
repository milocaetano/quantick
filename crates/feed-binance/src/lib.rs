//! quantick-feed-binance — live aggTrades feed from Binance public endpoints.
//!
//! Produces the trade stream that `quantick-engine` consumes. Requires no API
//! key: Binance market-data endpoints are public.
//!
//! The [`wire`] module is the pure, deterministic translation layer from
//! Binance's aggTrade JSON to the engine's [`quantick_engine::Trade`]. Network
//! code (REST backfill, live WebSocket, reconnect) builds on top of it in later
//! milestones.

pub mod wire;
