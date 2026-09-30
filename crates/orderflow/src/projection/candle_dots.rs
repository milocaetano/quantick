//! Price-local aggression from exact native footprint levels. Adjacent
//! candles and native price rows coarsen independently with their axes.
//!
//! Zoomed out, neighbouring candles fold into one group: `k` candles, a power
//! of two held by [`CandleGroupMemory`], grouped by absolute slot
//! (`slot / k`). Slots count from the series' first bar, so panning never
//! regroups, and a group cut by the view edge still sums every member
//! ([`CandleDotView::input_slots`]). A mark's area is its exact volume against
//! the largest visible mark.

use std::collections::BTreeMap;
use std::ops::Range;

use quantick_engine::BarFootprint;
use rust_decimal::Decimal;

use super::{PriceWindow, normalized_area_size};
use rust_decimal::prelude::ToPrimitive as _;

/// Compact radius ceiling, shared by all marks in the frame. Horizontal
/// column and vertical price-band spacing can reduce this common cap.
pub const CANDLE_MARK_MAX_RADIUS_PX: f32 = 4.0;

/// Minimum horizontal group spacing. The common cap also respects vertical
/// band spacing; each individual radius remains strictly proportional.
pub const CANDLE_GROUP_MIN_WIDTH_PX: f32 = 6.0;

/// The hysteresis band: a held group halves only once the half would be this
/// many times [`CANDLE_GROUP_MIN_WIDTH_PX`] wide (7.5 px). Between the two
/// widths a steady zoom keeps its rung, and at the chart's default 8 px per
/// bar every rung returns to one candle per mark.
pub const CANDLE_GROUP_HOLD_BAND: f32 = 1.25;

/// Candles per mark on offer: powers of two, the widest when none fits.
const CANDLE_GROUP_LADDER: [usize; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

/// Price bands target six pixels; a held band uses the same dead band as
/// the horizontal groups. Every band is anchored to the absolute price grid.
pub const CANDLE_PRICE_MIN_HEIGHT_PX: f32 = 6.0;

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
    /// Pixels between neighbouring candles: one bar's slot.
    pub candle_width_px: f32,
    /// Visible global slots `[start, end)`; a group with none of them draws
    /// no mark.
    pub visible: (usize, usize),
    /// Candles per mark, from [`CandleGroupMemory::choose`]; 1 draws one mark
    /// per candle.
    pub candles_per_mark: usize,
    /// Native ticks in each absolute price band.
    pub price_ticks_per_mark: usize,
    /// Height of the visible price window in logical pixels.
    pub height_px: f32,
}

/// A price band's exact quantities at their weighted execution price, with
/// no trade-time claim. Every band in a candle group shares its slot span;
/// at one candle per group `last_slot == slot`.
#[derive(Debug, Clone, PartialEq)]
pub struct CandleDot {
    pub slot: usize,
    pub last_slot: usize,
    pub price: Decimal,
    /// Lowest and highest execution prices contributing to this mark.
    pub price_low: Decimal,
    pub price_high: Decimal,
    pub buy_quantity: Decimal,
    pub sell_quantity: Decimal,
    pub trade_count: u64,
    pub radius_px: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandleDotFrame {
    pub marks: Vec<CandleDot>,
    pub full_quantity: Decimal,
    pub candles_per_mark: usize,
    pub price_ticks_per_mark: usize,
    /// One common radius cap; every mark's area follows its exact quantity.
    pub maximum_radius_px: f32,
}

/// The candles one mark stands for, held across frames so a steady zoom
/// keeps its rung. Only the candle width decides it, so panning never
/// regroups; each pane keeps one.
#[derive(Debug, Clone, Default)]
pub struct CandleGroupMemory {
    candles: Option<usize>,
}

/// Sticky vertical detail, independent of horizontal zoom and panning.
#[derive(Debug, Clone, Default)]
pub struct CandlePriceMemory {
    ticks: Option<usize>,
}

impl CandlePriceMemory {
    pub fn choose(&mut self, native_tick_height_px: f32) -> usize {
        if !native_tick_height_px.is_finite() || native_tick_height_px <= 0.0 {
            return self.ticks.unwrap_or(1);
        }
        let width = |ticks: usize| ticks as f32 * native_tick_height_px;
        let held = self.ticks.filter(|&ticks| {
            width(ticks) >= CANDLE_PRICE_MIN_HEIGHT_PX
                && (ticks == 1
                    || width(ticks / 2) < CANDLE_PRICE_MIN_HEIGHT_PX * CANDLE_GROUP_HOLD_BAND)
        });
        let ticks = held.unwrap_or_else(|| {
            let mut ticks = 1_usize;
            while width(ticks) < CANDLE_PRICE_MIN_HEIGHT_PX {
                let Some(next) = ticks.checked_mul(2) else {
                    break;
                };
                ticks = next;
            }
            ticks
        });
        self.ticks = Some(ticks);
        ticks
    }
}

impl CandleDotGrid {
    /// Pixel height of one native tick on this price axis.
    #[must_use]
    pub fn tick_height_px(self, prices: PriceWindow, height_px: f32) -> f32 {
        self.step
            .checked_div(prices.high - prices.low)
            .and_then(|ratio| ratio.to_f32())
            .unwrap_or(0.0)
            * height_px
    }
}

impl CandleGroupMemory {
    /// The rung for candles `candle_width_px` apart: the held one while its
    /// group is at least [`CANDLE_GROUP_MIN_WIDTH_PX`] wide and its half is
    /// under [`CANDLE_GROUP_HOLD_BAND`] times that, otherwise the smallest
    /// rung at least that wide.
    pub fn choose(&mut self, candle_width_px: f32) -> usize {
        let width = |candles: usize| candles as f32 * candle_width_px;
        let halve_at = CANDLE_GROUP_MIN_WIDTH_PX * CANDLE_GROUP_HOLD_BAND;
        let held = self.candles.filter(|&candles| {
            width(candles) >= CANDLE_GROUP_MIN_WIDTH_PX
                && (candles == 1 || width(candles / 2) < halve_at)
        });
        let candles = held.unwrap_or_else(|| {
            let fits = |candles: &usize| width(*candles) >= CANDLE_GROUP_MIN_WIDTH_PX;
            let widest = CANDLE_GROUP_LADDER[CANDLE_GROUP_LADDER.len() - 1];
            CANDLE_GROUP_LADDER.into_iter().find(fits).unwrap_or(widest)
        });
        self.candles = Some(candles);
        candles
    }
}

impl CandleDotView {
    /// The slots the visible groups own: `visible` widened to whole groups,
    /// by at most `candles_per_mark - 1` on each side.
    #[must_use]
    pub fn input_slots(&self) -> Range<usize> {
        let candles = self.candles_per_mark.max(1);
        let start = self.visible.0 / candles * candles;
        let end = self.visible.1.div_ceil(candles).saturating_mul(candles);
        start..end.max(start)
    }

    /// The chart's trade-built ladders over [`Self::input_slots`]: `closed[i]`
    /// is slot `first_slot + i`, and `partial` is the forming bar's.
    pub fn trade_built<'a>(
        self,
        closed: &'a [BarFootprint],
        first_slot: usize,
        partial: Option<(usize, &'a BarFootprint)>,
    ) -> impl Iterator<Item = CandleFootprint<'a>> {
        let slots = self.input_slots();
        let index = |slot: usize| slot.saturating_sub(first_slot).min(closed.len());
        let from = index(slots.start);
        let partial = partial.filter(|(slot, _)| slots.contains(slot));
        closed[from..index(slots.end)]
            .iter()
            .zip(first_slot + from..)
            .map(|(ladder, slot)| (slot, ladder))
            .chain(partial)
            .map(|(slot, ladder)| CandleFootprint {
                slot,
                ladder,
                source: CandleFootprintSource::TradeBuilt,
            })
    }
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

/// Exact sums for one price band, with the actual execution span.
#[derive(Debug, Clone, Copy)]
struct BandSums {
    low: Decimal,
    high: Decimal,
    buy: Decimal,
    sell: Decimal,
    moment: Decimal,
    count: u64,
}

impl BandSums {
    fn add(self, other: Self) -> Option<Self> {
        Some(Self {
            low: self.low.min(other.low),
            high: self.high.max(other.high),
            buy: self.buy.checked_add(other.buy)?,
            sell: self.sell.checked_add(other.sell)?,
            moment: self.moment.checked_add(other.moment)?,
            count: self.count.checked_add(other.count)?,
        })
    }
}

#[derive(Debug)]
struct CandleBands {
    first: usize,
    last: usize,
    bands: BTreeMap<Decimal, Option<BandSums>>,
}

/// Sum native execution rows by absolute candle group and absolute price band.
/// Incompatible, approximate and cap-folded ladders cannot claim exact prices.
/// Each band is filtered by its weighted execution price only after summing
/// all members, including candles outside the view edge. Overflow suppresses
/// the affected whole group rather than publishing partial quantities.
///
/// Every visible mark shares a single area scale and radius cap. Small
/// quantities may produce tiny marks: there is no radius floor or saturation.
pub fn project_candle_dots<'a>(
    inputs: impl IntoIterator<Item = CandleFootprint<'a>>,
    grid: Option<CandleDotGrid>,
    view: CandleDotView,
) -> CandleDotFrame {
    let mut frame = CandleDotFrame {
        marks: Vec::new(),
        full_quantity: Decimal::ONE,
        candles_per_mark: view.candles_per_mark,
        price_ticks_per_mark: view.price_ticks_per_mark,
        maximum_radius_px: 0.0,
    };
    let Some(grid) = grid else { return frame };
    let Some(band_step) = grid
        .step
        .checked_mul(Decimal::from(view.price_ticks_per_mark))
    else {
        return frame;
    };
    let candles = view.candles_per_mark;
    if grid.step <= Decimal::ZERO
        || !view.candle_width_px.is_finite()
        || view.candle_width_px <= 0.0
        || !view.height_px.is_finite()
        || view.height_px <= 0.0
        || view.prices.high <= view.prices.low
        || candles == 0
        || view.visible.1 <= view.visible.0
        || band_step <= Decimal::ZERO
    {
        return frame;
    }
    let band_height =
        grid.tick_height_px(view.prices, view.height_px) * view.price_ticks_per_mark as f32;
    frame.maximum_radius_px = CANDLE_MARK_MAX_RADIUS_PX
        .min(view.candle_width_px * candles as f32 / 2.0)
        .min(band_height / 2.0);
    let owned = view.input_slots();
    let mut groups: BTreeMap<usize, Option<CandleBands>> = BTreeMap::new();
    for input in inputs {
        if !owned.contains(&input.slot)
            || input.source != CandleFootprintSource::TradeBuilt
            || !grid.preserves_prices(input.ladder)
            || input.ladder.levels().is_empty()
        {
            continue;
        }
        let entry = groups.entry(input.slot / candles).or_insert_with(|| {
            Some(CandleBands {
                first: input.slot,
                last: input.slot,
                bands: BTreeMap::new(),
            })
        });
        let Some(group) = entry.as_mut() else {
            continue;
        };
        group.first = group.first.min(input.slot);
        group.last = group.last.max(input.slot);
        if !add_ladder(group, input.ladder, band_step) {
            *entry = None;
        }
    }
    for group in groups.into_values().flatten() {
        for sums in group.bands.into_values().flatten() {
            let Some(price) = sums
                .buy
                .checked_add(sums.sell)
                .and_then(|quantity| sums.moment.checked_div(quantity))
            else {
                continue;
            };
            if price < view.prices.low || price > view.prices.high {
                continue;
            }
            frame.marks.push(CandleDot {
                slot: group.first,
                last_slot: group.last,
                price,
                price_low: sums.low,
                price_high: sums.high,
                buy_quantity: sums.buy,
                sell_quantity: sums.sell,
                trade_count: sums.count,
                radius_px: 0.0,
            });
        }
    }
    // Readback totals must remain exact as well as each individual band.
    // Suppress an unrepresentable frame rather than clamp its stated totals.
    let totals = frame.marks.iter().try_fold(
        (Decimal::ZERO, Decimal::ZERO, 0_u64),
        |(buy, sell, count), mark| {
            Some((
                buy.checked_add(mark.buy_quantity)?,
                sell.checked_add(mark.sell_quantity)?,
                count.checked_add(mark.trade_count)?,
            ))
        },
    );
    if totals.is_none() {
        frame.marks.clear();
    }
    frame.full_quantity = frame
        .marks
        .iter()
        .map(|dot| dot.buy_quantity.saturating_add(dot.sell_quantity))
        .max()
        .unwrap_or(Decimal::ONE);
    frame.resize();
    frame
}

fn add_ladder(group: &mut CandleBands, ladder: &BarFootprint, band_step: Decimal) -> bool {
    for (&bucket, level) in ladder.levels() {
        let Some(price) = ladder.group().checked_mul(Decimal::from(bucket)) else {
            return false;
        };
        let Some(quantity) = level.buy.checked_add(level.sell) else {
            return false;
        };
        if quantity <= Decimal::ZERO {
            continue;
        }
        let Some(moment) = price.checked_mul(quantity) else {
            return false;
        };
        let Some(band) = price.checked_div(band_step).map(|index| index.floor()) else {
            return false;
        };
        let sums = BandSums {
            low: price,
            high: price,
            buy: level.buy,
            sell: level.sell,
            moment,
            count: level.trade_count,
        };
        group
            .bands
            .entry(band)
            .and_modify(|entry| *entry = entry.and_then(|previous| previous.add(sums)))
            .or_insert(Some(sums));
        if group.bands.get(&band).is_some_and(Option::is_none) {
            return false;
        }
    }
    true
}

impl CandleDotFrame {
    fn resize(&mut self) {
        for dot in &mut self.marks {
            dot.radius_px = self.maximum_radius_px
                * normalized_area_size(
                    dot.buy_quantity.saturating_add(dot.sell_quantity),
                    self.full_quantity,
                );
        }
    }
}

/// Hold the largest observed quantity for one horizontal/vertical tier.
/// Panning a heavy band off screen does not inflate the retained marks. A
/// heavier newly observed band raises the common reference for all marks.
#[derive(Debug, Clone, Default)]
pub struct CandleScaleMemory {
    held: BTreeMap<(usize, usize), Decimal>,
}

impl CandleScaleMemory {
    pub fn apply(&mut self, frame: &mut CandleDotFrame) {
        if frame.marks.is_empty() {
            return;
        }
        let (candles, ticks) = (frame.candles_per_mark, frame.price_ticks_per_mark);
        let reference = self
            .held
            .entry((candles, ticks))
            .or_insert(frame.full_quantity);
        *reference = (*reference).max(frame.full_quantity);
        frame.full_quantity = *reference;
        frame.resize();
    }
}
