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

    pub const fn dark(self) -> FontSpec {
        use FontFamily::{Monospace, Proportional};
        let (family, size) = match self {
            Self::Heading => (Proportional, 18.0),
            Self::Body | Self::Button => (Proportional, 12.5),
            Self::Drawing => (Proportional, 12.0),
            Self::Monospace => (Monospace, 12.0),
            Self::Small => (Proportional, 9.0),
            Self::ChartAxis => (Monospace, 11.0),
            Self::ChartLabel => (Proportional, 11.0),
            Self::Footprint => (Monospace, 10.0),
        };
        FontSpec { family, size }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Typography([FontSpec; FontRole::ALL.len()]);

impl Default for Typography {
    fn default() -> Self {
        Self(FontRole::ALL.map(FontRole::dark))
    }
}

impl Typography {
    pub fn get(&self, role: FontRole) -> FontSpec {
        self.0[role as usize]
    }

    pub(crate) fn resolve(values: &BTreeMap<String, FontOverride>) -> Result<Self, ThemeError> {
        let mut result = Self::default();
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
