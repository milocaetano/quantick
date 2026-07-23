//! Feed & asset configuration, loaded from a TOML file rather than hard-coded.
//!
//! The chart's feed and symbol selectors are driven entirely by an [`AppConfig`]:
//! which feeds exist, which backend ([`ProviderKind`]) streams each one, which
//! symbols they offer, and what to open on. Nothing about the exchange or the
//! asset lives in code as a constant.
//!
//! Resolution order (see [`load`]): the `QUANTICK_CONFIG` env path, then
//! `quantick.toml` in the working directory, then the built-in default embedded
//! at compile time. An external file that is present but malformed is a hard
//! error — a bad config is surfaced, never silently ignored (data-honesty rule).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The built-in default configuration, compiled into the binary so the app runs
/// with no external file present.
const EMBEDDED_DEFAULT: &str = include_str!("../config/feeds.toml");

/// Environment variable naming an explicit config file path.
pub const CONFIG_ENV: &str = "QUANTICK_CONFIG";

/// Conventional config file name looked up in the working directory.
pub const CONFIG_FILENAME: &str = "quantick.toml";

/// Which backend streams a feed. This is the one place a config string is mapped
/// to a code path; adding a provider means adding a variant here and a matching
/// arm in [`crate::feed::spawn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Binance public aggTrades (REST backfill + live WebSocket).
    Binance,
    /// MetaTrader 5 via the local QuantickBridge EA (see `bridge/mt5/`).
    MetaTrader,
}

impl ProviderKind {
    /// Whether this provider actually streams data today. Future providers
    /// land as config-visible placeholders first, labelled "(soon)" in the UI.
    #[must_use]
    pub fn is_implemented(self) -> bool {
        matches!(self, ProviderKind::Binance | ProviderKind::MetaTrader)
    }
}

/// Aggressor-side policy for MetaTrader feeds. MT5 tick flags are broker-
/// dependent: on the B3 broker probed on 2026-07-23 every tick carried the
/// BUY bit, so trusting flags would chart 100% buys. See the
/// `quantick-feed-mt5` docs for the full story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mt5SideSource {
    /// Infer the side by the tick rule (uptick = buy). The safe default.
    TickRule,
    /// Trust the BUY/SELL tick flags. Only for brokers verified honest
    /// (verify with `tools/mt5/record_ticks.py`).
    Flags,
}

/// Settings for the MetaTrader bridge listener (`[metatrader]` in the TOML).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MetaTraderSettings {
    /// Address the feed listens on; the QuantickBridge EA dials it.
    pub listen_addr: String,
    /// How the aggressor side of each trade is decided.
    pub side_source: Mt5SideSource,
}

impl Default for MetaTraderSettings {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:9100".to_string(),
            side_source: Mt5SideSource::TickRule,
        }
    }
}

/// One selectable feed: a named backend and the symbols it offers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FeedConfig {
    /// Stable identifier, referenced by [`AppConfig::default_feed`] (unique).
    pub id: String,
    /// Human label shown in the feed selector.
    pub name: String,
    /// Which backend streams this feed.
    pub provider: ProviderKind,
    /// Assets offered for this feed; the first is used as a fallback when the
    /// current symbol is not valid for a newly selected feed.
    pub symbols: Vec<String>,
}

/// The whole feed/asset configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AppConfig {
    /// The feed id the chart opens on.
    pub default_feed: String,
    /// The symbol the chart opens on (must belong to `default_feed`).
    pub default_symbol: String,
    /// Every selectable feed.
    pub feeds: Vec<FeedConfig>,
    /// MetaTrader bridge settings; defaults apply when the section is absent.
    #[serde(default)]
    pub metatrader: MetaTraderSettings,
}

impl AppConfig {
    /// The feed with the given id, if any.
    #[must_use]
    pub fn feed(&self, id: &str) -> Option<&FeedConfig> {
        self.feeds.iter().find(|f| f.id == id)
    }

    /// The provider backing feed `id`, if the feed exists.
    #[must_use]
    pub fn provider_of(&self, id: &str) -> Option<ProviderKind> {
        self.feed(id).map(|f| f.provider)
    }

    /// Validate internal consistency: at least one feed, unique ids, non-empty
    /// symbol lists, and a default selection that actually resolves.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message describing the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        if self.feeds.is_empty() {
            return Err("no feeds configured; add at least one [[feeds]] entry".to_string());
        }
        for (i, feed) in self.feeds.iter().enumerate() {
            if feed.id.trim().is_empty() {
                return Err(format!("feed #{i} has an empty id"));
            }
            if feed.symbols.is_empty() {
                return Err(format!("feed '{}' lists no symbols", feed.id));
            }
            if self.feeds.iter().filter(|f| f.id == feed.id).count() > 1 {
                return Err(format!("duplicate feed id '{}'", feed.id));
            }
        }
        let Some(default) = self.feed(&self.default_feed) else {
            return Err(format!(
                "default_feed '{}' is not among the configured feeds",
                self.default_feed
            ));
        };
        if !default.symbols.contains(&self.default_symbol) {
            return Err(format!(
                "default_symbol '{}' is not offered by feed '{}'",
                self.default_symbol, self.default_feed
            ));
        }
        Ok(())
    }
}

/// Where a loaded [`AppConfig`] came from, for honest logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// An explicit path from the [`CONFIG_ENV`] environment variable.
    EnvPath(PathBuf),
    /// The conventional [`CONFIG_FILENAME`] in the working directory.
    WorkingDir(PathBuf),
    /// The built-in default embedded in the binary.
    Embedded,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::EnvPath(p) => write!(f, "{} ({CONFIG_ENV})", p.display()),
            ConfigSource::WorkingDir(p) => write!(f, "{}", p.display()),
            ConfigSource::Embedded => write!(f, "<built-in default>"),
        }
    }
}

/// Something went wrong loading the configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// The file at the given path could not be read.
    Read { path: PathBuf, message: String },
    /// The TOML could not be parsed.
    Parse {
        source: ConfigSource,
        message: String,
    },
    /// The parsed config failed validation.
    Invalid {
        source: ConfigSource,
        message: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read { path, message } => {
                write!(f, "cannot read config '{}': {message}", path.display())
            }
            ConfigError::Parse { source, message } => {
                write!(f, "invalid TOML in config {source}: {message}")
            }
            ConfigError::Invalid { source, message } => {
                write!(f, "config {source} is inconsistent: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parse and validate a config from a TOML string tagged with its `source`.
fn parse(text: &str, source: ConfigSource) -> Result<AppConfig, ConfigError> {
    let config: AppConfig = toml::from_str(text).map_err(|e| ConfigError::Parse {
        source: source.clone(),
        message: e.to_string(),
    })?;
    config.validate().map_err(|message| ConfigError::Invalid {
        source: source.clone(),
        message,
    })?;
    Ok(config)
}

/// Load the config, following the resolution order documented on this module.
///
/// Returns the config together with where it came from. An external file (env
/// path or working-directory file) that is present but unreadable, unparseable,
/// or invalid is a hard error; the embedded default is only used when no external
/// file exists.
///
/// # Errors
///
/// Returns [`ConfigError`] when a present external file cannot be read, parsed,
/// or validated. The embedded default is validated in tests, so it never errors.
pub fn load() -> Result<(AppConfig, ConfigSource), ConfigError> {
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        let path = PathBuf::from(path);
        let source = ConfigSource::EnvPath(path.clone());
        let text = std::fs::read_to_string(&path).map_err(|e| ConfigError::Read {
            path,
            message: e.to_string(),
        })?;
        return Ok((parse(&text, source.clone())?, source));
    }

    let cwd_path = Path::new(CONFIG_FILENAME);
    if cwd_path.is_file() {
        let source = ConfigSource::WorkingDir(cwd_path.to_path_buf());
        let text = std::fs::read_to_string(cwd_path).map_err(|e| ConfigError::Read {
            path: cwd_path.to_path_buf(),
            message: e.to_string(),
        })?;
        return Ok((parse(&text, source.clone())?, source));
    }

    let config = parse(EMBEDDED_DEFAULT, ConfigSource::Embedded)?;
    Ok((config, ConfigSource::Embedded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_default_parses_and_validates() {
        let config = parse(EMBEDDED_DEFAULT, ConfigSource::Embedded).expect("embedded default");
        assert_eq!(config.default_feed, "binance");
        let binance = config.feed("binance").expect("binance feed");
        assert_eq!(binance.provider, ProviderKind::Binance);
        assert!(binance.symbols.contains(&"BTCUSDT".to_string()));
        assert!(binance.symbols.contains(&"ETHUSDT".to_string()));

        let mt5 = config.feed("metatrader").expect("metatrader feed");
        assert_eq!(mt5.provider, ProviderKind::MetaTrader);
        assert!(mt5.symbols.contains(&"WIN$N".to_string()));
        assert_eq!(config.metatrader.side_source, Mt5SideSource::TickRule);
        assert!(!config.metatrader.listen_addr.is_empty());
    }

    #[test]
    fn provider_lookup_by_id() {
        let (config, _) = sample();
        assert_eq!(config.provider_of("binance"), Some(ProviderKind::Binance));
        assert_eq!(config.provider_of("nope"), None);
    }

    #[test]
    fn provider_kind_deserializes_case_insensitively_lowercase() {
        let text = r#"
            default_feed = "mt"
            default_symbol = "EURUSD"
            [[feeds]]
            id = "mt"
            name = "MetaTrader 5"
            provider = "metatrader"
            symbols = ["EURUSD"]
        "#;
        let config = parse(text, ConfigSource::Embedded).unwrap();
        assert_eq!(config.provider_of("mt"), Some(ProviderKind::MetaTrader));
        assert!(ProviderKind::MetaTrader.is_implemented());
        assert!(ProviderKind::Binance.is_implemented());
        // No [metatrader] section: defaults apply.
        assert_eq!(config.metatrader, MetaTraderSettings::default());
    }

    #[test]
    fn metatrader_settings_are_read_from_their_section() {
        let text = r#"
            default_feed = "mt"
            default_symbol = "WIN$N"
            [[feeds]]
            id = "mt"
            name = "MetaTrader 5"
            provider = "metatrader"
            symbols = ["WIN$N"]
            [metatrader]
            listen_addr = "127.0.0.1:9200"
            side_source = "flags"
        "#;
        let config = parse(text, ConfigSource::Embedded).unwrap();
        assert_eq!(config.metatrader.listen_addr, "127.0.0.1:9200");
        assert_eq!(config.metatrader.side_source, Mt5SideSource::Flags);
    }

    #[test]
    fn default_feed_must_exist() {
        let text = r#"
            default_feed = "ghost"
            default_symbol = "BTCUSDT"
            [[feeds]]
            id = "binance"
            name = "Binance"
            provider = "binance"
            symbols = ["BTCUSDT"]
        "#;
        let err = parse(text, ConfigSource::Embedded).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
    }

    #[test]
    fn default_symbol_must_belong_to_default_feed() {
        let text = r#"
            default_feed = "binance"
            default_symbol = "DOGEUSDT"
            [[feeds]]
            id = "binance"
            name = "Binance"
            provider = "binance"
            symbols = ["BTCUSDT", "ETHUSDT"]
        "#;
        let err = parse(text, ConfigSource::Embedded).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
    }

    #[test]
    fn duplicate_feed_ids_are_rejected() {
        let text = r#"
            default_feed = "binance"
            default_symbol = "BTCUSDT"
            [[feeds]]
            id = "binance"
            name = "Binance"
            provider = "binance"
            symbols = ["BTCUSDT"]
            [[feeds]]
            id = "binance"
            name = "Binance 2"
            provider = "binance"
            symbols = ["ETHUSDT"]
        "#;
        let err = parse(text, ConfigSource::Embedded).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
    }

    #[test]
    fn empty_feeds_are_rejected() {
        let text = r#"
            default_feed = "binance"
            default_symbol = "BTCUSDT"
            feeds = []
        "#;
        let err = parse(text, ConfigSource::Embedded).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
    }

    #[test]
    fn unknown_provider_is_a_parse_error() {
        let text = r#"
            default_feed = "x"
            default_symbol = "Y"
            [[feeds]]
            id = "x"
            name = "X"
            provider = "kraken"
            symbols = ["Y"]
        "#;
        let err = parse(text, ConfigSource::Embedded).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    }

    /// A minimal valid config for lookups in tests.
    fn sample() -> (AppConfig, ConfigSource) {
        let text = r#"
            default_feed = "binance"
            default_symbol = "BTCUSDT"
            [[feeds]]
            id = "binance"
            name = "Binance"
            provider = "binance"
            symbols = ["BTCUSDT", "ETHUSDT"]
        "#;
        (
            parse(text, ConfigSource::Embedded).unwrap(),
            ConfigSource::Embedded,
        )
    }
}
