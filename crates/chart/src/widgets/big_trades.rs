//! The Big Trades pane's view-model: one text row per big print or merge,
//! a bubble sized by notional, the threshold chips and the five-minute
//! footer.

use quantick_appearance::Token;
use quantick_civil::{TzOffset, civil_utc};
use quantick_engine::Side;
use quantick_orderflow::widgets::big_trades::{BigTrade, BigTrades, MIN_NOTIONAL_PRESETS};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::{WidgetCell, price_text};
use crate::footprint_projection::fmt_qty;
use crate::geometry::compact_value;

/// The list's column heads; the blanks sit over the side and the bubble.
pub const BIG_TRADES_COLUMNS: [&str; 6] = ["TIME", "", "PRICE", "SIZE", "USD", ""];

/// The threshold chips, one per [`MIN_NOTIONAL_PRESETS`] entry.
pub const NOTIONAL_PRESET_LABELS: [&str; MIN_NOTIONAL_PRESETS.len()] = ["50k", "250k", "1M"];

/// What the pane says before any tape arrives.
pub const BIG_TRADES_EMPTY: &str = "waiting for trades";

/// A row at or above this notional is drawn emphasised.
pub const EMPHASIS_NOTIONAL: u32 = 1_000_000;

/// Bubble radius of a row with no notional.
pub const BUBBLE_MIN_RADIUS_PX: f32 = 3.0;

/// Bubble radius of the largest row on display.
pub const BUBBLE_MAX_RADIUS_PX: f32 = 9.0;

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
    /// Threshold chips and whether each is the one in force.
    pub presets: Vec<(String, bool)>,
    /// The `merge fills` chip.
    pub merge_fills: bool,
    /// Newest first.
    pub rows: Vec<BigTradeRowView>,
    /// `last 5 min: buys $8.40M · sells $5.10M`; empty with no tape.
    pub footer: String,
    /// Set, with no rows, when there is nothing to list.
    pub empty: Option<String>,
}

impl BigTradesView {
    /// The pane before its first print.
    #[must_use]
    pub fn empty_state(min_notional: Decimal, merge_fills: bool) -> Self {
        Self {
            presets: presets(min_notional),
            merge_fills,
            rows: Vec::new(),
            footer: String::new(),
            empty: Some(BIG_TRADES_EMPTY.to_owned()),
        }
    }
}

/// Lay `model` out, filtered at `min_notional`, prices at `tick`'s
/// precision and times in `tz`; the empty state when there is no tape.
#[must_use]
pub fn big_trades_view(
    model: Option<&BigTrades>,
    min_notional: Decimal,
    merge_fills: bool,
    tick: Decimal,
    tz: TzOffset,
) -> BigTradesView {
    let Some(model) = model else {
        return BigTradesView::empty_state(min_notional, merge_fills);
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
        .map(|row| row_view(row, largest, tick, tz))
        .collect();
    let empty = model
        .rows
        .is_empty()
        .then(|| format!("no trades at or above {} yet", dollars(min_notional)));
    BigTradesView {
        presets: presets(min_notional),
        merge_fills,
        rows,
        footer: format!(
            "last 5 min: buys {} · sells {}",
            dollars(model.window_buy_notional),
            dollars(model.window_sell_notional)
        ),
        empty,
    }
}

fn row_view(row: &BigTrade, largest: Decimal, tick: Decimal, tz: TzOffset) -> BigTradeRowView {
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
        civil_utc(row.timestamp_ms.saturating_add(tz.offset_ms()));
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
        price: WidgetCell::new(price_text(row.price, tick), ink),
        size: WidgetCell::new(fmt_qty(row.quantity), side_token),
        fills: row
            .merged
            .then(|| WidgetCell::new(format!("×{}", row.fill_count), Token::TextFaint)),
        notional: WidgetCell::new(dollars(row.notional), ink),
        bubble_radius_px: BUBBLE_MIN_RADIUS_PX
            + (BUBBLE_MAX_RADIUS_PX - BUBBLE_MIN_RADIUS_PX) * share.sqrt() as f32,
        bubble_token: side_token,
        emphasised,
    }
}

fn presets(min_notional: Decimal) -> Vec<(String, bool)> {
    MIN_NOTIONAL_PRESETS
        .iter()
        .zip(NOTIONAL_PRESET_LABELS)
        .map(|(value, label)| (label.to_owned(), Decimal::from(*value) == min_notional))
        .collect()
}

fn dollars(value: Decimal) -> String {
    format!("${}", compact_value(value.to_f64().unwrap_or(0.0)))
}
