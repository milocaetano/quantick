//! Deterministic tick → [`Trade`] mapping, with an explicit policy for every
//! field MT5 does not honestly provide.
//!
//! MT5 is missing three things the engine's [`Trade`] needs, and each gets a
//! labelled policy — never a silent guess:
//!
//! - **No exchange trade id** → `agg_id` is the bridge's session `seq`
//!   (synthetic; good for gap detection, not stable across sessions).
//! - **Timestamps in server wall time** → converted to true UTC using the
//!   bridge-declared offset (`server_utc_offset_s`), refreshed on heartbeats.
//! - **Unreliable aggressor flags** → [`SideMode`] picks the policy. On the
//!   B3 broker probed on 2026-07-23, *every* tick (live and history) carried
//!   the BUY bit (`flags = 1080`), so trusting flags there would chart 100%
//!   buys. [`SideMode::TickRule`] (López de Prado's tick rule: uptick = buy,
//!   downtick = sell, unchanged = carry) is the default for such feeds; every
//!   trade records where its side came from ([`SideSource`]), and everything
//!   undeterminable is dropped and counted ([`MapStats`]), never invented.
//!
//! Pure and synchronous: no I/O, no clocks — same ticks in, same trades out.

use rust_decimal::Decimal;
use std::str::FromStr as _;

use quantick_engine::{Side, Trade};

use crate::protocol::{AggressorFlag, Tick, aggressor_from_flags, flags};

/// How the aggressor side is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideMode {
    /// Trust the BUY/SELL tick flags; drop ticks where they are absent or
    /// ambiguous. Use only on brokers whose flags are known-good.
    Flags,
    /// Ignore the flags and classify by the tick rule (uptick = buy, downtick
    /// = sell, unchanged = same as previous). The default for B3 feeds, whose
    /// flags were observed to be unusable.
    TickRule,
}

/// Where a mapped trade's side actually came from — kept per trade so the
/// inference is auditable, not hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideSource {
    /// The exchange flag said so (only in [`SideMode::Flags`]).
    ExchangeFlag,
    /// Inferred from a price change (tick rule).
    TickRule,
    /// Price unchanged; side carried from the previous trade (tick rule).
    Carried,
}

/// Why a tick did not become a trade. Every reason is counted in [`MapStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// `last` was not a parseable positive decimal.
    BadPrice,
    /// A trade tick with `volume == 0` — nothing was exchanged.
    ZeroVolume,
    /// [`SideMode::Flags`]: neither BUY nor SELL bit set.
    NoAggressorFlag,
    /// [`SideMode::Flags`]: both BUY and SELL bits set.
    AmbiguousFlags,
    /// [`SideMode::TickRule`]: no prior price movement yet, so no side can be
    /// inferred (the first trades of a session, until the price first moves).
    NoTickRuleContext,
}

/// The outcome of mapping one tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapOutcome {
    /// A genuine trade, ready for the engine, with its side's provenance.
    Trade {
        /// The engine-ready trade.
        trade: Trade,
        /// Where the aggressor side came from.
        source: SideSource,
    },
    /// A quote-only tick (no LAST flag): honest market data, but not a trade —
    /// bars are built from trades only.
    QuoteOnly,
    /// A trade-like tick that could not honestly become a trade.
    Dropped(DropReason),
}

/// Counters for everything the mapper did — the honest ledger an operator (or
/// an AI reading logs) uses to judge feed quality. All fields public on
/// purpose: they are data, not behaviour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MapStats {
    /// Trades emitted with side taken from exchange flags.
    pub side_from_flag: u64,
    /// Trades emitted with side inferred by a price change.
    pub side_from_tick_rule: u64,
    /// Trades emitted with side carried from the previous trade.
    pub side_carried: u64,
    /// Quote-only ticks seen (not trades; not charted).
    pub quote_only: u64,
    /// Drops: unparseable/non-positive price.
    pub dropped_bad_price: u64,
    /// Drops: trade tick with zero volume.
    pub dropped_zero_volume: u64,
    /// Drops: flags mode, no aggressor bit.
    pub dropped_no_aggressor_flag: u64,
    /// Drops: flags mode, both aggressor bits.
    pub dropped_ambiguous_flags: u64,
    /// Drops: tick-rule mode, before the first price movement.
    pub dropped_no_tick_rule_context: u64,
}

impl MapStats {
    /// Total trades emitted.
    #[must_use]
    pub fn trades(&self) -> u64 {
        self.side_from_flag + self.side_from_tick_rule + self.side_carried
    }

    /// Total ticks dropped.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped_bad_price
            + self.dropped_zero_volume
            + self.dropped_no_aggressor_flag
            + self.dropped_ambiguous_flags
            + self.dropped_no_tick_rule_context
    }

    /// Emit the whole ledger as one structured log line (AI-first: a log
    /// excerpt alone answers "what did the mapper do and why").
    pub fn log_summary(&self, symbol: &str) {
        tracing::info!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_MAP_SUMMARY",
            symbol,
            trades = self.trades(),
            side_from_flag = self.side_from_flag,
            side_from_tick_rule = self.side_from_tick_rule,
            side_carried = self.side_carried,
            quote_only = self.quote_only,
            dropped = self.dropped(),
            dropped_bad_price = self.dropped_bad_price,
            dropped_zero_volume = self.dropped_zero_volume,
            dropped_no_aggressor_flag = self.dropped_no_aggressor_flag,
            dropped_ambiguous_flags = self.dropped_ambiguous_flags,
            dropped_no_tick_rule_context = self.dropped_no_tick_rule_context,
            "mt5 tick mapping summary"
        );
    }
}

/// Stateful tick → trade mapper for one bridge session.
///
/// The state is exactly what the tick rule needs (previous trade price and
/// side) plus the server-time offset; feeding the same tick sequence always
/// produces the same trades.
#[derive(Debug)]
pub struct TickMapper {
    mode: SideMode,
    /// `server_time - utc`, in milliseconds (from hello, refreshed by
    /// heartbeats).
    offset_ms: i64,
    prev_price: Option<Decimal>,
    prev_side: Option<Side>,
    /// The honest ledger of everything mapped, dropped and why.
    pub stats: MapStats,
}

impl TickMapper {
    /// A mapper for one session with the given side policy and the hello's
    /// `server_utc_offset_s`.
    #[must_use]
    pub fn new(mode: SideMode, server_utc_offset_s: i64) -> Self {
        Self {
            mode,
            // Saturating: the offset is declared by the bridge (any local
            // process may connect), so an absurd value must not panic the feed
            // task via i64 overflow. A realistic offset (±14 h) is unaffected.
            offset_ms: server_utc_offset_s.saturating_mul(1000),
            prev_price: None,
            prev_side: None,
            stats: MapStats::default(),
        }
    }

    /// Refresh the server-time offset (heartbeats may recompute it, e.g.
    /// across a DST change on brokers that observe one).
    pub fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        self.offset_ms = offset_s.saturating_mul(1000);
    }

    /// Map one tick. Updates the tick-rule state and the stats ledger.
    pub fn map(&mut self, tick: &Tick) -> MapOutcome {
        // No LAST bit → the tick is a quote update, not a trade.
        if tick.flags & flags::LAST == 0 {
            self.stats.quote_only += 1;
            return MapOutcome::QuoteOnly;
        }

        let Ok(price) = Decimal::from_str(&tick.last) else {
            self.stats.dropped_bad_price += 1;
            return MapOutcome::Dropped(DropReason::BadPrice);
        };
        if price <= Decimal::ZERO {
            self.stats.dropped_bad_price += 1;
            return MapOutcome::Dropped(DropReason::BadPrice);
        }
        if tick.volume == 0 {
            self.stats.dropped_zero_volume += 1;
            return MapOutcome::Dropped(DropReason::ZeroVolume);
        }

        // Decide the side per the configured policy.
        let decided = match self.mode {
            SideMode::Flags => match aggressor_from_flags(tick.flags) {
                AggressorFlag::Buy => Ok((Side::Buy, SideSource::ExchangeFlag)),
                AggressorFlag::Sell => Ok((Side::Sell, SideSource::ExchangeFlag)),
                AggressorFlag::Ambiguous => Err(DropReason::AmbiguousFlags),
                AggressorFlag::Absent => Err(DropReason::NoAggressorFlag),
            },
            SideMode::TickRule => match self.prev_price {
                Some(prev) if price > prev => Ok((Side::Buy, SideSource::TickRule)),
                Some(prev) if price < prev => Ok((Side::Sell, SideSource::TickRule)),
                Some(_) => match self.prev_side {
                    Some(side) => Ok((side, SideSource::Carried)),
                    None => Err(DropReason::NoTickRuleContext),
                },
                None => Err(DropReason::NoTickRuleContext),
            },
        };

        // The price is real either way: it must feed the next tick-rule
        // comparison even when this tick's own side was undeterminable.
        self.prev_price = Some(price);

        match decided {
            Ok((side, source)) => {
                self.prev_side = Some(side);
                match source {
                    SideSource::ExchangeFlag => self.stats.side_from_flag += 1,
                    SideSource::TickRule => self.stats.side_from_tick_rule += 1,
                    SideSource::Carried => self.stats.side_carried += 1,
                }
                MapOutcome::Trade {
                    trade: Trade {
                        agg_id: tick.seq,
                        // Saturating: `time_ms` and `offset_ms` both originate
                        // from the untrusted bridge; overflow must not panic.
                        timestamp_ms: tick.time_ms.saturating_sub(self.offset_ms),
                        price,
                        quantity: Decimal::from(tick.volume),
                        side,
                    },
                    source,
                }
            }
            Err(reason) => {
                match reason {
                    DropReason::NoAggressorFlag => self.stats.dropped_no_aggressor_flag += 1,
                    DropReason::AmbiguousFlags => self.stats.dropped_ambiguous_flags += 1,
                    DropReason::NoTickRuleContext => self.stats.dropped_no_tick_rule_context += 1,
                    // BadPrice / ZeroVolume returned earlier.
                    DropReason::BadPrice | DropReason::ZeroVolume => unreachable!(),
                }
                MapOutcome::Dropped(reason)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trade tick like the real B3 recording: LAST|VOLUME plus the
    /// undocumented 1024 bit, and whatever aggressor bits the case needs.
    fn tick(seq: u64, last: &str, volume: u64, aggressor_bits: u32) -> Tick {
        Tick {
            seq,
            time_ms: 1_784_824_300_000 + seq as i64,
            bid: "0".to_string(),
            ask: "0".to_string(),
            last: last.to_string(),
            volume,
            flags: flags::LAST | flags::VOLUME | 1024 | aggressor_bits,
        }
    }

    #[test]
    fn tick_rule_classifies_up_down_and_carry() {
        let mut m = TickMapper::new(SideMode::TickRule, -10_800);
        // First trade: no context yet — dropped, honestly.
        assert_eq!(
            m.map(&tick(1, "100", 1, flags::BUY)),
            MapOutcome::Dropped(DropReason::NoTickRuleContext)
        );
        // Uptick → buy (the bogus BUY flag is ignored in this mode).
        let MapOutcome::Trade { trade, source } = m.map(&tick(2, "101", 2, flags::BUY)) else {
            panic!("expected trade");
        };
        assert_eq!(trade.side, Side::Buy);
        assert_eq!(source, SideSource::TickRule);
        // Unchanged → carried buy.
        let MapOutcome::Trade { trade, source } = m.map(&tick(3, "101", 1, 0)) else {
            panic!("expected trade");
        };
        assert_eq!(trade.side, Side::Buy);
        assert_eq!(source, SideSource::Carried);
        // Downtick → sell.
        let MapOutcome::Trade { trade, source } = m.map(&tick(4, "100", 1, 0)) else {
            panic!("expected trade");
        };
        assert_eq!(trade.side, Side::Sell);
        assert_eq!(source, SideSource::TickRule);

        assert_eq!(m.stats.trades(), 3);
        assert_eq!(m.stats.dropped_no_tick_rule_context, 1);
    }

    #[test]
    fn equal_prices_before_any_movement_are_dropped_not_guessed() {
        let mut m = TickMapper::new(SideMode::TickRule, 0);
        assert!(matches!(
            m.map(&tick(1, "100", 1, 0)),
            MapOutcome::Dropped(_)
        ));
        // Same price again: still no movement, still no side to carry.
        assert!(matches!(
            m.map(&tick(2, "100", 1, 0)),
            MapOutcome::Dropped(_)
        ));
        assert_eq!(m.stats.dropped_no_tick_rule_context, 2);
        // First movement finally classifies.
        assert!(matches!(
            m.map(&tick(3, "101", 1, 0)),
            MapOutcome::Trade {
                source: SideSource::TickRule,
                ..
            }
        ));
    }

    #[test]
    fn flags_mode_trusts_and_refuses_flags_explicitly() {
        let mut m = TickMapper::new(SideMode::Flags, 0);
        assert!(matches!(
            m.map(&tick(1, "100", 1, flags::BUY)),
            MapOutcome::Trade { source: SideSource::ExchangeFlag, trade } if trade.side == Side::Buy
        ));
        assert!(matches!(
            m.map(&tick(2, "100", 1, flags::SELL)),
            MapOutcome::Trade { source: SideSource::ExchangeFlag, trade } if trade.side == Side::Sell
        ));
        assert_eq!(
            m.map(&tick(3, "100", 1, flags::BUY | flags::SELL)),
            MapOutcome::Dropped(DropReason::AmbiguousFlags)
        );
        assert_eq!(
            m.map(&tick(4, "100", 1, 0)),
            MapOutcome::Dropped(DropReason::NoAggressorFlag)
        );
        assert_eq!(m.stats.side_from_flag, 2);
        assert_eq!(m.stats.dropped_ambiguous_flags, 1);
        assert_eq!(m.stats.dropped_no_aggressor_flag, 1);
    }

    #[test]
    fn quote_only_ticks_never_become_trades() {
        let mut m = TickMapper::new(SideMode::TickRule, 0);
        let quote = Tick {
            seq: 1,
            time_ms: 0,
            bid: "99".to_string(),
            ask: "101".to_string(),
            last: "0".to_string(),
            volume: 0,
            flags: flags::BID | flags::ASK, // no LAST
        };
        assert_eq!(m.map(&quote), MapOutcome::QuoteOnly);
        assert_eq!(m.stats.quote_only, 1);
        assert_eq!(m.stats.trades(), 0);
    }

    #[test]
    fn bad_price_and_zero_volume_are_dropped_and_counted() {
        let mut m = TickMapper::new(SideMode::TickRule, 0);
        assert_eq!(
            m.map(&tick(1, "not-a-price", 1, 0)),
            MapOutcome::Dropped(DropReason::BadPrice)
        );
        assert_eq!(
            m.map(&tick(2, "0", 1, 0)),
            MapOutcome::Dropped(DropReason::BadPrice)
        );
        assert_eq!(
            m.map(&tick(3, "100", 0, 0)),
            MapOutcome::Dropped(DropReason::ZeroVolume)
        );
        assert_eq!(m.stats.dropped_bad_price, 2);
        assert_eq!(m.stats.dropped_zero_volume, 1);
    }

    #[test]
    fn timestamps_convert_server_time_to_utc() {
        // B3: server = UTC−3 → offset −10800 s. A tick stamped 16:32 BRT
        // (server epoch) must surface as 19:32 UTC.
        let mut m = TickMapper::new(SideMode::TickRule, -10_800);
        m.map(&tick(1, "100", 1, 0)); // context
        let MapOutcome::Trade { trade, .. } = m.map(&tick(2, "101", 1, 0)) else {
            panic!("expected trade");
        };
        let raw = 1_784_824_300_000 + 2;
        assert_eq!(trade.timestamp_ms, raw + 10_800_000);
    }

    #[test]
    fn heartbeat_offset_refresh_applies_to_later_ticks() {
        let mut m = TickMapper::new(SideMode::TickRule, 0);
        m.map(&tick(1, "100", 1, 0));
        m.set_server_utc_offset_s(-3600);
        let MapOutcome::Trade { trade, .. } = m.map(&tick(2, "101", 1, 0)) else {
            panic!("expected trade");
        };
        assert_eq!(trade.timestamp_ms, (1_784_824_300_000 + 2) + 3_600_000);
    }

    #[test]
    fn extreme_server_offset_does_not_panic() {
        // The hello's `server_utc_offset_s` comes from the bridge, which any
        // local process can impersonate. An absurd offset must not overflow the
        // i64 conversion (`offset_s * 1000`, then `time_ms - offset_ms`) and
        // panic the feed task — the arithmetic saturates instead.
        let mut m = TickMapper::new(SideMode::TickRule, i64::MAX);
        m.map(&tick(1, "100", 1, 0)); // context
        let MapOutcome::Trade { trade, .. } = m.map(&tick(2, "101", 1, 0)) else {
            panic!("expected trade");
        };
        // offset_ms saturates to i64::MAX; a positive time_ms minus it stays a
        // (large) negative in range — no panic.
        assert!(trade.timestamp_ms < 0);

        // A crafted heartbeat offset refresh must be equally safe. offset_ms
        // saturates to i64::MIN, so `time_ms - i64::MIN` saturates to i64::MAX.
        m.set_server_utc_offset_s(i64::MIN);
        let MapOutcome::Trade { trade, .. } = m.map(&tick(3, "102", 1, 0)) else {
            panic!("expected trade");
        };
        assert_eq!(trade.timestamp_ms, i64::MAX);
    }

    #[test]
    fn agg_id_is_the_synthetic_bridge_seq() {
        let mut m = TickMapper::new(SideMode::TickRule, 0);
        m.map(&tick(7, "100", 1, 0));
        let MapOutcome::Trade { trade, .. } = m.map(&tick(8, "101", 1, 0)) else {
            panic!("expected trade");
        };
        assert_eq!(trade.agg_id, 8);
    }
}
