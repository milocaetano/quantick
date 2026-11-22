//! Shipped configuration builders for tests in consumer crates.

use crate::config::{AppConfig, EMBEDDED_DEFAULT};

#[derive(Clone)]
pub struct AppConfigBuilder(AppConfig);

impl AppConfigBuilder {
    #[must_use]
    pub fn shipped() -> Self {
        Self(toml::from_str(EMBEDDED_DEFAULT).expect("the shipped feed fixture is valid"))
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
    fn shipped_builder_repeats_the_valid_fixture() {
        let builder = AppConfigBuilder::shipped();
        assert!(!builder.build().feeds.is_empty());
    }
}
