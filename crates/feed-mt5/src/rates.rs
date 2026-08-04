//! Historical candle → engine [`Bar`] mapping.
//!
//! MetaTrader's `CopyRates` is the only way to see further back than the tick
//! window, and what it returns is a candle, not a tape. Three things the engine
//! expects from a [`Bar`] are therefore not available, and each gets a stated
//! policy rather than a plausible-looking value:
//!
//! - **No aggressor split.** The terminal reports one volume per candle and
//!   nothing about who crossed the spread. Both sides carry half of it, which
//!   keeps [`Bar::volume`] exact and makes [`Bar::delta`] identically zero.
//!   Zero here means *not measured* — never "measured, and found balanced".
//!   Anything reading delta off these bars would be reading an artefact of the
//!   representation, and [`RateStats::mapped`] is what says how many bars are
//!   in that category.
//! - **No trade count.** `trade_count` is 0, meaning "not reported". It cannot
//!   be confused with "no trades happened": a candle with no activity is not
//!   emitted at all.
//! - **Bucket times, not trade times.** `open_time` is the interval's start and
//!   `close_time` its last millisecond — the span the candle *covers*. An
//!   engine bar built from trades stamps its first and last print instead, so
//!   its times sit strictly inside its bucket. The two series meet at a seam,
//!   and pretending otherwise would misplace every candle by up to an interval.
//!
//! Timestamps arrive in server wall time like every other MT5 timestamp and are
//! converted with the same bridge-declared offset the tick mapper uses.
//!
//! Pure and synchronous: no I/O, no clocks — same rows in, same bars out.

use rust_decimal::Decimal;
use std::str::FromStr as _;

use quantick_engine::Bar;

use crate::protocol::RateRow;

/// What a block of candles produced, and what it cost.
///
/// Kept per session so a chart that looks short can be explained from the log
/// rather than guessed at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateStats {
    /// Candles mapped into bars.
    pub mapped: u64,
    /// Rows dropped because they could not be read as a candle: a price that
    /// is not a number, a non-positive price, or a high below its own low.
    pub dropped_unreadable: u64,
    /// Rows the venue itself reported as empty (no volume).
    ///
    /// Dropped rather than emitted, following the engine's empty-interval rule:
    /// a bar with a carried-forward price would fabricate a price for a moment
    /// that had no trading, and the gap left behind is the honest record.
    pub dropped_empty: u64,
}

impl RateStats {
    /// Log one line summarising a completed block.
    pub fn log_summary(&self, symbol: &str, interval_ms: i64) {
        tracing::info!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_RATES_SUMMARY",
            symbol = %symbol,
            interval_ms,
            mapped = self.mapped,
            dropped_unreadable = self.dropped_unreadable,
            dropped_empty = self.dropped_empty,
            "historical candle block mapped"
        );
    }
}

/// Maps wire candles to engine bars for one session.
#[derive(Debug, Clone)]
pub struct RateMapper {
    interval_ms: i64,
    offset_ms: i64,
    /// Running tally of what this mapper did.
    pub stats: RateStats,
}

impl RateMapper {
    /// A mapper for candles covering `interval_ms`, stamped in a server clock
    /// `server_utc_offset_s` seconds away from UTC.
    #[must_use]
    pub fn new(interval_ms: i64, server_utc_offset_s: i64) -> Self {
        Self {
            interval_ms,
            offset_ms: server_utc_offset_s.saturating_mul(1000),
            stats: RateStats::default(),
        }
    }

    /// Map one wire row, or `None` when it is unreadable or empty (both
    /// counted in [`stats`](Self::stats)).
    #[must_use]
    pub fn map(&mut self, row: &RateRow) -> Option<Bar> {
        let RateRow(time_ms, open, high, low, close, volume) = row;
        let Some((open, high, low, close)) = read_prices(open, high, low, close) else {
            self.stats.dropped_unreadable = self.stats.dropped_unreadable.saturating_add(1);
            return None;
        };
        let Ok(volume) = Decimal::from_str(volume) else {
            self.stats.dropped_unreadable = self.stats.dropped_unreadable.saturating_add(1);
            return None;
        };
        if volume <= Decimal::ZERO {
            self.stats.dropped_empty = self.stats.dropped_empty.saturating_add(1);
            return None;
        }

        let open_time = time_ms.saturating_sub(self.offset_ms);
        // The bucket's last millisecond, so consecutive candles never share a
        // timestamp. `interval_ms - 1` is computed on the interval rather than
        // by subtracting from the sum, which would go backwards past i64::MAX.
        let close_time = open_time.saturating_add(self.interval_ms.saturating_sub(1));
        // Half each side. Exact for any volume a venue reports: a Decimal
        // carries 28 significant digits and halving costs at most one.
        let half = volume / Decimal::TWO;
        self.stats.mapped = self.stats.mapped.saturating_add(1);
        Some(Bar {
            open_time,
            close_time,
            open,
            high,
            low,
            close,
            buy_volume: half,
            sell_volume: half,
            // Not reported by the terminal. Never confusable with "no trades":
            // an empty candle is dropped above rather than mapped to zero.
            trade_count: 0,
        })
    }
}

/// Parse the four prices, rejecting anything that is not a coherent candle.
fn read_prices(
    open: &str,
    high: &str,
    low: &str,
    close: &str,
) -> Option<(Decimal, Decimal, Decimal, Decimal)> {
    let open = Decimal::from_str(open).ok()?;
    let high = Decimal::from_str(high).ok()?;
    let low = Decimal::from_str(low).ok()?;
    let close = Decimal::from_str(close).ok()?;
    // A traded instrument has no non-positive price, and a high below its own
    // low is not a candle at all. Both mean the row is corrupt; charting it
    // would put a spike on the screen that no market ever printed.
    if open <= Decimal::ZERO || high <= Decimal::ZERO || low <= Decimal::ZERO {
        return None;
    }
    if high < low || open > high || open < low || close > high || close < low {
        return None;
    }
    Some((open, high, low, close))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(time_ms: i64, o: &str, h: &str, l: &str, c: &str, v: &str) -> RateRow {
        RateRow(
            time_ms,
            o.to_string(),
            h.to_string(),
            l.to_string(),
            c.to_string(),
            v.to_string(),
        )
    }

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn a_candle_maps_to_its_bucket_in_utc() {
        // B3 server time is UTC−3, so a bucket stamped 16:31:00 local is
        // 19:31:00 UTC. The bar covers the whole minute, ending one
        // millisecond before the next bucket starts.
        let mut mapper = RateMapper::new(60_000, -10_800);
        let bar = mapper
            .map(&row(
                1_784_824_260_000,
                "177790",
                "177850",
                "177780",
                "177800",
                "1234",
            ))
            .expect("a well-formed candle maps");

        assert_eq!(bar.open_time, 1_784_824_260_000 + 10_800_000);
        assert_eq!(bar.close_time, bar.open_time + 59_999);
        assert_eq!(bar.open, dec("177790"));
        assert_eq!(bar.high, dec("177850"));
        assert_eq!(bar.low, dec("177780"));
        assert_eq!(bar.close, dec("177800"));
        assert_eq!(mapper.stats.mapped, 1);
    }

    #[test]
    fn volume_is_exact_and_delta_is_identically_zero() {
        // The representation decision, pinned: the terminal reports no
        // aggressor split, so delta must read as "not measured" rather than as
        // a balanced market — while the volume it *did* report stays exact.
        let mut mapper = RateMapper::new(60_000, 0);
        for reported in ["1234", "1", "0.00000001", "7"] {
            let bar = mapper
                .map(&row(0, "10", "11", "9", "10.5", reported))
                .expect("maps");
            assert_eq!(
                bar.volume(),
                dec(reported),
                "the venue's own volume survives: {reported}"
            );
            assert_eq!(bar.delta(), Decimal::ZERO, "delta cannot be measured here");
            assert_eq!(bar.trade_count, 0, "the terminal reports no count");
        }
    }

    #[test]
    fn an_empty_candle_is_dropped_not_carried_forward() {
        // The engine's empty-interval rule: a gap is the honest record that
        // nothing traded. A bar at the previous close would invent a price.
        let mut mapper = RateMapper::new(60_000, 0);
        assert!(mapper.map(&row(0, "10", "10", "10", "10", "0")).is_none());
        assert_eq!(mapper.stats.dropped_empty, 1);
        assert_eq!(mapper.stats.mapped, 0);
    }

    #[test]
    fn an_incoherent_row_is_dropped_and_counted() {
        let mut mapper = RateMapper::new(60_000, 0);
        // Not a number, and a price no instrument trades at.
        assert!(mapper.map(&row(0, "abc", "1", "1", "1", "1")).is_none());
        assert!(mapper.map(&row(0, "0", "1", "1", "1", "1")).is_none());
        assert!(mapper.map(&row(0, "-5", "1", "1", "1", "1")).is_none());
        // High below low, and a close outside the range it claims.
        assert!(mapper.map(&row(0, "10", "9", "11", "10", "1")).is_none());
        assert!(mapper.map(&row(0, "10", "11", "9", "12", "1")).is_none());
        // Unreadable volume.
        assert!(mapper.map(&row(0, "10", "11", "9", "10", "x")).is_none());
        assert_eq!(mapper.stats.dropped_unreadable, 6);
        assert_eq!(mapper.stats.mapped, 0);
    }

    #[test]
    fn an_absurd_offset_saturates_instead_of_panicking() {
        // The offset comes off the wire, so it is attacker-shaped input.
        let mut mapper = RateMapper::new(i64::MAX, i64::MIN);
        let bar = mapper
            .map(&row(i64::MAX, "10", "11", "9", "10", "1"))
            .expect("maps");
        assert_eq!(bar.open_time, i64::MAX);
        assert_eq!(bar.close_time, i64::MAX);
    }
}
