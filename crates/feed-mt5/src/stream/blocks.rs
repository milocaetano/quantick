//! The historical candle block collected by one bridge session.

use std::collections::BTreeMap;

use tracing::warn;

use quantick_engine::Bar;

use crate::protocol;
use crate::rates::RateMapper;

/// Most candles one block may deliver.
///
/// The bridge caps what it sends (`--rates-max-bars`) and logs the shortfall,
/// but the bridge is the side this one cannot vouch for: a misconfigured or
/// hostile one could stream candles until the feed runs out of memory. Ninety
/// days of one-minute buckets is ~130 000, so this is comfortably above any
/// legitimate block while still being a bound.
pub(super) const MAX_BARS_PER_BLOCK: usize = 1_000_000;

/// The historical candle block being received, between `rates_start` and
/// `rates_end`.
///
/// Bars land in a [`BTreeMap`] keyed by `open_time` rather than a `Vec`: the
/// terminal can repeat a bucket across chunk boundaries, and a map both settles
/// that (last write wins — a repeat is a correction) and hands back one
/// ascending series without a sort whose tie-breaking would be an unstated
/// rule. Same block in, same series out, whatever order the chunks arrived in.
pub(super) struct RatesBlock {
    interval_ms: i64,
    mapper: RateMapper,
    bars: BTreeMap<i64, Bar>,
    /// Whether the cap has been hit, so it is reported once rather than per row.
    truncated: bool,
}

impl RatesBlock {
    pub(super) fn new(interval_ms: i64, server_utc_offset_s: i64) -> Self {
        Self {
            interval_ms,
            mapper: RateMapper::new(interval_ms, server_utc_offset_s),
            bars: BTreeMap::new(),
            truncated: false,
        }
    }

    /// Map and absorb one chunk. Unreadable rows are counted, not fatal: one
    /// corrupt candle in ninety days is a gap, not a reason to lose the block.
    pub(super) fn absorb(&mut self, chunk: &protocol::RateChunk) {
        for row in &chunk.bars {
            if self.bars.len() >= MAX_BARS_PER_BLOCK {
                if !self.truncated {
                    self.truncated = true;
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_RATES_TRUNCATED",
                        max_bars = MAX_BARS_PER_BLOCK as u64,
                        action = "keep_oldest_stop_absorbing",
                        "the candle block exceeded the cap; ignoring the rest of it"
                    );
                }
                return;
            }
            if let Some(bar) = self.mapper.map(row) {
                self.bars.insert(bar.open_time, bar);
            }
        }
    }

    pub(super) fn len(&self) -> usize {
        self.bars.len()
    }

    /// Close the block: log what it cost, and hand back the ascending series.
    pub(super) fn finish(self, symbol: &str) -> (i64, Vec<Bar>, bool) {
        self.mapper.stats.log_summary(symbol, self.interval_ms);
        // Clipping here is the same kind of shortfall the bridge reports with
        // its own `partial`: bars that exist and were not delivered.
        let clipped = self.truncated;
        (self.interval_ms, self.bars.into_values().collect(), clipped)
    }
}
