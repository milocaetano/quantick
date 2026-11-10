//! The Big Trades pane's view-model: one text row per big print or merge,
//! a bubble sized by notional, the threshold chips and the five-minute
//! footer.

use quantick_appearance::Token;
use quantick_civil::{TzOffset, civil_utc};
use quantick_engine::Side;
use quantick_orderflow::widgets::big_trades::{
    BIG_TRADES_WINDOW_MS, BigTrade, BigTrades, MIN_NOTIONAL_PRESETS,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::{WidgetCell, price_text};
use crate::footprint_projection::fmt_qty;

/// What the pane says before any tape arrives.
pub const BIG_TRADES_EMPTY: &str = "waiting for trades";

/// A row at or above the largest preset is drawn emphasised.
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
            presets: presets(min_notional),
            merge_fills,
            rows: Vec::new(),
            footer: String::new(),
            empty: Some(BIG_TRADES_EMPTY.to_owned()),
        }
    }
}

/// How `model` is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTradesLayout {
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
pub fn big_trades_view(model: Option<&BigTrades>, layout: &BigTradesLayout) -> BigTradesView {
    let Some(model) = model else {
        return BigTradesView::empty_state(layout.min_notional, layout.merge_fills, layout.unit);
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
        presets: presets(layout.min_notional),
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

fn row_view(row: &BigTrade, largest: Decimal, layout: &BigTradesLayout) -> BigTradeRowView {
    let side_token = match row.side {
        Side::Buy => Token::Buy,
        Side::Sell => Token::Sell,
    };
    let emphasised = row.notional >= Decimal::from(EMPHASIS_NOTIONAL);
    let ink = if emphasised {
        Token::TextPrimary
    } else {
        Token::TextMuted
    };
    let (_, _, _, hour, minute, second) =
        civil_utc(row.timestamp_ms.saturating_add(layout.tz.offset_ms()));
    let share = if largest > Decimal::ZERO {
        (row.notional / largest)
            .to_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    BigTradeRowView {
        time: WidgetCell::new(
            format!("{hour:02}:{minute:02}:{second:02}"),
            Token::TextFaint,
        ),
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

fn presets(min_notional: Decimal) -> Vec<(String, bool)> {
    MIN_NOTIONAL_PRESETS
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

fn amount(value: Decimal, unit: NotionalUnit) -> String {
    format!("{}{}", unit.prefix, fmt_qty(value))
}
