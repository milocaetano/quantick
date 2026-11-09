//! Price gridlines spaced by room on screen rather than by a fixed count.
//!
//! A fixed ask of eight lines spreads them over whatever height the chart
//! has, so a tall chart gets tall cells. TradingView instead keeps its price
//! lines a steady distance apart: the smallest round step whose lines stand
//! at least a given number of pixels apart, from 1, 2, 2.5, 4 and 5 times a
//! power of ten. Its cells come out about twice as wide as they are tall.

/// Multipliers of a power of ten a step may take, smallest first. Each is at
/// most twice the one before, so the spacing chosen stays under twice the floor.
const STEP_MANTISSAS: [f64; 5] = [1.0, 2.0, 2.5, 4.0, 5.0];

/// Most lines one call returns, whatever the inputs: a guard, not a policy.
const MAX_LINES: usize = 256;

/// The round price step whose lines stand at least `min_spacing_px` apart on
/// an axis `height_px` tall spanning `[lo, hi]`, or `None` for an empty or
/// degenerate axis.
#[must_use]
pub fn step(lo: f64, hi: f64, height_px: f32, min_spacing_px: f32) -> Option<f64> {
    let span = hi - lo;
    if !(span.is_finite() && span > 0.0 && height_px.is_finite() && height_px > 0.0) {
        return None;
    }
    if !(min_spacing_px.is_finite() && min_spacing_px > 0.0) {
        return None;
    }
    let least = span * f64::from(min_spacing_px) / f64::from(height_px);
    let decade = 10_f64.powf(least.log10().floor());
    [decade, decade * 10.0]
        .into_iter()
        .flat_map(|power| STEP_MANTISSAS.map(|mantissa| mantissa * power))
        .find(|candidate| *candidate >= least * (1.0 - 1e-9))
}

/// The prices of the lines: every multiple of [`step`] inside `[lo, hi]`.
/// Computed from the multiple's index, so no error accumulates up the axis.
#[must_use]
pub fn ticks(lo: f64, hi: f64, height_px: f32, min_spacing_px: f32) -> Vec<f64> {
    let Some(step) = step(lo, hi, height_px, min_spacing_px) else {
        return Vec::new();
    };
    let first = (lo / step).ceil();
    (0..MAX_LINES)
        .map(|index| (first + index as f64) * step)
        .take_while(|price| *price <= hi + step * 1e-9)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_is_the_smallest_round_one_that_keeps_the_floor() {
        // 800 px over 400 price units: 2 px per unit, so 40 px needs 20.
        assert_eq!(step(75_400.0, 75_800.0, 800.0, 40.0), Some(20.0));
        // 45 px needs 22.5: 25 is the next round step.
        assert_eq!(step(75_400.0, 75_800.0, 800.0, 45.0), Some(25.0));
        // 61 px needs 30.5: 40, the step TradingView's light chart shows.
        assert_eq!(step(75_400.0, 75_800.0, 800.0, 61.0), Some(40.0));
        // 81 px needs 40.5: 50.
        assert_eq!(step(75_400.0, 75_800.0, 800.0, 81.0), Some(50.0));
        // Fractions round the same way: 40 px needs 0.004, itself round.
        let fine = step(1.0, 1.1, 1000.0, 40.0).unwrap();
        assert!((fine - 0.004).abs() < 1e-12, "{fine}");
        let finer = step(1.0, 1.1, 1000.0, 41.0).unwrap();
        assert!((finer - 0.005).abs() < 1e-12, "{finer}");
    }

    #[test]
    fn lines_stand_between_one_and_two_floors_apart() {
        for height in [200.0_f32, 517.0, 800.0, 1333.0] {
            for span in [0.37, 12.0, 400.0, 9_999.0] {
                let step = step(100.0, 100.0 + span, height, 40.0).unwrap();
                let spacing = step / span * f64::from(height);
                assert!(
                    (40.0..80.0001).contains(&spacing),
                    "{height} {span}: {spacing}"
                );
            }
        }
    }

    #[test]
    fn ticks_are_exact_multiples_inside_the_range() {
        let ticks = ticks(75_401.3, 75_799.0, 800.0, 40.0);
        assert_eq!(ticks.first(), Some(&75_420.0));
        assert_eq!(ticks.last(), Some(&75_780.0));
        assert_eq!(ticks.len(), 19);
        assert!(ticks.windows(2).all(|pair| pair[1] - pair[0] == 20.0));
    }

    #[test]
    fn degenerate_axes_draw_nothing() {
        assert!(ticks(1.0, 1.0, 800.0, 40.0).is_empty());
        assert!(ticks(2.0, 1.0, 800.0, 40.0).is_empty());
        assert!(ticks(1.0, 2.0, 0.0, 40.0).is_empty());
        assert!(ticks(1.0, 2.0, 800.0, 0.0).is_empty());
        assert!(ticks(f64::NAN, 2.0, 800.0, 40.0).is_empty());
        assert!(ticks(1.0, 2.0, f32::INFINITY, 40.0).is_empty());
        assert!(ticks(0.0, 1e12, 1e9, 1e-9).len() <= MAX_LINES);
    }
}
