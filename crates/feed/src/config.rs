//! The feed-shaped half of quantick's TOML configuration.
//!
//! Which backend streams a feed, what that backend can and cannot report, and
//! how the MetaTrader listener is addressed. These types describe the adapters
//! beside them, so they are declared here rather than in the application;
//! `quantick-app`'s `config` module re-exports every one of them, and owns the
//! `AppConfig` that holds them and the file it is loaded from.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Which backend streams a feed. This is the one place a config string is mapped
/// to a code path; adding a provider means adding a variant here and a matching
/// arm in [`crate::spawn`].
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
    /// Whether a session of this provider *may* carry the venue's deal
    /// counter — the static half, narrowed at hello the way
    /// [`capabilities`](Self::capabilities) is: a MetaTrader session on a
    /// quoted CFD declares none. What a config validator asks before a
    /// chart opens on `trades:N` or records; the interface still asks the
    /// session.
    #[must_use]
    pub fn may_count_deals(self) -> bool {
        match self {
            ProviderKind::Binance | ProviderKind::Hyperliquid => false,
            ProviderKind::MetaTrader => true,
        }
    }

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
    /// [`crate::FeedHandle::capabilities`]).
    #[must_use]
    pub fn capabilities(self) -> FeedCapabilities {
        match self {
            ProviderKind::Binance => FeedCapabilities {
                book_capture: true,
                history_paging: true,
                traded_volume: true,
                deal_counter: false,
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
                deal_counter: false,
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
                deal_counter: false,
                ohlcv_history: false,
                ohlcv_generation: 0,
            },
        }
    }

    /// The one line to try when this provider has stopped delivering and the
    /// feed itself has no more specific reason to give.
    ///
    /// Sibling of [`capabilities`](Self::capabilities) and answered the same
    /// way: the interface asks the provider, never branches on its name. A
    /// venue added later writes its own sentence here and every stall readout
    /// in the application gets it, rather than a generic instruction that fits
    /// none of them — the chains genuinely differ, and "check that the
    /// terminal is running" is nonsense for a public WebSocket.
    ///
    /// One sentence, capitalized, ending in a full stop: it is shown on its own
    /// under a headline, and also embedded after a clause (see
    /// [`crate::stall`]).
    #[must_use]
    pub fn recovery_hint(self) -> &'static str {
        match self {
            ProviderKind::Binance | ProviderKind::Hyperliquid => {
                "Check this machine's internet connection, then reconnect."
            }
            // The one provider whose data path runs through another
            // application the trader can see and restart, which is why its
            // hint names that application rather than the network.
            ProviderKind::MetaTrader => {
                "Check that MetaTrader 5 is running and the QuantickBridge chart is still attached."
            }
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
    /// Its live prints come with the venue's own count of exchange deals.
    ///
    /// MetaTrader folds several deals into one tick and publishes only the
    /// session's running total, which the bridge stamps on every live tick
    /// (`deals` in `bridge/mt5/PROTOCOL.md`). A deal bar — the chart's
    /// `trades` kind — can be cut only where that count exists, so the kind
    /// is offered only here. Per session, learned at hello; never true of a
    /// quoted CFD, of a bridge older than the stamp, or of a feed that has
    /// not said so.
    pub deal_counter: bool,
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
            deal_counter: false,
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
pub fn split_host_port(addr: &str) -> Option<(&str, u16)> {
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
    ///
    /// Public because the application's `AppConfig::validate` calls it as part
    /// of validating the whole file; these rules are about these settings, so
    /// they are checked here.
    ///
    /// # Errors
    ///
    /// The message names the offending key and why it could never work.
    pub fn validate(&self) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
