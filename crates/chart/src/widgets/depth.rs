//! The Depth pane's view-model: the DOM ladder as text cells, colour roles
//! and bar widths, plus the footer's spread and near-touch sums.

use quantick_appearance::Token;
use quantick_orderflow::widgets::depth::{
    DepthModel, DepthRow, NEAR_TOUCH_FRACTION, UNATTRIBUTED_L2_REDUCTION,
};
use rust_decimal::Decimal;

use super::{WidgetBar, WidgetCell, bar_px, price_text};
use crate::footprint_projection::fmt_qty;

/// The ladder's column heads, left to right.
pub const DEPTH_COLUMNS: [&str; 6] = ["SOLD", "BID", "PRICE", "ASK", "BOUGHT", "VOLUME"];

/// The footer's note when the published book misses the touch on a side.
pub const DEPTH_CLIPPED: &str = "book cut to the chart's price window";

/// What the pane says before there is a book or a print to centre on.
pub const DEPTH_EMPTY: &str = "waiting for the order book";

/// One ladder row as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthRowView {
    pub sold: WidgetCell,
    pub bid: WidgetCell,
    pub price: WidgetCell,
    pub ask: WidgetCell,
    pub bought: WidgetCell,
    /// Resting bid against the largest resting size on display.
    pub bid_bar: WidgetBar,
    /// Resting ask, same scale as `bid_bar`.
    pub ask_bar: WidgetBar,
    /// The VOLUME column's sell part, against the busiest traded row.
    pub sold_bar: WidgetBar,
    /// The VOLUME column's buy part, drawn after `sold_bar`.
    pub bought_bar: WidgetBar,
    /// The row holding the last price.
    pub last: bool,
    /// A resting wall sits in this row.
    pub wall: bool,
    /// `unattributed L2 reduction: bid 2` when size left with no trade.
    pub reduction: Option<String>,
}

/// The whole pane body.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthView {
    /// The header chip: `tick 0.50`.
    pub tick: String,
    /// Top of the ladder first.
    pub rows: Vec<DepthRowView>,
    /// Footer segments, left to right.
    pub footer: Vec<String>,
    /// Set, with no rows, when there is nothing to draw yet.
    pub empty: Option<&'static str>,
}

impl DepthView {
    /// The pane before its first book.
    #[must_use]
    pub fn empty_state(tick: Decimal) -> Self {
        Self {
            tick: tick_label(tick),
            rows: Vec::new(),
            footer: Vec::new(),
            empty: Some(DEPTH_EMPTY),
        }
    }
}

/// Lay `model` out with resting bars up to `resting_bar_px` and volume bars
/// up to `volume_bar_px`; the empty state when there is no model.
#[must_use]
pub fn depth_view(
    model: Option<&DepthModel>,
    tick: Decimal,
    resting_bar_px: f32,
    volume_bar_px: f32,
) -> DepthView {
    let Some(model) = model else {
        return DepthView::empty_state(tick);
    };
    let max_resting = model
        .rows
        .iter()
        .map(|row| row.bid.max(row.ask))
        .max()
        .unwrap_or(Decimal::ZERO);
    let max_traded = model
        .rows
        .iter()
        .map(|row| row.sold + row.bought)
        .max()
        .unwrap_or(Decimal::ZERO);
    let rows = model
        .rows
        .iter()
        .map(|row| DepthRowView {
            sold: size_cell(row.sold, Token::Sell),
            bid: resting_cell(row, row.bid, Token::Buy),
            price: WidgetCell::new(
                price_text(row.price, tick),
                if row.last {
                    Token::Accent
                } else {
                    Token::TextPrimary
                },
            ),
            ask: resting_cell(row, row.ask, Token::Sell),
            bought: size_cell(row.bought, Token::Buy),
            bid_bar: WidgetBar {
                width_px: bar_px(row.bid, max_resting, resting_bar_px),
                token: Token::Buy,
            },
            ask_bar: WidgetBar {
                width_px: bar_px(row.ask, max_resting, resting_bar_px),
                token: Token::Sell,
            },
            sold_bar: WidgetBar {
                width_px: bar_px(row.sold, max_traded, volume_bar_px),
                token: Token::Sell,
            },
            bought_bar: WidgetBar {
                width_px: bar_px(row.bought, max_traded, volume_bar_px),
                token: Token::Buy,
            },
            last: row.last,
            wall: row.wall,
            reduction: reduction_text(row),
        })
        .collect();
    let mut footer = vec![match model.spread {
        Some(spread) => format!("spread {}", price_text(spread, tick)),
        None => "spread —".to_owned(),
    }];
    if let Some((bid, ask)) = model.near_touch {
        footer.push(format!(
            "bid {} | ask {} within {}%",
            fmt_qty(bid),
            fmt_qty(ask),
            (NEAR_TOUCH_FRACTION * Decimal::ONE_HUNDRED).normalize()
        ));
    }
    if model.clipped {
        footer.push(DEPTH_CLIPPED.to_owned());
    }
    DepthView {
        tick: tick_label(tick),
        rows,
        footer,
        empty: None,
    }
}

fn tick_label(tick: Decimal) -> String {
    format!("tick {tick}")
}

/// A quantity in its side's colour; blank and faint when zero.
fn size_cell(size: Decimal, token: Token) -> WidgetCell {
    if size <= Decimal::ZERO {
        return WidgetCell::new("", Token::TextFaint);
    }
    WidgetCell::new(fmt_qty(size), token)
}

/// A resting size: the wall's in primary ink so it reads first.
fn resting_cell(row: &DepthRow, size: Decimal, side: Token) -> WidgetCell {
    let mut cell = size_cell(size, side);
    if row.wall && size > Decimal::ZERO && size == row.bid.max(row.ask) {
        cell.token = Token::TextPrimary;
    }
    cell
}

fn reduction_text(row: &DepthRow) -> Option<String> {
    let label = row.reduction_label()?;
    debug_assert_eq!(label, UNATTRIBUTED_L2_REDUCTION);
    let sides: Vec<String> = [("bid", row.bid_unattributed), ("ask", row.ask_unattributed)]
        .into_iter()
        .filter(|(_, size)| *size > Decimal::ZERO)
        .map(|(side, size)| format!("{side} {}", fmt_qty(size)))
        .collect();
    Some(format!("{label}: {}", sides.join(", ")))
}
