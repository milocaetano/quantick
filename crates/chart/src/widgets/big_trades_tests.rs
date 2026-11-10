use quantick_appearance::Token;
use quantick_civil::TzOffset;
use quantick_engine::Side;
use quantick_orderflow::widgets::big_trades::{BigTrade, BigTrades, MIN_NOTIONAL_PRESETS};
use rust_decimal::Decimal;

use super::WidgetCell;
use super::big_trades::{
    BIG_TRADES_EMPTY, BigTradesLayout, BigTradesView, NotionalUnit, big_trades_view,
};

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn trade(
    timestamp_ms: i64,
    side: Side,
    price: &str,
    quantity: &str,
    notional: &str,
    fill_count: usize,
) -> BigTrade {
    BigTrade {
        agg_id: timestamp_ms as u64,
        timestamp_ms,
        side,
        price: d(price),
        quantity: d(quantity),
        notional: d(notional),
        fill_count,
        merged: fill_count > 1,
    }
}

/// 2026-11-10 00:13:52 UTC.
const T0: i64 = 1_794_269_632_000;

pub(super) fn model() -> BigTrades {
    BigTrades {
        rows: vec![
            trade(T0, Side::Buy, "76200.00", "13.1234", "1000000", 1),
            trade(T0 - 1_000, Side::Sell, "76183.3333", "4.72", "360000", 3),
            trade(T0 - 3_000, Side::Buy, "76185.50", "3.28", "250000", 1),
        ],
        window_buy_notional: d("8400000"),
        window_sell_notional: d("5100000"),
    }
}

pub(super) fn layout(
    min_notional: &str,
    merge_fills: bool,
    unit: NotionalUnit,
) -> BigTradesLayout<'static> {
    BigTradesLayout {
        presets: &MIN_NOTIONAL_PRESETS,
        min_notional: d(min_notional),
        merge_fills,
        tick: d("0.50"),
        tz: TzOffset::new(0),
        unit,
    }
}

fn cell(text: &str, token: Token) -> WidgetCell {
    WidgetCell {
        text: text.to_owned(),
        token,
    }
}

#[test]
fn view_matches_golden() {
    let view = big_trades_view(Some(&model()), &layout("250000", true, NotionalUnit::USD));
    assert_eq!(view.empty, None);
    assert_eq!(view.columns, ["TIME", "", "PRICE", "SIZE", "USD", ""]);
    assert_eq!(
        view.presets,
        vec![
            ("50k".to_owned(), false),
            ("250k".to_owned(), true),
            ("1M".to_owned(), false),
        ]
    );
    assert!(view.merge_fills);

    let first = &view.rows[0];
    assert_eq!(first.time, cell("00:13:52", Token::TextFaint));
    assert_eq!(first.side, cell("BUY", Token::Buy));
    assert_eq!(first.price, cell("76200.00", Token::TextPrimary));
    assert_eq!(first.size, cell("13.12", Token::Buy));
    assert_eq!(first.fills, None);
    assert_eq!(first.notional, cell("$1.0M", Token::TextPrimary));
    assert!(first.emphasised);
    assert_eq!(first.bubble_token, Token::Buy);

    let merged = &view.rows[1];
    assert_eq!(merged.time, cell("00:13:51", Token::TextFaint));
    assert_eq!(merged.side, cell("SELL", Token::Sell));
    // A merged average price is written at the tick's precision.
    assert_eq!(merged.price, cell("76183.33", Token::TextMuted));
    assert_eq!(merged.size, cell("4.72", Token::Sell));
    assert_eq!(merged.fills, Some(cell("×3", Token::TextFaint)));
    assert_eq!(merged.notional, cell("$360.0k", Token::TextMuted));
    assert!(!merged.emphasised);

    let third = &view.rows[2];
    assert_eq!(third.time.text, "00:13:49");
    assert_eq!(third.price.text, "76185.50");

    // Bubble area follows notional: 3 px at nothing, 9 px at the largest.
    let radii: Vec<f32> = view.rows.iter().map(|row| row.bubble_radius_px).collect();
    let golden = [9.0, 6.6, 6.0];
    for (radius, expected) in radii.iter().zip(golden) {
        assert!((radius - expected).abs() < 1e-4, "{radii:?}");
    }

    assert_eq!(view.footer, "last 5 min: buys $8.4M · sells $5.1M");
    for row in &view.rows {
        assert!(!row.price.text.contains(','), "{}", row.price.text);
    }
}

#[test]
fn nothing_over_the_threshold_says_so() {
    let empty = BigTrades {
        rows: Vec::new(),
        window_buy_notional: Decimal::ZERO,
        window_sell_notional: Decimal::ZERO,
    };
    let view = big_trades_view(Some(&empty), &layout("1000000", false, NotionalUnit::USD));
    assert!(view.rows.is_empty());
    assert_eq!(
        view.empty,
        Some("no trades at or above $1.0M yet".to_owned())
    );
}

#[test]
fn no_tape_is_the_empty_state() {
    let view = big_trades_view(None, &layout("250000", true, NotionalUnit::USD));
    assert_eq!(
        view,
        BigTradesView::empty_state(d("250000"), true, NotionalUnit::USD)
    );
    assert_eq!(view.empty, Some(BIG_TRADES_EMPTY.to_owned()));
}

#[test]
fn a_points_quoted_notional_is_not_called_dollars() {
    let view = big_trades_view(Some(&model()), &layout("250000", true, NotionalUnit::QUOTE));
    assert_eq!(view.columns[4], "NOTIONAL");
    assert_eq!(view.rows[0].notional.text, "1.0M");
    assert!(!view.footer.contains('$'), "{}", view.footer);
}
