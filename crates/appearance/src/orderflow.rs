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
        Ok(resolved)
    }
}

impl OrderflowAppearance {
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
        Ok(())
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
}
