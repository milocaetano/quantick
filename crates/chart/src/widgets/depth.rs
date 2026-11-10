//! The Depth pane's view-model: the DOM ladder as text cells, colour roles
//! and bar widths, plus the footer's spread and near-touch sums.

use quantick_appearance::Token;
use quantick_orderflow::widgets::depth::{
    DepthModel, DepthRow, NEAR_TOUCH_FRACTION, UNATTRIBUTED_L2_REDUCTION,
};
use rust_decimal::Decimal;

use super::scene::{Align, BAND_OPACITY, ROW_PX, WidgetScene, row_y, rows_between};
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

/// Each resting bar's longest width, as a share of the pane's width.
pub const DEPTH_RESTING_BAR_SHARE: f32 = 0.12;

/// The VOLUME column's longest bar, as a share of the pane's width.
pub const DEPTH_VOLUME_BAR_SHARE: f32 = 0.16;

/// Column anchors, as shares of the width: SOLD, BID, PRICE, ASK, BOUGHT,
/// and where the VOLUME bars start.
const DEPTH_COLUMN_SHARES: [f32; 6] = [0.13, 0.30, 0.46, 0.60, 0.78, 0.82];

/// Ladder rows on each side of the last price that fit a `height` body
/// beside its column heads and two footer rows.
#[must_use]
pub fn depth_rows_per_side(height: f32) -> usize {
    rows_between(0.0, height).saturating_sub(4) / 2
}

impl DepthView {
    /// Lay the ladder out in a `width` by `height` body: heads, rows top
    /// down, the last price's row banded, and the footer at the bottom.
    #[must_use]
    pub fn paint(&self, width: f32, height: f32) -> WidgetScene {
        let chips = vec![(self.tick.clone(), true)];
        if let Some(empty) = self.empty {
            return WidgetScene::empty(empty, chips, width, height);
        }
        let x = DEPTH_COLUMN_SHARES.map(|share| share * width);
        let mut scene = WidgetScene {
            chips,
            ..WidgetScene::default()
        };
        let aligns = [Align::Right, Align::Right, Align::Centre, Align::Left];
        let mut heads: Vec<(&str, f32, Align)> = (0..4)
            .map(|index| (DEPTH_COLUMNS[index], x[index], aligns[index]))
            .collect();
        heads.extend([
            (DEPTH_COLUMNS[4], x[4], Align::Right),
            (DEPTH_COLUMNS[5], x[5], Align::Left),
        ]);
        scene.heads(ROW_PX / 2.0, &heads);
        let fit = rows_between(ROW_PX, height - 2.0 * ROW_PX);
        let skip = self.rows.len().saturating_sub(fit) / 2;
        for (index, row) in self.rows.iter().skip(skip).take(fit).enumerate() {
            let y = row_y(ROW_PX, index);
            let band = [0.0, y - ROW_PX / 2.0, width, ROW_PX];
            if row.last {
                scene.rect(band, Token::Accent, BAND_OPACITY);
            }
            if row.reduction.is_some() {
                scene.rect([0.0, band[1], 3.0, ROW_PX], Token::Warn, 1.0);
            }
            let bar_h = ROW_PX - 4.0;
            let top = y - bar_h / 2.0;
            let bid_w = row.bid_bar.width_px;
            scene.rect([x[1] - bid_w, top, bid_w, bar_h], row.bid_bar.token, 0.35);
            scene.rect(
                [x[3], top, row.ask_bar.width_px, bar_h],
                row.ask_bar.token,
                0.35,
            );
            let sold_w = row.sold_bar.width_px;
            scene.rect([x[5], top, sold_w, bar_h], row.sold_bar.token, 0.8);
            let bought = [x[5] + sold_w, top, row.bought_bar.width_px, bar_h];
            scene.rect(bought, row.bought_bar.token, 0.8);
            scene.cell([x[0], y], &row.sold, Align::Right);
            scene.cell([x[1], y], &row.bid, Align::Right);
            scene.cell([x[2], y], &row.price, Align::Centre);
            scene.cell([x[3] + 2.0, y], &row.ask, Align::Left);
            scene.cell([x[4], y], &row.bought, Align::Right);
            scene.rows += 1;
        }
        let reduction = self.rows.iter().find_map(|row| {
            let label = row.reduction.as_ref()?;
            Some(format!("{label} at {}", row.price.text))
        });
        if let Some(reduction) = reduction {
            scene.footer(height - ROW_PX, &reduction);
        }
        scene.footer(height, &self.footer.join("   "));
        scene
    }
}
