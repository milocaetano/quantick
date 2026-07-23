//! Binance Spot diff-depth support.
//!
//! This module is deliberately separate from the existing aggTrade pipeline.
//! It owns Binance-specific wire decoding, REST snapshot retrieval and the
//! snapshot + diff synchronization protocol. Consumers receive typed status
//! and book events, while the deterministic order-book state lives in
//! `quantick-orderbook`.

pub mod reconnect;
pub mod snapshot;
pub mod stream;
pub mod sync;
pub mod wire;

pub use reconnect::run_depth_with_reconnect;
pub use snapshot::{
    BINANCE_DEPTH_REST_BASE, BinanceDepthHttp, DepthSnapshotError, DepthSnapshotSource,
    MAX_DEPTH_LIMIT,
};
pub use stream::{
    DEFAULT_DEPTH_BUFFER_CAPACITY, DepthEvent, DepthResyncReason, DepthSessionConfig,
    DepthSessionError, DepthStatus, decode_depth_text, depth_stream_url, run_depth_session,
};
pub use sync::{BootstrapOutcome, DepthApplyError, DepthSynchronizer, SyncApplyOutcome, SyncPhase};
pub use wire::{
    DepthLevel, DepthSnapshot, DepthUpdate, DepthWireError, parse_snapshot, parse_update,
};
