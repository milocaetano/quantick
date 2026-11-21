//! Shipped configuration builders for tests in consumer crates.

use crate::config::{AppConfig, EMBEDDED_DEFAULT, FeedConfig};

#[derive(Clone)]
pub struct AppConfigBuilder(AppConfig);

impl AppConfigBuilder {
    #[must_use]
    pub fn shipped() -> Self {
        Self(toml::from_str(EMBEDDED_DEFAULT).expect("the shipped feed fixture is valid"))
    }
    #[must_use]
    pub fn default_market(mut self, feed: &str, symbol: &str) -> Self {
        self.0.default_feed = feed.to_owned();
        self.0.default_symbol = symbol.to_owned();
        self
    }
    #[must_use]
    pub fn feeds(mut self, feeds: Vec<FeedConfig>) -> Self {
        self.0.feeds = feeds;
        self
    }
    #[must_use]
    pub fn build(&self) -> AppConfig {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_builder_overrides_without_mutating_repeated_builds() {
        let builder = AppConfigBuilder::shipped().default_market("binance", "BTCUSDT");
        assert_eq!(builder.build().default_symbol, "BTCUSDT");
        assert!(!builder.build().feeds.is_empty());
    }
}
