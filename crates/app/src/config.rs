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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The built-in default configuration, compiled into the binary so the app runs
/// with no external file present.
const EMBEDDED_DEFAULT: &str = include_str!("../config/feeds.toml");

/// Environment variable naming an explicit config file path.
pub const CONFIG_ENV: &str = "QUANTICK_CONFIG";

/// Optional startup-only override for [`AppConfig::default_feed`].
///
/// Unlike [`CONFIG_ENV`], this changes only the initial selection; it never
/// replaces the configured feed catalog.
pub const DEFAULT_FEED_ENV: &str = "QUANTICK_DEFAULT_FEED";

/// Optional startup-only override for [`AppConfig::default_symbol`].
///
/// The value is validated against the selected feed before either default is
/// changed, so a bad pair cannot leave the config half-mutated.
pub const DEFAULT_SYMBOL_ENV: &str = "QUANTICK_DEFAULT_SYMBOL";

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
    /// Hyperliquid public perpetual trades and complete L2 images.
    Hyperliquid,
    /// MetaTrader 5 via the local QuantickBridge EA (see `bridge/mt5/`).
    MetaTrader,
}

impl ProviderKind {
    /// Whether this provider actually streams data today. Future providers
    /// land as config-visible placeholders first, labelled "(soon)" in the UI.
    #[must_use]
    pub fn is_implemented(self) -> bool {
        matches!(
            self,
            ProviderKind::Binance | ProviderKind::Hyperliquid | ProviderKind::MetaTrader
        )
    }

    /// What this provider's backend can do before a session says otherwise.
    ///
    /// The UI asks the capability, never the provider name: a feature gate
    /// written as "is this Binance?" has to be found and edited every time a
    /// venue is added, and silently withholds a feature the new venue supports.
    ///
    /// This is the answer for the provider as such; a running feed may narrow
    /// it once it learns what its symbol actually offers (see
    /// [`crate::feed::FeedHandle::capabilities`]).
    #[must_use]
    pub fn capabilities(self) -> FeedCapabilities {
        match self {
            ProviderKind::Binance => FeedCapabilities {
                book_capture: true,
                history_paging: true,
                traded_volume: true,
            },
            // `recentTrades` is a short recovery window, not a pageable
            // historical API. Trades and the visible 20-level book are factual;
            // older history is withheld rather than synthesized from candles.
            ProviderKind::Hyperliquid => FeedCapabilities {
                book_capture: true,
                history_paging: false,
                traded_volume: true,
            },
            // The bridge streams the terminal's Depth of Market. Whether a
            // given session really has one (symbol, account, EA version) is
            // runtime information the feed reports honestly; it is not
            // something to assume either way from here.
            ProviderKind::MetaTrader => FeedCapabilities {
                book_capture: true,
                history_paging: false,
                traded_volume: true,
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
    /// Its prints carry a volume the venue really traded.
    ///
    /// False on a broker-quoted instrument — an index CFD prints nothing, so
    /// the feed charts one synthetic unit per tick (see
    /// `quantick_feed_mt5::TapeKind`). Everything that measures *size* rather
    /// than *movement* has to withhold itself there: a volume bar would just be
    /// a tick bar with a misleading name, and a bubble layer would draw one
    /// identical circle per print.
    pub traded_volume: bool,
}

impl FeedCapabilities {
    /// Nothing is available — the honest answer for a feed that does not
    /// resolve to a provider.
    #[must_use]
    pub fn none() -> Self {
        Self {
            book_capture: false,
            history_paging: false,
            traded_volume: false,
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
    /// Address the feed listens on; a bridge dials it. The endpoint for every
    /// symbol [`ports`](Self::ports) does not name.
    pub listen_addr: String,
    /// Per-symbol listen ports (`[metatrader.ports]` in the TOML).
    ///
    /// MQL5 sockets are client-only, so a MetaTrader "connection" is really an
    /// EA on a chart dialing us, and one port carries one symbol's stream. To
    /// chart XAUUSD and US500 from the same terminal at once, each gets its own
    /// port here and its own EA with the matching `InpPort`.
    ///
    /// A [`BTreeMap`] rather than a hash map so iteration order — and therefore
    /// the order validation reports problems in — is the same on every run.
    pub ports: BTreeMap<String, u16>,
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
            ports: BTreeMap::new(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: true,
            bridge_command: vec![
                "python".to_string(),
                "bridge/mt5/quantick_bridge.py".to_string(),
            ],
        }
    }
}

/// The host a local bridge can always reach, used when the bind address names
/// none it could dial.
const LOOPBACK: &str = "127.0.0.1";

/// Where one symbol's bridge listener lives.
///
/// The two sides of a per-symbol port have to agree: quantick binds it and the
/// EA dials it. Deriving both from one [`MetaTraderSettings::endpoint_for`]
/// call is what keeps the listener and the autostarted bridge's `--port` from
/// drifting apart — a drift whose only symptom is a chart that never fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mt5Endpoint {
    /// Address the feed binds for this symbol.
    pub listen_addr: String,
    /// Host and port a bridge dials to reach it, or `None` when
    /// [`listen_addr`](MetaTraderSettings::listen_addr) has no `host:port`
    /// shape — the autostart then stays off rather than launching a bridge
    /// that cannot reach us.
    pub dial: Option<(String, u16)>,
    /// Whether the port came from [`MetaTraderSettings::ports`] rather than
    /// the shared default. Logged at spawn, because "this symbol was given a
    /// port" and "this symbol fell through to the shared one" are different
    /// answers to why two charts are fighting over one listener.
    pub from_map: bool,
}

/// Split `host:port`, rejecting anything that is not both. The bracketed IPv6
/// form (`[::1]:9100`) splits correctly: only the last colon is a separator.
fn split_host_port(addr: &str) -> Option<(&str, u16)> {
    let (host, port) = addr.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some((host, port.parse().ok()?))
}

/// The address a bridge dials to reach a listener bound to `host`. A wildcard
/// bind is not an address to dial; loopback is what a local bridge reaches.
fn dial_host(host: &str) -> &str {
    if host == "0.0.0.0" || host == "[::]" {
        LOOPBACK
    } else {
        host
    }
}

impl MetaTraderSettings {
    /// Where the bridge for `symbol` listens: its own port when
    /// [`ports`](Self::ports) names one, the [`listen_addr`](Self::listen_addr)
    /// default otherwise.
    ///
    /// A mapped symbol keeps the default address's host and swaps only the
    /// port, so a deployment that binds a specific interface does not have to
    /// repeat it per symbol.
    #[must_use]
    pub fn endpoint_for(&self, symbol: &str) -> Mt5Endpoint {
        match (self.ports.get(symbol), split_host_port(&self.listen_addr)) {
            (Some(&port), Some((host, _))) => Mt5Endpoint {
                listen_addr: format!("{host}:{port}"),
                dial: Some((dial_host(host).to_string(), port)),
                from_map: true,
            },
            // Mapped, but the default address names no host to inherit.
            // `validate` rejects that config; a hand-built one still gets the
            // port it asked for rather than silently sharing the default's.
            (Some(&port), None) => Mt5Endpoint {
                listen_addr: format!("{LOOPBACK}:{port}"),
                dial: Some((LOOPBACK.to_string(), port)),
                from_map: true,
            },
            (None, Some((host, port))) => Mt5Endpoint {
                listen_addr: self.listen_addr.clone(),
                dial: Some((dial_host(host).to_string(), port)),
                from_map: false,
            },
            (None, None) => Mt5Endpoint {
                listen_addr: self.listen_addr.clone(),
                dial: None,
                from_map: false,
            },
        }
    }

    /// Check the listener settings on their own: a bind address that resolves,
    /// and a port map in which no two symbols could collide.
    ///
    /// Every rule here describes a config whose only symptom at runtime is a
    /// chart that stays empty, which is exactly the kind of thing to refuse at
    /// load instead.
    fn validate(&self) -> Result<(), String> {
        let Some((_, default_port)) = split_host_port(&self.listen_addr) else {
            return Err(format!(
                "metatrader listen_addr '{}' is not a host:port address",
                self.listen_addr
            ));
        };
        let mut taken: BTreeMap<u16, &str> = BTreeMap::new();
        for (symbol, &port) in &self.ports {
            if symbol.trim().is_empty() {
                return Err("[metatrader.ports] has an entry with an empty symbol".to_string());
            }
            if port == 0 {
                return Err(format!(
                    "[metatrader.ports] gives '{symbol}' port 0; that binds whatever the OS \
                     hands out, and an EA has no way to dial it"
                ));
            }
            if port == default_port {
                return Err(format!(
                    "[metatrader.ports] gives '{symbol}' port {port}, which is already the \
                     listen_addr default '{}' every unmapped symbol uses",
                    self.listen_addr
                ));
            }
            if let Some(other) = taken.insert(port, symbol) {
                return Err(format!(
                    "[metatrader.ports] gives port {port} to both '{other}' and '{symbol}'; \
                     one port carries one symbol"
                ));
            }
        }
        Ok(())
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
    /// Bubble preset applied when this feed is selected, by name.
    ///
    /// A market dictates how its tape reads — the B3 mini index wants the
    /// candle summary that a dense BTC tape does not — so a feed may declare
    /// the look it opens with. Absent, nothing changes: the panel keeps
    /// whatever preset is active, exactly as before the field existed. The
    /// name must exist in the bubble presets file; an unknown name is reported
    /// and ignored rather than silently altering the panel.
    #[serde(default)]
    pub bubble_preset: Option<String>,
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

    /// Data-honesty label for how feed `id` decides the aggressor side of a
    /// trade, or `None` when the venue reports true sides. Shown verbatim on
    /// the status bar; this sits next to the provider → code-path map because
    /// it is provider knowledge, not a UI affordance gate.
    #[must_use]
    pub fn side_note(&self, id: &str) -> Option<&'static str> {
        match self.provider_of(id)? {
            ProviderKind::Binance | ProviderKind::Hyperliquid => None,
            ProviderKind::MetaTrader => Some(match self.metatrader.side_source {
                Mt5SideSource::TickRule => "side: inferred (tick rule)",
                Mt5SideSource::Flags => "side: broker flags",
            }),
        }
    }

    /// Validate internal consistency: at least one feed, unique ids, non-empty
    /// symbol lists, a default selection that actually resolves, and a
    /// MetaTrader port map no two symbols can collide in.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message describing the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        self.metatrader.validate()?;
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
            // Whether the name resolves is the presets file's business (checked
            // when the preset is applied); an empty name is a config typo.
            if feed
                .bubble_preset
                .as_ref()
                .is_some_and(|name| name.trim().is_empty())
            {
                return Err(format!("feed '{}' names an empty bubble_preset", feed.id));
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

/// A startup-only feed/symbol override does not resolve against the loaded
/// catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupSelectionError {
    /// An environment variable could not be represented as UTF-8.
    NonUnicode { variable: &'static str },
    /// The requested feed id is absent from the loaded catalog.
    FeedNotConfigured {
        feed: String,
        available: Vec<String>,
    },
    /// The requested symbol does not belong to the requested feed.
    SymbolNotOffered {
        feed: String,
        symbol: String,
        available: Vec<String>,
    },
}

impl std::fmt::Display for StartupSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupSelectionError::NonUnicode { variable } => {
                write!(f, "{variable} is not valid Unicode")
            }
            StartupSelectionError::FeedNotConfigured { feed, available } => write!(
                f,
                "{DEFAULT_FEED_ENV}='{feed}' is not a configured feed; available feeds: {}",
                available.join(", ")
            ),
            StartupSelectionError::SymbolNotOffered {
                feed,
                symbol,
                available,
            } => write!(
                f,
                "{DEFAULT_SYMBOL_ENV}='{symbol}' is not offered by feed '{feed}'; available symbols: {}",
                available.join(", ")
            ),
        }
    }
}

impl std::error::Error for StartupSelectionError {}

/// Change only the startup feed/symbol selection, preserving the loaded feed
/// catalog and every provider setting.
///
/// Validation is atomic: neither default is changed unless the requested pair
/// resolves. A feed override without a symbol override retains the configured
/// default symbol, which can therefore produce a clear incompatibility error
/// when the two feeds use different symbol names.
///
/// # Errors
///
/// Returns [`StartupSelectionError`] when the feed is absent or the symbol is
/// not offered by that feed.
pub fn apply_startup_selection(
    config: &mut AppConfig,
    feed_override: Option<&str>,
    symbol_override: Option<&str>,
) -> Result<(), StartupSelectionError> {
    let feed = feed_override.map_or_else(|| config.default_feed.clone(), str::to_owned);
    let symbol = symbol_override.map_or_else(|| config.default_symbol.clone(), str::to_owned);

    let Some(feed_config) = config.feed(&feed) else {
        return Err(StartupSelectionError::FeedNotConfigured {
            feed,
            available: config.feeds.iter().map(|item| item.id.clone()).collect(),
        });
    };
    if !feed_config.symbols.contains(&symbol) {
        return Err(StartupSelectionError::SymbolNotOffered {
            feed,
            symbol,
            available: feed_config.symbols.clone(),
        });
    }

    config.default_feed = feed;
    config.default_symbol = symbol;
    Ok(())
}

fn optional_env(variable: &'static str) -> Result<Option<String>, StartupSelectionError> {
    match std::env::var(variable) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(StartupSelectionError::NonUnicode { variable })
        }
    }
}

/// Apply [`DEFAULT_FEED_ENV`] and [`DEFAULT_SYMBOL_ENV`] to a loaded config.
///
/// This thin environment adapter delegates all selection behavior to
/// [`apply_startup_selection`], which stays deterministic and can be tested
/// without mutating process-global environment variables.
///
/// # Errors
///
/// Returns [`StartupSelectionError`] for non-Unicode environment values or a
/// feed/symbol pair that does not resolve against `config`.
pub fn apply_startup_selection_from_env(
    config: &mut AppConfig,
) -> Result<(), StartupSelectionError> {
    let feed = optional_env(DEFAULT_FEED_ENV)?;
    let symbol = optional_env(DEFAULT_SYMBOL_ENV)?;
    apply_startup_selection(config, feed.as_deref(), symbol.as_deref())
}

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
        assert_eq!(
            config
                .feeds
                .iter()
                .map(|feed| feed.id.as_str())
                .collect::<Vec<_>>(),
            [
                "binance",
                "hyperliquid",
                "metatrader-tickmill",
                "metatrader-b3"
            ]
        );
        let binance = config.feed("binance").expect("binance feed");
        assert_eq!(binance.provider, ProviderKind::Binance);
        assert!(binance.symbols.contains(&"BTCUSDT".to_string()));
        assert!(binance.symbols.contains(&"ETHUSDT".to_string()));

        let hyperliquid = config.feed("hyperliquid").expect("Hyperliquid feed");
        assert_eq!(hyperliquid.provider, ProviderKind::Hyperliquid);
        assert_eq!(hyperliquid.symbols, ["BTC", "ETH", "HYPE", "SOL", "ZEC"]);
        assert_eq!(
            hyperliquid.provider.capabilities(),
            FeedCapabilities {
                book_capture: true,
                history_paging: false,
                traded_volume: true,
            }
        );
        assert_eq!(config.side_note("hyperliquid"), None);

        // One terminal serves one account, so the two brokers are two feeds
        // rather than one list that half-resolves whoever is logged in.
        let b3 = config.feed("metatrader-b3").expect("B3 feed");
        assert_eq!(b3.provider, ProviderKind::MetaTrader);
        assert_eq!(b3.symbols, ["WIN$N", "WDO$N"]);
        let tickmill = config.feed("metatrader-tickmill").expect("Tickmill feed");
        assert_eq!(tickmill.provider, ProviderKind::MetaTrader);
        assert_eq!(tickmill.symbols, ["XAUUSD", "US500", "US30"]);
        assert_eq!(config.metatrader.side_source, Mt5SideSource::TickRule);
        assert_eq!(config.metatrader.listen_addr, "127.0.0.1:9100");

        // The Tickmill symbols each own a port, so they stream together.
        assert_eq!(
            config.metatrader.endpoint_for("XAUUSD").listen_addr,
            "127.0.0.1:9101"
        );
        assert_eq!(
            config.metatrader.endpoint_for("US500").listen_addr,
            "127.0.0.1:9102"
        );
        assert_eq!(
            config.metatrader.endpoint_for("US30").listen_addr,
            "127.0.0.1:9103"
        );
        // The B3 pair shares the default, which is the honest shape of "one
        // terminal, one account, one at a time".
        assert_eq!(
            config.metatrader.endpoint_for("WIN$N").listen_addr,
            "127.0.0.1:9100"
        );

        // Both MetaTrader feeds open on the pie summary; Binance declares
        // nothing and keeps whatever the presets file says.
        assert_eq!(b3.bubble_preset.as_deref(), Some("live lane pie"));
        assert_eq!(tickmill.bubble_preset.as_deref(), Some("live lane pie"));
        assert_eq!(binance.bubble_preset, None);
    }

    #[test]
    fn startup_selection_override_preserves_the_whole_catalog() {
        let mut config = parse(EMBEDDED_DEFAULT, ConfigSource::Embedded).expect("embedded default");
        let feeds_before = config.feeds.clone();
        let metatrader_before = config.metatrader.clone();

        apply_startup_selection(&mut config, Some("hyperliquid"), Some("BTC"))
            .expect("valid startup selection");

        assert_eq!(config.default_feed, "hyperliquid");
        assert_eq!(config.default_symbol, "BTC");
        assert_eq!(config.feeds, feeds_before, "the catalog is not an override");
        assert_eq!(config.metatrader, metatrader_before);
        assert_eq!(
            config
                .feeds
                .iter()
                .map(|feed| feed.id.as_str())
                .collect::<Vec<_>>(),
            [
                "binance",
                "hyperliquid",
                "metatrader-tickmill",
                "metatrader-b3"
            ]
        );
    }

    #[test]
    fn startup_selection_rejects_an_unknown_feed_without_mutating_config() {
        let mut config = parse(EMBEDDED_DEFAULT, ConfigSource::Embedded).expect("embedded default");
        let before = config.clone();

        let error = apply_startup_selection(&mut config, Some("ghost"), Some("BTC"))
            .expect_err("unknown feed");

        assert_eq!(config, before, "failed selection is atomic");
        assert_eq!(
            error.to_string(),
            "QUANTICK_DEFAULT_FEED='ghost' is not a configured feed; available feeds: \
             binance, hyperliquid, metatrader-tickmill, metatrader-b3"
        );
    }

    #[test]
    fn startup_selection_rejects_a_symbol_outside_the_selected_feed() {
        let mut config = parse(EMBEDDED_DEFAULT, ConfigSource::Embedded).expect("embedded default");
        let before = config.clone();

        let error = apply_startup_selection(&mut config, Some("hyperliquid"), Some("BTCUSDT"))
            .expect_err("symbol belongs to Binance, not Hyperliquid");

        assert_eq!(config, before, "failed selection is atomic");
        assert_eq!(
            error.to_string(),
            "QUANTICK_DEFAULT_SYMBOL='BTCUSDT' is not offered by feed 'hyperliquid'; available symbols: BTC, ETH, HYPE, SOL, ZEC"
        );
    }

    #[test]
    fn an_empty_bubble_preset_is_a_config_error() {
        let text = r#"
            default_feed = "b"
            default_symbol = "AAA"
            [[feeds]]
            id = "b"
            name = "B"
            provider = "binance"
            symbols = ["AAA"]
            bubble_preset = "  "
        "#;
        let err = parse(text, ConfigSource::Embedded).expect_err("blank name");
        assert!(
            err.to_string().contains("bubble_preset"),
            "the message names the field: {err}"
        );
    }

    #[test]
    fn provider_lookup_by_id() {
        let (config, _) = sample();
        assert_eq!(config.provider_of("binance"), Some(ProviderKind::Binance));
        assert_eq!(config.provider_of("nope"), None);
    }

    #[test]
    fn side_note_labels_inferred_sides_and_stays_silent_on_venue_truth() {
        let (config, _) = sample();
        // Binance reports true aggressor sides — nothing to disclaim.
        assert_eq!(config.side_note("binance"), None);
        assert_eq!(config.side_note("nope"), None);

        let text = r#"
            default_feed = "mt"
            default_symbol = "WINQ26"
            [[feeds]]
            id = "mt"
            name = "MetaTrader 5"
            provider = "metatrader"
            symbols = ["WINQ26"]
        "#;
        let mut config = parse(text, ConfigSource::Embedded).unwrap();
        assert_eq!(
            config.side_note("mt"),
            Some("side: inferred (tick rule)"),
            "the tick-rule default must be disclosed"
        );
        config.metatrader.side_source = Mt5SideSource::Flags;
        assert_eq!(config.side_note("mt"), Some("side: broker flags"));
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
        assert!(ProviderKind::Hyperliquid.is_implemented());
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

    /// Settings listening on `addr`, mapping `ports`.
    fn listening(addr: &str, ports: &[(&str, u16)]) -> MetaTraderSettings {
        MetaTraderSettings {
            listen_addr: addr.to_string(),
            ports: ports
                .iter()
                .map(|(symbol, port)| ((*symbol).to_string(), *port))
                .collect(),
            ..MetaTraderSettings::default()
        }
    }

    #[test]
    fn the_bridge_dial_address_comes_from_the_listen_address() {
        let at = |addr: &str| listening(addr, &[]).endpoint_for("WIN$N");
        let dialing = |addr: &str| at(addr).dial;

        assert_eq!(dialing("127.0.0.1:9100"), Some(("127.0.0.1".into(), 9100)));
        // A wildcard bind is not something a bridge can dial.
        assert_eq!(dialing("0.0.0.0:9100"), Some(("127.0.0.1".into(), 9100)));
        assert_eq!(dialing("[::]:9100"), Some(("127.0.0.1".into(), 9100)));
        // A bracketed IPv6 literal splits on the last colon, not the first.
        assert_eq!(dialing("[::1]:9100"), Some(("[::1]".into(), 9100)));

        // Nothing dial-able: the caller must not launch a bridge that cannot
        // reach us, so there is no address to hand it. The bind address is
        // still passed through verbatim, and reported by `validate`.
        for broken in ["9100", "127.0.0.1:", ":9100", "127.0.0.1:not-a-port"] {
            assert_eq!(dialing(broken), None, "{broken}");
            assert_eq!(at(broken).listen_addr, broken);
        }
    }

    #[test]
    fn a_mapped_symbol_gets_its_own_port_and_everyone_else_the_default() {
        let settings = listening("127.0.0.1:9100", &[("XAUUSD", 9101), ("US500", 9102)]);

        let gold = settings.endpoint_for("XAUUSD");
        assert_eq!(gold.listen_addr, "127.0.0.1:9101");
        assert_eq!(gold.dial, Some(("127.0.0.1".into(), 9101)));
        assert!(gold.from_map);

        // Both sides of the agreement come from one call: the port quantick
        // binds is the port the autostarted bridge is told to dial.
        let index = settings.endpoint_for("US500");
        assert_eq!(index.listen_addr, "127.0.0.1:9102");
        assert_eq!(index.dial.map(|(_, port)| port), Some(9102));

        let unmapped = settings.endpoint_for("WIN$N");
        assert_eq!(unmapped.listen_addr, "127.0.0.1:9100");
        assert!(!unmapped.from_map, "it fell through to the shared default");
    }

    #[test]
    fn a_mapped_symbol_inherits_the_default_addresss_host() {
        // A deployment that binds a specific interface says so once.
        let settings = listening("0.0.0.0:9100", &[("XAUUSD", 9101)]);
        let gold = settings.endpoint_for("XAUUSD");
        assert_eq!(gold.listen_addr, "0.0.0.0:9101", "the host carries over");
        assert_eq!(
            gold.dial,
            Some(("127.0.0.1".into(), 9101)),
            "and a wildcard is still not something a bridge can dial"
        );
    }

    #[test]
    fn ports_are_read_from_their_sub_table() {
        let text = r#"
            default_feed = "mt"
            default_symbol = "XAUUSD"
            [[feeds]]
            id = "mt"
            name = "MetaTrader 5"
            provider = "metatrader"
            symbols = ["XAUUSD", "US500"]
            [metatrader]
            listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            XAUUSD = 9101
            US500 = 9102
        "#;
        let config = parse(text, ConfigSource::Embedded).unwrap();
        assert_eq!(
            config.metatrader.ports,
            BTreeMap::from([("XAUUSD".to_string(), 9101), ("US500".to_string(), 9102)])
        );
        // Absent, the map is simply empty: every symbol shares listen_addr,
        // exactly as before the field existed.
        assert!(MetaTraderSettings::default().ports.is_empty());
    }

    /// Parse a config whose `[metatrader]` section is `section`.
    fn with_metatrader(section: &str) -> Result<AppConfig, ConfigError> {
        let text = format!(
            r#"
            default_feed = "mt"
            default_symbol = "XAUUSD"
            [[feeds]]
            id = "mt"
            name = "MetaTrader 5"
            provider = "metatrader"
            symbols = ["XAUUSD", "US500"]
            [metatrader]
            {section}
        "#
        );
        parse(&text, ConfigSource::Embedded)
    }

    #[test]
    fn two_symbols_may_not_share_a_port() {
        // The collision that would otherwise surface as one chart streaming
        // and the other silently refusing every connection.
        let err = with_metatrader(
            r#"listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            US500 = 9101
            XAUUSD = 9101"#,
        )
        .expect_err("duplicate port");
        let message = err.to_string();
        assert!(message.contains("[metatrader.ports]"), "{message}");
        assert!(message.contains("9101"), "{message}");
        assert!(
            message.contains("US500") && message.contains("XAUUSD"),
            "both claimants are named: {message}"
        );
    }

    #[test]
    fn a_mapped_port_may_not_be_the_default_one() {
        // Subtler than a duplicate: it collides with whichever *unmapped*
        // symbol is streaming, which the map does not list at all.
        let err = with_metatrader(
            r#"listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            XAUUSD = 9100"#,
        )
        .expect_err("collides with the default");
        let message = err.to_string();
        assert!(message.contains("listen_addr"), "{message}");
        assert!(message.contains("XAUUSD"), "{message}");
    }

    #[test]
    fn a_mapped_port_must_be_one_an_ea_can_dial() {
        // Port 0 binds whatever the OS hands out. Fine for a test harness that
        // reads the bound address back; useless in a file an EA is configured
        // against by hand.
        let err = with_metatrader(
            r#"listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            XAUUSD = 0"#,
        )
        .expect_err("port 0");
        assert!(err.to_string().contains("XAUUSD"), "{err}");

        let err = with_metatrader(
            r#"listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            "  " = 9101"#,
        )
        .expect_err("empty symbol");
        assert!(err.to_string().contains("[metatrader.ports]"), "{err}");
    }

    #[test]
    fn a_listen_addr_that_cannot_bind_is_reported_at_load() {
        // It would otherwise only show up as MT5_BIND_FAILED, minutes later,
        // in a log nobody has open.
        let err = with_metatrader(r#"listen_addr = "9100""#).expect_err("no host");
        let message = err.to_string();
        assert!(message.contains("listen_addr"), "{message}");
        assert!(message.contains("host:port"), "{message}");

        // And the ordinary one still loads.
        assert!(with_metatrader(r#"listen_addr = "127.0.0.1:9100""#).is_ok());
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
