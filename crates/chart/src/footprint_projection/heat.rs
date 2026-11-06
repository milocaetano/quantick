//! Percentile cuts for visible footprint display rows.
pub use quantick_appearance::heat::HEAT_STEP_COUNT;
use quantick_engine::FootprintLevel;
use rust_decimal::{Decimal, prelude::ToPrimitive as _};
use std::collections::BTreeMap;

/// The heat ramp's scale: where each step's boundary falls, in quantity, for
/// the ladders currently on screen.
///
/// **Ranks, not ratios.** Dividing a cell by a fixed reference sounds right
/// and is not: per-cell volume is heavily skewed and the shape of that skew
/// changes with the market, so one denominator paints every cell on the floor
/// in a quiet stretch and saturates half of them in a busy one. Measured on a
/// real capture, ratio-to-p95 put **47% of cells in the top step** — the
/// brightest colour on screen was also the most common one, which leaves
/// nothing for it to stand out against.
///
/// Cutting the visible distribution at fixed *percentiles* fixes both ends by
/// construction: the busiest cells are always the top step and the quiet ones
/// always the floor, whatever the regime. The cuts are uneven on purpose —
/// most rows are ordinary, so the ramp spends its bright steps on the tail
/// that is worth seeing.
///
/// Visible ladders, not the newest N of the series: the denominator has to
/// describe what the trader is looking at. Reading the series instead made
/// the colours depend on where the replay's live edge happened to be.
const HEAT_PERCENTILES: [usize; 5] = [45, 68, 83, 93, 98];

/// One step's lower bound in quantity, ascending.  when there is
/// nothing on screen to measure — a ramp with an invented scale is a colour
/// key that means whatever it likes.
pub type HeatScale = [f64; 5];

pub fn heat_scale<'a>(
    rows: impl Iterator<Item = &'a BTreeMap<i64, FootprintLevel>>,
) -> Option<HeatScale> {
    let mut sides: Vec<f64> = rows
        .flat_map(BTreeMap::values)
        .flat_map(|level| {
            [
                level.buy.to_f64().unwrap_or(0.0),
                level.sell.to_f64().unwrap_or(0.0),
            ]
        })
        .filter(|volume| *volume > 0.0)
        .collect();
    // Display rows, not capture buckets. Cutting the raw grid and colouring
    // the merged one is a scale for a different chart: a drawn cell is the sum
    // of `k` buckets, so the same cut lands at a different place in the
    // distribution at every zoom and every instrument tick.
    if sides.is_empty() {
        return None;
    }
    sides.sort_by(f64::total_cmp);
    let last = sides.len() - 1;
    Some(HEAT_PERCENTILES.map(|pct| sides[last * pct / 100]))
}

/// Which heat step a quantity falls in. `reference` absent (no closed bars
/// yet) puts everything on the floor rather than inventing a scale.
pub fn heat_step(qty: Decimal, scale: Option<HeatScale>) -> usize {
    let Some(scale) = scale else { return 0 };
    let value = qty.to_f64().unwrap_or(0.0);
    // One past the last boundary it clears: below every cut is the floor.
    scale.iter().filter(|cut| value >= **cut).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint_projection::regroup;
    use quantick_engine::{DEFAULT_LEVEL_CAP, FootprintBuilder, Side, Trade};
    use rust_decimal::prelude::FromPrimitive as _;
    /// One closed ladder holding `quantities`, one per price level — the
    /// shape the heat scale reads, built through the engine rather than by
    /// hand so the test cannot drift from what the app actually folds.
    fn ladder_of(quantities: &[Decimal]) -> quantick_engine::BarFootprint {
        let mut builder = FootprintBuilder::new(Decimal::ONE, DEFAULT_LEVEL_CAP);
        for (i, qty) in quantities.iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: Decimal::from(1_000 + i as i64),
                quantity: *qty,
                side: Side::Buy,
            });
        }
        builder.close().expect("a closed ladder")
    }

    /// The ramp spreads its steps over whatever distribution is on screen.
    ///
    /// This is the claim ranks buy over ratios, and it is worth an assertion
    /// because both failure modes shipped once. Measured on a capture, a fixed
    /// denominator put 47% of cells in the top step — the brightest colour on
    /// screen was also the most common, so nothing stood out against anything
    /// — and an earlier reference put nearly all of them on the floor.
    ///
    /// Per-cell volume is heavily skewed, so the test feeds a skewed
    /// distribution rather than a flat one: a ramp that only behaves on
    /// uniform data would prove nothing about a tape.
    #[test]
    fn the_heat_ramp_spreads_over_a_skewed_distribution() {
        // A long tail: most rows ordinary, a handful enormous.
        let quantities: Vec<Decimal> = (1..=400)
            .map(|i| {
                let skewed = (f64::from(i) / 400.0).powf(4.0) * 5_000.0 + 1.0;
                Decimal::from_f64(skewed).unwrap_or(Decimal::ONE)
            })
            .collect();
        let ladder = ladder_of(&quantities);
        let scale = heat_scale(std::iter::once(&regroup(&ladder, 1))).expect("a scale");

        let mut population = [0_usize; HEAT_STEP_COUNT];
        for qty in &quantities {
            population[heat_step(*qty, Some(scale))] += 1;
        }
        let total: usize = population.iter().sum();
        assert_eq!(total, quantities.len());

        // Every step is used, and none of them swallows the screen. The top
        // step is deliberately the rarest — it is the one that has to mean
        // something when it appears.
        for (step, count) in population.iter().enumerate() {
            assert!(*count > 0, "step {step} is unreachable: {population:?}");
            let share = 100 * count / total;
            assert!(
                share <= 55,
                "step {step} holds {share}% of the screen: {population:?}"
            );
        }
        assert!(
            population[HEAT_STEP_COUNT - 1] < population[0],
            "the brightest step must be rarer than the floor: {population:?}"
        );
    }

    /// The cuts rise, so a bigger quantity never lands on a colder colour.
    #[test]
    fn the_heat_scale_is_monotonic() {
        let quantities: Vec<Decimal> = (1..=200).map(Decimal::from).collect();
        let ladder = ladder_of(&quantities);
        let scale = heat_scale(std::iter::once(&regroup(&ladder, 1))).expect("a scale");
        for pair in scale.windows(2) {
            assert!(pair[1] >= pair[0], "cuts not ascending: {scale:?}");
        }
        let mut previous = 0;
        for qty in &quantities {
            let step = heat_step(*qty, Some(scale));
            assert!(
                step >= previous,
                "step fell at {qty}: {step} after {previous}"
            );
            previous = step;
        }
    }

    /// Nothing on screen means no scale, and no scale means the floor — never
    /// a colour key invented from an empty set.
    #[test]
    fn an_empty_screen_has_no_heat_scale() {
        assert!(heat_scale(std::iter::empty()).is_none());
        assert_eq!(heat_step(Decimal::from(1_000), None), 0);
    }
}
