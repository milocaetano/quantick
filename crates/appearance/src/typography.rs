//! Named text roles. Font families select bundled fonts and retain glyph fallback.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::ThemeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FontFamily {
    Proportional,
    Monospace,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FontSpec {
    pub family: FontFamily,
    pub size: f32,
}

/// Omitted properties inherit that role's Dark setting independently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FontOverride {
    pub family: Option<FontFamily>,
    pub size: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum FontRole {
    Heading,
    Body,
    Button,
    Monospace,
    Small,
    ChartAxis,
    ChartLabel,
    Footprint,
    Drawing,
}

impl FontRole {
    pub const ALL: [Self; 9] = [
        Self::Heading,
        Self::Body,
        Self::Button,
        Self::Monospace,
        Self::Small,
        Self::ChartAxis,
        Self::ChartLabel,
        Self::Footprint,
        Self::Drawing,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Body => "body",
            Self::Button => "button",
            Self::Monospace => "monospace",
            Self::Small => "small",
            Self::ChartAxis => "chart_axis",
            Self::ChartLabel => "chart_label",
            Self::Footprint => "footprint",
            Self::Drawing => "drawing",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Typography([FontSpec; FontRole::ALL.len()]);

impl Default for Typography {
    fn default() -> Self {
        crate::ResolvedTheme::bundled_dark().typography.clone()
    }
}

impl Typography {
    pub fn get(&self, role: FontRole) -> FontSpec {
        self.0[role as usize]
    }

    pub(crate) fn resolve(
        values: &BTreeMap<String, FontOverride>,
        fallback: Option<&Self>,
    ) -> Result<Self, ThemeError> {
        let mut fonts = Vec::with_capacity(FontRole::ALL.len());
        for role in FontRole::ALL {
            let value = values.get(role.key()).copied().unwrap_or_default();
            let inherited = fallback.map(|fonts| fonts.get(role));
            let required = |property| {
                ThemeError::new(
                    format!("typography.{}.{property}", role.key()),
                    "required in a complete theme",
                )
            };
            fonts.push(FontSpec {
                family: value
                    .family
                    .or(inherited.map(|font| font.family))
                    .ok_or_else(|| required("family"))?,
                size: value
                    .size
                    .or(inherited.map(|font| font.size))
                    .ok_or_else(|| required("size"))?,
            });
        }
        let mut result = Self(fonts.try_into().expect("one font per role"));
        for (key, value) in values {
            let field = format!("typography.{key}");
            let role = FontRole::ALL
                .into_iter()
                .find(|role| role.key() == key)
                .ok_or_else(|| ThemeError::new(&field, "unknown text role"))?;
            if let Some(size) = value.size
                && (!size.is_finite() || !(6.0..=72.0).contains(&size))
            {
                return Err(ThemeError::new(
                    format!("{field}.size"),
                    "must be finite and between 6 and 72 logical pixels",
                ));
            }
            let font = &mut result.0[role as usize];
            if let Some(family) = value.family {
                font.family = family;
            }
            if let Some(size) = value.size {
                font.size = size;
            }
        }
        Ok(result)
    }
}
