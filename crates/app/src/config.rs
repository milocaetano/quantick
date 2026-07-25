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

    /// What this provider's backend can do.
    ///
    /// The UI asks the capability, never the provider name: a feature gate
    /// written as "is this Binance?" has to be found and edited every time a
    /// venue is added, and silently withholds a feature the new venue supports.
    #[must_use]
    pub fn capabilities(self) -> FeedCapabilities {
        match self {
            ProviderKind::Binance => FeedCapabilities {
                book_capture: true,
                history_paging: true,
            },
            // The bridge streams the terminal's Depth of Market. Whether a
            // given session really has one (symbol, account, EA version) is
            // runtime information the feed reports honestly; it is not
            // something to assume either way from here.
            ProviderKind::MetaTrader => FeedCapabilities {
                book_capture: true,
                history_paging: false,
            },
        }
    }
}

/// What a feed's backend can actually do, so UI affordances follow capability
/// instead of a hard-coded list of providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedCapabilities {
    /// Can stream synchronized L2 depth for the order-flow heatmap.
    pub book_capture: bool,
    /// Can fetch trades older than what is loaded, on demand.
    pub history_paging: bool,
}

impl FeedCapabilities {
    /// Nothing is available — the honest answer for a feed that does not
    /// resolve to a provider.
    #[must_use]
    pub fn none() -> Self {
        Self {
            book_capture: false,
            history_paging: false,
        }
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
    /// Address the feed listens on; a bridge dials it.
    pub listen_addr: String,
    /// How the aggressor side of each trade is decided.
    pub side_source: Mt5SideSource,
    /// Whether quantick starts a bridge itself when none dials in.
    ///
    /// Selecting a MetaTrader feed should be one action, not two. With this on,
    /// the chart waits briefly for a bridge that is already running (a
    /// hand-started script, or the Expert Advisor sitting on a chart) and only
    /// then launches its own — so turning it on never fights a setup that
    /// already works.
    pub bridge_autostart: bool,
    /// The bridge to launch, as program plus arguments.
    ///
    /// `--symbol`, `--host` and `--port` are appended from the running feed, so
    /// this never has to repeat what quantick already knows. Resolved against
    /// quantick's working directory.
    pub bridge_command: Vec<String>,
}

impl Default for MetaTraderSettings {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:9100".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: true,
            bridge_command: vec![
                "python".to_string(),
                "bridge/mt5/quantick_bridge.py".to_string(),
            ],
        }
    }
}

impl MetaTraderSettings {
    /// Host and port a bridge should dial, parsed from [`listen_addr`](Self::listen_addr).
    ///
    /// Returns `None` when the address has no `host:port` shape — the autostart
    /// then stays off rather than launching a bridge that cannot reach us.
    #[must_use]
    pub fn bridge_endpoint(&self) -> Option<(&str, &str)> {
        let (host, port) = self.listen_addr.rsplit_once(':')?;
        if host.is_empty() || port.parse::<u16>().is_err() {
            return None;
        }
        // A wildcard bind is not an address to dial; loopback is what a local
        // bridge actually reaches.
        let host = if host == "0.0.0.0" || host == "[::]" {
            "127.0.0.1"
        } else {
            host
        };
        Some((host, port))
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
    fn the_bridge_dial_address_comes_from_the_listen_address() {
        let at = |addr: &str| MetaTraderSettings {
            listen_addr: addr.to_string(),
            ..MetaTraderSettings::default()
        };
        assert_eq!(
            at("127.0.0.1:9100").bridge_endpoint(),
            Some(("127.0.0.1", "9100"))
        );
        // A wildcard bind is not something a bridge can dial.
        assert_eq!(
            at("0.0.0.0:9100").bridge_endpoint(),
            Some(("127.0.0.1", "9100"))
        );
        // Nothing dial-able: the caller must not launch a bridge that cannot
        // reach us, so there is no address to hand it.
        assert_eq!(at("9100").bridge_endpoint(), None);
        assert_eq!(at("127.0.0.1:").bridge_endpoint(), None);
        assert_eq!(at(":9100").bridge_endpoint(), None);
        assert_eq!(at("127.0.0.1:not-a-port").bridge_endpoint(), None);
    }

    #[test]
    fn bridge_autostart_is_on_by_default_and_overridable() {
        let defaults = MetaTraderSettings::default();
        assert!(defaults.bridge_autostart);
        assert_eq!(defaults.bridge_command.first().unwrap(), "python");

        let text = r#"
            default_feed = "mt"
            default_symbol = "WINQ26"
            [[feeds]]
            id = "mt"
            name = "MetaTrader 5"
            provider = "metatrader"
            symbols = ["WINQ26"]
            [metatrader]
            bridge_autostart = false
            bridge_command = ["py", "-3", "bridge/mt5/quantick_bridge.py"]
        "#;
        let config = parse(text, ConfigSource::Embedded).unwrap();
        assert!(!config.metatrader.bridge_autostart);
        assert_eq!(
            config.metatrader.bridge_command,
            ["py", "-3", "bridge/mt5/quantick_bridge.py"]
        );
        // Untouched keys keep their defaults.
        assert_eq!(config.metatrader.listen_addr, "127.0.0.1:9100");
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
