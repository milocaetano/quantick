//! Peak spheres: a zoom region's volume-by-price profile, smoothed, divided
//! at the valleys between its significant peaks, one sphere per peak.
//!
//! The profile is read on the native price grid the region's own prints
//! name (the greatest common step between their prices), smoothed by an
//! exact integer Gaussian ([`PEAK_SMOOTHING_KERNEL`]). A peak counts when its
//! prominence — how far it rises above the higher of its two saddles — is at
//! least [`PEAK_PROMINENCE_PERCENT`] of the region's highest smoothed row.
//! Neighbouring peaks are cut apart at the lowest row between them; each
//! segment's prints become one sphere at their volume-weighted price and
//! place. Nothing here reads the camera, so a pan or an axis fit never
//! regroups a mass: only the zoom rung that chooses the region does.
use super::constants::{
    PEAK_BALANCED_STRENGTH, PEAK_LABEL_RANK, PEAK_PROFILE_MAX_ROWS, PEAK_PROMINENCE_PERCENT,
    PEAK_RADIUS_SHARES, PEAK_RANK_STOPS, PEAK_SMOOTHING_KERNEL,
};
use super::{FlowTapeDot, FlowTapeFrame, FlowTapeView, Group, MergeDisc as _};
use rust_decimal::{Decimal, prelude::ToPrimitive as _};

/// Empty rows kept on each side of a profile, so the smoothing's tails stay
/// whole and a hill at the region's highest or lowest price is still a hill.
const PAD_ROWS: usize = PEAK_SMOOTHING_KERNEL.len() - 1;

/// One region window's native cells divided at the valleys of their
/// volume-by-price profile: one group per peak, lowest price first.
pub(super) fn divide_at_peaks(window: Vec<Group>) -> Vec<Group> {
    if window.len() < 2 {
        return window;
    }
    let prices: Vec<Decimal> = window.iter().map(price).collect();
    let grid = Grid::of(&prices);
    let rows: Vec<usize> = prices.iter().map(|price| grid.row(*price)).collect();
    let mut raw = vec![Decimal::ZERO; grid.rows];
    for (group, &row) in window.iter().zip(&rows) {
        raw[row] += group.quantity();
    }
    let cuts = valleys(&smooth(&raw));
    let mut segments: Vec<Option<Group>> = (0..=cuts.len()).map(|_| None).collect();
    for (group, row) in window.into_iter().zip(rows) {
        let segment = &mut segments[cuts.partition_point(|cut| *cut <= row)];
        match segment {
            Some(held) => held.absorb(group, f64::INFINITY),
            None => *segment = Some(group),
        }
    }
    segments.into_iter().flatten().collect()
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
fn price(group: &Group) -> Decimal {
    group.moment.price_quantity() / group.quantity()
}

/// The rows a region's prices fall on.
struct Grid {
    low: Decimal,
    step: Decimal,
    /// Rows including [`PAD_ROWS`] on each side.
    rows: usize,
}

impl Grid {
    /// The finest grid every price lies on: the greatest common step of the
    /// prices above the lowest, coarsened by a whole factor when the span
    /// would need more than [`PEAK_PROFILE_MAX_ROWS`] rows.
    fn of(prices: &[Decimal]) -> Self {
        let low = prices.iter().copied().min().unwrap_or_default();
        let high = prices.iter().copied().max().unwrap_or_default();
        let gcd = prices
            .iter()
            .fold(Decimal::ZERO, |step, price| gcd(step, *price - low));
        let mut step = if gcd > Decimal::ZERO {
            gcd
        } else {
            Decimal::ONE
        };
        let span = |step: Decimal| {
            ((high - low) / step)
                .floor()
                .to_usize()
                .unwrap_or(usize::MAX)
        };
        let needed = span(step).saturating_add(1);
        if needed > PEAK_PROFILE_MAX_ROWS {
            step *= Decimal::from(needed.div_ceil(PEAK_PROFILE_MAX_ROWS));
        }
        Self {
            low,
            step,
            rows: span(step).min(PEAK_PROFILE_MAX_ROWS - 1) + 1 + 2 * PAD_ROWS,
        }
    }

    fn row(&self, price: Decimal) -> usize {
        let row = ((price - self.low) / self.step).floor().to_usize();
        PAD_ROWS + row.unwrap_or_default().min(self.rows - 1 - 2 * PAD_ROWS)
    }
}

/// Euclid's greatest common divisor, exact on decimals.
fn gcd(mut a: Decimal, mut b: Decimal) -> Decimal {
    while b > Decimal::ZERO {
        (a, b) = (b, a % b);
    }
    a
}

/// `raw` convolved with the kernel, out of range read as empty. The weights
/// are integers, so every row is exact whatever order it is summed in.
fn smooth(raw: &[Decimal]) -> Vec<Decimal> {
    let weights = PEAK_SMOOTHING_KERNEL.map(Decimal::from);
    (0..raw.len())
        .map(|row| {
            let mut sum = weights[0] * raw[row];
            for (distance, weight) in weights.iter().enumerate().skip(1) {
                if let Some(below) = row.checked_sub(distance) {
                    sum += *weight * raw[below];
                }
                if let Some(above) = raw.get(row + distance) {
                    sum += *weight * *above;
                }
            }
            sum
        })
        .collect()
}

/// Where neighbouring significant peaks of `smoothed` are cut apart: the
/// first lowest row between each pair, as the first row of the upper part.
fn valleys(smoothed: &[Decimal]) -> Vec<usize> {
    let poc = smoothed.iter().copied().max().unwrap_or_default();
    let floor = poc * Decimal::from(PEAK_PROMINENCE_PERCENT);
    let peaks: Vec<usize> = local_maxima(smoothed)
        .into_iter()
        .filter(|&peak| prominence(smoothed, peak) * Decimal::ONE_HUNDRED >= floor)
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

/// The radius share [`PEAK_RANK_STOPS`] and [`PEAK_RADIUS_SHARES`] give `rank`.
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
    /// The frame's quantities, smallest first.
    fn sorted_quantities(&self) -> Vec<Decimal> {
        let mut sorted: Vec<_> = self.dots.iter().map(|dot| dot.mark.quantity).collect();
        sorted.sort_unstable();
        sorted
    }

    /// Under peak spheres, size every mass by its rank among the masses of
    /// the frame (those whose centres the painted clip and the axis hold),
    /// not by area against a reference: the median small, the rarest large,
    /// and a mass under a quarter of the largest still a dot. Ties share a
    /// rank, so equal masses draw equal.
    pub(super) fn size_peaks_by_rank(&mut self) {
        if !self.view.region.divides_at_peaks() {
            return;
        }
        let sorted = self.sorted_quantities();
        for dot in &mut self.dots {
            let share = radius_share(rank(&sorted, dot.mark.quantity));
            dot.mark.size = share;
            dot.radius = self.view.radius_limit * share;
            dot.opening_oversized = false;
        }
    }

    /// The least quantity that carries a label under peak spheres: the top
    /// fifth by rank ([`PEAK_LABEL_RANK`]). `None` when the frame does not
    /// divide its regions at peaks, or holds no mass.
    #[must_use]
    pub fn peak_label_floor(&self) -> Option<Decimal> {
        if !self.view.region.divides_at_peaks() {
            return None;
        }
        let sorted = self.sorted_quantities();
        sorted
            .iter()
            .copied()
            .find(|quantity| rank(&sorted, *quantity) >= PEAK_LABEL_RANK)
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
