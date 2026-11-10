//! The Big Trades pane's view-model: one text row per big print or merge,
//! a bubble sized by notional, the threshold chips and the five-minute
//! footer.

use quantick_appearance::Token;
use quantick_civil::TzOffset;
use quantick_engine::Side;
use quantick_orderflow::widgets::big_trades::{
    BIG_TRADES_WINDOW_MS, BigTrade, BigTrades, MIN_NOTIONAL_PRESETS,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::scene::{Align, BAND_OPACITY, ROW_PX, ScenePrim, WidgetScene, row_y, rows_between};
use super::{WidgetCell, amount, clock_text, price_text};
use crate::footprint_projection::fmt_qty;

/// What the pane says before any tape arrives.
pub const BIG_TRADES_EMPTY: &str = "waiting for trades";

/// A row at or above the largest preset is drawn emphasised; this is the
/// largest of the default presets, for a layout that offers none.
pub const EMPHASIS_NOTIONAL: u32 = MIN_NOTIONAL_PRESETS[MIN_NOTIONAL_PRESETS.len() - 1];

/// Bubble radius of a row with no notional.
pub const BUBBLE_MIN_RADIUS_PX: f32 = 3.0;

/// Bubble radius of the largest row on display.
pub const BUBBLE_MAX_RADIUS_PX: f32 = 9.0;

/// How a notional is headed and written. Price times quantity is a quote
/// amount only where the instrument says so: a dollar-quoted pair is
/// [`USD`](Self::USD); a points-quoted future is not money and says
/// [`QUOTE`](Self::QUOTE). The caller picks from its feed's config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotionalUnit {
    /// The notional column's head.
    pub header: &'static str,
    /// Written before every amount.
    pub prefix: &'static str,
}

impl NotionalUnit {
    /// A pair quoted in dollars or a dollar stablecoin.
    pub const USD: Self = Self {
        header: "USD",
        prefix: "$",
    };
    /// Price times quantity in whatever the price is quoted in.
    pub const QUOTE: Self = Self {
        header: "NOTIONAL",
        prefix: "",
    };
}

/// One list row as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct BigTradeRowView {
    pub time: WidgetCell,
    pub side: WidgetCell,
    pub price: WidgetCell,
    pub size: WidgetCell,
    /// `×3` beside the size of a merged row.
    pub fills: Option<WidgetCell>,
    pub notional: WidgetCell,
    /// Area grows with notional, relative to the largest row on display.
    pub bubble_radius_px: f32,
    pub bubble_token: Token,
    /// At or above [`EMPHASIS_NOTIONAL`].
    pub emphasised: bool,
}

/// The whole pane body.
#[derive(Debug, Clone, PartialEq)]
pub struct BigTradesView {
    /// Column heads; the blanks sit over the side and the bubble.
    pub columns: [&'static str; 6],
    /// Threshold chips and whether each is the one in force.
    pub presets: Vec<(String, bool)>,
    /// The `merge fills` chip.
    pub merge_fills: bool,
    /// Newest first.
    pub rows: Vec<BigTradeRowView>,
    /// `last 5 min: buys $8.4M · sells $5.1M`; empty with no tape.
    pub footer: String,
    /// Set, with no rows, when there is nothing to list.
    pub empty: Option<String>,
}

impl BigTradesView {
    /// The pane before its first print.
    #[must_use]
    pub fn empty_state(min_notional: Decimal, merge_fills: bool, unit: NotionalUnit) -> Self {
        Self {
            columns: columns(unit),
            presets: presets(&MIN_NOTIONAL_PRESETS, min_notional),
            merge_fills,
            rows: Vec::new(),
            footer: String::new(),
            empty: Some(BIG_TRADES_EMPTY.to_owned()),
        }
    }
}

/// How `model` is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTradesLayout<'a> {
    /// The threshold chips offered, in the unit's amounts; the feed's
    /// config names them, so a points-quoted future offers its own.
    pub presets: &'a [u32],
    /// The threshold in force, for the chips and the empty text.
    pub min_notional: Decimal,
    /// Whether the merge chip is on.
    pub merge_fills: bool,
    /// Prices are written at this tick's precision.
    pub tick: Decimal,
    /// Times are written in this zone.
    pub tz: TzOffset,
    pub unit: NotionalUnit,
}

/// Lay `model` out; the empty state when there is no tape.
#[must_use]
pub fn big_trades_view(model: Option<&BigTrades>, layout: &BigTradesLayout<'_>) -> BigTradesView {
    let Some(model) = model else {
        return BigTradesView {
            presets: presets(layout.presets, layout.min_notional),
            ..BigTradesView::empty_state(layout.min_notional, layout.merge_fills, layout.unit)
        };
    };
    let largest = model
        .rows
        .iter()
        .map(|row| row.notional)
        .max()
        .unwrap_or(Decimal::ZERO);
    let rows = model
        .rows
        .iter()
        .map(|row| row_view(row, largest, layout))
        .collect();
    let empty = model.rows.is_empty().then(|| {
        format!(
            "no trades at or above {} yet",
            amount(layout.min_notional, layout.unit)
        )
    });
    BigTradesView {
        columns: columns(layout.unit),
        presets: presets(layout.presets, layout.min_notional),
        merge_fills: layout.merge_fills,
        rows,
        footer: format!(
            "last {} min: buys {} · sells {}",
            BIG_TRADES_WINDOW_MS / 60_000,
            amount(model.window_buy_notional, layout.unit),
            amount(model.window_sell_notional, layout.unit)
        ),
        empty,
    }
}

fn columns(unit: NotionalUnit) -> [&'static str; 6] {
    ["TIME", "", "PRICE", "SIZE", unit.header, ""]
}

fn row_view(row: &BigTrade, largest: Decimal, layout: &BigTradesLayout<'_>) -> BigTradeRowView {
    let side_token = match row.side {
        Side::Buy => Token::Buy,
        Side::Sell => Token::Sell,
    };
    let top = layout
        .presets
        .iter()
        .max()
        .copied()
        .unwrap_or(EMPHASIS_NOTIONAL);
    let emphasised = row.notional >= Decimal::from(top);
    let ink = if emphasised {
        Token::TextPrimary
    } else {
        Token::TextMuted
    };
    let share = if largest > Decimal::ZERO {
        (row.notional / largest)
            .to_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    BigTradeRowView {
        time: WidgetCell::new(clock_text(row.timestamp_ms, layout.tz), Token::TextFaint),
        side: WidgetCell::new(
            match row.side {
                Side::Buy => "BUY",
                Side::Sell => "SELL",
            },
            side_token,
        ),
        price: WidgetCell::new(price_text(row.price, layout.tick), ink),
        size: WidgetCell::new(fmt_qty(row.quantity), side_token),
        fills: row
            .merged
            .then(|| WidgetCell::new(format!("×{}", row.fill_count), Token::TextFaint)),
        notional: WidgetCell::new(amount(row.notional, layout.unit), ink),
        bubble_radius_px: BUBBLE_MIN_RADIUS_PX
            + (BUBBLE_MAX_RADIUS_PX - BUBBLE_MIN_RADIUS_PX) * share.sqrt() as f32,
        bubble_token: side_token,
        emphasised,
    }
}

fn presets(values: &[u32], min_notional: Decimal) -> Vec<(String, bool)> {
    values
        .iter()
        .map(|value| (preset_label(*value), Decimal::from(*value) == min_notional))
        .collect()
}

/// `50k`, `250k`, `1M`: a whole preset in its largest whole unit.
fn preset_label(value: u32) -> String {
    match value {
        v if v >= 1_000_000 && v % 1_000_000 == 0 => format!("{}M", v / 1_000_000),
        v if v >= 1_000 && v % 1_000 == 0 => format!("{}k", v / 1_000),
        v => v.to_string(),
    }
}

/// Column anchors, as shares of the width: TIME, side, PRICE, SIZE,
/// notional, and the bubble's centre.
const BIG_TRADES_COLUMN_SHARES: [f32; 6] = [0.02, 0.19, 0.47, 0.66, 0.88, 0.95];

impl BigTradesView {
    /// Lay the list out in a `width` by `height` body: heads, rows newest
    /// first, the emphasised ones banded, and the window totals at the
    /// bottom. The chips carry the presets and the merge switch.
    #[must_use]
    pub fn paint(&self, width: f32, height: f32) -> WidgetScene {
        let mut chips = self.presets.clone();
        chips.push(("merge fills".to_owned(), self.merge_fills));
        let x = BIG_TRADES_COLUMN_SHARES.map(|share| share * width);
        let mut scene = WidgetScene {
            chips,
            ..WidgetScene::default()
        };
        let aligns = [
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
        ];
        let heads: Vec<(&str, f32, Align)> = (0..5)
            .map(|index| (self.columns[index], x[index], aligns[index]))
            .collect();
        scene.heads(ROW_PX / 2.0, &heads);
        if let Some(empty) = &self.empty {
            scene.text(
                [width / 2.0, height / 2.0],
                empty,
                Token::TextMuted,
                Align::Centre,
            );
            scene.empty = Some(empty.clone());
        }
        let fit = rows_between(ROW_PX, height - ROW_PX);
        for (index, row) in self.rows.iter().take(fit).enumerate() {
            let y = row_y(ROW_PX, index);
            if row.emphasised {
                let band = [0.0, y - ROW_PX / 2.0, width, ROW_PX];
                scene.rect(band, Token::TextFaint, BAND_OPACITY);
            }
            scene.cell([x[0], y], &row.time, Align::Left);
            scene.cell([x[1], y], &row.side, Align::Left);
            scene.cell([x[2], y], &row.price, Align::Right);
            scene.cell([x[3], y], &row.size, Align::Right);
            if let Some(fills) = &row.fills {
                scene.cell([x[3] + 2.0, y], fills, Align::Left);
            }
            scene.cell([x[4], y], &row.notional, Align::Right);
            let bubble = ScenePrim::Circle([x[5], y], row.bubble_radius_px, row.bubble_token);
            scene.prims.push(bubble);
            scene.rows += 1;
        }
        scene.footer(height, &self.footer);
        scene
    }
}
