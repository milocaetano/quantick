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

use serde::{Deserialize, Serialize};

use crate::symbols_file::AddedSymbols;

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
                ohlcv_history: true,
                ohlcv_generation: 0,
            },
            // `recentTrades` is a short recovery window, not a pageable
            // historical API. Trades and the visible 20-level book are factual;
            // older history is withheld rather than synthesized from candles.
            //
            // Candles are a different matter: served as candles they are the
            // venue's own record, and `candleSnapshot` reaches back months.
            // What stays forbidden is the inverse — inventing trades out of
            // them to fill the tape.
            ProviderKind::Hyperliquid => FeedCapabilities {
                book_capture: true,
                history_paging: false,
                traded_volume: true,
                ohlcv_history: true,
                ohlcv_generation: 0,
            },
            // The bridge streams the terminal's Depth of Market. Whether a
            // given session really has one (symbol, account, EA version) is
            // runtime information the feed reports honestly; it is not
            // something to assume either way from here.
            //
            // Candle history starts **false**, unlike the other two, because on
            // MetaTrader it does not mean "this provider can serve candles" — it
            // means "a block is in hand". Nothing can be fetched here: the
            // bridge pushes when it pushes, and a consumer that asked while the
            // optimistic answer stood would get an honest empty reply, cache it,
            // and — seeing no rising edge afterwards — never ask again. The flag
            // rises once, when the block actually arrives.
            ProviderKind::MetaTrader => FeedCapabilities {
                book_capture: true,
                history_paging: false,
                traded_volume: true,
                ohlcv_history: false,
                ohlcv_generation: 0,
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
    /// Can serve venue-native candle history for the time pane.
    ///
    /// Deliberately not folded into
    /// [`history_paging`](Self::history_paging): that one is about reaching
    /// further back in the *tape*, on demand and repeatedly, and the two do not
    /// travel together. Hyperliquid publishes months of candles and no pageable
    /// trade history at all; a recording is the mirror image, holding every
    /// tick it captured and no candles whatsoever.
    pub ohlcv_history: bool,
    /// Bumped whenever the venue-history answer *changed* — a new block
    /// arrived, or an old one was replaced by a better one.
    ///
    /// [`ohlcv_history`](Self::ohlcv_history) alone cannot carry this. It is a
    /// latch on the one provider that pushes: MetaTrader raises it when its
    /// first block lands and never lowers it, so a consumer that asked, was
    /// answered with an empty block (a cold terminal, a paging failure), and
    /// cached that emptiness would watch for a rising edge that can never come
    /// again — while the full block from the next routine reconnect sits held.
    /// A counter has no such ceiling: every block moves it, including a
    /// replacement for one already delivered.
    ///
    /// Zero, and staying zero forever, is the correct answer for a pull-based
    /// feed: Binance and Hyperliquid answer whenever they are asked, so nothing
    /// ever changes behind the consumer's back. Read this as "the answer
    /// changed, ask again if you care", never as "how many blocks exist".
    pub ohlcv_generation: u64,
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
            ohlcv_history: false,
            ohlcv_generation: 0,
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
    pub from_ports_map: bool,
}

/// Split `host:port`, rejecting anything that is not both. The bracketed IPv6
/// form (`[::1]:9100`) splits correctly: only the last colon is a separator.
/// The host and port of a `host:port` bind address.
///
/// One splitter, shared: the evidence bundle reduces the same address to the
/// port it publishes and the loopback flag it derives, and two parsers of one
/// format disagree the first time either is touched.
pub(crate) fn split_host_port(addr: &str) -> Option<(&str, u16)> {
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
                from_ports_map: true,
            },
            // Mapped, but the default address names no host to inherit.
            // `validate` rejects that config; a hand-built one still gets the
            // port it asked for rather than silently sharing the default's.
            (Some(&port), None) => Mt5Endpoint {
                listen_addr: format!("{LOOPBACK}:{port}"),
                dial: Some((LOOPBACK.to_string(), port)),
                from_ports_map: true,
            },
            (None, Some((host, port))) => Mt5Endpoint {
                listen_addr: self.listen_addr.clone(),
                dial: Some((dial_host(host).to_string(), port)),
                from_ports_map: false,
            },
            (None, None) => Mt5Endpoint {
                listen_addr: self.listen_addr.clone(),
                dial: None,
                from_ports_map: false,
            },
        }
    }

    /// Check the listener settings on their own: a `listen_addr` that splits
    /// into a non-empty host and a `u16` port, and a port map in which no two
    /// symbols could collide.
    ///
    /// This checks the shape of an address, not whether it can be reached — a
    /// host that does not resolve, or a port another process already holds, is
    /// a runtime fact (reported as `MT5_BIND_FAILED`) that no amount of
    /// parsing here would predict.
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
        // Same reasoning as a mapped port of 0: an ephemeral bind is an address
        // nobody can be configured against, and every unmapped symbol lands here.
        if default_port == 0 {
            return Err(format!(
                "metatrader listen_addr '{}' asks for port 0; that binds whatever the OS \
                 hands out, and an EA has no way to dial it",
                self.listen_addr
            ));
        }
        let mut taken: BTreeMap<u16, &str> = BTreeMap::new();
        for (symbol, &port) in &self.ports {
            if symbol.trim().is_empty() {
                return Err("[metatrader.ports] has an entry with an empty symbol".to_string());
            }
            // A padded key silently maps nothing: lookups come from a feed's
            // symbol list, which carries no such padding.
            if symbol != symbol.trim() {
                return Err(format!(
                    "[metatrader.ports] key '{symbol}' has leading or trailing whitespace; \
                     it would never match the symbol a feed offers"
                ));
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

/// The canvas layout a feed declares its tabs open on (`default_layout` in
/// the TOML), named for what each layout shows.
///
/// A config-side twin of `crate::tab::CanvasLayout` rather than that enum
/// itself, so the TOML vocabulary — part of the user-facing config contract —
/// cannot drift when the canvas grows a layout a config should not name.
/// Serialized as well as deserialized: the saved workspace
/// ([`crate::ui_state`]) writes a canvas layout back out, and it must speak
/// the vocabulary the config reads — one name for one layout, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum DeclaredLayout {
    /// The flow pane alone — the factory default.
    #[serde(rename = "flow")]
    Flow,
    /// A full-window timeframe chart.
    #[serde(rename = "time")]
    Time,
    /// Timeframe left, flow right, on the draggable divider.
    #[serde(rename = "time+flow")]
    TimeAndFlow,
    /// Two timeframe charts stacked left, flow right.
    #[serde(rename = "time+time+flow")]
    TimeTimeAndFlow,
}

impl DeclaredLayout {
    /// Parse the same names the serde renames above accept, for callers
    /// outside serde (the `QUANTICK_LAYOUT` env hook). One vocabulary, one
    /// place: a name added here must be added to the renames, and the
    /// `layout_names_agree_between_serde_and_parse` test holds the two
    /// together.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "flow" => Some(DeclaredLayout::Flow),
            "time" => Some(DeclaredLayout::Time),
            "time+flow" => Some(DeclaredLayout::TimeAndFlow),
            "time+time+flow" => Some(DeclaredLayout::TimeTimeAndFlow),
            _ => None,
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
    /// Bubble presets applied per symbol, by exact symbol name — overriding
    /// [`bubble_preset`](Self::bubble_preset) for the symbols named here.
    ///
    /// An instrument can dictate its read more precisely than its venue: the
    /// B3 mini index wants regional aggregation that the mini dollar on the
    /// same feed does not. A symbol with no entry falls back to the feed's
    /// declared preset, and then to whatever the panel has active — exactly
    /// the ladder [`bubble_preset`](Self::bubble_preset) already describes.
    /// Keys must be symbols this feed offers (the added-symbols sidecar
    /// included), because a key that matches nothing has silence as its only
    /// symptom; whether the preset *name* resolves stays the presets file's
    /// business, reported when it is applied.
    #[serde(default)]
    pub symbol_bubble_presets: BTreeMap<String, String>,
    /// The canvas layout a tab on this feed opens showing.
    ///
    /// Startup-scoped, like `default_feed`: it decides what a *new* tab looks
    /// like and never overrides a layout the user has since chosen. Absent,
    /// nothing changes — the factory default stays the flow pane, quantick's
    /// identity (UX audit §3).
    #[serde(default)]
    pub default_layout: Option<DeclaredLayout>,
    /// The bar spec a tab on this feed opens on, as `kind:parameter` —
    /// `time:1m`, `tick:50`, `volume:5`, `dollar:500000`, `imbalance:100`
    /// (also `imbalance:volume:500` / `imbalance:dollar:500`)
    /// (see `crate::state::BarSpec::parse`). It sets the flow pane's opening
    /// spec; when [`default_layout`](Self::default_layout) shows a time pane
    /// and this names a time spec, that pane opens on its interval too.
    /// Absent, the factory default spec applies, exactly as before the field
    /// existed.
    #[serde(default)]
    pub default_bars: Option<String>,
}

impl FeedConfig {
    /// The bubble preset declared for `symbol` on this feed, if any: the
    /// symbol's own entry when it has one, the feed-wide declaration
    /// otherwise.
    #[must_use]
    pub fn bubble_preset_for(&self, symbol: &str) -> Option<&str> {
        self.symbol_bubble_presets
            .get(symbol)
            .map(String::as_str)
            .or(self.bubble_preset.as_deref())
    }
}

/// Paper-trading options (`[paper]` in the TOML).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct PaperSettings {
    /// Where the simulator saves closed trades — the per-symbol journals,
    /// and the exports beside them. Absent, the journal lives in the
    /// user's documents folder (`Documents/Quantick/paper-trades`), the
    /// one home that does not move with the working directory; set, it
    /// pins the journal somewhere else, relative paths resolving against
    /// quantick's working directory. The `QUANTICK_TRADES_DIR` environment
    /// variable still overrides either for one run (an env var is an
    /// explicit request, like every autostart hook). The same folder is
    /// the integration port for other producers: anything that writes the
    /// `quantick-trades` format here — a bot runner, another tool — shows
    /// up in the Trades ledger, the report and the export.
    pub trades_dir: Option<String>,
}

impl PaperSettings {
    fn validate(&self) -> Result<(), String> {
        if let Some(dir) = &self.trades_dir
            && dir.trim().is_empty()
        {
            return Err(
                "paper.trades_dir must not be empty - drop the key to use the \
                 documents-folder default"
                    .to_string(),
            );
        }
        Ok(())
    }
}

/// How far a *load older* press reaches (`[history]` in the TOML).
///
/// Two shapes of market, one reach. The defaults are sized against B3's index
/// future — the tape they were verified on — and neither of them is a fact
/// about every venue: a market with a real lunch break wants a longer gap
/// before a pause reads as a close, and a trader comparing an opening range
/// may want more of yesterday than three hours. Both are the kind of value
/// `arch-review` puts in a config file rather than a `const`: a `const` still
/// costs a rebuild, and a rebuild is the one thing the trader cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// A stretch with no prints longer than this reads as the market having
    /// been closed rather than as a quiet patch. Minutes.
    pub session_gap_minutes: u32,
    /// How far past a session's last print the *previous session* reach keeps
    /// going, so the day before is on screen to compare against rather than
    /// merely touched. Minutes.
    pub previous_session_lead_minutes: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            session_gap_minutes: (crate::history_reach::SESSION_GAP_MS / 60_000) as u32,
            previous_session_lead_minutes: (crate::history_reach::PREVIOUS_SESSION_LEAD_MS / 60_000)
                as u32,
        }
    }
}

impl HistorySettings {
    /// The two settings as the reach reads them: milliseconds.
    #[must_use]
    pub fn reach_bounds(&self) -> crate::history_reach::ReachBounds {
        crate::history_reach::ReachBounds {
            session_gap_ms: i64::from(self.session_gap_minutes) * 60_000,
            previous_session_lead_ms: i64::from(self.previous_session_lead_minutes) * 60_000,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.session_gap_minutes == 0 {
            return Err(
                "history.session_gap_minutes must be at least 1 - a gap of zero \
                 would read every print as a new session"
                    .to_string(),
            );
        }
        Ok(())
    }
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
    /// Paper-trading settings; defaults apply when the section is absent.
    #[serde(default)]
    pub paper: PaperSettings,
    /// How far a *load older* press reaches; defaults apply when the section
    /// is absent.
    #[serde(default)]
    pub history: HistorySettings,
}

impl AppConfig {
    /// The feed with the given id, if any.
    #[must_use]
    pub fn feed(&self, id: &str) -> Option<&FeedConfig> {
        self.feeds.iter().find(|f| f.id == id)
    }

    /// Fold the user's added symbols into the catalog, in place.
    ///
    /// Config order first, additions after it, no duplicates: what the file
    /// ships stays where it is and what the user added lands at the end of
    /// the feed's list, which is the order the picker and the SOURCE combo
    /// then show.
    ///
    /// Run *before* [`Self::validate`], so a user-added symbol is checked like
    /// any other — the MetaTrader port cross-check in particular has to see
    /// the catalog the app will actually run on, not the one on disk.
    ///
    /// An entry for a feed the config no longer has is dropped with a log
    /// line: a renamed feed should cost its additions, not the launch.
    pub fn merge_added_symbols(&mut self, added: &AddedSymbols) {
        for (feed_id, symbols) in added.entries() {
            let Some(feed) = self.feeds.iter_mut().find(|feed| feed.id == feed_id) else {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "SYMBOL_CATALOG_UNKNOWN_FEED",
                    feed = %feed_id,
                    symbols = symbols.len(),
                    action = "entries_skipped",
                    "the added-symbols file names a feed the config does not have"
                );
                continue;
            };
            for symbol in symbols {
                if !feed.symbols.iter().any(|existing| existing == symbol) {
                    feed.symbols.push(symbol.clone());
                }
            }
        }
    }

    /// Add `symbol` to feed `id`'s catalog. `false` when the feed is unknown
    /// or already offers it.
    pub fn add_symbol(&mut self, feed_id: &str, symbol: &str) -> bool {
        let Some(feed) = self.feeds.iter_mut().find(|feed| feed.id == feed_id) else {
            return false;
        };
        if feed.symbols.iter().any(|existing| existing == symbol) {
            return false;
        }
        feed.symbols.push(symbol.to_owned());
        true
    }

    /// Drop `symbol` from feed `id`'s catalog. `false` when the feed is
    /// unknown or does not offer it.
    ///
    /// Refuses to empty a feed: `validate` rejects a feed with no symbols, so
    /// a catalog that could reach that state is one the app could not reload.
    pub fn remove_symbol(&mut self, feed_id: &str, symbol: &str) -> bool {
        let Some(feed) = self.feeds.iter_mut().find(|feed| feed.id == feed_id) else {
            return false;
        };
        if feed.symbols.len() <= 1 {
            return false;
        }
        let before = feed.symbols.len();
        feed.symbols.retain(|existing| existing != symbol);
        feed.symbols.len() != before
    }

    /// The display name of feed `id`, or its id when the config has no such
    /// feed — a borrow, because the chrome reads it every frame.
    #[must_use]
    pub fn feed_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.feed(id).map_or(id, |feed| feed.name.as_str())
    }

    /// The symbol feed `id` should show given a `wanted` selection.
    ///
    /// `Some(wanted)` when the feed offers it, otherwise the feed's first
    /// symbol — picking a feed should not leave the chart pointed at an
    /// instrument that feed does not have. `None` when the feed is unknown or
    /// lists nothing, which is the caller's cue to leave the selection alone
    /// rather than invent one.
    ///
    /// One rule, because there are two places that need it: the toolbar's
    /// SOURCE group correcting a live selection, and the new-tab picker
    /// deciding what its Open button would actually open.
    #[must_use]
    pub fn resolve_symbol(&self, feed_id: &str, wanted: &str) -> Option<String> {
        let feed = self.feed(feed_id)?;
        if feed.symbols.iter().any(|symbol| symbol == wanted) {
            return Some(wanted.to_owned());
        }
        feed.symbols.first().cloned()
    }

    /// The provider backing feed `id`, if the feed exists.
    #[must_use]
    pub fn provider_of(&self, id: &str) -> Option<ProviderKind> {
        self.feed(id).map(|f| f.provider)
    }

    /// The bar spec feed `id` declares its tabs open on, if it declares one.
    ///
    /// `validate` already proved the string parses, so `None` normally means
    /// "nothing declared". A spec that stopped parsing anyway (a config
    /// mutated in a test, say) is reported and treated as undeclared rather
    /// than trusted half-way.
    #[must_use]
    pub fn startup_spec_for(&self, id: &str) -> Option<crate::state::BarSpec> {
        let bars = self.feed(id)?.default_bars.as_deref()?;
        match crate::state::BarSpec::parse(bars) {
            Ok(spec) => Some(spec),
            Err(message) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "DEFAULT_BARS_INVALID",
                    feed = %id,
                    spec = %bars,
                    reason = %message,
                    action = "open_factory_default_spec",
                    "feed declares a default_bars that does not parse; ignoring"
                );
                None
            }
        }
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

    /// Every MetaTrader feed offering `symbol`, by id.
    fn metatrader_feeds_offering(&self, symbol: &str) -> Vec<&str> {
        self.feeds
            .iter()
            .filter(|feed| {
                feed.provider == ProviderKind::MetaTrader
                    && feed.symbols.iter().any(|offered| offered == symbol)
            })
            .map(|feed| feed.id.as_str())
            .collect()
    }

    /// Check the MetaTrader port map against the feed catalog it exists to
    /// serve. Both failures here are silent at runtime, which is what makes
    /// them worth a load-time refusal:
    ///
    /// - a key no MetaTrader feed offers is a typo or a leftover, and its only
    ///   symptom is the symbol quietly using the shared port instead;
    /// - a symbol two MetaTrader feeds both claim resolves to one port for
    ///   both, so two brokers quoting `US500` would fight over one listener
    ///   while the map looks perfectly well-formed.
    fn validate_ports_against_catalog(&self) -> Result<(), String> {
        for symbol in self.metatrader.ports.keys() {
            match self.metatrader_feeds_offering(symbol).as_slice() {
                [] => {
                    return Err(format!(
                        "[metatrader.ports] maps '{symbol}', which no metatrader feed offers; \
                         it would silently fall back to the shared listen_addr port"
                    ));
                }
                [_] => {}
                [first, rest @ ..] => {
                    return Err(format!(
                        "[metatrader.ports] maps '{symbol}', which is offered by {} metatrader \
                         feeds ('{first}' and '{}'); one port cannot carry both",
                        rest.len() + 1,
                        rest.join("', '")
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validate internal consistency: at least one feed, unique ids, non-empty
    /// symbol lists, a default selection that actually resolves, and a
    /// MetaTrader port map no two symbols can collide in — checked both on its
    /// own and against the feeds it names.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message describing the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        self.metatrader.validate()?;
        self.paper.validate()?;
        self.history.validate()?;
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
            // Same rule the port map keys follow: a symbol is matched by exact
            // string against what a venue calls an instrument, so padding is a
            // silent mismatch and an empty entry names nothing at all. Checked
            // for every feed, because the catalog the app runs on includes
            // whatever the added-symbols sidecar merged into it.
            for symbol in &feed.symbols {
                if symbol.trim().is_empty() {
                    return Err(format!("feed '{}' lists an empty symbol", feed.id));
                }
                if symbol.trim() != symbol {
                    return Err(format!(
                        "feed '{}' lists symbol '{symbol}' with surrounding whitespace; \
                         a venue matches its instrument names exactly",
                        feed.id
                    ));
                }
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
            // Same two rules, one entry at a time: a key that names no symbol
            // of this feed would have silence as its only symptom, and an
            // empty preset name is a config typo. Whether the name resolves
            // stays the presets file's business.
            for (symbol, preset) in &feed.symbol_bubble_presets {
                if !feed.symbols.contains(symbol) {
                    return Err(format!(
                        "feed '{}' maps a bubble preset for symbol '{symbol}', which it does \
                         not offer",
                        feed.id
                    ));
                }
                if preset.trim().is_empty() {
                    return Err(format!(
                        "feed '{}' names an empty bubble preset for symbol '{symbol}'",
                        feed.id
                    ));
                }
            }
            // The same live-control rules apply to a declared opening spec: a
            // config must not open a chart no control could have produced,
            // and the only symptom of a typo here would be a silently
            // factory-default chart.
            if let Some(bars) = &feed.default_bars {
                crate::state::BarSpec::parse(bars).map_err(|message| {
                    format!("feed '{}' has an invalid default_bars: {message}", feed.id)
                })?;
            }
        }
        // Needs the catalog, so it waits until the catalog is known good.
        self.validate_ports_against_catalog()?;
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

/// Whether either startup-selection env var named the market this run opens
/// on.
///
/// The saved workspace ([`crate::ui_state`]) otherwise decides it, and this is
/// how the two are ordered: an env var is an explicit request for this one
/// run, so a validation run pinned to `QUANTICK_DEFAULT_SYMBOL` must not find
/// itself on yesterday's cockpit instead.
#[must_use]
pub fn startup_selection_came_from_env() -> bool {
    std::env::var_os(DEFAULT_FEED_ENV).is_some() || std::env::var_os(DEFAULT_SYMBOL_ENV).is_some()
}

/// Parse a config from a TOML string tagged with its `source`, fold in the
/// user's added symbols, and validate the result.
///
/// The merge happens before validation on purpose: a symbol added from the UI
/// is part of the catalog the app runs on, so it has to pass the same checks —
/// the MetaTrader port cross-check most of all.
fn parse(text: &str, source: ConfigSource, added: &AddedSymbols) -> Result<AppConfig, ConfigError> {
    let mut config: AppConfig = toml::from_str(text).map_err(|e| ConfigError::Parse {
        source: source.clone(),
        message: e.to_string(),
    })?;
    config.merge_added_symbols(added);
    config.validate().map_err(|message| ConfigError::Invalid {
        source: source.clone(),
        message,
    })?;
    Ok(config)
}

/// Parse with the user's added symbols, falling back to the config alone when
/// those additions are what makes it invalid.
///
/// A file the app wrote must never be able to stop the app starting. The
/// picker refuses an addition that would not validate, so reaching this needs
/// a hand-edited sidecar or a config that changed underneath one — both real,
/// and neither a reason to exit with an error naming a file the user did not
/// break. The additions are dropped, loudly, and the chart opens.
///
/// A config that fails on its own is still a hard error: that file *is* the
/// user's, and silently running on something else would be worse than saying
/// so.
fn parse_with_additions(
    text: &str,
    source: ConfigSource,
    added: &AddedSymbols,
    added_path: &Path,
) -> Result<AppConfig, ConfigError> {
    let merged = match parse(text, source.clone(), added) {
        Ok(config) => return Ok(config),
        Err(error) => error,
    };
    let clean = parse(text, source, &AddedSymbols::default())?;
    tracing::warn!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "SYMBOL_CATALOG_REJECTED",
        path = %added_path.display(),
        entries = added.entries().map(|(_, symbols)| symbols.len()).sum::<usize>(),
        feeds = added.entries().count(),
        reason = %merged,
        action = "start_without_added_symbols",
        "the added-symbols file does not fit the current config; ignoring it"
    );
    Ok(clean)
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
    let added_path = crate::symbols_file::default_path();
    let added = crate::symbols_file::load(&added_path);
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        let path = PathBuf::from(path);
        let source = ConfigSource::EnvPath(path.clone());
        let text = std::fs::read_to_string(&path).map_err(|e| ConfigError::Read {
            path,
            message: e.to_string(),
        })?;
        return Ok((
            parse_with_additions(&text, source.clone(), &added, &added_path)?,
            source,
        ));
    }

    let cwd_path = Path::new(CONFIG_FILENAME);
    if cwd_path.is_file() {
        let source = ConfigSource::WorkingDir(cwd_path.to_path_buf());
        let text = std::fs::read_to_string(cwd_path).map_err(|e| ConfigError::Read {
            path: cwd_path.to_path_buf(),
            message: e.to_string(),
        })?;
        return Ok((
            parse_with_additions(&text, source.clone(), &added, &added_path)?,
            source,
        ));
    }

    let config = parse_with_additions(
        EMBEDDED_DEFAULT,
        ConfigSource::Embedded,
        &added,
        &added_path,
    )?;
    Ok((config, ConfigSource::Embedded))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog the app runs on is the file's list plus the user's, in
    /// that order and without repeats.
    #[test]
    fn added_symbols_land_after_the_config_list_without_repeating_it() {
        let mut config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("the shipped config");
        let feed = config.feeds[0].id.clone();
        let shipped = config.feeds[0].symbols.clone();
        let mut added = AddedSymbols::default();
        // One brand new, and one the config already ships.
        added.add(&feed, "WINQ26");
        added.add(&feed, &shipped[0]);

        config.merge_added_symbols(&added);

        let merged = &config.feeds[0].symbols;
        assert_eq!(
            &merged[..shipped.len()],
            &shipped[..],
            "what the file ships stays where it is"
        );
        assert_eq!(
            merged.last().map(String::as_str),
            Some("WINQ26"),
            "and the addition lands after it"
        );
        assert_eq!(
            merged.iter().filter(|s| *s == &shipped[0]).count(),
            1,
            "adding one the config already has changes nothing"
        );
    }

    /// A file the app wrote must never be able to stop the app starting.
    ///
    /// The picker refuses an addition that would not validate, so reaching
    /// this needs a hand-edited sidecar — or a config that changed underneath
    /// one that was fine when it was written. Either way the additions go, the
    /// warning names the file that actually went wrong, and the chart opens.
    #[test]
    fn a_sidecar_that_breaks_the_port_cross_check_does_not_stop_the_launch() {
        let text = "\
            default_feed = \"tickmill\"\n\
            default_symbol = \"US500\"\n\
            [[feeds]]\n\
            id = \"tickmill\"\n\
            name = \"MetaTrader 5 — Tickmill\"\n\
            provider = \"metatrader\"\n\
            symbols = [\"US500\"]\n\
            [[feeds]]\n\
            id = \"b3\"\n\
            name = \"MetaTrader 5 — B3\"\n\
            provider = \"metatrader\"\n\
            symbols = [\"WIN$N\"]\n\
            [metatrader]\n\
            listen_addr = \"127.0.0.1:9100\"\n\
            [metatrader.ports]\n\
            US500 = 9102\n";

        // On its own the config is fine: one feed offers the mapped symbol.
        assert!(parse(text, ConfigSource::Embedded, &AddedSymbols::default()).is_ok());

        // The sidecar makes a second MetaTrader feed offer it, and one port
        // cannot carry two feeds — the cross-check rejects the merged catalog.
        let mut added = AddedSymbols::default();
        added.add("b3", "US500");
        assert!(
            parse(text, ConfigSource::Embedded, &added).is_err(),
            "the merged catalog really is invalid; the repair below is not vacuous"
        );

        let repaired = parse_with_additions(
            text,
            ConfigSource::Embedded,
            &added,
            std::path::Path::new("quantick-symbols.toml"),
        )
        .expect("a bad sidecar costs its entries, not the launch");
        assert_eq!(
            repaired.feed("b3").expect("the feed").symbols,
            ["WIN$N"],
            "the poisoning addition was dropped"
        );
        assert!(repaired.validate().is_ok());
    }

    /// A config that is broken on its own is still a hard error: that file is
    /// the user's, and quietly running on something else would be worse.
    #[test]
    fn a_config_broken_without_the_sidecar_still_fails() {
        let text = "\
            default_feed = \"mt\"\n\
            default_symbol = \"NOPE\"\n\
            [[feeds]]\n\
            id = \"mt\"\n\
            name = \"MetaTrader 5\"\n\
            provider = \"metatrader\"\n\
            symbols = [\"WIN$N\"]\n";
        let mut added = AddedSymbols::default();
        added.add("mt", "WINQ26");

        assert!(
            parse_with_additions(
                text,
                ConfigSource::Embedded,
                &added,
                std::path::Path::new("quantick-symbols.toml"),
            )
            .is_err(),
            "dropping the additions cannot fix a default_symbol nothing offers"
        );
    }

    /// A symbol is matched by exact string against what a venue calls an
    /// instrument, so padding is a silent mismatch — in the config or in
    /// anything the sidecar merged into it.
    #[test]
    fn padded_and_empty_catalog_symbols_are_rejected() {
        let base = "\
            default_feed = \"a\"\n\
            default_symbol = \"AAA\"\n\
            [[feeds]]\n\
            id = \"a\"\n\
            name = \"A\"\n\
            provider = \"binance\"\n\
            symbols = [\"AAA\"]\n";
        assert!(parse(base, ConfigSource::Embedded, &AddedSymbols::default()).is_ok());

        for bad in [" AAA", "AAA ", "  ", ""] {
            let mut added = AddedSymbols::default();
            added.add("a", bad);
            let error = parse(base, ConfigSource::Embedded, &added)
                .expect_err("a padded or empty symbol is not a symbol");
            assert!(
                format!("{error}").contains("symbol"),
                "the message has to say what is wrong: {error}"
            );
        }
    }

    /// A feed that was renamed or dropped should cost its additions, never the
    /// launch.
    #[test]
    fn added_symbols_for_an_unknown_feed_are_ignored() {
        let mut config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("the shipped config");
        let before = config.feeds.clone();
        let mut added = AddedSymbols::default();
        added.add("a-feed-that-was-renamed", "WINQ26");

        config.merge_added_symbols(&added);

        assert_eq!(config.feeds, before, "nothing was invented for a dead id");
        assert!(config.validate().is_ok(), "and the config still loads");
    }

    /// The merge runs before validation, so an addition is checked like any
    /// other symbol — which is what lets a mapped MetaTrader port find it.
    #[test]
    fn an_added_metatrader_symbol_reaches_the_port_cross_check() {
        let text = "\
            default_feed = \"mt\"\n\
            default_symbol = \"WIN$N\"\n\
            [[feeds]]\n\
            id = \"mt\"\n\
            name = \"MetaTrader 5\"\n\
            provider = \"metatrader\"\n\
            symbols = [\"WIN$N\"]\n\
            [metatrader]\n\
            listen_addr = \"127.0.0.1:9100\"\n\
            [metatrader.ports]\n\
            WINQ26 = 9101\n";

        // Without the addition the map names a symbol no feed offers, which is
        // exactly the mistake the cross-check exists to catch.
        let bare = parse(text, ConfigSource::Embedded, &AddedSymbols::default());
        assert!(
            bare.is_err(),
            "a port mapping a symbol nothing offers is a config error"
        );

        let mut added = AddedSymbols::default();
        added.add("mt", "WINQ26");
        let config = parse(text, ConfigSource::Embedded, &added)
            .expect("the addition makes the mapping legitimate");
        assert!(config.feeds[0].symbols.iter().any(|s| s == "WINQ26"));
        assert_eq!(
            config.metatrader.endpoint_for("WINQ26").listen_addr,
            "127.0.0.1:9101",
            "and the added contract listens on its own port"
        );
        assert_eq!(
            config.metatrader.endpoint_for("WIN$N").listen_addr,
            "127.0.0.1:9100",
            "while an unmapped symbol keeps the shared one"
        );
    }

    /// The shipped default is two charts: a timeframe pane for context beside
    /// a flow pane reading the tape by tick (user decision 2026-08-07).
    ///
    /// Asserted on the file rather than trusted to a comment in it, because
    /// "what does quantick open on" is the first impression the product makes
    /// and nothing else in the suite would notice it changing.
    #[test]
    fn the_shipped_default_opens_two_charts_with_the_flow_pane_on_tick_bars() {
        let config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("the shipped config");
        let opening = config
            .feed(&config.default_feed)
            .expect("default_feed is validated to exist");

        assert_eq!(
            opening.default_layout,
            Some(DeclaredLayout::TimeAndFlow),
            "the default open is the timeframe chart beside the flow chart"
        );
        assert_eq!(
            config.startup_spec_for(&config.default_feed),
            Some(crate::state::BarSpec::Tick(50)),
            "and the flow chart reads the tape by tick, not by the clock"
        );
    }

    /// A feed must keep at least one symbol: `validate` rejects an empty list,
    /// so a catalog edit that could empty one is an edit the app could not
    /// reload.
    #[test]
    fn the_last_symbol_of_a_feed_cannot_be_removed() {
        let mut config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("the shipped config");
        let feed = config.feeds[0].id.clone();
        config.feeds[0].symbols.truncate(1);
        let only = config.feeds[0].symbols[0].clone();

        assert!(!config.remove_symbol(&feed, &only));
        assert_eq!(config.feeds[0].symbols, [only]);
    }

    #[test]
    fn resolving_a_symbol_keeps_a_valid_one_and_falls_back_otherwise() {
        let config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("the shipped config");
        let feed = config.feeds.first().expect("the shipped config has feeds");
        let (id, first) = (feed.id.clone(), feed.symbols[0].clone());

        assert_eq!(
            config.resolve_symbol(&id, &first),
            Some(first.clone()),
            "a symbol the feed offers is kept"
        );
        assert_eq!(
            config.resolve_symbol(&id, "NOT-A-SYMBOL"),
            Some(first),
            "one it does not falls back to the feed's first"
        );
        assert_eq!(
            config.resolve_symbol("not-a-feed", "ANY"),
            None,
            "an unknown feed resolves nothing, so the caller leaves the
             selection where it is rather than inventing one"
        );
    }

    #[test]
    fn embedded_default_parses_and_validates() {
        let config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("embedded default");
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
                ohlcv_history: true,
                ohlcv_generation: 0,
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
        // The mini index alone reads regionally; the mini dollar beside it
        // falls back to the feed-wide look. That ladder is the whole point of
        // per-symbol declarations.
        assert_eq!(b3.bubble_preset_for("WIN$N"), Some("mini index regions"));
        assert_eq!(b3.bubble_preset_for("WDO$N"), Some("live lane pie"));
        assert_eq!(binance.bubble_preset_for("BTCUSDT"), None);

        // The default open is the split: timeframe context beside the flow
        // chart (user decision 2026-08-06). The other feeds declare nothing
        // and open on the factory flow pane.
        assert_eq!(binance.default_layout, Some(DeclaredLayout::TimeAndFlow));
        assert_eq!(hyperliquid.default_layout, None);
        assert_eq!(tickmill.default_layout, None);
        assert_eq!(b3.default_layout, None);
    }

    #[test]
    fn startup_selection_override_preserves_the_whole_catalog() {
        let mut config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("embedded default");
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
        let mut config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("embedded default");
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
        let mut config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("embedded default");
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
        let err =
            parse(text, ConfigSource::Embedded, &AddedSymbols::default()).expect_err("blank name");
        assert!(
            err.to_string().contains("bubble_preset"),
            "the message names the field: {err}"
        );
    }

    /// An unknown preset name is ignored at runtime with only a log line as
    /// its symptom — for the WIN$N declaration that would silently disable
    /// the whole regional read. The shipped config and the shipped presets
    /// file are held together here instead.
    #[test]
    fn every_declared_bubble_preset_resolves_in_the_shipped_presets_file() {
        let presets = crate::bubble_presets::parse(include_str!("../config/bubbles.toml"))
            .expect("shipped presets file");
        let config = parse(
            EMBEDDED_DEFAULT,
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
        .expect("embedded default");
        for feed in &config.feeds {
            let declared = feed
                .bubble_preset
                .iter()
                .chain(feed.symbol_bubble_presets.values());
            for name in declared {
                assert!(
                    presets.get(name).is_some(),
                    "feed '{}' declares bubble preset '{name}', which the shipped \
                     bubbles.toml does not define",
                    feed.id
                );
            }
        }
    }

    #[test]
    fn a_symbol_preset_for_an_unoffered_symbol_is_a_config_error() {
        let text = r#"
            default_feed = "b"
            default_symbol = "AAA"
            [[feeds]]
            id = "b"
            name = "B"
            provider = "binance"
            symbols = ["AAA"]
            symbol_bubble_presets = { "GHOST" = "default" }
        "#;
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default())
            .expect_err("key names no symbol of the feed");
        assert!(
            err.to_string().contains("GHOST"),
            "the message names the key: {err}"
        );
    }

    #[test]
    fn an_empty_symbol_preset_name_is_a_config_error() {
        let text = r#"
            default_feed = "b"
            default_symbol = "AAA"
            [[feeds]]
            id = "b"
            name = "B"
            provider = "binance"
            symbols = ["AAA"]
            symbol_bubble_presets = { "AAA" = "  " }
        "#;
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default())
            .expect_err("blank preset name");
        assert!(
            err.to_string().contains("AAA"),
            "the message names the symbol: {err}"
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
        let mut config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
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
        let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
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
        let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
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
        assert!(gold.from_ports_map);

        // Both sides of the agreement come from one call: the port quantick
        // binds is the port the autostarted bridge is told to dial.
        let index = settings.endpoint_for("US500");
        assert_eq!(index.listen_addr, "127.0.0.1:9102");
        assert_eq!(index.dial.map(|(_, port)| port), Some(9102));

        let unmapped = settings.endpoint_for("WIN$N");
        assert_eq!(unmapped.listen_addr, "127.0.0.1:9100");
        assert!(
            !unmapped.from_ports_map,
            "it fell through to the shared default"
        );
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
        let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
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
        parse(&text, ConfigSource::Embedded, &AddedSymbols::default())
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

        // Port 0 is rejected here for the same reason it is in the map: every
        // unmapped symbol lands on this address, and an ephemeral port is not
        // something an EA can be configured against.
        let err = with_metatrader(r#"listen_addr = "127.0.0.1:0""#).expect_err("ephemeral port");
        let message = err.to_string();
        assert!(message.contains("listen_addr"), "{message}");
        assert!(message.contains("port 0"), "{message}");

        // And the ordinary one still loads.
        assert!(with_metatrader(r#"listen_addr = "127.0.0.1:9100""#).is_ok());
    }

    #[test]
    fn a_mapped_symbol_must_be_one_a_metatrader_feed_offers() {
        // A typo here has no symptom at all: the symbol silently falls back to
        // the shared port and fights whatever is already on it.
        let err = with_metatrader(
            r#"listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            XAUUSDD = 9101"#,
        )
        .expect_err("typo'd symbol");
        let message = err.to_string();
        assert!(message.contains("XAUUSDD"), "{message}");
        assert!(message.contains("no metatrader feed offers"), "{message}");

        // Padding is the same failure wearing a disguise — a feed's symbol
        // list carries none, so the key would never match.
        let err = with_metatrader(
            r#"listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            "XAUUSD " = 9101"#,
        )
        .expect_err("padded key");
        assert!(err.to_string().contains("whitespace"), "{err}");
    }

    #[test]
    fn a_symbol_two_metatrader_feeds_offer_cannot_be_mapped() {
        // Two brokers both quoting US500 is ordinary. Mapping it gives both one
        // port, and the map looks perfectly well-formed while they fight.
        let text = r#"
            default_feed = "tickmill"
            default_symbol = "US500"
            [[feeds]]
            id = "tickmill"
            name = "Tickmill"
            provider = "metatrader"
            symbols = ["US500"]
            [[feeds]]
            id = "other-broker"
            name = "Other"
            provider = "metatrader"
            symbols = ["US500"]
            [metatrader]
            listen_addr = "127.0.0.1:9100"
            [metatrader.ports]
            US500 = 9102
        "#;
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default())
            .expect_err("two feeds claim US500");
        let message = err.to_string();
        assert!(message.contains("US500"), "{message}");
        assert!(
            message.contains("tickmill") && message.contains("other-broker"),
            "both claimants are named: {message}"
        );

        // The same two feeds are fine as long as the shared symbol is not
        // mapped — they simply cannot stream at the same time.
        let unmapped = text.replace("[metatrader.ports]\n            US500 = 9102", "");
        assert!(parse(&unmapped, ConfigSource::Embedded, &AddedSymbols::default()).is_ok());
    }

    #[test]
    fn a_mapped_port_survives_a_default_address_with_no_host() {
        // Only reachable by building the settings in code — `validate` refuses
        // this file. It is pinned because dropping the mapped port here would
        // discard the one thing the caller stated explicitly, in favour of an
        // address that cannot bind at all.
        let settings = listening("garbage", &[("XAUUSD", 9101)]);
        let gold = settings.endpoint_for("XAUUSD");
        assert_eq!(gold.listen_addr, "127.0.0.1:9101");
        assert_eq!(gold.dial, Some(("127.0.0.1".into(), 9101)));
        assert!(gold.from_ports_map);

        // An unmapped symbol has nothing to salvage: the bad address passes
        // through and the bind reports it.
        let other = settings.endpoint_for("WIN$N");
        assert_eq!(other.listen_addr, "garbage");
        assert_eq!(other.dial, None);
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
        let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
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
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap_err();
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
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap_err();
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
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
    }

    #[test]
    fn empty_feeds_are_rejected() {
        let text = r#"
            default_feed = "binance"
            default_symbol = "BTCUSDT"
            feeds = []
        "#;
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap_err();
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
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    }

    /// The declared opening layout and bar spec are read from the feed entry,
    /// in the same vocabulary the View menu and the BARS group speak.
    #[test]
    fn declared_layout_and_bars_are_read_and_resolved() {
        let text = r#"
            default_feed = "b"
            default_symbol = "AAA"
            [[feeds]]
            id = "b"
            name = "B"
            provider = "binance"
            symbols = ["AAA"]
            default_layout = "time+flow"
            default_bars = "time:5m"
        "#;
        let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
        assert_eq!(
            config.feeds[0].default_layout,
            Some(DeclaredLayout::TimeAndFlow)
        );
        assert_eq!(
            config.startup_spec_for("b"),
            Some(crate::state::BarSpec::Time(300_000))
        );
        assert_eq!(config.startup_spec_for("nope"), None);

        // Absent, both fields change nothing — the factory defaults stand,
        // exactly as before the fields existed.
        let (undeclared, _) = sample();
        assert_eq!(undeclared.feeds[0].default_layout, None);
        assert_eq!(undeclared.startup_spec_for("binance"), None);
    }

    /// The TOML names and [`DeclaredLayout::parse`] are one vocabulary: what
    /// a config file may say, the `QUANTICK_LAYOUT` hook accepts, and
    /// nothing else on either side.
    #[test]
    fn layout_names_agree_between_serde_and_parse() {
        for (name, expected) in [
            ("flow", DeclaredLayout::Flow),
            ("time", DeclaredLayout::Time),
            ("time+flow", DeclaredLayout::TimeAndFlow),
        ] {
            assert_eq!(DeclaredLayout::parse(name), Some(expected), "{name}");
            let toml = format!("layout = \"{name}\"");
            #[derive(Deserialize)]
            struct Probe {
                layout: DeclaredLayout,
            }
            let probe: Probe = toml::from_str(&toml).expect(name);
            assert_eq!(probe.layout, expected, "{name}");
        }
        assert_eq!(DeclaredLayout::parse("grid"), None);
        assert_eq!(DeclaredLayout::parse(" time "), Some(DeclaredLayout::Time));
    }

    /// A `default_bars` no control could produce is refused at load, naming
    /// the feed — its only runtime symptom would be a silently
    /// factory-default chart.
    #[test]
    fn an_invalid_default_bars_is_refused_at_load() {
        for bad in ["time:0", "tick:0", "bananas:9", "time:25h", "50"] {
            let text = format!(
                r#"
                default_feed = "b"
                default_symbol = "AAA"
                [[feeds]]
                id = "b"
                name = "B"
                provider = "binance"
                symbols = ["AAA"]
                default_bars = "{bad}"
            "#
            );
            let err =
                parse(&text, ConfigSource::Embedded, &AddedSymbols::default()).expect_err(bad);
            let message = err.to_string();
            assert!(message.contains("default_bars"), "{message}");
            assert!(message.contains("'b'"), "the feed is named: {message}");
        }

        // An unknown layout name is a parse error, like an unknown provider.
        let text = r#"
            default_feed = "b"
            default_symbol = "AAA"
            [[feeds]]
            id = "b"
            name = "B"
            provider = "binance"
            symbols = ["AAA"]
            default_layout = "grid"
        "#;
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    }

    /// The reach's shipped numbers and its defaults are one number each — a
    /// file that says nothing must behave exactly like the constants the
    /// module documents, or the two records start drifting the first time one
    /// of them is tuned.
    #[test]
    fn an_absent_history_section_defaults_to_the_reach_constants() {
        let (config, _) = sample();
        let bounds = config.history.reach_bounds();
        assert_eq!(bounds.session_gap_ms, crate::history_reach::SESSION_GAP_MS);
        assert_eq!(
            bounds.previous_session_lead_ms,
            crate::history_reach::PREVIOUS_SESSION_LEAD_MS
        );
    }

    /// And a venue with different hours reaches the campaign through the file,
    /// with no rebuild.
    #[test]
    fn a_configured_session_gap_and_lead_reach_the_campaign() {
        let text = r#"
            default_feed = "binance"
            default_symbol = "BTCUSDT"
            [[feeds]]
            id = "binance"
            name = "Binance"
            provider = "binance"
            symbols = ["BTCUSDT"]
            [history]
            session_gap_minutes = 90
            previous_session_lead_minutes = 360
        "#;
        let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
        let bounds = config.history.reach_bounds();
        assert_eq!(bounds.session_gap_ms, 90 * 60_000);
        assert_eq!(bounds.previous_session_lead_ms, 360 * 60_000);
    }

    /// A gap of zero would make every print its own session, so the file is
    /// refused rather than quietly repaired.
    #[test]
    fn a_session_gap_of_zero_is_refused() {
        let text = r#"
            default_feed = "binance"
            default_symbol = "BTCUSDT"
            [[feeds]]
            id = "binance"
            name = "Binance"
            provider = "binance"
            symbols = ["BTCUSDT"]
            [history]
            session_gap_minutes = 0
        "#;
        let err = parse(text, ConfigSource::Embedded, &AddedSymbols::default())
            .expect_err("a zero gap is not a configuration");
        assert!(
            format!("{err}").contains("session_gap_minutes"),
            "the message has to name the key: {err}"
        );
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
            parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap(),
            ConfigSource::Embedded,
        )
    }
}
