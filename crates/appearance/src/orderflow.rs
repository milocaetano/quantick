//! Validated order-flow colors. RGB-only bubble and ramp boundaries reject alpha.
use crate::{ColorValue, ThemeError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderflowOverride {
    #[serde(default)]
    pub colors: BTreeMap<String, ColorValue>,
    pub ramp: Option<[ColorValue; 9]>,
    /// Where each ramp colour sits along the intensity, from 0 to 1: the
    /// first at 0, the last at 1, each past the one before.
    pub ramp_stops: Option<[f32; 9]>,
    /// An exponent over each cell's intensity before the ramp reads it, on top
    /// of the look's own gamma: below 1 lifts weak liquidity off a white ground.
    pub heat_gamma: Option<f32>,
    /// Cells at or above this intensity paint their ramp colour opaque, fading
    /// in linearly below it; left out, a cell's opacity follows its intensity,
    /// which suits a ramp that starts at the dark ground.
    pub heat_fade_below: Option<f32>,
}

/// Exact premultiplied palette bytes plus an opaque thermal ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderflowAppearance {
    #[serde(default = "legacy_crown_backing")]
    pub crown_backing: [u8; 4],
    #[serde(default = "legacy_separator")]
    pub separator: [u8; 4],
    pub buy: [u8; 4],
    pub sell: [u8; 4],
    pub consumption: [u8; 4],
    pub depth_only: [u8; 4],
    pub bubble_text: [u8; 4],
    pub gap_fill: [u8; 4],
    pub gap_boundary: [u8; 4],
    pub lane_now: [u8; 4],
    pub muted_text: [u8; 4],
    pub legend_text: [u8; 4],
    pub legend_background: [u8; 4],
    pub legend_border: [u8; 4],
    pub bid_tint: [u8; 4],
    pub ask_tint: [u8; 4],
    pub preview_grid: [u8; 4],
    pub preview_path: [u8; 4],
    pub ramp: [[u8; 3]; 9],
    /// Where each ramp colour sits, in thousandths of the intensity.
    #[serde(
        default = "legacy_ramp_stops",
        skip_serializing_if = "is_legacy_ramp_stops"
    )]
    pub ramp_stops: [u16; 9],
    /// Flat bubbles ringed in these rims, buy then sell; `None` keeps the
    /// look's own render mode and its side-coloured rims. Shading drawn for a
    /// dark ground muddies on white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flat_rims: Option<[[u8; 4]; 2]>,
    /// The palette's own heat gamma, in thousandths; `None` reads the ramp at
    /// the look's intensity unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat_gamma: Option<u16>,
    /// The intensity, in thousandths, from which a cell paints opaque; `None`
    /// keeps opacity proportional to intensity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat_fade_below: Option<u16>,
    /// Drawn for the surface it was resolved on: a pane's own palette choice,
    /// picked for another ground, yields to it. Never saved.
    #[serde(skip)]
    pub surface_bound: bool,
}

/// The positions every ramp had before themes could author them, in thousandths.
pub const LEGACY_RAMP_STOPS: [u16; 9] = [0, 90, 220, 380, 550, 700, 830, 930, 1000];
/// One whole intensity in the units [`OrderflowAppearance::ramp_stops`] counts.
pub const RAMP_STOP_UNIT: f32 = 1000.0;
/// The gentlest and the steepest heat gamma a palette may author: the range
/// the look's own gamma is held to.
pub const HEAT_GAMMA_MIN: f32 = 0.1;
pub const HEAT_GAMMA_MAX: f32 = 5.0;
/// The lowest and the highest intensity a palette may fade its heat in below.
pub const HEAT_FADE_BELOW_MIN: f32 = 0.01;
pub const HEAT_FADE_BELOW_MAX: f32 = 1.0;

const fn legacy_ramp_stops() -> [u16; 9] {
    LEGACY_RAMP_STOPS
}
fn is_legacy_ramp_stops(stops: &[u16; 9]) -> bool {
    *stops == LEGACY_RAMP_STOPS
}

/// Missing version-1 snapshot fields retain the historical decoration.
pub const fn legacy_crown_backing() -> [u8; 4] {
    [0, 0, 0, 110]
}
/// Missing version-1 snapshot fields retain the historical decoration.
pub const fn legacy_separator() -> [u8; 4] {
    [0, 0, 0, 170]
}

impl OrderflowOverride {
    pub(crate) fn resolve(
        &self,
        fallback: Option<&OrderflowAppearance>,
    ) -> Result<OrderflowAppearance, ThemeError> {
        let mut colors = self.colors.clone();
        let mut color = |key: &str, old: Option<[u8; 4]>, opaque: bool| {
            let field = format!("orderflow.colors.{key}");
            let bytes = match colors.remove(key) {
                Some(value) => value.resolve(&field)?.to_array(),
                None => {
                    old.ok_or_else(|| ThemeError::new(&field, "required in a complete theme"))?
                }
            };
            if opaque && bytes[3] != 255 {
                return Err(ThemeError::new(
                    field,
                    "must be opaque: bubble presets and thermal ramps use RGB",
                ));
            }
            Ok(bytes)
        };
        let mut resolved = OrderflowAppearance {
            crown_backing: color(
                "crown_backing",
                fallback.map(|old| old.crown_backing),
                false,
            )?,
            separator: color("separator", fallback.map(|old| old.separator), false)?,
            buy: color("buy", fallback.map(|old| old.buy), true)?,
            sell: color("sell", fallback.map(|old| old.sell), true)?,
            consumption: color("consumption", fallback.map(|old| old.consumption), true)?,
            depth_only: color("depth_only", fallback.map(|old| old.depth_only), false)?,
            bubble_text: color("bubble_text", fallback.map(|old| old.bubble_text), true)?,
            gap_fill: color("gap_fill", fallback.map(|old| old.gap_fill), false)?,
            gap_boundary: color("gap_boundary", fallback.map(|old| old.gap_boundary), false)?,
            lane_now: color("lane_now", fallback.map(|old| old.lane_now), false)?,
            muted_text: color("muted_text", fallback.map(|old| old.muted_text), false)?,
            legend_text: color("legend_text", fallback.map(|old| old.legend_text), false)?,
            legend_background: color(
                "legend_background",
                fallback.map(|old| old.legend_background),
                false,
            )?,
            legend_border: color(
                "legend_border",
                fallback.map(|old| old.legend_border),
                false,
            )?,
            bid_tint: color("bid_tint", fallback.map(|old| old.bid_tint), true)?,
            ask_tint: color("ask_tint", fallback.map(|old| old.ask_tint), true)?,
            preview_grid: color("preview_grid", fallback.map(|old| old.preview_grid), false)?,
            preview_path: color("preview_path", fallback.map(|old| old.preview_path), false)?,
            ramp: fallback.map_or([[0; 3]; 9], |old| old.ramp),
            ramp_stops: fallback.map_or(LEGACY_RAMP_STOPS, |old| old.ramp_stops),
            flat_rims: None,
            heat_gamma: fallback.and_then(|old| old.heat_gamma),
            heat_fade_below: fallback.and_then(|old| old.heat_fade_below),
            surface_bound: false,
        };
        resolved.flat_rims = match (colors.remove("buy_rim"), colors.remove("sell_rim")) {
            (Some(buy), Some(sell)) => Some([
                buy.resolve("orderflow.colors.buy_rim")?.to_array(),
                sell.resolve("orderflow.colors.sell_rim")?.to_array(),
            ]),
            (None, None) => fallback.and_then(|old| old.flat_rims),
            _ => {
                return Err(ThemeError::new(
                    "orderflow.colors.buy_rim",
                    "buy_rim and sell_rim are authored together",
                ));
            }
        };
        if let Some(key) = colors.keys().next() {
            return Err(ThemeError::new(
                format!("orderflow.colors.{key}"),
                "unknown semantic color",
            ));
        }
        match &self.ramp {
            Some(ramp) => {
                for (index, value) in ramp.iter().enumerate() {
                    let field = format!("orderflow.ramp[{index}]");
                    let [r, g, b, a] = value.resolve(&field)?.to_array();
                    if a != 255 {
                        return Err(ThemeError::new(
                            field,
                            "must be opaque: thermal ramps use RGB",
                        ));
                    }
                    resolved.ramp[index] = [r, g, b];
                }
            }
            None if fallback.is_none() => {
                return Err(ThemeError::new(
                    "orderflow.ramp",
                    "required in a complete theme",
                ));
            }
            None => {}
        }
        if let Some(stops) = &self.ramp_stops {
            resolved.ramp_stops = ramp_stops(stops)?;
        }
        if let Some(gamma) = self.heat_gamma {
            if !(gamma.is_finite() && (HEAT_GAMMA_MIN..=HEAT_GAMMA_MAX).contains(&gamma)) {
                return Err(heat_gamma_error());
            }
            // In range, so the rounded value fits.
            resolved.heat_gamma = Some((gamma * RAMP_STOP_UNIT).round() as u16);
        }
        if let Some(fade) = self.heat_fade_below {
            if !(fade.is_finite() && (HEAT_FADE_BELOW_MIN..=HEAT_FADE_BELOW_MAX).contains(&fade)) {
                return Err(heat_fade_below_error());
            }
            // In range, so the rounded value fits.
            resolved.heat_fade_below = Some((fade * RAMP_STOP_UNIT).round() as u16);
        }
        Ok(resolved)
    }
}

/// Authored positions in thousandths: 0 first, 1 last, each past the one before.
fn ramp_stops(stops: &[f32; 9]) -> Result<[u16; 9], ThemeError> {
    let mut thousandths = [0_u16; 9];
    for (slot, stop) in thousandths.iter_mut().zip(stops) {
        if !stop.is_finite() || !(0.0..=1.0).contains(stop) {
            return Err(ramp_stops_error());
        }
        // In range, so the rounded value fits.
        *slot = (stop * RAMP_STOP_UNIT).round() as u16;
    }
    rising_ramp_stops(&thousandths)?;
    Ok(thousandths)
}

/// Stops in thousandths run from 0 to 1000, each past the one before: a
/// theme's authored ones and a saved snapshot's alike, or the ramp divides
/// by a zero-wide step.
fn rising_ramp_stops(thousandths: &[u16; 9]) -> Result<(), ThemeError> {
    let rising = thousandths.windows(2).all(|pair| pair[0] < pair[1]);
    if thousandths[0] != 0 || thousandths[8] != 1000 || !rising {
        return Err(ramp_stops_error());
    }
    Ok(())
}

fn heat_gamma_error() -> ThemeError {
    ThemeError::new(
        "orderflow.heat_gamma",
        format!("must be finite and between {HEAT_GAMMA_MIN} and {HEAT_GAMMA_MAX}"),
    )
}

fn heat_fade_below_error() -> ThemeError {
    ThemeError::new(
        "orderflow.heat_fade_below",
        format!("must be finite and between {HEAT_FADE_BELOW_MIN} and {HEAT_FADE_BELOW_MAX}"),
    )
}

fn ramp_stops_error() -> ThemeError {
    ThemeError::new(
        "orderflow.ramp_stops",
        "must run from 0 to 1, each stop past the one before",
    )
}

impl OrderflowAppearance {
    /// Where ramp colour `index` sits along the intensity, from 0 to 1.
    #[must_use]
    pub fn ramp_stop(&self, index: usize) -> f32 {
        f32::from(self.ramp_stops[index]) / RAMP_STOP_UNIT
    }

    /// The exponent the palette lays over each cell's intensity, if it authors one.
    #[must_use]
    pub fn heat_gamma(&self) -> Option<f32> {
        self.heat_gamma
            .map(|thousandths| f32::from(thousandths) / RAMP_STOP_UNIT)
    }

    /// The intensity from which the palette paints a cell opaque, if it authors one.
    #[must_use]
    pub fn heat_fade_below(&self) -> Option<f32> {
        self.heat_fade_below
            .map(|thousandths| f32::from(thousandths) / RAMP_STOP_UNIT)
    }

    pub fn validate(&self) -> Result<(), ThemeError> {
        for (key, rgba) in [
            ("buy", self.buy),
            ("sell", self.sell),
            ("consumption", self.consumption),
            ("bubble_text", self.bubble_text),
            ("bid_tint", self.bid_tint),
            ("ask_tint", self.ask_tint),
        ] {
            if rgba[3] != 255 {
                return Err(ThemeError::new(
                    format!("orderflow.colors.{key}"),
                    "must be opaque: bubble presets and thermal ramps use RGB",
                ));
            }
        }
        if let Some(gamma) = self.heat_gamma()
            && !(HEAT_GAMMA_MIN..=HEAT_GAMMA_MAX).contains(&gamma)
        {
            return Err(heat_gamma_error());
        }
        if let Some(fade) = self.heat_fade_below()
            && !(HEAT_FADE_BELOW_MIN..=HEAT_FADE_BELOW_MAX).contains(&fade)
        {
            return Err(heat_fade_below_error());
        }
        rising_ramp_stops(&self.ramp_stops)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Appearance, DARK, LIGHT, ResolvedTheme};
    #[test]
    fn missing_orderflow_fields_fall_back_and_alpha_rejection_is_atomic() {
        let dark = ResolvedTheme::default();
        let partial = ResolvedTheme::parse(
            "version = 1\nname = 'Partial'\n[orderflow.colors]\nbuy = '#123456'",
        )
        .unwrap();
        assert_eq!(partial.orderflow.buy, [18, 52, 86, 255]);
        assert_eq!(partial.orderflow.ramp, dark.orderflow.ramp);
        for key in [
            "buy",
            "sell",
            "consumption",
            "bubble_text",
            "bid_tint",
            "ask_tint",
        ] {
            let mut state = Appearance::default();
            state.apply(LIGHT).unwrap();
            let before = state.clone();
            let source =
                format!("version = 1\nname = 'Invalid'\n[orderflow.colors]\n{key} = '#12345680'");
            let error = state.apply(&source).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("orderflow.colors.{key}"))
            );
            assert_eq!(state.resolved(), before.resolved());
            assert_eq!(state.revision(), before.revision());
        }
        let mut document = crate::ThemeDocument::parse(DARK).unwrap();
        document.orderflow.ramp.as_mut().unwrap()[4] = crate::ColorValue::Hex("#12345680".into());
        assert!(
            document
                .resolve()
                .unwrap_err()
                .to_string()
                .contains("orderflow.ramp[4]")
        );
        document.orderflow.ramp = None;
        document
            .orderflow
            .colors
            .insert("typo".into(), crate::ColorValue::Hex("#123456".into()));
        assert!(
            document
                .resolve()
                .unwrap_err()
                .to_string()
                .contains("orderflow.colors.typo")
        );
    }

    /// A saved snapshot's stops pass the theme's own rule: one that does not
    /// start at 0, end at 1 or rise at every step is refused, where it would
    /// otherwise divide the ramp by a zero-wide step.
    #[test]
    fn a_snapshot_with_stops_that_do_not_rise_is_refused() {
        let mut palette = ResolvedTheme::default().orderflow;
        assert!(palette.validate().is_ok());
        let legacy = crate::orderflow::LEGACY_RAMP_STOPS;
        let mut duplicate = legacy;
        duplicate[3] = duplicate[2];
        let mut late_start = legacy;
        late_start[0] = 10;
        let mut short_end = legacy;
        short_end[8] = 990;
        let mut falling = legacy;
        falling.swap(4, 5);
        for stops in [duplicate, late_start, short_end, falling] {
            palette.ramp_stops = stops;
            let error = palette.validate().unwrap_err();
            assert_eq!(error.field, "orderflow.ramp_stops", "{stops:?}");
        }
    }
}
