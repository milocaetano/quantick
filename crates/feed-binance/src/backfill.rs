//! REST backfill of recent historical aggTrades.
//!
//! On startup the chart opens already populated: this fetches the most recent
//! `target` aggregate trades over Binance's public REST endpoint and maps them
//! to engine [`Trade`]s, in ascending `agg_id` order, ready to replay through a
//! bar builder before the live stream takes over.
//!
//! The HTTP layer is abstracted behind [`AggTradeSource`] so the paging and
//! ordering logic is unit-testable without a network; [`BinanceHttp`] is the
//! real implementation, exercised by an `#[ignore]`d live test.

use std::collections::BTreeMap;

use quantick_engine::Trade;
use tracing::{debug, info, instrument, warn};

use crate::wire::{self, AggTrade, MapError};

/// Binance's public REST base URL.
pub const BINANCE_REST_BASE: &str = "https://api.binance.com";

/// Binance caps `limit` on `aggTrades` at 1000.
pub const MAX_PAGE_LIMIT: u32 = 1000;

/// Something went wrong fetching or decoding feed data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// Transport / HTTP-status error (message preserved, reqwest not leaked).
    Http(String),
    /// The response body was not valid aggTrade JSON.
    Decode(String),
    /// A decoded aggTrade could not be mapped to a `Trade`.
    Map(MapError),
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::Http(m) => write!(f, "http error: {m}"),
            FeedError::Decode(m) => write!(f, "decode error: {m}"),
            FeedError::Map(e) => write!(f, "mapping error: {e}"),
        }
    }
}

impl std::error::Error for FeedError {}

impl From<MapError> for FeedError {
    fn from(e: MapError) -> Self {
        FeedError::Map(e)
    }
}

/// A source of aggTrade pages, following Binance's `aggTrades` semantics.
///
/// `fetch(symbol, from_id, limit)`: with `from_id = None`, returns the most
/// recent `limit` trades; with `from_id = Some(f)`, returns up to `limit` trades
/// with `agg_id >= f`. Results are ascending by `agg_id`.
///
/// The `async fn` here is only ever used through static dispatch (the backfill
/// function is generic over the source), so the future's `Send`-ness is decided
/// at the concrete call site — `BinanceHttp`'s is `Send`, so backfill can be
/// spawned onto a multi-thread runtime.
#[allow(async_fn_in_trait)]
pub trait AggTradeSource {
    /// Fetch one page of aggTrades. See the [trait docs](AggTradeSource).
    ///
    /// # Errors
    ///
    /// Returns [`FeedError`] on transport, status, or decode failure.
    async fn fetch(
        &self,
        symbol: &str,
        from_id: Option<u64>,
        limit: u32,
    ) -> Result<Vec<AggTrade>, FeedError>;
}

/// The real Binance REST source.
#[derive(Debug, Clone)]
pub struct BinanceHttp {
    base_url: String,
    client: reqwest::Client,
}

impl BinanceHttp {
    /// A source pointing at the public Binance REST base URL.
    #[must_use]
    pub fn new() -> Self {
        Self::with_base_url(BINANCE_REST_BASE)
    }

    /// A source pointing at a custom base URL (for a proxy or a test server).
    #[must_use]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl Default for BinanceHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl AggTradeSource for BinanceHttp {
    async fn fetch(
        &self,
        symbol: &str,
        from_id: Option<u64>,
        limit: u32,
    ) -> Result<Vec<AggTrade>, FeedError> {
        let limit = limit.min(MAX_PAGE_LIMIT);
        let url = format!("{}/api/v3/aggTrades", self.base_url);
        let mut query: Vec<(&str, String)> =
            vec![("symbol", symbol.to_string()), ("limit", limit.to_string())];
        if let Some(f) = from_id {
            query.push(("fromId", f.to_string()));
        }
        debug!(target: "quantick::feed", symbol, ?from_id, limit, "fetching aggTrades page");

        let resp = self
            .client
            .get(&url)
            .query(&query)
            .send()
            .await
            .map_err(|e| FeedError::Http(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| FeedError::Http(e.to_string()))?;
        if !status.is_success() {
            warn!(target: "quantick::feed", %status, symbol, "aggTrades request returned non-success");
            return Err(FeedError::Http(format!("HTTP {status}: {body}")));
        }
        wire::parse_rest(&body).map_err(|e| FeedError::Decode(e.to_string()))
    }
}

/// Backfill the most recent `target` trades for `symbol`, ascending by agg_id.
///
/// Pages backward from the newest batch, deduping by `agg_id` (a [`BTreeMap`]
/// keeps the merge ordered and deterministic), until `target` trades are
/// collected or the earliest available trade is reached. Returns fewer than
/// `target` if the symbol has less history.
///
/// # Errors
///
/// Returns [`FeedError`] on any fetch/decode/map failure. Mapping fails fast: a
/// malformed price is surfaced, never silently dropped (data-honesty rule).
#[instrument(target = "quantick::feed", skip(source), fields(pages = tracing::field::Empty))]
pub async fn backfill<S: AggTradeSource>(
    source: &S,
    symbol: &str,
    target: usize,
) -> Result<Vec<Trade>, FeedError> {
    let mut collected: BTreeMap<u64, AggTrade> = BTreeMap::new();
    let mut pages = 0u32;

    // Newest batch first (no from_id => most recent trades).
    let newest = source.fetch(symbol, None, MAX_PAGE_LIMIT).await?;
    pages += 1;
    for t in newest {
        collected.insert(t.agg_id, t);
    }

    // Page backward until we have enough or run out of history.
    while collected.len() < target {
        let Some(&earliest) = collected.keys().next() else {
            break; // nothing at all was returned
        };
        if earliest == 0 {
            break; // reached the very first trade
        }
        let from = earliest.saturating_sub(u64::from(MAX_PAGE_LIMIT));
        let batch = source.fetch(symbol, Some(from), MAX_PAGE_LIMIT).await?;
        pages += 1;
        let before = collected.len();
        for t in batch {
            collected.entry(t.agg_id).or_insert(t);
        }
        if collected.len() == before {
            debug!(target: "quantick::feed", "no new trades from page; stopping backfill");
            break; // no progress; avoid looping forever
        }
    }

    // Keep the most recent `target`, ascending, and map to engine trades.
    let all: Vec<&AggTrade> = collected.values().collect();
    let start = all.len().saturating_sub(target);
    let trades = all[start..]
        .iter()
        .map(|a| a.to_trade())
        .collect::<Result<Vec<Trade>, _>>()?;

    tracing::Span::current().record("pages", pages);
    info!(
        target: "quantick::feed",
        symbol,
        target,
        fetched = trades.len(),
        pages,
        "backfill complete"
    );
    Ok(trades)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory source that mimics Binance's `aggTrades` paging semantics.
    struct FakeSource {
        /// Trades ascending by agg_id.
        trades: Vec<AggTrade>,
    }

    impl FakeSource {
        /// `count` trades with agg_ids `1..=count`.
        fn with_count(count: u64) -> Self {
            let trades = (1..=count)
                .map(|id| AggTrade {
                    agg_id: id,
                    price: "100.0".to_string(),
                    quantity: "1.0".to_string(),
                    trade_time_ms: 1_000 + id as i64,
                    is_buyer_maker: id % 2 == 0,
                })
                .collect();
            Self { trades }
        }
    }

    impl AggTradeSource for FakeSource {
        async fn fetch(
            &self,
            _symbol: &str,
            from_id: Option<u64>,
            limit: u32,
        ) -> Result<Vec<AggTrade>, FeedError> {
            let limit = limit as usize;
            let page = match from_id {
                None => {
                    let start = self.trades.len().saturating_sub(limit);
                    self.trades[start..].to_vec()
                }
                Some(f) => self
                    .trades
                    .iter()
                    .filter(|t| t.agg_id >= f)
                    .take(limit)
                    .cloned()
                    .collect(),
            };
            Ok(page)
        }
    }

    #[tokio::test]
    async fn backfill_within_one_page_returns_the_most_recent() {
        let source = FakeSource::with_count(500);
        let trades = backfill(&source, "BTCUSDT", 50).await.unwrap();
        assert_eq!(trades.len(), 50);
        assert_eq!(trades.first().unwrap().agg_id, 451);
        assert_eq!(trades.last().unwrap().agg_id, 500);
    }

    #[tokio::test]
    async fn backfill_pages_backward_across_multiple_requests() {
        let source = FakeSource::with_count(2500);
        let trades = backfill(&source, "BTCUSDT", 1500).await.unwrap();
        assert_eq!(trades.len(), 1500);
        // Most recent 1500 of 2500 = ids 1001..=2500, strictly ascending.
        assert_eq!(trades.first().unwrap().agg_id, 1001);
        assert_eq!(trades.last().unwrap().agg_id, 2500);
        for pair in trades.windows(2) {
            assert!(pair[1].agg_id > pair[0].agg_id);
        }
    }

    #[tokio::test]
    async fn backfill_returns_all_when_history_is_short() {
        let source = FakeSource::with_count(30);
        let trades = backfill(&source, "BTCUSDT", 1000).await.unwrap();
        assert_eq!(trades.len(), 30);
    }

    #[tokio::test]
    async fn backfill_of_empty_symbol_is_empty() {
        let source = FakeSource::with_count(0);
        let trades = backfill(&source, "BTCUSDT", 100).await.unwrap();
        assert!(trades.is_empty());
    }

    #[tokio::test]
    async fn map_error_fails_fast() {
        let source = FakeSource {
            trades: vec![AggTrade {
                agg_id: 1,
                price: "oops".to_string(),
                quantity: "1.0".to_string(),
                trade_time_ms: 0,
                is_buyer_maker: false,
            }],
        };
        let err = backfill(&source, "BTCUSDT", 10).await.unwrap_err();
        assert!(matches!(err, FeedError::Map(_)), "{err}");
    }

    #[tokio::test]
    #[ignore = "hits the live Binance REST API; run manually with --ignored"]
    async fn live_backfill_returns_ordered_trades() {
        let source = BinanceHttp::new();
        let trades = backfill(&source, "BTCUSDT", 50).await.unwrap();
        assert!(!trades.is_empty());
        for pair in trades.windows(2) {
            assert!(pair[1].agg_id > pair[0].agg_id);
        }
    }
}
