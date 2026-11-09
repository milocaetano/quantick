//! The versioned authoring contract and all-or-nothing resolution.

mod bookmap;
mod chart;
mod title_strip;
pub use bookmap::BookmapOverride;
pub use chart::{AXIS_RESERVATION_RANGE, CANDLE_OUTLINE_WIDTH_RANGE, ChartAxes};
pub use title_strip::TitleStripOverride;

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
    pub candle_fill_opacity: Option<f32>,
    /// Candle contour width a fresh chart takes, in logical pixels.
    #[serde(default)]
    pub candle_outline_width: Option<f32>,
    /// Candle contour and wick opacities a fresh chart takes, in `[0, 1]`.
    #[serde(default)]
    pub candle_outline_opacity: Option<f32>,
    #[serde(default)]
    pub candle_wick_opacity: Option<f32>,
    /// Whether a fresh chart draws vertical gridlines at its time labels.
    #[serde(default)]
    pub chart_time_grid: Option<bool>,
    #[serde(default)]
    pub chart_axes: Option<ChartAxes>,
    #[serde(default)]
    pub colors: BTreeMap<String, ColorValue>,
    #[serde(default)]
    pub typography: BTreeMap<String, FontOverride>,
    #[serde(default)]
    pub footprint_heat: crate::heat::HeatOverride,
    #[serde(default)]
    pub orderflow: crate::orderflow::OrderflowOverride,
    #[serde(default)]
    pub bookmap: Option<crate::BookmapOverride>,
    #[serde(default)]
    pub title_strip: Option<TitleStripOverride>,
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

    pub(crate) fn resolve_with(
        &self,
        fallback: Option<&ResolvedTheme>,
    ) -> Result<ResolvedTheme, ThemeError> {
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
                // Before profiles had a dedicated token, authored generic
                // drawing colors also supplied their new-profile defaults.
                None if token == Token::DrawingProfile
                    && fallback.is_some()
                    && self.colors.contains_key(Token::DrawingDefault.key()) =>
                {
                    colors[Token::DrawingDefault as usize]
                }
                // Older themes paint the chart on their own canvas.
                None if token == Token::ChartCanvas
                    && fallback.is_some()
                    && self.colors.contains_key(Token::Canvas.key()) =>
                {
                    colors[Token::Canvas as usize]
                }
                // Older themes used control for both widgets and chart grids.
                None if token == Token::ChartGrid
                    && fallback.is_some()
                    && self.colors.contains_key(Token::Control.key()) =>
                {
                    colors[Token::Control as usize]
                }
                // Older themes predate the strip, brand and replay tokens: the
                // strip is the document's own inset, the other two its own
                // mode's bundled values, so an authored Light file stays Light.
                None if token == Token::TabStrip
                    && fallback.is_some()
                    && self.colors.contains_key(Token::Inset.key()) =>
                {
                    colors[Token::Inset as usize]
                }
                None if MODE_TOKENS.contains(&token)
                    && fallback.is_some_and(|theme| theme.dark != self.dark) =>
                {
                    ResolvedTheme::bundled(self.dark).color(token)
                }
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
            Token::ChartCanvas,
            Token::Control,
            Token::ChartGrid,
            Token::Buy,
            Token::Sell,
            Token::CandleBullOutline,
            Token::CandleBearOutline,
            Token::CandleWick,
            Token::LastPriceUp,
            Token::LastPriceDown,
        ] {
            if colors[token as usize].to_array()[3] != 255 {
                return Err(ThemeError::new(
                    format!("colors.{}", token.key()),
                    "must be opaque: chart styles and saved candle presets use RGB",
                ));
            }
        }
        let candle_fill_opacity = self
            .candle_fill_opacity
            .or_else(|| fallback.map(|theme| theme.candle_fill_opacity))
            .ok_or_else(|| {
                ThemeError::new("candle_fill_opacity", "required in a complete theme")
            })?;
        if !candle_fill_opacity.is_finite() || !(0.0..=1.0).contains(&candle_fill_opacity) {
            return Err(ThemeError::new(
                "candle_fill_opacity",
                "must be finite and between 0 and 1 inclusive",
            ));
        }
        let mut resolved = ResolvedTheme {
            name: self.name.clone(),
            dark: self.dark,
            candle_fill_opacity,
            candle_outline_width: chart::outline_width(self.candle_outline_width, fallback)?,
            candle_outline_opacity: chart::opacity(
                "candle_outline_opacity",
                self.candle_outline_opacity,
                fallback.map(|theme| theme.candle_outline_opacity),
            )?,
            candle_wick_opacity: chart::opacity(
                "candle_wick_opacity",
                self.candle_wick_opacity,
                fallback.map(|theme| theme.candle_wick_opacity),
            )?,
            chart_time_grid: chart::time_grid(self.chart_time_grid, fallback)?,
            chart_axes: ChartAxes::resolve(self.chart_axes, fallback)?,
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
            bookmap: None,
            title_strip: None,
        };
        resolved.bookmap = self
            .bookmap
            .as_ref()
            .map(|surface| surface.resolve(&resolved).map(Box::new))
            .transpose()?;
        resolved.title_strip = self
            .title_strip
            .as_ref()
            .map(|surface| surface.resolve(&resolved).map(Box::new))
            .transpose()?;
        Ok(resolved)
    }
}

/// The tokens an older document takes from its own mode's bundled theme.
/// Both bundled themes write every one, so resolving Light for
/// [`ResolvedTheme::bundled`] never asks its own lock again.
pub(crate) const MODE_TOKENS: [Token; 22] = [
    Token::TabStrip,
    Token::Brand,
    Token::ReplayInk,
    Token::ToolbarSheet,
    Token::TabIdleFill,
    Token::TabIdleOutline,
    Token::TabBand,
    Token::HomeGlow,
    Token::AxisInk,
    Token::PriceAxisRule,
    Token::TimeAxisRule,
    Token::LastPriceUp,
    Token::LastPriceDown,
    Token::ReplayPlate,
    Token::ReplayPlateInk,
    Token::OrderPlate,
    Token::OrderPlateInk,
    Token::PlateGain,
    Token::PlateLoss,
    Token::HoverFill,
    Token::ControlEdge,
    Token::ConnectionAction,
];

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub name: String,
    pub dark: bool,
    pub candle_fill_opacity: f32,
    pub candle_outline_width: f32,
    pub candle_outline_opacity: f32,
    pub candle_wick_opacity: f32,
    pub chart_time_grid: bool,
    pub chart_axes: ChartAxes,
    colors: [PremultipliedRgba; Token::ALL.len()],
    pub typography: Typography,
    pub footprint_heat: crate::heat::HeatRamp,
    pub orderflow: crate::orderflow::OrderflowAppearance,
    pub(crate) bookmap: Option<Box<ResolvedTheme>>,
    pub(crate) title_strip: Option<Box<ResolvedTheme>>,
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

    /// The bundled theme of one mode, for values an older document of that
    /// mode never wrote.
    pub fn bundled(dark: bool) -> &'static Self {
        static LIGHT: OnceLock<ResolvedTheme> = OnceLock::new();
        if dark {
            return Self::bundled_dark();
        }
        LIGHT.get_or_init(|| {
            // Light writes the three mode tokens, so this never asks itself.
            ResolvedTheme::parse(crate::LIGHT).expect("bundled Light theme must be valid")
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
