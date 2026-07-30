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
//! A fourth gap is not MT5's doing but the venue's: some symbols have **no
//! tape at all**. A broker-quoted CFD moves bid and ask and never prints a
//! trade, so [`TapeKind::Quotes`] switches the mapper to charting what does
//! exist — one synthetic print per tick, at the mid, carrying one unit. Those
//! prints say "a tick happened here, at this price"; they never claim traded
//! volume, and [`MapStats::trades_from_quotes`] keeps the count separable.
//!
//! Pure and synchronous: no I/O, no clocks — same ticks in, same trades out.

use rust_decimal::Decimal;
use std::str::FromStr as _;

use quantick_engine::{Side, Trade};

use crate::protocol::{AggressorFlag, TapeKind, Tick, aggressor_from_flags, flags};

/// Quantity carried by a print synthesised from a quote: one tick, one unit.
///
/// Not a traded size — the venue traded nothing. It exists so tick bars can
/// count, and it is why a quote-driven feed must keep volume-based bar types
/// and footprints switched off.
const SYNTHETIC_QUANTITY: Decimal = Decimal::ONE;

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
    /// [`TapeKind::Quotes`]: bid or ask was missing or non-positive, so there
    /// is no two-sided quote to take a mid from.
    MissingQuote,
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
    /// bars are built from trades only. Only under [`TapeKind::Trades`]; where
    /// the venue prints nothing, the quote *is* the data (see
    /// [`TickMapper::map`]).
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
    /// Quote-only ticks seen (not trades; not charted). Always 0 under
    /// [`TapeKind::Quotes`], where a quote is exactly what gets charted.
    pub quote_only: u64,
    /// How many of the emitted trades were synthesised from a quote rather
    /// than printed by the venue — a subset of [`MapStats::trades`], kept
    /// separate so "this chart is quote-derived" stays a countable fact.
    pub trades_from_quotes: u64,
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
    /// Drops: quote mode, tick without a usable two-sided quote.
    pub dropped_missing_quote: u64,
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
            + self.dropped_missing_quote
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
            trades_from_quotes = self.trades_from_quotes,
            dropped = self.dropped(),
            dropped_bad_price = self.dropped_bad_price,
            dropped_zero_volume = self.dropped_zero_volume,
            dropped_no_aggressor_flag = self.dropped_no_aggressor_flag,
            dropped_ambiguous_flags = self.dropped_ambiguous_flags,
            dropped_no_tick_rule_context = self.dropped_no_tick_rule_context,
            dropped_missing_quote = self.dropped_missing_quote,
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
    /// What the venue prints, as the bridge declared it in its hello.
    tape: TapeKind,
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
    /// `server_utc_offset_s`, for a venue that prints trades.
    #[must_use]
    pub fn new(mode: SideMode, server_utc_offset_s: i64) -> Self {
        Self {
            mode,
            tape: TapeKind::Trades,
            // Saturating: the offset is declared by the bridge (any local
            // process may connect), so an absurd value must not panic the feed
            // task via i64 overflow. A realistic offset (±14 h) is unaffected.
            offset_ms: server_utc_offset_s.saturating_mul(1000),
            prev_price: None,
            prev_side: None,
            stats: MapStats::default(),
        }
    }

    /// Map for a venue of the given kind. A session keeps one kind from its
    /// hello to its last tick, so the same recording always maps the same way.
    #[must_use]
    pub fn with_tape(mut self, tape: TapeKind) -> Self {
        self.tape = tape;
        self
    }

    /// What this mapper assumes the venue prints.
    #[must_use]
    pub fn tape(&self) -> TapeKind {
        self.tape
    }

    /// Refresh the server-time offset (heartbeats may recompute it, e.g.
    /// across a DST change on brokers that observe one).
    pub fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        self.offset_ms = offset_s.saturating_mul(1000);
    }

    /// Map one tick. Updates the tick-rule state and the stats ledger.
    pub fn map(&mut self, tick: &Tick) -> MapOutcome {
        match self.tape {
            TapeKind::Trades => self.map_printed_trade(tick),
            TapeKind::Quotes => self.map_quote_as_print(tick),
        }
    }

    /// The venue prints trades: only a tick carrying LAST and a real volume is
    /// one, and the quotes around it are market data the bars do not use.
    fn map_printed_trade(&mut self, tick: &Tick) -> MapOutcome {
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
            SideMode::TickRule => self.tick_rule(price),
        };

        self.finish(tick, price, Decimal::from(tick.volume), decided)
    }

    /// The venue only quotes: nothing is ever printed, so the honest thing to
    /// chart is the tick itself. Every two-sided quote becomes one synthetic
    /// print at the mid, sized [`SYNTHETIC_QUANTITY`].
    ///
    /// The mid rather than bid or ask because a fixed spread would otherwise
    /// print as real movement — the probed US500 quotes a constant 0.30, which
    /// as bid/ask alternation would chart 30 points of chop that never
    /// happened. The mid keeps every price move a move in the market.
    ///
    /// The aggressor flags and any LAST field are ignored here on purpose: a
    /// venue that prints nothing has no aggressor to report, and mixing the odd
    /// printed trade into a synthetic series would put two different meanings
    /// of "volume" on one chart.
    fn map_quote_as_print(&mut self, tick: &Tick) -> MapOutcome {
        let (Ok(bid), Ok(ask)) = (Decimal::from_str(&tick.bid), Decimal::from_str(&tick.ask))
        else {
            self.stats.dropped_bad_price += 1;
            return MapOutcome::Dropped(DropReason::BadPrice);
        };
        // A one-sided or empty quote is common in recorded history, where MT5
        // leaves the side it does not know at "0". There is no mid to take.
        if bid <= Decimal::ZERO || ask <= Decimal::ZERO {
            self.stats.dropped_missing_quote += 1;
            return MapOutcome::Dropped(DropReason::MissingQuote);
        }
        // Checked: prices come from an untrusted local bridge, and Decimal
        // overflow must drop the tick, never panic the feed task.
        let Some(mid) = bid
            .checked_add(ask)
            .and_then(|sum| sum.checked_div(Decimal::TWO))
        else {
            self.stats.dropped_bad_price += 1;
            return MapOutcome::Dropped(DropReason::BadPrice);
        };

        // The tick rule is the only side policy available: with no print there
        // is no aggressor flag to trust, whatever `side_source` is configured.
        let decided = self.tick_rule(mid);
        self.finish(tick, mid, SYNTHETIC_QUANTITY, decided)
    }

    /// López de Prado's tick rule: uptick = buy, downtick = sell, unchanged =
    /// carry the previous side. The one place the rule lives, so both venue
    /// kinds classify identically.
    fn tick_rule(&self, price: Decimal) -> Result<(Side, SideSource), DropReason> {
        match self.prev_price {
            Some(prev) if price > prev => Ok((Side::Buy, SideSource::TickRule)),
            Some(prev) if price < prev => Ok((Side::Sell, SideSource::TickRule)),
            Some(_) => match self.prev_side {
                Some(side) => Ok((side, SideSource::Carried)),
                None => Err(DropReason::NoTickRuleContext),
            },
            None => Err(DropReason::NoTickRuleContext),
        }
    }

    /// Record the decision and turn it into an outcome — the single exit both
    /// venue kinds take, so the ledger can never disagree with what was
    /// emitted.
    fn finish(
        &mut self,
        tick: &Tick,
        price: Decimal,
        quantity: Decimal,
        decided: Result<(Side, SideSource), DropReason>,
    ) -> MapOutcome {
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
                if self.tape == TapeKind::Quotes {
                    self.stats.trades_from_quotes += 1;
                }
                MapOutcome::Trade {
                    trade: Trade {
                        agg_id: tick.seq,
                        // Saturating: `time_ms` and `offset_ms` both originate
                        // from the untrusted bridge; overflow must not panic.
                        timestamp_ms: tick.time_ms.saturating_sub(self.offset_ms),
                        price,
                        quantity,
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
                    // BadPrice / ZeroVolume / MissingQuote returned earlier.
                    DropReason::BadPrice | DropReason::ZeroVolume | DropReason::MissingQuote => {
                        unreachable!("returned before the side decision")
                    }
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

    /// A quote tick as the Tickmill US500 recording sends them: both sides
    /// priced, `last` zero, no volume, only the BID|ASK bits.
    fn quote(seq: u64, bid: &str, ask: &str) -> Tick {
        Tick {
            seq,
            time_ms: 1_785_327_308_000 + seq as i64,
            bid: bid.to_string(),
            ask: ask.to_string(),
            last: "0.00".to_string(),
            volume: 0,
            flags: flags::BID | flags::ASK,
        }
    }

    #[test]
    fn quote_driven_prints_the_mid_at_one_unit() {
        let mut m = TickMapper::new(SideMode::TickRule, 0).with_tape(TapeKind::Quotes);
        // First quote: a price, but no movement to classify yet.
        assert_eq!(
            m.map(&quote(1, "7447.81", "7448.11")),
            MapOutcome::Dropped(DropReason::NoTickRuleContext)
        );
        // Mid rises 7447.96 → 7447.97: a buy, one unit, at the mid.
        let MapOutcome::Trade { trade, source } = m.map(&quote(2, "7447.82", "7448.12")) else {
            panic!("expected a synthetic print");
        };
        assert_eq!(trade.price, Decimal::from_str("7447.97").unwrap());
        assert_eq!(trade.quantity, Decimal::ONE);
        assert_eq!(trade.side, Side::Buy);
        assert_eq!(source, SideSource::TickRule);
        assert_eq!(trade.agg_id, 2);

        // Mid falls: a sell.
        let MapOutcome::Trade { trade, .. } = m.map(&quote(3, "7447.50", "7447.80")) else {
            panic!("expected a synthetic print");
        };
        assert_eq!(trade.side, Side::Sell);

        assert_eq!(m.stats.trades(), 2);
        assert_eq!(m.stats.trades_from_quotes, 2, "both came from quotes");
        assert_eq!(m.stats.quote_only, 0, "quotes are the data here");
    }

    #[test]
    fn a_half_tick_mid_keeps_its_exact_decimal() {
        // An odd cent sum halves to three decimals. Rounding it to the
        // symbol's two would throw away half the resolution the quote has.
        let mut m = TickMapper::new(SideMode::TickRule, 0).with_tape(TapeKind::Quotes);
        m.map(&quote(1, "7447.80", "7448.10")); // context: mid 7447.95
        let MapOutcome::Trade { trade, .. } = m.map(&quote(2, "7447.81", "7448.10")) else {
            panic!("expected a synthetic print");
        };
        assert_eq!(trade.price, Decimal::from_str("7447.955").unwrap());
    }

    #[test]
    fn quote_driven_needs_both_sides_of_the_quote() {
        let mut m = TickMapper::new(SideMode::TickRule, 0).with_tape(TapeKind::Quotes);
        assert_eq!(
            m.map(&quote(1, "0.00", "7448.11")),
            MapOutcome::Dropped(DropReason::MissingQuote)
        );
        assert_eq!(
            m.map(&quote(2, "7447.81", "0.00")),
            MapOutcome::Dropped(DropReason::MissingQuote)
        );
        assert_eq!(
            m.map(&quote(3, "nonsense", "7448.11")),
            MapOutcome::Dropped(DropReason::BadPrice)
        );
        assert_eq!(m.stats.dropped_missing_quote, 2);
        assert_eq!(m.stats.dropped_bad_price, 1);
        assert_eq!(m.stats.trades(), 0);
    }

    #[test]
    fn quote_driven_ignores_flags_and_any_printed_last() {
        // Even in Flags mode, and even when a tick claims a LAST price with a
        // BUY aggressor, a quote-driven session charts the mid and infers the
        // side itself: the venue prints nothing, so there is nothing to trust.
        let mut m = TickMapper::new(SideMode::Flags, 0).with_tape(TapeKind::Quotes);
        let mut printed = quote(1, "100.00", "101.00");
        printed.last = "500.00".to_string();
        printed.volume = 42;
        printed.flags = flags::LAST | flags::VOLUME | flags::BUY;
        assert_eq!(
            m.map(&printed),
            MapOutcome::Dropped(DropReason::NoTickRuleContext)
        );

        let mut printed = quote(2, "99.00", "100.00");
        printed.last = "500.00".to_string();
        printed.volume = 42;
        printed.flags = flags::LAST | flags::VOLUME | flags::BUY;
        let MapOutcome::Trade { trade, source } = m.map(&printed) else {
            panic!("expected a synthetic print");
        };
        assert_eq!(trade.price, Decimal::from_str("99.5").unwrap());
        assert_eq!(trade.quantity, Decimal::ONE, "never the claimed volume");
        assert_eq!(trade.side, Side::Sell, "mid fell 100.5 → 99.5");
        assert_eq!(source, SideSource::TickRule);
        assert_eq!(m.stats.side_from_flag, 0);
    }

    #[test]
    fn a_tape_venue_is_the_default_and_is_unchanged() {
        // Nothing about the printed-trade path depends on the new field: the
        // default mapper still refuses a quote outright.
        let m = TickMapper::new(SideMode::TickRule, 0);
        assert_eq!(m.tape(), TapeKind::Trades);
        let mut m = m;
        assert_eq!(m.map(&quote(1, "99.00", "101.00")), MapOutcome::QuoteOnly);
        assert_eq!(m.stats.trades_from_quotes, 0);
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
