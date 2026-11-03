//! The versioned authoring contract and all-or-nothing resolution.

use std::{collections::BTreeMap, sync::OnceLock};

use serde::{Deserialize, Serialize};

use crate::{FontOverride, PremultipliedRgba, ThemeError, Token, Typography, color};

/// Hex colors are straight sRGB; the explicit table form carries historical
/// renderer bytes without a second premultiplication or a lossy round trip.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ColorValue {
    Hex(String),
    Premultiplied(PremultipliedValue),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PremultipliedValue {
    pub premultiplied: [u8; 4],
}

impl ColorValue {
    pub(crate) fn resolve(&self, field: &str) -> Result<PremultipliedRgba, ThemeError> {
        match self {
            Self::Premultiplied(value) => {
                Ok(PremultipliedRgba::from_premultiplied(value.premultiplied))
            }
            Self::Hex(value) => {
                let hex = value
                    .strip_prefix('#')
                    .ok_or_else(|| ThemeError::new(field, "expected #RRGGBB or #RRGGBBAA"))?;
                if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(ThemeError::new(field, "expected #RRGGBB or #RRGGBBAA"));
                }
                let mut rgba = [255; 4];
                for (channel, pair) in rgba.iter_mut().zip(hex.as_bytes().as_chunks::<2>().0) {
                    // The ASCII check above makes conversion and byte boundaries infallible.
                    let pair = std::str::from_utf8(pair).expect("ASCII hex");
                    *channel = u8::from_str_radix(pair, 16).expect("validated hex");
                }
                Ok(color::rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeDocument {
    pub version: u32,
    pub name: String,
    #[serde(default = "dark_mode")]
    pub dark: bool,
    #[serde(default)]
    pub colors: BTreeMap<String, ColorValue>,
    #[serde(default)]
    pub typography: BTreeMap<String, FontOverride>,
    #[serde(default)]
    pub footprint_heat: crate::heat::HeatOverride,
    #[serde(default)]
    pub orderflow: crate::orderflow::OrderflowOverride,
}

fn dark_mode() -> bool {
    true
}

impl ThemeDocument {
    pub fn parse(source: &str) -> Result<Self, ThemeError> {
        toml::from_str(source).map_err(|error| ThemeError::new("document", error.to_string()))
    }

    pub fn resolve(&self) -> Result<ResolvedTheme, ThemeError> {
        self.resolve_with(Some(ResolvedTheme::bundled_dark()))
    }

    /// A complete document supplies every value without consulting fallback.
    pub fn resolve_complete(&self) -> Result<ResolvedTheme, ThemeError> {
        self.resolve_with(None)
    }

    fn resolve_with(&self, fallback: Option<&ResolvedTheme>) -> Result<ResolvedTheme, ThemeError> {
        if self.version != 1 {
            return Err(ThemeError::new(
                "version",
                format!("unsupported theme version {}; expected 1", self.version),
            ));
        }
        if self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err(ThemeError::new(
                "name",
                "must contain 1 to 80 characters and no control characters",
            ));
        }
        let mut colors = [PremultipliedRgba::from_premultiplied([0; 4]); Token::ALL.len()];
        for token in Token::ALL {
            let field = format!("colors.{}", token.key());
            colors[token as usize] = match self.colors.get(token.key()) {
                Some(value) => value.resolve(&field)?,
                None => fallback
                    .map(|theme| theme.color(token))
                    .ok_or_else(|| ThemeError::new(field, "required in a complete theme"))?,
            };
        }
        for (key, value) in &self.colors {
            let field = format!("colors.{key}");
            let token = Token::from_key(key)
                .ok_or_else(|| ThemeError::new(&field, "unknown semantic color"))?;
            colors[token as usize] = value.resolve(&field)?;
        }
        // These semantic fields cross existing RGB style/preset boundaries.
        // Reject unsupported alpha before publishing any part of the theme.
        for token in [
            Token::Canvas,
            Token::Control,
            Token::Buy,
            Token::Sell,
            Token::CandleBullOutline,
            Token::CandleBearOutline,
            Token::CandleWick,
        ] {
            if colors[token as usize].to_array()[3] != 255 {
                return Err(ThemeError::new(
                    format!("colors.{}", token.key()),
                    "must be opaque: chart styles and saved candle presets use RGB",
                ));
            }
        }
        Ok(ResolvedTheme {
            name: self.name.clone(),
            dark: self.dark,
            colors,
            typography: Typography::resolve(
                &self.typography,
                fallback.map(|theme| &theme.typography),
            )?,
            orderflow: self
                .orderflow
                .resolve(fallback.map(|theme| &theme.orderflow))?,
            footprint_heat: self
                .footprint_heat
                .resolve(fallback.map(|theme| &theme.footprint_heat))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub name: String,
    pub dark: bool,
    colors: [PremultipliedRgba; Token::ALL.len()],
    pub typography: Typography,
    pub footprint_heat: crate::heat::HeatRamp,
    pub orderflow: crate::orderflow::OrderflowAppearance,
}

impl ResolvedTheme {
    /// Immutable process-wide fallback, parsed once through the same validated
    /// complete-document resolver used by external themes. Never application state.
    pub fn bundled_dark() -> &'static Self {
        static DARK: OnceLock<ResolvedTheme> = OnceLock::new();
        DARK.get_or_init(|| {
            ThemeDocument::parse(crate::DARK)
                .and_then(|document| document.resolve_complete())
                .expect("bundled Dark theme must be complete and valid")
        })
    }

    pub fn parse(source: &str) -> Result<Self, ThemeError> {
        ThemeDocument::parse(source)?.resolve()
    }

    /// Indexed lookup: rendering never parses keys or locks a global palette.
    pub fn color(&self, token: Token) -> PremultipliedRgba {
        self.colors[token as usize]
    }
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self::bundled_dark().clone()
    }
}
