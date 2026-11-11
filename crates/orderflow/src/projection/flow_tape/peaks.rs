//! Peak spheres: a zoom region's volume-by-price profile, smoothed, divided
//! at the valleys between its significant peaks, one sphere per peak.
//!
//! The profile's rows are the smallest power-of-two multiple of the tape's
//! tick (the engine's [`PriceGrid`](quantick_engine::PriceGrid)) at least
//! [`PEAK_SIGMA_RADIUS_SHARE`] of the look's full-size radius tall at the
//! held price scale, and it is smoothed by an exact integer Gaussian of one
//! row ([`PEAK_SMOOTHING_KERNEL`]). Two peaks closer on screen than about one
//! largest sphere's diameter therefore read as one mass. A peak counts when
//! its prominence is at least [`PEAK_PROMINENCE_PERCENT`] of the region's
//! highest smoothed ordinary row (the opening burst stays out of that
//! reference). Neighbouring peaks are cut apart at the lowest row between
//! them; each segment's prints become one sphere at their volume-weighted
//! price and place, sized by its rank among the masses in view
//! ([`FlowScaleBasis::RankedVisibleMasses`]). The cuts read the held price
//! scale and the data, never the clip or the axis' offset, so a pan never
//! regroups a mass.
use super::constants::{
    PEAK_BALANCED_STRENGTH, PEAK_LABEL_SHARE_DIVISOR, PEAK_MIN_RADIUS_PX, PEAK_PROFILE_MAX_ROWS,
    PEAK_PROMINENCE_PERCENT, PEAK_RADIUS_SHARES, PEAK_RANK_STOPS, PEAK_ROW_MAX_DOUBLINGS,
    PEAK_SIGMA_RADIUS_SHARE, PEAK_SMOOTHING_KERNEL,
};
use super::{FlowScaleBasis, FlowTapeDot, FlowTapeFrame, FlowTapeView, Group, MergeDisc as _};
use rust_decimal::{Decimal, prelude::ToPrimitive as _};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Empty rows kept on each side of a profile, so the smoothing's tails stay
/// whole and a hill at the region's highest or lowest price is still a hill.
const PAD_ROWS: usize = PEAK_SMOOTHING_KERNEL.len() - 1;

/// Each region window's cuts: the lowest price of every segment after the
/// first.
pub(super) type PeakCutsByWindow = BTreeMap<i64, Vec<Decimal>>;

/// How a view's peak regions are cut: the tape's tick, and the cuts a worker
/// kept per region window for the view's price step.
#[derive(Clone, Copy, Default)]
pub(super) struct PeakDivision<'a> {
    pub(super) tick: Option<Decimal>,
    pub(super) kept: Option<&'a PeakCutsByWindow>,
}

impl PeakDivision<'_> {
    /// Region window `key`'s cells divided at the valleys of their profile:
    /// one group per peak, lowest price first.
    pub(super) fn divide(&self, key: i64, window: Vec<Group>, view: FlowTapeView) -> Vec<Group> {
        let cuts = match self.kept.and_then(|kept| kept.get(&key)) {
            Some(cuts) => Cow::Borrowed(cuts.as_slice()),
            None => Cow::Owned(peak_cuts(&window, view, self.tick)),
        };
        let mut segments: Vec<Option<Group>> = (0..=cuts.len()).map(|_| None).collect();
        for group in window {
            let at = price(&group).map_or(0, |price| cuts.partition_point(|cut| *cut <= price));
            let segment = &mut segments[at];
            match segment {
                Some(held) => held.absorb(group, f64::INFINITY),
                None => *segment = Some(group),
            }
        }
        segments.into_iter().flatten().collect()
    }
}

/// The key a worker's kept cuts hold for: the region, the tape's tick, the
/// profile row it resolves to (the raw row height only while the tape has
/// not named its tick), and the opening windows kept out of the prominence
/// reference. An axis fit that keeps the row keeps every cut.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PeakKey {
    region: super::FlowRegionSpan,
    tick: Option<Decimal>,
    row: Result<Decimal, u64>,
    opening_windows: Vec<i64>,
}

impl PeakKey {
    pub(super) fn of(view: FlowTapeView, tick: Option<Decimal>, opening_windows: &[i64]) -> Self {
        Self {
            region: view.region,
            tick,
            row: match tick.filter(|tick| *tick > Decimal::ZERO) {
                Some(tick) => Ok(row_step(tick, row_target(view))),
                None => Err(row_target(view).to_bits()),
            },
            opening_windows: opening_windows.to_vec(),
        }
    }
}

/// Where a window's cells are cut apart: the lowest price of each segment
/// after the first.
pub(super) fn peak_cuts(
    window: &[Group],
    view: FlowTapeView,
    tick: Option<Decimal>,
) -> Vec<Decimal> {
    let prices: Vec<Option<Decimal>> = window.iter().map(price).collect();
    let known = || prices.iter().flatten().copied();
    let (Some(low), Some(high)) = (known().min(), known().max()) else {
        return Vec::new();
    };
    let tick = tick
        .filter(|tick| *tick > Decimal::ZERO)
        .unwrap_or_else(|| finest_unit(known()));
    let Some(grid) = Grid::of(low, high, row_step(tick, row_target(view))) else {
        return Vec::new();
    };
    let mut raw = vec![Decimal::ZERO; grid.rows];
    let mut ordinary = vec![Decimal::ZERO; grid.rows];
    for (group, price) in window.iter().zip(&prices) {
        let row = price.map_or(PAD_ROWS, |price| grid.row(price));
        raw[row] = raw[row].saturating_add(group.quantity());
        let plain = group.quantity() - group.opening.min(group.quantity());
        ordinary[row] = ordinary[row].saturating_add(plain);
    }
    let smoothed = smooth(&raw);
    let reference = smooth(&ordinary)
        .into_iter()
        .max()
        .filter(|poc| *poc > Decimal::ZERO)
        .unwrap_or_else(|| smoothed.iter().copied().max().unwrap_or_default());
    valleys(&smoothed, reference)
        .into_iter()
        .filter_map(|row| grid.price_of(row))
        .collect()
}

/// Drop the divided masses the price axis does not show, answering their
/// executions. A peak region is divided whole before the axis reads it, so
/// an axis fit or a vertical pan never regroups the masses it keeps.
pub(super) fn drop_off_axis(groups: &mut Vec<Group>, view: FlowTapeView) -> usize {
    if !view.region.divides_at_peaks() {
        return 0;
    }
    let mut executions = 0;
    groups.retain(|group| {
        let inside = (0.0..=1.0).contains(&group.y());
        if !inside {
            executions += group.members.len();
        }
        inside
    });
    executions
}

/// A native cell's price: every print of a cell is at one price.
fn price(group: &Group) -> Option<Decimal> {
    group.moment.price_quantity().checked_div(group.quantity())
}

/// The finest step the prices' own digits can name, for a tape whose grid is
/// not known yet.
fn finest_unit(prices: impl Iterator<Item = Decimal>) -> Decimal {
    Decimal::new(1, prices.map(|price| price.scale()).max().unwrap_or(0))
}

/// The row height, in price units, the view asks for: a smoothing sigma of
/// [`PEAK_SIGMA_RADIUS_SHARE`] of the full-size radius at the view's held
/// price scale (`height_px` over the axis' span). Zero when unmeasurable.
fn row_target(view: FlowTapeView) -> f64 {
    let span = (view.prices.high - view.prices.low).to_f64().unwrap_or(0.0);
    let px_per_price = f64::from(view.height_px) / span;
    let target = PEAK_SIGMA_RADIUS_SHARE * f64::from(view.radius_limit) / px_per_price;
    if target.is_finite() && target > 0.0 {
        target
    } else {
        0.0
    }
}

/// The smallest power-of-two multiple of `tick` at least `target` tall.
fn row_step(tick: Decimal, target: f64) -> Decimal {
    let mut step = tick;
    for _ in 0..PEAK_ROW_MAX_DOUBLINGS {
        if step.to_f64().is_some_and(|step| step >= target) {
            break;
        }
        match step.checked_mul(Decimal::TWO) {
            Some(wider) => step = wider,
            None => break,
        }
    }
    step
}

/// The rows a region's prices fall on, anchored at whole multiples of the
/// step so the same price always lands in the same row.
struct Grid {
    /// The first row's index on the absolute grid.
    base: Decimal,
    step: Decimal,
    /// Rows including [`PAD_ROWS`] on each side.
    rows: usize,
}

impl Grid {
    /// The grid of `step` over `low..=high`, the step doubled while the span
    /// would need more than [`PEAK_PROFILE_MAX_ROWS`] rows.
    fn of(low: Decimal, high: Decimal, mut step: Decimal) -> Option<Self> {
        loop {
            let index = |price: Decimal| Some(price.checked_div(step)?.floor());
            let (base, top) = (index(low)?, index(high)?);
            let span = top.checked_sub(base)?.to_usize()?;
            if span < PEAK_PROFILE_MAX_ROWS {
                return Some(Self {
                    base,
                    step,
                    rows: span + 1 + 2 * PAD_ROWS,
                });
            }
            step = step.checked_mul(Decimal::TWO)?;
        }
    }

    fn row(&self, price: Decimal) -> usize {
        let row = price
            .checked_div(self.step)
            .and_then(|index| index.floor().checked_sub(self.base))
            .and_then(|row| row.to_usize())
            .unwrap_or(0);
        PAD_ROWS + row.min(self.rows - 1 - 2 * PAD_ROWS)
    }

    /// The lowest price of `row`.
    fn price_of(&self, row: usize) -> Option<Decimal> {
        let offset = Decimal::from(row.checked_sub(PAD_ROWS)?);
        self.base.checked_add(offset)?.checked_mul(self.step)
    }
}

/// `raw` convolved with the kernel, out of range read as empty: each occupied
/// row adds its weighted share to the rows within reach, so empty stretches
/// cost nothing. The weights are integers, so every row is exact.
fn smooth(raw: &[Decimal]) -> Vec<Decimal> {
    let weights = PEAK_SMOOTHING_KERNEL.map(Decimal::from);
    let reach = weights.len() - 1;
    let mut smoothed = vec![Decimal::ZERO; raw.len()];
    for (row, quantity) in raw.iter().enumerate() {
        if quantity.is_zero() {
            continue;
        }
        for target in row.saturating_sub(reach)..(row + reach + 1).min(raw.len()) {
            let weight = weights[target.abs_diff(row)];
            smoothed[target] = smoothed[target].saturating_add(weight.saturating_mul(*quantity));
        }
    }
    smoothed
}

/// Where neighbouring significant peaks of `smoothed` are cut apart: the
/// first lowest row between each pair, as the first row of the upper part.
/// A peak is significant when its prominence reaches
/// [`PEAK_PROMINENCE_PERCENT`] of `reference`.
fn valleys(smoothed: &[Decimal], reference: Decimal) -> Vec<usize> {
    let floor = reference.saturating_mul(Decimal::from(PEAK_PROMINENCE_PERCENT));
    let peaks: Vec<usize> = local_maxima(smoothed)
        .into_iter()
        .filter(|&peak| prominence(smoothed, peak).saturating_mul(Decimal::ONE_HUNDRED) >= floor)
        .collect();
    peaks
        .windows(2)
        .map(|pair| {
            let between = &smoothed[pair[0]..=pair[1]];
            let lowest = between.iter().copied().min().unwrap_or_default();
            pair[0]
                + between
                    .iter()
                    .position(|value| *value == lowest)
                    .unwrap_or(0)
        })
        .collect()
}

/// Rows higher than the row before them and not lower than any row up to
/// the next lower one: a flat top answers its middle row (left of centre).
fn local_maxima(values: &[Decimal]) -> Vec<usize> {
    let mut peaks = Vec::new();
    let last = values.len().saturating_sub(1);
    let mut row = 1;
    while row < last {
        if values[row - 1] < values[row] {
            let mut ahead = row + 1;
            while ahead < last && values[ahead] == values[row] {
                ahead += 1;
            }
            if values[ahead] < values[row] {
                peaks.push((row + ahead - 1) / 2);
                row = ahead;
            }
        }
        row += 1;
    }
    peaks
}

/// How far `peak` rises above the higher of its saddles: the lowest row on
/// each side of its flat top before higher ground, or the profile's end.
/// An equal top to the left counts as higher ground and one to the right
/// does not, so of twin tops only the first stands on its own: a shallow dip
/// between equal hills never divides them.
fn prominence(values: &[Decimal], peak: usize) -> Decimal {
    let height = values[peak];
    let mut left = peak;
    while left > 0 && values[left - 1] == height {
        left -= 1;
    }
    let mut right = peak;
    while right + 1 < values.len() && values[right + 1] == height {
        right += 1;
    }
    let lowest = |side: &mut dyn Iterator<Item = Decimal>| side.min().unwrap_or(height);
    let left = lowest(
        &mut values[..left]
            .iter()
            .rev()
            .copied()
            .take_while(|value| *value < height),
    );
    let right = lowest(
        &mut values[right + 1..]
            .iter()
            .copied()
            .take_while(|value| *value <= height),
    );
    height - left.max(right)
}

/// The share of `sorted` at or below `quantity`.
fn rank(sorted: &[Decimal], quantity: Decimal) -> f32 {
    sorted.partition_point(|held| *held <= quantity) as f32 / sorted.len().max(1) as f32
}

/// The share of the radius range [`PEAK_RANK_STOPS`] and
/// [`PEAK_RADIUS_SHARES`] give `rank`; linear between stops.
fn radius_share(rank: f32) -> f32 {
    let last = PEAK_RANK_STOPS.len() - 1;
    if rank >= PEAK_RANK_STOPS[last] {
        return PEAK_RADIUS_SHARES[last];
    }
    let at = PEAK_RANK_STOPS[1..]
        .iter()
        .position(|stop| rank <= *stop)
        .unwrap_or(last - 1);
    let (from, to) = (PEAK_RANK_STOPS[at], PEAK_RANK_STOPS[at + 1]);
    let t = ((rank - from) / (to - from)).clamp(0.0, 1.0);
    PEAK_RADIUS_SHARES[at] + t * (PEAK_RADIUS_SHARES[at + 1] - PEAK_RADIUS_SHARES[at])
}

impl FlowTapeFrame {
    /// The peak spheres' scale: every mass's radius is its rank among the
    /// frame's masses (those the painted clip and the axis hold), from
    /// [`PEAK_MIN_RADIUS_PX`] up to the full radius, ties sharing a rank so
    /// equal volume is an equal sphere. With the opening ignored, a mass
    /// holding the daily opening stays out of the population that defines the
    /// ranks but is drawn where its volume would rank, so it is never under
    /// the minimum and equal volume is still an equal sphere; one past the
    /// largest ranked mass keeps the opening's faint treatment. The frame's
    /// reference, basis and opening flag describe exactly this. Every
    /// finished frame is ranked once, before it is ordered for paint.
    pub(super) fn rank_peaks(&mut self) {
        if !self.view.region.divides_at_peaks() || self.dots.is_empty() {
            return;
        }
        let ignore = self.view.exclude_opening;
        let mut sorted: Vec<Decimal> = self
            .dots
            .iter()
            .filter(|dot| !(ignore && dot.opening_anchor))
            .map(|dot| dot.mark.quantity)
            .collect();
        // Openings alone in view are ranked among themselves.
        let excluding = !sorted.is_empty() && sorted.len() < self.dots.len();
        if sorted.is_empty() {
            sorted = self.dots.iter().map(|dot| dot.mark.quantity).collect();
        }
        sorted.sort_unstable();
        let largest = sorted[sorted.len() - 1];
        let labelled = (sorted.len() / PEAK_LABEL_SHARE_DIVISOR).max(1);
        let labelled_from = sorted[sorted.len() - labelled];
        let full = self.view.radius_limit;
        let least = PEAK_MIN_RADIUS_PX.min(full);
        for dot in &mut self.dots {
            let share = radius_share(rank(&sorted, dot.mark.quantity));
            dot.radius = full - (1.0 - share) * (full - least);
            dot.mark.size = if full > 0.0 { dot.radius / full } else { 0.0 };
            dot.opening_oversized = excluding && dot.opening_anchor && dot.mark.quantity > largest;
        }
        self.effective_reference = Some(largest);
        self.scale_basis = FlowScaleBasis::RankedVisibleMasses {
            masses: sorted.len(),
            labelled_from,
        };
        self.opening_exclusion_effective = excluding;
    }

    /// The least quantity that carries a label under peak spheres: the top
    /// fifth ([`PEAK_LABEL_SHARE_DIVISOR`]), as the ranking found it. `None`
    /// when the frame does not rank its masses.
    #[must_use]
    pub fn peak_label_floor(&self) -> Option<Decimal> {
        match self.scale_basis {
            FlowScaleBasis::RankedVisibleMasses { labelled_from, .. } => Some(labelled_from),
            _ => None,
        }
    }

    /// How strongly a peak sphere's fill is drawn: a balanced mass at
    /// [`PEAK_BALANCED_STRENGTH`], a one-sided one at full strength, linear in
    /// `|buy - sell| / (buy + sell)`. 1 when the frame does not divide its
    /// regions at peaks.
    #[must_use]
    pub fn peak_fill_strength(&self, dot: &FlowTapeDot) -> f32 {
        if !self.view.region.divides_at_peaks() || dot.mark.quantity <= Decimal::ZERO {
            return 1.0;
        }
        let sell = dot.mark.quantity - dot.mark.buy_quantity;
        let imbalance = ((dot.mark.buy_quantity - sell).abs() / dot.mark.quantity)
            .to_f32()
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        PEAK_BALANCED_STRENGTH * (1.0 - imbalance) + imbalance
    }
}
