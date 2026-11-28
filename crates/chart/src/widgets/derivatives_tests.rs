use quantick_appearance::Token;
use quantick_civil::TzOffset;
use quantick_orderflow::widgets::liquidations::{
    LiquidationBucket, LiquidationTotals, Liquidations,
};
use quantick_orderflow::widgets::ports::{LiquidatedSide, LiquidationPrint};
use rust_decimal::Decimal;

use super::WidgetCell;
use super::big_trades::NotionalUnit;
use super::liquidations::{
    LIQUIDATIONS_NOT_PROVIDED, LiquidationBarView, LiquidationRowView, LiquidationTotalsView,
    LiquidationsLayout, liquidations_view,
};

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn cell(text: &str, token: Token) -> WidgetCell {
    WidgetCell {
        text: text.to_owned(),
        token,
    }
}

/// 2026-11-10 00:13:52 UTC.
const T0: i64 = 1_794_269_632_000;

pub(super) fn liquidations_model() -> Liquidations {
    let totals = |window_ms, long: &str, short: &str| LiquidationTotals {
        window_ms,
        long_notional: d(long),
        short_notional: d(short),
    };
    let bucket = |start_ms, long: &str, short: &str| LiquidationBucket {
        start_ms,
        long_notional: d(long),
        short_notional: d(short),
    };
    Liquidations {
        totals: [
            totals(60_000, "420000", "30000"),
            totals(300_000, "2910000", "180000"),
            totals(3_600_000, "11600000000", "3400000"),
        ],
        buckets: vec![bucket(0, "100", "0"), bucket(60_000, "50", "200")],
        rows: vec![
            LiquidationPrint {
                timestamp_ms: T0,
                side: LiquidatedSide::Long,
                price: d("75979.8"),
                quantity: d("12.4"),
            },
            LiquidationPrint {
                timestamp_ms: T0 - 5_000,
                side: LiquidatedSide::Short,
                price: d("75992.1"),
                quantity: d("0.48"),
            },
        ],
    }
}

pub(super) fn liquidations_layout() -> LiquidationsLayout {
    LiquidationsLayout {
        tick: d("0.1"),
        tz: TzOffset::new(0),
        unit: NotionalUnit::USD,
        histogram_width_px: 100.0,
        histogram_height_px: 40.0,
    }
}

#[test]
fn views_match_golden_and_empty_state() {
    // Liquidations.
    let view = liquidations_view(Some(&liquidations_model()), &liquidations_layout());
    assert_eq!(view.columns, ["TIME", "LIQUIDATED", "PRICE", "SIZE", "USD"]);
    let totals = |label: &str, long: &str, short: &str| LiquidationTotalsView {
        label: label.to_owned(),
        long: cell(long, Token::Sell),
        short: cell(short, Token::Buy),
    };
    assert_eq!(
        view.totals,
        vec![
            totals("1m", "L $420.0k", "S $30.0k"),
            totals("5m", "L $2.9M", "S $180.0k"),
            // From a billion the figure switches to the axis spelling.
            totals("1h", "L $11.60B", "S $3.4M"),
        ]
    );
    // Longs hang below the baseline, shorts stand on it; the largest side
    // of any minute fills half the height.
    assert_eq!(view.baseline_y_px, 20.0);
    assert_eq!(
        view.bars,
        vec![
            LiquidationBarView {
                x_px: 1.0,
                width_px: 48.0,
                long_px: 10.0,
                short_px: 0.0,
            },
            LiquidationBarView {
                x_px: 51.0,
                width_px: 48.0,
                long_px: 5.0,
                short_px: 20.0,
            },
        ]
    );
    assert_eq!(
        view.rows,
        vec![
            LiquidationRowView {
                time: cell("00:13:52", Token::TextFaint),
                side: cell("LONG", Token::Sell),
                price: cell("75979.8", Token::TextPrimary),
                size: cell("12.4", Token::Sell),
                notional: cell("$942.1k", Token::TextPrimary),
                emphasised: true,
            },
            LiquidationRowView {
                time: cell("00:13:47", Token::TextFaint),
                side: cell("SHORT", Token::Buy),
                price: cell("75992.1", Token::TextMuted),
                size: cell("0.48", Token::Buy),
                notional: cell("$36.5k", Token::TextMuted),
                emphasised: false,
            },
        ]
    );
    assert_eq!(view.empty, None);
    let empty = liquidations_view(None, &liquidations_layout());
    assert_eq!(empty.empty.as_deref(), Some(LIQUIDATIONS_NOT_PROVIDED));
    assert!(empty.totals.is_empty() && empty.bars.is_empty() && empty.rows.is_empty());
}
