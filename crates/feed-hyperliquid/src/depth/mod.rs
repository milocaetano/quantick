//! Hyperliquid `l2Book` snapshots mapped to quantick's neutral depth stream.

mod mapper;
mod reconnect;
mod stream;
mod wire;

pub use mapper::{BookMapError, BookMapper, BookStats, HYPERLIQUID_LEVELS_PER_SIDE};
pub use reconnect::run_depth_with_reconnect;
pub use stream::{DepthSessionError, run_depth_session};
pub use wire::{BookImage, BookMessage, RawLevel, parse_book_message};

pub use quantick_orderbook::{DepthEvent, DepthResyncReason, DepthStatus};
