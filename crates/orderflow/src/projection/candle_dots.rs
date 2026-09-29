//! One quiet, weighted-price mark per candle from exact native footprint
//! levels, or per group of candles once they are too narrow to read.
//!
//! Zoomed out, neighbouring candles fold into one mark: `k` candles, a power
//! of two held by [`CandleGroupMemory`], grouped by absolute slot
//! (`slot / k`). Slots count from the series' first bar, so panning never
//! regroups, and a group cut by the view edge still sums every member
//! ([`CandleDotView::input_slots`]). A mark's area is its exact volume against
//! the largest visible mark.

use std::collections::BTreeMap;
use std::ops::Range;

use quantick_engine::BarFootprint;
use rust_decimal::Decimal;

use super::{MIN_DOT_RADIUS_PX, PriceWindow, normalized_area_size};

/// The largest radius a mark reaches, in pixels; half its group's width may
/// reduce it. 6 rather than 4, so the heaviest group of a zoomed-out chart
/// reads as clearly larger than a quiet one while staying a quiet overlay.
pub const CANDLE_MARK_MAX_RADIUS_PX: f32 = 6.0;

/// The narrowest a group of candles is held at, in pixels. A mark's cap is
/// half its group's width, so it never falls under 3 px and a quiet group
/// and a heavy one still differ in size.
pub const CANDLE_GROUP_MIN_WIDTH_PX: f32 = 6.0;

/// The hysteresis band: a held group halves only once the half would be this
/// many times [`CANDLE_GROUP_MIN_WIDTH_PX`] wide (7.5 px). Between the two
/// widths a steady zoom keeps its rung, and at the chart's default 8 px per
/// bar every rung returns to one candle per mark.
pub const CANDLE_GROUP_HOLD_BAND: f32 = 1.25;

/// Candles per mark on offer: powers of two, the widest when none fits.
const CANDLE_GROUP_LADDER: [usize; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

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
}

/// A group's whole quantities at their weighted price, with no trade-time
/// claim: drawn at the centre of `slot..=last_slot`, the first and last of
/// its candles with trade-built ladders. One candle per mark has
/// `last_slot == slot`.
#[derive(Debug, Clone, PartialEq)]
pub struct CandleDot {
    pub slot: usize,
    pub last_slot: usize,
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

/// The candles one mark stands for, held across frames so a steady zoom
/// keeps its rung. Only the candle width decides it, so panning never
/// regroups; each pane keeps one.
#[derive(Debug, Clone, Default)]
pub struct CandleGroupMemory {
    candles: Option<usize>,
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

/// Exact sums over a group's candles: their first and last slots, both sides,
/// the price moment `Σ price × quantity`, and the trade count.
#[derive(Debug, Clone, Copy)]
struct GroupSums {
    first: usize,
    last: usize,
    buy: Decimal,
    sell: Decimal,
    moment: Decimal,
    count: u64,
}

impl GroupSums {
    /// One trade-built candle's sums, `None` when it traded nothing or its
    /// rows overflow.
    fn of(slot: usize, ladder: &BarFootprint) -> Option<Self> {
        let group = ladder.group();
        let (buy, sell, moment, count) = ladder.levels().iter().try_fold(
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
        )?;
        (buy.checked_add(sell)? > Decimal::ZERO).then_some(Self {
            first: slot,
            last: slot,
            buy,
            sell,
            moment,
            count,
        })
    }

    fn add(self, other: Self) -> Option<Self> {
        Some(Self {
            first: self.first.min(other.first),
            last: self.last.max(other.last),
            buy: self.buy.checked_add(other.buy)?,
            sell: self.sell.checked_add(other.sell)?,
            moment: self.moment.checked_add(other.moment)?,
            count: self.count.checked_add(other.count)?,
        })
    }
}

/// Sum each supplied trade-built ladder into its group of
/// `view.candles_per_mark` candles (`slot / k`) before filtering the group's
/// weighted center. A row's lower bound is an execution price only when its
/// grid is exactly aligned with the observed native grid. Coarsened and
/// approximated ladders stay out rather than inventing a candle's price
/// moment, and a group whose sums overflow draws nothing rather than part of
/// itself.
///
/// The cap is half the group's width, so neighbouring whole groups never
/// overlap; area is the group's volume against the largest visible mark's.
/// A mark whose area-true radius falls under [`MIN_DOT_RADIUS_PX`] is lifted
/// to it so a traded group never vanishes: ratios between marks at or above
/// that floor stay exact.
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
    let candles = view.candles_per_mark;
    if grid.step <= Decimal::ZERO
        || !view.candle_width_px.is_finite()
        || view.candle_width_px <= 0.0
        || view.prices.high <= view.prices.low
        || candles == 0
    {
        return frame;
    }
    // One mark occupies its group's columns, so native price-row spacing
    // cannot make a wide-range group's single mark disappear.
    let maximum = CANDLE_MARK_MAX_RADIUS_PX.min(view.candle_width_px * candles as f32 / 2.0);
    let mut groups: BTreeMap<usize, Option<GroupSums>> = BTreeMap::new();
    for input in inputs {
        if input.source != CandleFootprintSource::TradeBuilt || !grid.preserves_prices(input.ladder)
        {
            continue;
        }
        let Some(sums) = GroupSums::of(input.slot, input.ladder) else {
            continue;
        };
        groups
            .entry(input.slot / candles)
            .and_modify(|group| *group = group.and_then(|group| group.add(sums)))
            .or_insert(Some(sums));
    }
    let owned = view.input_slots();
    // Groups leave the map in slot order, one mark each: the marks are
    // sorted by slot.
    for (group, sums) in groups {
        let Some(sums) = sums.filter(|_| owned.contains(&(group * candles))) else {
            continue;
        };
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
            slot: sums.first,
            last_slot: sums.last,
            price,
            buy_quantity: sums.buy,
            sell_quantity: sums.sell,
            trade_count: sums.count,
            radius_px: maximum,
        });
    }
    frame.full_quantity = frame
        .marks
        .iter()
        .map(|dot| dot.buy_quantity.saturating_add(dot.sell_quantity))
        .max()
        .unwrap_or(Decimal::ONE);
    let floor = MIN_DOT_RADIUS_PX.min(maximum);
    for dot in &mut frame.marks {
        let size = normalized_area_size(
            dot.buy_quantity.saturating_add(dot.sell_quantity),
            frame.full_quantity,
        );
        dot.radius_px = (dot.radius_px * size).max(floor);
    }
    frame
}
