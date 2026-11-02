//! Which contract follows which (`contract_rolls`): only the config knows a
//! venue's calendar, so no symbol or expiry rule lives in code.

use super::FeedConfig;

impl FeedConfig {
    /// The contract this feed's config says follows `symbol`.
    #[must_use]
    pub fn next_contract(&self, symbol: &str) -> Option<&str> {
        self.contract_rolls.get(symbol).map(String::as_str)
    }

    /// Typos only. A target the feed does not offer is not one: the symbols
    /// sidecar can take a symbol away at any time, so that is answered when
    /// the roll is asked for.
    pub(super) fn validate_contract_rolls(&self) -> Result<(), String> {
        for (symbol, next) in &self.contract_rolls {
            if next.trim().is_empty() {
                return Err(format!(
                    "feed '{}' rolls symbol '{symbol}' to an empty contract",
                    self.id
                ));
            }
            if next == symbol {
                return Err(format!(
                    "feed '{}' rolls symbol '{symbol}' to itself",
                    self.id
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::AppConfig;

    fn config(rolls: &str) -> Result<AppConfig, String> {
        let config: AppConfig = toml::from_str(&format!(
            r#"
default_feed = "b3"
default_symbol = "AAAZ26"
[[feeds]]
id = "b3"
name = "B3"
provider = "metatrader"
symbols = ["AAAZ26", "AAAG27"]
{rolls}
"#
        ))
        .map_err(|error| error.to_string())?;
        config.validate()?;
        Ok(config)
    }

    #[test]
    fn the_next_contract_comes_from_the_config_and_only_from_it() {
        let config = config(r#"contract_rolls = { "AAAZ26" = "AAAG27" }"#).unwrap();
        let feed = &config.feeds[0];
        assert_eq!(feed.next_contract("AAAZ26"), Some("AAAG27"));
        assert_eq!(feed.next_contract("AAAG27"), None);
    }

    #[test]
    fn a_feed_without_the_key_rolls_nothing() {
        let config = config("").unwrap();
        assert_eq!(config.feeds[0].next_contract("AAAZ26"), None);
    }

    #[test]
    fn a_typo_in_a_roll_is_refused_when_the_config_loads() {
        let empty = config(r#"contract_rolls = { "AAAZ26" = " " }"#).unwrap_err();
        assert!(empty.contains("empty contract"), "{empty}");
        let itself = config(r#"contract_rolls = { "AAAZ26" = "AAAZ26" }"#).unwrap_err();
        assert!(itself.contains("to itself"), "{itself}");
    }
}
