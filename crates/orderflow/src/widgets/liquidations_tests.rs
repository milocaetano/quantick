use rust_decimal::Decimal;

use super::liquidations::{
    LIQUIDATION_WINDOWS_MS, LiquidationBucket, LiquidationTotals, LiquidationsParams, liquidations,
};
use super::ports::{LiquidatedSide, LiquidationPrint};

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn print(timestamp_ms: i64, side: LiquidatedSide, price: &str, quantity: &str) -> LiquidationPrint {
    LiquidationPrint {
        timestamp_ms,
        side,
        price: d(price),
        quantity: d(quantity),
    }
}

/// Thirty seconds into the minute that starts at 10 800 000.
const NOW_MS: i64 = 10_830_000;

fn prints() -> Vec<LiquidationPrint> {
    use LiquidatedSide::{Long, Short};
    // Out of order on purpose: the model sorts.
    vec![
        // Before the hour.
        print(7_000_000, Long, "10", "100"),
        // In the hour only, older than the minute buckets shown.
        print(10_000_000, Long, "10", "5"),
        // Exactly one minute before the clock: in 5m, not in 1m.
        print(10_770_000, Long, "100", "3"),
        print(10_829_000, Long, "100", "2"),
        print(10_700_000, Short, "50", "4"),
        print(10_800_000, Short, "100", "1"),
        // Stamped after the clock (a clock behind the venue): still recent.
        print(10_831_000, Short, "10", "1"),
    ]
}

fn totals(window_ms: i64, long: &str, short: &str) -> LiquidationTotals {
    LiquidationTotals {
        window_ms,
        long_notional: d(long),
        short_notional: d(short),
    }
}

fn bucket(start_ms: i64, long: &str, short: &str) -> LiquidationBucket {
    LiquidationBucket {
        start_ms,
        long_notional: d(long),
        short_notional: d(short),
    }
}

#[test]
fn totals_and_minute_buckets() {
    let params = LiquidationsParams {
        buckets: 3,
        max_rows: 4,
    };
    let model = liquidations(&prints(), &params, NOW_MS);

    assert_eq!(LIQUIDATION_WINDOWS_MS, [60_000, 300_000, 3_600_000]);
    assert_eq!(
        model.totals,
        [
            totals(60_000, "200", "110"),
            totals(300_000, "500", "310"),
            totals(3_600_000, "550", "310"),
        ]
    );
    assert_eq!(
        model.buckets,
        vec![
            bucket(10_680_000, "0", "200"),
            bucket(10_740_000, "300", "0"),
            bucket(10_800_000, "200", "110"),
        ]
    );
    let rows: Vec<i64> = model.rows.iter().map(|row| row.timestamp_ms).collect();
    assert_eq!(rows, vec![10_831_000, 10_829_000, 10_800_000, 10_770_000]);
}

#[test]
fn input_order_does_not_change_the_model() {
    let params = LiquidationsParams {
        buckets: 30,
        max_rows: 50,
    };
    let forward = prints();
    let mut backward = prints();
    backward.reverse();
    assert_eq!(
        liquidations(&forward, &params, NOW_MS),
        liquidations(&backward, &params, NOW_MS)
    );
}

#[test]
fn no_prints_reads_as_zero_totals_and_empty_buckets() {
    let params = LiquidationsParams {
        buckets: 2,
        max_rows: 10,
    };
    let model = liquidations(&[], &params, NOW_MS);
    assert!(model.rows.is_empty());
    assert!(
        model
            .totals
            .iter()
            .all(|t| t.long_notional.is_zero() && t.short_notional.is_zero())
    );
    assert_eq!(
        model.buckets,
        vec![
            bucket(10_740_000, "0", "0"),
            bucket(10_800_000, "0", "0")
        ]
    );
}
