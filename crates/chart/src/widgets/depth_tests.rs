use quantick_appearance::Token;
use quantick_orderflow::widgets::depth::{DepthModel, DepthRow};
use rust_decimal::Decimal;

use super::depth::{DEPTH_COLUMNS, DEPTH_EMPTY, DepthView, depth_view};
use super::{WidgetBar, WidgetCell};

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn model_row(price: &str, sold: &str, bid: &str, ask: &str, bought: &str) -> DepthRow {
    DepthRow {
        price: d(price),
        sold: d(sold),
        bid: d(bid),
        ask: d(ask),
        bought: d(bought),
        last: false,
        wall: false,
        bid_unattributed: Decimal::ZERO,
        ask_unattributed: Decimal::ZERO,
    }
}

fn model() -> DepthModel {
    let mut rows = vec![
        model_row("76201.00", "0", "0", "4", "0"),
        model_row("76200.50", "0", "0", "1.25", "3"),
        model_row("76200.00", "2", "0.5", "0", "1"),
        model_row("76199.50", "0", "16", "0", "0"),
    ];
    rows[1].ask_unattributed = d("0.75");
    rows[2].last = true;
    rows[3].wall = true;
    DepthModel {
        rows,
        last_price: d("76200.00"),
        spread: Some(d("0.50")),
        near_touch: Some((d("16.5"), d("5.25"))),
    }
}

fn cell(text: &str, token: Token) -> WidgetCell {
    WidgetCell {
        text: text.to_owned(),
        token,
    }
}

fn blank() -> WidgetCell {
    cell("", Token::TextFaint)
}

fn bar(width_px: f32, token: Token) -> WidgetBar {
    WidgetBar { width_px, token }
}

#[test]
fn view_matches_golden() {
    let view = depth_view(Some(&model()), d("0.50"), 64.0, 80.0);
    assert_eq!(view.empty, None);
    assert_eq!(
        DEPTH_COLUMNS,
        ["SOLD", "BID", "PRICE", "ASK", "BOUGHT", "VOLUME"]
    );
    assert_eq!(view.tick, "tick 0.50");

    let texts: Vec<[&str; 5]> = view
        .rows
        .iter()
        .map(|row| {
            [
                row.sold.text.as_str(),
                row.bid.text.as_str(),
                row.price.text.as_str(),
                row.ask.text.as_str(),
                row.bought.text.as_str(),
            ]
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            ["", "", "76201.00", "4", ""],
            ["", "", "76200.50", "1.25", "3"],
            ["2", "0.5", "76200.00", "", "1"],
            ["", "16", "76199.50", "", ""],
        ]
    );

    // Colour roles: sides in their side colour, the last price in the
    // accent, the wall's size in primary ink, empty cells faint.
    let top = &view.rows[0];
    assert_eq!(top.sold, blank());
    assert_eq!(top.ask, cell("4", Token::Sell));
    assert_eq!(top.price, cell("76201.00", Token::TextPrimary));
    let last = &view.rows[2];
    assert!(last.last);
    assert_eq!(last.price, cell("76200.00", Token::Accent));
    assert_eq!(last.sold, cell("2", Token::Sell));
    assert_eq!(last.bid, cell("0.5", Token::Buy));
    assert_eq!(last.bought, cell("1", Token::Buy));
    let wall = &view.rows[3];
    assert!(wall.wall);
    assert_eq!(wall.bid, cell("16", Token::TextPrimary));

    // Resting bars against the largest resting size (16 -> 64 px), volume
    // bars against the largest traded row (3 -> 80 px).
    let resting: Vec<(WidgetBar, WidgetBar)> = view
        .rows
        .iter()
        .map(|row| (row.bid_bar, row.ask_bar))
        .collect();
    assert_eq!(
        resting,
        vec![
            (bar(0.0, Token::Buy), bar(16.0, Token::Sell)),
            (bar(0.0, Token::Buy), bar(5.0, Token::Sell)),
            (bar(2.0, Token::Buy), bar(0.0, Token::Sell)),
            (bar(64.0, Token::Buy), bar(0.0, Token::Sell)),
        ]
    );
    let volume: Vec<(f32, f32)> = view
        .rows
        .iter()
        .map(|row| (row.sold_bar.width_px, row.bought_bar.width_px))
        .collect();
    let golden = [
        (0.0, 0.0),
        (0.0, 80.0),
        (160.0 / 3.0, 80.0 / 3.0),
        (0.0, 0.0),
    ];
    for ((sold, bought), (gold_sold, gold_bought)) in volume.iter().zip(golden) {
        assert!((sold - gold_sold).abs() < 1e-4, "{volume:?}");
        assert!((bought - gold_bought).abs() < 1e-4, "{volume:?}");
    }
    assert_eq!(view.rows[2].sold_bar.token, Token::Sell);
    assert_eq!(view.rows[2].bought_bar.token, Token::Buy);

    assert_eq!(
        view.rows[1].reduction.as_deref(),
        Some("unattributed L2 reduction: ask 0.75")
    );
    assert!(
        view.rows
            .iter()
            .filter(|row| row.reduction.is_some())
            .count()
            == 1
    );

    assert_eq!(
        view.footer,
        vec![
            "spread 0.50".to_owned(),
            "bid 16.5 | ask 5.25 within 0.1%".to_owned()
        ]
    );
    assert_no_thousands_separator(&view);
}

#[test]
fn no_model_is_the_empty_state() {
    let view = depth_view(None, d("0.50"), 64.0, 80.0);
    assert_eq!(view, DepthView::empty_state(d("0.50")));
    assert!(view.rows.is_empty());
    assert_eq!(view.empty, Some(DEPTH_EMPTY));
}

#[test]
fn a_one_sided_book_says_so_in_the_footer() {
    let mut model = model();
    model.spread = None;
    model.near_touch = None;
    let view = depth_view(Some(&model), d("0.50"), 64.0, 80.0);
    assert_eq!(view.footer, vec!["spread —".to_owned()]);
}

fn assert_no_thousands_separator(view: &DepthView) {
    for row in &view.rows {
        assert!(!row.price.text.contains(','), "{}", row.price.text);
    }
}
