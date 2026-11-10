//! The Liquidations pane's model: long and short notional over the last
//! minute, five minutes and hour of a clock the caller passes in, one bucket
//! per clock minute for the histogram, and the newest prints first.
//!
//! A print stamped after the clock (a clock behind the venue) still counts as
//! recent and lands in the newest bucket, so the totals, the histogram and the
//! list never disagree about a print the list shows.

use rust_decimal::Decimal;

use super::ports::{LiquidatedSide, LiquidationPrint};

/// The spans the header totals cover, each ending at the clock.
pub const LIQUIDATION_WINDOWS_MS: [i64; 3] = [60_000, 300_000, 3_600_000];

/// One histogram bucket per clock minute.
pub const LIQUIDATION_BUCKET_MS: i64 = 60_000;

/// How much the model keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidationsParams {
    /// Histogram buckets, the newest holding the clock.
    pub buckets: usize,
    /// Most list rows, newest first. Totals and buckets ignore this cap.
    pub max_rows: usize,
}

/// Long and short notional over one window ending at the clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationTotals {
    pub window_ms: i64,
    pub long_notional: Decimal,
    pub short_notional: Decimal,
}

/// Long and short notional inside one clock minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationBucket {
    /// Start of the minute, epoch milliseconds.
    pub start_ms: i64,
    pub long_notional: Decimal,
    pub short_notional: Decimal,
}

/// The pane's model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Liquidations {
    /// One per [`LIQUIDATION_WINDOWS_MS`], in that order.
    pub totals: [LiquidationTotals; 3],
    /// Oldest first; the last is the minute holding the clock.
    pub buckets: Vec<LiquidationBucket>,
    /// Newest first, at most [`LiquidationsParams::max_rows`].
    pub rows: Vec<LiquidationPrint>,
}

/// Total, bucket and list `prints` (any order) against the clock `now_ms`.
#[must_use]
pub fn liquidations(
    prints: &[LiquidationPrint],
    params: &LiquidationsParams,
    now_ms: i64,
) -> Liquidations {
    let mut totals = LIQUIDATION_WINDOWS_MS.map(|window_ms| LiquidationTotals {
        window_ms,
        long_notional: Decimal::ZERO,
        short_notional: Decimal::ZERO,
    });
    let newest_start = now_ms.div_euclid(LIQUIDATION_BUCKET_MS) * LIQUIDATION_BUCKET_MS;
    let oldest_start =
        newest_start.saturating_sub(LIQUIDATION_BUCKET_MS * (params.buckets as i64 - 1).max(0));
    let mut buckets: Vec<LiquidationBucket> = (0..params.buckets)
        .map(|index| LiquidationBucket {
            start_ms: oldest_start + LIQUIDATION_BUCKET_MS * index as i64,
            long_notional: Decimal::ZERO,
            short_notional: Decimal::ZERO,
        })
        .collect();

    for print in prints {
        let notional = print.notional();
        for total in &mut totals {
            if print.timestamp_ms > now_ms.saturating_sub(total.window_ms) {
                add(
                    print.side,
                    notional,
                    &mut total.long_notional,
                    &mut total.short_notional,
                );
            }
        }
        if print.timestamp_ms >= oldest_start && !buckets.is_empty() {
            let index = ((print.timestamp_ms - oldest_start) / LIQUIDATION_BUCKET_MS) as usize;
            let bucket = &mut buckets[index.min(params.buckets - 1)];
            add(
                print.side,
                notional,
                &mut bucket.long_notional,
                &mut bucket.short_notional,
            );
        }
    }

    let mut rows = prints.to_vec();
    rows.sort_by(|a, b| {
        (b.timestamp_ms, b.side, b.price, b.quantity).cmp(&(
            a.timestamp_ms,
            a.side,
            a.price,
            a.quantity,
        ))
    });
    rows.truncate(params.max_rows);
    Liquidations {
        totals,
        buckets,
        rows,
    }
}

fn add(side: LiquidatedSide, notional: Decimal, long: &mut Decimal, short: &mut Decimal) {
    match side {
        LiquidatedSide::Long => *long += notional,
        LiquidatedSide::Short => *short += notional,
    }
}
