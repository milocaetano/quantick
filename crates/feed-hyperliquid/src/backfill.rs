//! Recent public trades from Hyperliquid's `info` endpoint.
//!
//! `recentTrades` is a short recovery window, not pageable history. This
//! optional utility remains available for consumers that need an explicit HTTP
//! sample; the desktop app uses the larger initial WebSocket recovery batch.

use std::time::Duration;

use tracing::{debug, warn};

use crate::wire::{RawTrade, parse_rest_trades};

/// Hyperliquid mainnet public info endpoint.
pub const HYPERLIQUID_INFO_URL: &str = "https://api.hyperliquid.xyz/info";

/// Bound startup recovery so an unavailable REST endpoint cannot prevent the
/// independently useful live WebSocket from starting forever.
const RECENT_TRADES_TIMEOUT: Duration = Duration::from_secs(10);

/// Failure fetching or decoding the recent-trades window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecentTradesError {
    /// Transport or non-success HTTP response.
    Http(String),
    /// Response body was not a trade array.
    Decode(String),
}

impl std::fmt::Display for RecentTradesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(message) => write!(f, "http error: {message}"),
            Self::Decode(message) => write!(f, "decode error: {message}"),
        }
    }
}

impl std::error::Error for RecentTradesError {}

/// Source abstraction for deterministic, network-free tests.
#[allow(async_fn_in_trait)]
pub trait RecentTradesSource {
    /// Fetch the venue's current recent-trades window.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or decode failure.
    async fn fetch(&self, symbol: &str) -> Result<Vec<RawTrade>, RecentTradesError>;
}

/// Real public HTTP source.
#[derive(Debug, Clone)]
pub struct HyperliquidHttp {
    endpoint: String,
    client: reqwest::Client,
}

impl HyperliquidHttp {
    /// Source pointed at mainnet.
    #[must_use]
    pub fn new() -> Self {
        Self::with_endpoint(HYPERLIQUID_INFO_URL)
    }

    /// Source pointed at a custom compatible endpoint.
    #[must_use]
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl Default for HyperliquidHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl RecentTradesSource for HyperliquidHttp {
    async fn fetch(&self, symbol: &str) -> Result<Vec<RawTrade>, RecentTradesError> {
        let symbol = symbol.to_uppercase();
        let request = serde_json::json!({ "type": "recentTrades", "coin": symbol });
        debug!(target: "quantick::feed", symbol, "fetching Hyperliquid recent trades");
        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .timeout(RECENT_TRADES_TIMEOUT)
            .send()
            .await
            .map_err(|error| RecentTradesError::Http(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| RecentTradesError::Http(error.to_string()))?;
        if !status.is_success() {
            warn!(target: "quantick::feed", %status, symbol, "recentTrades request failed");
            return Err(RecentTradesError::Http(format!("HTTP {status}: {body}")));
        }
        parse_rest_trades(&body).map_err(|error| RecentTradesError::Decode(error.to_string()))
    }
}

/// Fetch the short factual startup window, ordered oldest first.
///
/// # Errors
///
/// Returns the source's typed error without substituting candles or estimates.
pub async fn fetch_recent_trades<S: RecentTradesSource>(
    source: &S,
    symbol: &str,
) -> Result<Vec<RawTrade>, RecentTradesError> {
    let mut trades = source.fetch(symbol).await?;
    trades.sort_by_key(|trade| (trade.timestamp_ms, trade.tid));
    Ok(trades)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource;

    impl RecentTradesSource for FakeSource {
        async fn fetch(&self, _symbol: &str) -> Result<Vec<RawTrade>, RecentTradesError> {
            Ok(vec![
                RawTrade {
                    coin: "BTC".to_owned(),
                    side: "A".to_owned(),
                    price: "101".to_owned(),
                    size: "1".to_owned(),
                    timestamp_ms: 2,
                    tid: 20,
                },
                RawTrade {
                    coin: "BTC".to_owned(),
                    side: "B".to_owned(),
                    price: "100".to_owned(),
                    size: "2".to_owned(),
                    timestamp_ms: 1,
                    tid: 10,
                },
            ])
        }
    }

    #[tokio::test]
    async fn recent_window_is_oldest_first() {
        let trades = fetch_recent_trades(&FakeSource, "BTC").await.unwrap();
        assert_eq!(
            trades.iter().map(|trade| trade.tid).collect::<Vec<_>>(),
            [10, 20]
        );
    }
}
