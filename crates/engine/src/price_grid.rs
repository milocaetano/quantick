//! The price grid a tape actually prints on.
//!
//! An instrument trades in whole ticks, and a ladder grouped finer than that
//! tick is mostly rows nothing can ever land in. The venue knows the tick, but
//! it does not always say so: on this platform the number arrives only inside
//! a depth snapshot, so a market replay or a feed without L2 leaves the chart
//! guessing. The tape itself is not guessing — every print lands on the grid,
//! so the grid is the largest step that divides the distance between any two
//! prices the tape has shown.
//!
//! [`PriceGrid`] folds that answer one print at a time. It is a plain running
//! greatest common divisor, which gives it the two properties this has to have:
//!
//! - It only ever **narrows**. A new print either already sits on the current
//!   answer, or proves the grid is finer than the answer claimed. It never
//!   widens, so it settles rather than oscillating, and a consumer paying to
//!   regroup pays a bounded number of times.
//! - It is **honest about being wrong**. A print off the current grid is not
//!   snapped onto it and not rejected; it narrows the answer, because the tape
//!   is the evidence and the answer was the guess. A venue that really does
//!   print half-ticks says so this way, in one print.
//!
//! What it never does is claim a grid it has no grounds for. One print, or a
//! run of prints all at the same price, shows no distance at all and the answer
//! stays [`None`].

use rust_decimal::Decimal;

/// How many distinct steps the tape has to show before the grid is reported.
///
/// One step is grounds for an answer but weak grounds: two prints ten apart on
/// a five-tick instrument name ten, and only the third print can narrow it. The
/// answer is always safe — too coarse costs detail, too fine is the defect this
/// exists to prevent — but every narrowing costs a consumer a regroup, so it is
/// worth spending a handful of prints to skip the noisiest ones. Small enough
/// that a quiet open still reaches it in seconds.
const STEPS_BEFORE_REPORTING: u32 = 8;

/// The largest price step consistent with every print observed so far.
///
/// See the [module docs](self) for why this is a running GCD and what that
/// buys. Cheap enough for the per-trade path: one comparison and, only when a
/// print is off the current grid, a Euclid that runs on exact decimals.
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

    /// Fold one print's price in.
    ///
    /// The grid this converges on does not depend on the order prints arrive
    /// in — a GCD does not care. *When* it is first reported does: the evidence
    /// counted is the distance between one print and the next, so a tape that
    /// repeats a price ten times before moving has shown one distance, not ten,
    /// and the same prices in another order would have shown more. That is the
    /// tape being quiet rather than the answer being unstable, and it is why
    /// [`steps_seen`](Self::steps_seen) counts distances rather than prints.
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
            // The settled case, and on a live tape almost every print: the
            // distance is already a whole number of the grid we named, so the
            // answer cannot move and one modulo is the entire cost. Running
            // Euclid here instead would spend several on the per-trade path to
            // arrive back where it started.
            Some(step) if distance.checked_rem(step).is_some_and(|r| r.is_zero()) => {}
            // Off the grid: the tape has just proved the answer too coarse.
            // Normalized here rather than on every read, because a reader on
            // the per-trade path asks far more often than this changes. A GCD
            // this build cannot finish leaves the previous answer standing —
            // never a finer one nothing proved.
            Some(step) => {
                if let Some(narrowed) = gcd(step, distance) {
                    self.step = Some(narrowed.normalize());
                }
            }
            None => self.step = Some(distance.normalize()),
        }
    }

    /// The grid, once the tape has shown enough of it to name one.
    ///
    /// [`None`] while the evidence is a single distance or none at all — a
    /// caller that gets `None` has learned that the tape has not said, which is
    /// different from learning that the grid is fine.
    ///
    /// Already normalized, so a half-point grid reads `0.5` rather than
    /// carrying whatever scale the arithmetic left on it — this number reaches
    /// the trader on the ladder's status line. Normalizing happens where the
    /// answer changes, not here: this is read once per trade and changes a
    /// handful of times a session.
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

/// Greatest common divisor of two positive decimals, by Euclid, or [`None`]
/// when this build of `Decimal` cannot finish the job.
///
/// Two ways it gives up, and both return nothing rather than a number:
///
/// - **The remainder does not fit.** `Decimal`'s `%` *panics* when the dividend
///   cannot be rescaled to the divisor's scale, and a tick file is text a human
///   or another tool can write, so a price with twenty-five decimal places
///   beside an index-magnitude one is reachable input. A panic here would be a
///   panic on the per-trade path; `checked_rem` makes it an answer instead.
/// - **The sequence is longer than the cap.** The cap bounds what one print may
///   cost, and it sits below Euclid's worst case on a full mantissa on purpose.
///
/// Giving up must not invent a grid. The loop's `a` at that point is a
/// mid-sequence remainder that divides neither argument, and installing it
/// would be exactly the too-fine answer this module exists to prevent — so the
/// caller keeps what it had.
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

/// What one print may spend on Euclid.
///
/// Deliberately below the worst case for two full 28-digit mantissas (Lamé's
/// bound puts that near 134 steps): a real price grid converges in a handful,
/// and a pair that needs more is adversarial rather than a market. Exceeding it
/// keeps the previous answer, so the cap costs precision on input no instrument
/// produces and buys a bounded cost on the path every trade takes.
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
        // Reversing a tape is not a shuffle: it preserves every consecutive
        // distance and their count, so a test that only reverses passes for
        // any input at all and guards nothing. These are two genuinely
        // different walks over the same set of prices.
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
        // This test is where `STEPS_BEFORE_REPORTING` is pinned, so the counts
        // are written out rather than derived from it. Phrased against the
        // constant it would hold at any value — including 1, which is the
        // threshold not existing — and a gate no test can fail is decoration.
        // Changing the constant deliberately means changing these numbers.
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
