//! The price grid a tape actually prints on: the largest step dividing the distance between any two
//! prices shown, as a running GCD. It only narrows; an off-grid print narrows it, never snapped or
//! rejected. One print, or only repeated prices, shows no distance and the answer stays [`None`].

use rust_decimal::Decimal;

/// How many distinct steps the tape has to show before the grid is reported. One step is weak
/// grounds (two prints ten apart on a five-tick instrument name ten). Too coarse costs detail, too
/// fine is the defect avoided; each narrowing costs a regroup, so a few prints are spent.
const STEPS_BEFORE_REPORTING: u32 = 8;

/// The largest price step consistent with every print observed so far; see the [module docs](self).
/// Cheap for the per-trade path: one comparison, and Euclid only when a print is off the grid.
#[derive(Debug, Clone, Default)]
pub struct PriceGrid {
    /// The running GCD of the distances seen, once there has been one.
    step: Option<Decimal>,
    /// The previous print, to measure the next distance from.
    previous: Option<Decimal>,
    /// How many non-zero distances have been folded in.
    steps_seen: u32,
}

impl PriceGrid {
    /// A grid that has seen nothing and claims nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one print's price in. The converged grid is order-independent; when it is first
    /// reported is not: evidence is the distance between consecutive prints, so
    /// [`steps_seen`](Self::steps_seen) counts distances, not prints.
    pub fn observe(&mut self, price: Decimal) {
        let Some(previous) = self.previous.replace(price) else {
            return;
        };
        let distance = (price - previous).abs();
        if distance.is_zero() {
            // The same price twice shows no distance, so it is no evidence
            // about the grid — not even weak evidence.
            return;
        }
        self.steps_seen = self.steps_seen.saturating_add(1);
        match self.step {
            // Settled case, nearly every live print: the distance is already a multiple of the
            // grid, so one modulo is the entire cost.
            Some(step) if distance.checked_rem(step).is_some_and(|r| r.is_zero()) => {}
            // Off the grid: the answer was too coarse. Normalized here, not on every read. A GCD
            // that cannot finish leaves the previous answer standing, never a finer one nothing
            // proved.
            Some(step) => {
                if let Some(narrowed) = gcd(step, distance) {
                    self.step = Some(narrowed.normalize());
                }
            }
            None => self.step = Some(distance.normalize()),
        }
    }

    /// The grid once the tape has shown enough to name one; [`None`] means the tape has not said.
    /// Already normalized (a half-point grid reads `0.5`); normalized where it changes, not per
    /// read.
    #[must_use]
    pub fn step(&self) -> Option<Decimal> {
        self.step
            .filter(|_| self.steps_seen >= STEPS_BEFORE_REPORTING)
    }

    /// How many distances have been folded in — what the answer rests on.
    #[must_use]
    pub fn steps_seen(&self) -> u32 {
        self.steps_seen
    }
}

/// Greatest common divisor of two positive decimals by Euclid, or [`None`] when `Decimal` cannot
/// finish (`checked_rem` fails or the step cap is hit). It must not invent a grid: the caller keeps
/// its previous answer.
fn gcd(a: Decimal, b: Decimal) -> Option<Decimal> {
    let (mut a, mut b) = if a >= b { (a, b) } else { (b, a) };
    for _ in 0..GCD_STEPS_CAP {
        if b.is_zero() {
            return Some(a);
        }
        let remainder = a.checked_rem(b)?;
        a = b;
        b = remainder;
    }
    None
}

/// What one print may spend on Euclid. Below the worst case for two full 28-digit mantissas (about
/// 134 steps): a real grid converges in a handful. Exceeding it keeps the previous answer, bounding
/// the per-trade cost.
const GCD_STEPS_CAP: u32 = 64;

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn dec(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }

    /// Fold a whole tape of prices in one call.
    fn grid_of(prices: &[&str]) -> PriceGrid {
        let mut grid = PriceGrid::new();
        for price in prices {
            grid.observe(dec(price));
        }
        grid
    }

    #[test]
    fn a_grid_needs_a_distance_before_it_names_anything() {
        assert_eq!(PriceGrid::new().step(), None);
        assert_eq!(grid_of(&["100"]).step(), None);
        // The same print ten times shows no distance at all.
        assert_eq!(grid_of(&["100"; 10]).step(), None);
    }

    #[test]
    fn a_five_tick_instrument_is_named_five() {
        // WINV26's own shape: every print a multiple of five, and the
        // distances between them a mix of one, two and three ticks.
        let grid = grid_of(&[
            "174565", "174570", "174560", "174570", "174585", "174580", "174570", "174575",
            "174590",
        ]);
        assert_eq!(grid.step(), Some(dec("5")));
    }

    #[test]
    fn a_sub_unit_grid_is_named_exactly() {
        // WDOU26 trades in half-points.
        let grid = grid_of(&[
            "5216.0", "5216.5", "5217.0", "5216.5", "5215.5", "5217.5", "5218.0", "5217.0",
            "5216.0",
        ]);
        assert_eq!(grid.step(), Some(dec("0.5")));
        // Normalized, because this number reaches the trader's status line:
        // a half-point grid must read `0.5`, not `0.500`.
        assert_eq!(grid.step().unwrap().to_string(), "0.5");
    }

    #[test]
    fn the_answer_only_ever_narrows() {
        let mut grid = PriceGrid::new();
        let mut named: Vec<Option<Decimal>> = Vec::new();
        // Ten-point steps first, so the early answer is ten; then a five-point
        // step proves it was too coarse.
        for price in [
            "100", "110", "120", "130", "140", "150", "160", "170", "180", "185",
        ] {
            grid.observe(dec(price));
            named.push(grid.step());
        }
        let reported: Vec<Decimal> = named.into_iter().flatten().collect();
        assert_eq!(reported.first(), Some(&dec("10")));
        assert_eq!(reported.last(), Some(&dec("5")));
        assert!(
            reported.windows(2).all(|pair| pair[1] <= pair[0]),
            "the grid widened: {reported:?}",
        );
    }

    #[test]
    fn one_print_off_the_grid_narrows_it_rather_than_being_snapped_on() {
        // Nine clean five-point prints name five. A single print a point off
        // the grid is the tape saying the grid was never five — the answer
        // follows the tape rather than defending itself.
        let mut grid = grid_of(&[
            "100", "105", "110", "115", "120", "125", "130", "135", "140",
        ]);
        assert_eq!(grid.step(), Some(dec("5")));
        grid.observe(dec("141"));
        assert_eq!(grid.step(), Some(dec("1")));
    }

    #[test]
    fn the_order_prints_arrive_in_does_not_change_the_grid() {
        // Reversing a tape preserves every consecutive distance and their count, so it guards
        // nothing. These are two genuinely different walks over the same prices.
        let one = grid_of(&[
            "100", "115", "160", "110", "190", "135", "200", "165", "140", "185",
        ]);
        let other = grid_of(&[
            "165", "100", "200", "140", "115", "185", "110", "160", "190", "135",
        ]);
        assert_eq!(one.step(), other.step());
        assert_eq!(one.step(), Some(dec("5")));
    }

    #[test]
    fn seven_distances_name_nothing_and_the_eighth_names_the_grid() {
        // This test pins `STEPS_BEFORE_REPORTING`, so the counts are written out, not derived:
        // against the constant it would hold at any value, including 1. Changing the constant means
        // changing these numbers.
        let mut grid = PriceGrid::new();
        for price in ["100", "105", "110", "115", "120", "125", "130", "135"] {
            grid.observe(dec(price));
        }
        assert_eq!(grid.steps_seen(), 7);
        assert_eq!(
            grid.step(),
            None,
            "named a grid on seven distances, one short of the threshold",
        );

        grid.observe(dec("140"));
        assert_eq!(grid.steps_seen(), 8);
        assert_eq!(grid.step(), Some(dec("5")));
    }

    #[test]
    fn evidence_is_counted_in_distances_not_in_prints() {
        // A run of identical prints is a long tape and no evidence.
        let grid = grid_of(&["100"; 50]);
        assert_eq!(grid.steps_seen(), 0);
        assert_eq!(grid.step(), None);
    }

    #[test]
    fn a_grid_finer_than_the_prints_is_never_invented() {
        // Every distance here is a multiple of 25; nothing licenses a finer
        // answer, so nothing finer is reported.
        let grid = grid_of(&[
            "1000", "1025", "1075", "1050", "1100", "1150", "1125", "1200", "1175",
        ]);
        assert_eq!(grid.step(), Some(dec("25")));
    }

    #[test]
    fn gcd_of_exact_decimals_is_exact() {
        assert_eq!(gcd(dec("5"), dec("5")), Some(dec("5")));
        assert_eq!(gcd(dec("10"), dec("15")), Some(dec("5")));
        assert_eq!(gcd(dec("0.5"), dec("1.5")), Some(dec("0.5")));
        assert_eq!(gcd(dec("0.25"), dec("0.1")), Some(dec("0.05")));
        assert_eq!(gcd(dec("7"), dec("13")), Some(dec("1")));
    }
}
