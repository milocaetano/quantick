//! The Liquidations pane's view-model: the 1m / 5m / 1h long and short
//! totals, a per-minute histogram (longs below the baseline, shorts above)
//! and the newest forced closes as text rows.

use quantick_appearance::Token;
use quantick_civil::TzOffset;
use quantick_orderflow::widgets::liquidations::{LiquidationTotals, Liquidations};
use quantick_orderflow::widgets::ports::{LiquidatedSide, LiquidationPrint};
use rust_decimal::Decimal;

use super::big_trades::NotionalUnit;
use super::scene::{
    Align, BAND_OPACITY, INSET_PX, ROW_PX, TextSize, WidgetScene, row_y, rows_between,
};
use super::{WidgetCell, amount, bar_px, clock_text, price_text};
use crate::footprint_projection::fmt_qty;

/// What the pane says on a feed with no liquidation stream.
pub const LIQUIDATIONS_NOT_PROVIDED: &str = "feed does not provide liquidations";

/// A row at or above this notional is drawn emphasised.
pub const LIQUIDATION_EMPHASIS_NOTIONAL: u32 = 500_000;

/// Pixels left clear on each side of a histogram bar.
pub const LIQUIDATION_BAR_GAP_PX: f32 = 1.0;

/// One header total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationTotalsView {
    /// `1m`, `5m`, `1h`.
    pub label: String,
    /// `L $420.0k`, in the sell colour: longs were sold out.
    pub long: WidgetCell,
    /// `S $30.0k`, in the buy colour.
    pub short: WidgetCell,
}

/// One histogram minute, in pixels from the histogram's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidationBarView {
    pub x_px: f32,
    pub width_px: f32,
    /// Hangs down from the baseline, [`Token::Sell`].
    pub long_px: f32,
    /// Stands up on the baseline, [`Token::Buy`].
    pub short_px: f32,
}

/// One list row as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationRowView {
    pub time: WidgetCell,
    /// `LONG` or `SHORT`: the position that was closed.
    pub side: WidgetCell,
    pub price: WidgetCell,
    pub size: WidgetCell,
    pub notional: WidgetCell,
    /// At or above [`LIQUIDATION_EMPHASIS_NOTIONAL`].
    pub emphasised: bool,
}

/// The whole pane body.
#[derive(Debug, Clone, PartialEq)]
pub struct LiquidationsView {
    pub columns: [&'static str; 5],
    /// Shortest window first.
    pub totals: Vec<LiquidationTotalsView>,
    /// Distance of the histogram's baseline from its top.
    pub baseline_y_px: f32,
    /// Oldest minute first.
    pub bars: Vec<LiquidationBarView>,
    /// Newest first.
    pub rows: Vec<LiquidationRowView>,
    /// Set, with nothing else, when the feed has no liquidation stream.
    pub empty: Option<String>,
}

impl LiquidationsView {
    /// The pane on a feed that does not stream liquidations.
    #[must_use]
    pub fn empty_state(layout: &LiquidationsLayout) -> Self {
        Self {
            columns: columns(layout.unit),
            totals: Vec::new(),
            baseline_y_px: layout.histogram_height_px / 2.0,
            bars: Vec::new(),
            rows: Vec::new(),
            empty: Some(LIQUIDATIONS_NOT_PROVIDED.to_owned()),
        }
    }
}

/// How the model is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidationsLayout {
    /// Prices are written at this tick's precision.
    pub tick: Decimal,
    /// Times are written in this zone.
    pub tz: TzOffset,
    pub unit: NotionalUnit,
    pub histogram_width_px: f32,
    pub histogram_height_px: f32,
}

/// Lay `model` out; the empty state when the feed provides none.
#[must_use]
pub fn liquidations_view(
    model: Option<&Liquidations>,
    layout: &LiquidationsLayout,
) -> LiquidationsView {
    let Some(model) = model else {
        return LiquidationsView::empty_state(layout);
    };
    let baseline_y_px = layout.histogram_height_px / 2.0;
    let largest = model
        .buckets
        .iter()
        .map(|bucket| bucket.long_notional.max(bucket.short_notional))
        .max()
        .unwrap_or(Decimal::ZERO);
    let slot_px = if model.buckets.is_empty() {
        0.0
    } else {
        layout.histogram_width_px / model.buckets.len() as f32
    };
    let bars = model
        .buckets
        .iter()
        .enumerate()
        .map(|(index, bucket)| LiquidationBarView {
            x_px: index as f32 * slot_px + LIQUIDATION_BAR_GAP_PX,
            width_px: (slot_px - 2.0 * LIQUIDATION_BAR_GAP_PX).max(0.0),
            long_px: bar_px(bucket.long_notional, largest, baseline_y_px),
            short_px: bar_px(bucket.short_notional, largest, baseline_y_px),
        })
        .collect();
    LiquidationsView {
        columns: columns(layout.unit),
        totals: model
            .totals
            .iter()
            .map(|totals| totals_view(totals, layout.unit))
            .collect(),
        baseline_y_px,
        bars,
        rows: model.rows.iter().map(|row| row_view(row, layout)).collect(),
        empty: None,
    }
}

fn columns(unit: NotionalUnit) -> [&'static str; 5] {
    ["TIME", "LIQUIDATED", "PRICE", "SIZE", unit.header]
}

fn totals_view(totals: &LiquidationTotals, unit: NotionalUnit) -> LiquidationTotalsView {
    LiquidationTotalsView {
        label: window_label(totals.window_ms),
        long: WidgetCell::new(
            format!("L {}", amount(totals.long_notional, unit)),
            Token::Sell,
        ),
        short: WidgetCell::new(
            format!("S {}", amount(totals.short_notional, unit)),
            Token::Buy,
        ),
    }
}

/// `1m`, `1h`: a window in its largest whole unit.
fn window_label(window_ms: i64) -> String {
    match window_ms {
        ms if ms >= 3_600_000 && ms % 3_600_000 == 0 => format!("{}h", ms / 3_600_000),
        ms if ms >= 60_000 && ms % 60_000 == 0 => format!("{}m", ms / 60_000),
        ms => format!("{}s", ms / 1_000),
    }
}

fn row_view(row: &LiquidationPrint, layout: &LiquidationsLayout) -> LiquidationRowView {
    // A closed long was sold into the book; a closed short was bought.
    let (label, side_token) = match row.side {
        LiquidatedSide::Long => ("LONG", Token::Sell),
        LiquidatedSide::Short => ("SHORT", Token::Buy),
    };
    let notional = row.notional();
    let emphasised = notional >= Decimal::from(LIQUIDATION_EMPHASIS_NOTIONAL);
    let ink = if emphasised {
        Token::TextPrimary
    } else {
        Token::TextMuted
    };
    LiquidationRowView {
        time: WidgetCell::new(clock_text(row.timestamp_ms, layout.tz), Token::TextFaint),
        side: WidgetCell::new(label, side_token),
        price: WidgetCell::new(price_text(row.price, layout.tick), ink),
        size: WidgetCell::new(fmt_qty(row.quantity), side_token),
        notional: WidgetCell::new(amount(notional, layout.unit), ink),
        emphasised,
    }
}

/// The histogram's height, in rows.
pub const LIQUIDATION_HISTOGRAM_ROWS: f32 = 3.0;

/// Column anchors, as shares of the width: TIME, LIQUIDATED, PRICE, SIZE
/// and the notional.
const LIQUIDATION_COLUMN_SHARES: [f32; 5] = [0.02, 0.30, 0.56, 0.74, 0.97];

impl LiquidationsView {
    /// Lay the pane out in a `width` by `height` body: the window totals,
    /// the per-minute histogram around its baseline, then the list.
    #[must_use]
    pub fn paint(&self, width: f32, height: f32) -> WidgetScene {
        if let Some(empty) = &self.empty {
            return WidgetScene::empty(empty, Vec::new(), width, height);
        }
        let mut scene = WidgetScene::default();
        let slot = width / self.totals.len().max(1) as f32;
        for (index, totals) in self.totals.iter().enumerate() {
            let left = INSET_PX + slot * index as f32;
            let label_at = [left, ROW_PX / 2.0];
            scene.sized(label_at, &totals.label, Token::TextFaint, Align::Left, TextSize::Small);
            scene.cell([left, row_y(0.0, 1)], &totals.long, Align::Left);
            scene.cell([left + slot / 2.0, row_y(0.0, 1)], &totals.short, Align::Left);
        }
        let base = 2.0 * ROW_PX + self.baseline_y_px;
        scene.rect([0.0, base, width, 1.0], Token::Border, 1.0);
        for bar in &self.bars {
            let short = [bar.x_px, base - bar.short_px, bar.width_px, bar.short_px];
            scene.rect(short, Token::Buy, 1.0);
            scene.rect([bar.x_px, base, bar.width_px, bar.long_px], Token::Sell, 1.0);
        }
        let top = 2.0 * ROW_PX + 2.0 * self.baseline_y_px + ROW_PX / 2.0;
        let x = LIQUIDATION_COLUMN_SHARES.map(|share| share * width);
        let aligns = [Align::Left, Align::Right, Align::Right, Align::Right, Align::Right];
        let heads: Vec<(&str, f32, Align)> =
            (0..5).map(|index| (self.columns[index], x[index], aligns[index])).collect();
        scene.heads(row_y(top, 0), &heads);
        for (index, row) in self.rows.iter().take(rows_between(top + ROW_PX, height)).enumerate() {
            let y = row_y(top + ROW_PX, index);
            if row.emphasised {
                let band = [0.0, y - ROW_PX / 2.0, width, ROW_PX];
                scene.rect(band, Token::TextFaint, BAND_OPACITY);
            }
            let cells = [&row.time, &row.side, &row.price, &row.size, &row.notional];
            for (column, cell) in cells.into_iter().enumerate() {
                scene.cell([x[column], y], cell, aligns[column]);
            }
            scene.rows += 1;
        }
        scene
    }
}
