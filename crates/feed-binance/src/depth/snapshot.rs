//! REST order-book snapshot retrieval.
//!
//! The source is abstracted behind [`DepthSnapshotSource`] so synchronization
//! tests never need network access.

use tracing::{debug, warn};

use super::wire::{DepthSnapshot, DepthWireError, parse_snapshot};

/// Binance's public REST base URL for current Spot depth.
pub const BINANCE_DEPTH_REST_BASE: &str = "https://api.binance.com";

/// Maximum number of levels Binance returns per side.
pub const MAX_DEPTH_LIMIT: u16 = 5_000;

/// Snapshot fetch/decode failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthSnapshotError {
    /// Transport failure.
    Transport(String),
    /// Non-success HTTP response.
    HttpStatus {
        /// Numeric HTTP status.
        status: u16,
        /// Response body, retained for diagnostics.
        body: String,
    },
    /// Successful response with an invalid depth payload.
    Decode(DepthWireError),
}

impl std::fmt::Display for DepthSnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "depth snapshot transport error: {error}"),
            Self::HttpStatus { status, body } => {
                write!(f, "depth snapshot HTTP {status}: {body}")
            }
            Self::Decode(error) => write!(f, "depth snapshot decode error: {error}"),
        }
    }
}

impl std::error::Error for DepthSnapshotError {}

/// A source of current Binance-style depth snapshots.
#[allow(async_fn_in_trait)]
pub trait DepthSnapshotSource {
    /// Fetch up to `limit` current levels per side for `symbol`.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, HTTP-status or decode failure.
    async fn fetch_depth(
        &self,
        symbol: &str,
        limit: u16,
    ) -> Result<DepthSnapshot, DepthSnapshotError>;
}

/// Real public Binance REST snapshot source.
#[derive(Debug, Clone)]
pub struct BinanceDepthHttp {
    base_url: String,
    client: reqwest::Client,
}

impl BinanceDepthHttp {
    /// Source pointing at the public Binance REST endpoint.
    #[must_use]
    pub fn new() -> Self {
        Self::with_base_url(BINANCE_DEPTH_REST_BASE)
    }

    /// Source pointing at a custom base URL.
    #[must_use]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl Default for BinanceDepthHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl DepthSnapshotSource for BinanceDepthHttp {
    async fn fetch_depth(
        &self,
        symbol: &str,
        limit: u16,
    ) -> Result<DepthSnapshot, DepthSnapshotError> {
        let limit = limit.clamp(1, MAX_DEPTH_LIMIT);
        let url = format!("{}/api/v3/depth", self.base_url);
        debug!(
            target: "quantick::depth",
            schema_version = 1_u8,
            event_code = "depth_snapshot_request",
            symbol,
            limit,
            action = "fetch",
            "fetching Binance depth snapshot"
        );
        let response = self
            .client
            .get(url)
            .query(&[("symbol", symbol), ("limit", &limit.to_string())])
            .send()
            .await
            .map_err(|error| DepthSnapshotError::Transport(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| DepthSnapshotError::Transport(error.to_string()))?;
        if !status.is_success() {
            warn!(
                target: "quantick::depth",
                schema_version = 1_u8,
                event_code = "depth_snapshot_http_error",
                symbol,
                status = status.as_u16(),
                action = "retry",
                "Binance depth snapshot returned non-success"
            );
            return Err(DepthSnapshotError::HttpStatus {
                status: status.as_u16(),
                body,
            });
        }
        parse_snapshot(&body).map_err(DepthSnapshotError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_constants_match_binance_contract() {
        assert_eq!(MAX_DEPTH_LIMIT, 5_000);
        assert_eq!(BINANCE_DEPTH_REST_BASE, "https://api.binance.com");
    }
}
