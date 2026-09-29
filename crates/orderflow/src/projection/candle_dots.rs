//! One quiet, weighted-price dot per candle from exact native footprint levels.

use quantick_engine::BarFootprint;
use rust_decimal::Decimal;

use super::{PriceWindow, normalized_area_size};

/// Maximum radius in pixels; the candle column may reduce it.
const MAX_RADIUS_PX: f32 = 4.0;

/// Provenance is supplied by the ladder owner, never guessed from its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandleFootprintSource {
    TradeBuilt,
    Approximate,
}

#[derive(Debug, Clone, Copy)]
pub struct CandleFootprint<'a> {
    pub slot: usize,
    pub ladder: &'a BarFootprint,
    pub source: CandleFootprintSource,
}

/// The observed price grid and one price known to lie on it.
#[derive(Debug, Clone, Copy)]
pub struct CandleDotGrid {
    pub step: Decimal,
    pub reference_price: Decimal,
}

#[derive(Debug, Clone, Copy)]
pub struct CandleDotView {
    pub prices: PriceWindow,
    pub candle_width_px: f32,
}

/// The whole candle's quantities at their weighted price, with no trade-time claim.
#[derive(Debug, Clone, PartialEq)]
pub struct CandleDot {
    pub slot: usize,
    pub price: Decimal,
    pub buy_quantity: Decimal,
    pub sell_quantity: Decimal,
    pub trade_count: u64,
    pub radius_px: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandleDotFrame {
    pub marks: Vec<CandleDot>,
    pub full_quantity: Decimal,
}

impl CandleDotGrid {
    fn preserves_prices(self, ladder: &BarFootprint) -> bool {
        let group = ladder.group();
        !ladder.is_aggregated()
            && group > Decimal::ZERO
            && self.step >= group
            && self.step.checked_rem(group).is_some_and(|r| r.is_zero())
            && self
                .reference_price
                .checked_rem(group)
                .is_some_and(|r| r.is_zero())
    }
}

/// Sum each supplied trade-built ladder before filtering its weighted center.
/// A row's lower bound is an execution price only when its grid is exactly
/// aligned with the observed native grid. Coarsened and approximated ladders
/// stay out rather than inventing a candle's price moment.
pub fn project_candle_dots<'a>(
    inputs: impl IntoIterator<Item = CandleFootprint<'a>>,
    grid: Option<CandleDotGrid>,
    view: CandleDotView,
) -> CandleDotFrame {
    let mut frame = CandleDotFrame {
        marks: Vec::new(),
        full_quantity: Decimal::ONE,
    };
    let Some(grid) = grid else { return frame };
    if grid.step <= Decimal::ZERO
        || !view.candle_width_px.is_finite()
        || view.candle_width_px <= 0.0
        || view.prices.high <= view.prices.low
    {
        return frame;
    }
    // One mark occupies each candle column, so native price-row spacing cannot
    // make a wide-range candle's single dot disappear.
    let maximum = MAX_RADIUS_PX.min(view.candle_width_px / 2.0);
    for input in inputs {
        if input.source != CandleFootprintSource::TradeBuilt || !grid.preserves_prices(input.ladder)
        {
            continue;
        }
        let group = input.ladder.group();
        let facts = input.ladder.levels().iter().try_fold(
            (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, 0_u64),
            |(buy, sell, moment, count), (&bucket, level)| {
                let price = group.checked_mul(Decimal::from(bucket))?;
                let quantity = level.buy.checked_add(level.sell)?;
                Some((
                    buy.checked_add(level.buy)?,
                    sell.checked_add(level.sell)?,
                    moment.checked_add(price.checked_mul(quantity)?)?,
                    count.checked_add(level.trade_count)?,
                ))
            },
        );
        let Some((buy_quantity, sell_quantity, price_quantity, trade_count)) = facts else {
            continue;
        };
        let Some(quantity) = buy_quantity
            .checked_add(sell_quantity)
            .filter(|quantity| *quantity > Decimal::ZERO)
        else {
            continue;
        };
        let Some(price) = price_quantity.checked_div(quantity) else {
            continue;
        };
        if price < view.prices.low || price > view.prices.high {
            continue;
        }
        frame.marks.push(CandleDot {
            slot: input.slot,
            price,
            buy_quantity,
            sell_quantity,
            trade_count,
            radius_px: maximum,
        });
    }
    frame.full_quantity = frame
        .marks
        .iter()
        .map(|dot| dot.buy_quantity.saturating_add(dot.sell_quantity))
        .max()
        .unwrap_or(Decimal::ONE);
    for dot in &mut frame.marks {
        dot.radius_px *= normalized_area_size(
            dot.buy_quantity.saturating_add(dot.sell_quantity),
            frame.full_quantity,
        );
    }
    frame
        .marks
        .sort_by(|a, b| a.slot.cmp(&b.slot).then(a.price.cmp(&b.price)));
    frame
}
