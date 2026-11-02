//! Configuration-owned footprint ramp. Rendering performs indexed lookups only.
use crate::{ColorValue, PremultipliedRgba, ThemeError};
use serde::{Deserialize, Serialize};

pub const HEAT_STEP_COUNT: usize = 6;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeatOverride {
    pub buy: Option<[ColorValue; HEAT_STEP_COUNT]>,
    pub sell: Option<[ColorValue; HEAT_STEP_COUNT]>,
    pub ink: Option<[ColorValue; HEAT_STEP_COUNT]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeatRamp {
    pub buy: [PremultipliedRgba; HEAT_STEP_COUNT],
    pub sell: [PremultipliedRgba; HEAT_STEP_COUNT],
    pub ink: [PremultipliedRgba; HEAT_STEP_COUNT],
}

impl HeatOverride {
    pub(crate) fn resolve(&self, fallback: Option<&HeatRamp>) -> Result<HeatRamp, ThemeError> {
        fn ramp(
            name: &str,
            values: Option<&[ColorValue; HEAT_STEP_COUNT]>,
            fallback: Option<[PremultipliedRgba; HEAT_STEP_COUNT]>,
        ) -> Result<[PremultipliedRgba; HEAT_STEP_COUNT], ThemeError> {
            let field = format!("footprint_heat.{name}");
            let Some(values) = values else {
                return fallback
                    .ok_or_else(|| ThemeError::new(field, "required in a complete theme"));
            };
            let mut resolved = [PremultipliedRgba::from_premultiplied([0; 4]); HEAT_STEP_COUNT];
            for (index, value) in values.iter().enumerate() {
                resolved[index] = value.resolve(&format!("{field}[{index}]"))?;
            }
            Ok(resolved)
        }
        Ok(HeatRamp {
            buy: ramp("buy", self.buy.as_ref(), fallback.map(|ramp| ramp.buy))?,
            sell: ramp("sell", self.sell.as_ref(), fallback.map(|ramp| ramp.sell))?,
            ink: ramp("ink", self.ink.as_ref(), fallback.map(|ramp| ramp.ink))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ResolvedTheme,
        color::RgbaExt,
        contrast::{contrast_ratio, relative_luminance},
        palette,
    };
    const AA: f32 = 4.5;
    const HEAT_INK_FLIP_STEP: usize = 4;
    const HEAT_INK_MUTED_BELOW_STEP: usize = 2;
    fn heat_fill(buy: bool, step: usize) -> PremultipliedRgba {
        let ramp = &ResolvedTheme::bundled_dark().footprint_heat;
        if buy { ramp.buy[step] } else { ramp.sell[step] }
    }
    fn heat_ink(step: usize) -> PremultipliedRgba {
        ResolvedTheme::bundled_dark().footprint_heat.ink[step]
    }
    /// Every step of the heat ramp is readable with the ink that step selects.
    ///
    /// This is the assertion the ramp was *designed backwards from*: the ink
    /// flip sits where it does because between the two inks lies a band of
    /// luminance neither can serve.
    #[test]
    fn every_heat_step_is_readable_with_the_ink_it_picks() {
        for step in 0..HEAT_STEP_COUNT {
            for side in [true, false] {
                let fill = heat_fill(side, step);
                let ratio = contrast_ratio(heat_ink(step), fill);
                assert!(
                    ratio >= AA,
                    "step {step} on {side:?} ({fill:?}) with its ink: {ratio:.2}:1"
                );
            }
        }
    }

    /// Both ink boundaries are forced, not chosen.
    ///
    /// The ink rule is three-valued — muted, primary, dark — and boundaries in
    /// a rule are only honest when something makes them fall where they do.
    /// Maximum contrast is deliberately *not* the goal: a quiet cell keeps a
    /// quiet number, so the ramp and the digits agree instead of arguing. What
    /// has to hold is that neither boundary could move without breaking AA,
    /// which is what makes them arithmetic rather than taste.
    #[test]
    fn both_ink_boundaries_are_forced_by_contrast() {
        for side in [true, false] {
            // Dark ink is unusable below its flip, and the only usable one at
            // and above it — so the boundary sits exactly where it must.
            for step in 0..HEAT_INK_FLIP_STEP {
                let fill = heat_fill(side, step);
                assert!(
                    contrast_ratio(palette::CHIP_INK, fill) < AA,
                    "step {step} on {side:?}: dark ink would already work, so the flip is late"
                );
            }
            for step in HEAT_INK_FLIP_STEP..HEAT_STEP_COUNT {
                let fill = heat_fill(side, step);
                assert!(
                    contrast_ratio(palette::TEXT_PRIMARY, fill) < AA,
                    "step {step} on {side:?}: light ink still works, so the flip is early"
                );
            }
            // And muted ink runs out exactly where the ramp stops using it:
            // that is why step 1 sits at L* 22 rather than anywhere brighter.
            let last_muted = heat_fill(side, HEAT_INK_MUTED_BELOW_STEP - 1);
            let first_primary = heat_fill(side, HEAT_INK_MUTED_BELOW_STEP);
            assert!(
                contrast_ratio(palette::TEXT_MUTED, last_muted) >= AA,
                "{side:?}: the last muted step is already unreadable"
            );
            assert!(
                contrast_ratio(palette::TEXT_MUTED, first_primary) < AA,
                "{side:?}: muted ink would still work one step further up"
            );
        }
    }

    /// The heat ramp never reaches the hues the app reserves.
    ///
    /// [`palette::AMBER`] means \"not live\" and [`palette::POC`] is a line inside
    /// these very candles. The ramp heats toward orange deliberately — that is
    /// where the reference charts get their warmth — and orange is on the far
    /// side of red, not next door to yellow. This pins the distance so a later
    /// \"make the top a bit warmer\" cannot quietly collide with either.
    #[test]
    fn the_heat_ramp_stays_clear_of_the_reserved_hues() {
        let hue = |color: PremultipliedRgba| -> f32 {
            let (r, g, b) = (
                f32::from(color.r()) / 255.0,
                f32::from(color.g()) / 255.0,
                f32::from(color.b()) / 255.0,
            );
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let span = max - min;
            if span <= f32::EPSILON {
                return 0.0;
            }
            let h = if max == r {
                60.0 * (((g - b) / span) % 6.0)
            } else if max == g {
                60.0 * ((b - r) / span + 2.0)
            } else {
                60.0 * ((r - g) / span + 4.0)
            };
            (h + 360.0) % 360.0
        };
        let separation = |a: f32, b: f32| -> f32 {
            let d = (a - b).abs() % 360.0;
            d.min(360.0 - d)
        };
        const MIN_SEPARATION_DEG: f32 = 25.0;
        for reserved in [palette::AMBER, palette::POC] {
            let reserved_hue = hue(reserved);
            for step in 0..HEAT_STEP_COUNT {
                for side in [true, false] {
                    let fill = heat_fill(side, step);
                    let gap = separation(hue(fill), reserved_hue);
                    assert!(
                        gap >= MIN_SEPARATION_DEG,
                        "step {step} on {side:?} sits {gap:.0}° from a reserved hue"
                    );
                }
            }
        }
    }

    /// The heat ramp is ordered, and its two sides are isoluminant.
    ///
    /// Isoluminance is deliberate: under deuteranopia the two hues collapse,
    /// and what still separates bid from ask is the *position* of the column.
    /// That trade only holds while luminance carries the ordinal reading — so
    /// the ordering is the assertion, and the columns may never swap places.
    #[test]
    fn the_heat_ramp_is_ordered_and_isoluminant() {
        for side in [true, false] {
            for step in 1..HEAT_STEP_COUNT {
                let previous = relative_luminance(heat_fill(side, step - 1));
                let current = relative_luminance(heat_fill(side, step));
                assert!(
                    current > previous,
                    "{side:?} step {step} is not brighter than {}",
                    step - 1
                );
            }
        }
        for step in 0..HEAT_STEP_COUNT {
            let buy = relative_luminance(heat_fill(true, step));
            let sell = relative_luminance(heat_fill(false, step));
            assert!(
                (buy - sell).abs() < 0.02,
                "step {step}: buy L={buy:.3} vs sell L={sell:.3} — the sides must weigh the same"
            );
        }
    }
    #[test]
    fn light_heat_has_readable_ink_on_every_step() {
        let theme = ResolvedTheme::parse(crate::LIGHT).unwrap();
        for ramp in [theme.footprint_heat.buy, theme.footprint_heat.sell] {
            for (fill, ink) in ramp.into_iter().zip(theme.footprint_heat.ink) {
                assert!(contrast_ratio(fill, ink) >= AA, "{fill:?} with {ink:?}");
            }
        }
    }
}
