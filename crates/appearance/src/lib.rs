//! Shared, renderer-independent appearance configuration.
//!
//! A caller resolves TOML at its configuration boundary, then retains the
//! resulting immutable palette. No file access or ambient theme state lives here.

mod builtin;
pub mod color;
pub use builtin::Builtin;
mod cache;
pub use cache::RenderCache;
pub mod contrast;
mod document;
mod error;
pub mod heat;
pub mod orderflow;
pub mod palette;
pub mod panel_chrome;
pub mod paper;
mod state;
pub mod tab_strip;
mod typography;

pub use color::PremultipliedRgba;
pub use document::{
    AXIS_RESERVATION_RANGE, BookmapOverride, CANDLE_OUTLINE_WIDTH_RANGE, ChartAxes, ChartSurface,
    ColorValue, LitFlowOverride, PremultipliedValue, ResolvedTheme, ThemeDocument,
    TitleStripOverride,
};
pub use error::ThemeError;
pub use palette::Token;
pub use state::{Appearance, ApplyOutcome, Inheritance};
pub use typography::{AxisMetrics, FontFamily, FontOverride, FontRole, FontSpec, Typography};

pub const DARK: &str = include_str!("../themes/dark.toml");
pub const LIGHT: &str = include_str!("../themes/light.toml");

#[cfg(test)]
mod tests;

#[cfg(test)]
mod grid_tests;

#[cfg(test)]
mod surface_dump_tests;

mod render_palette;
pub use render_palette::RenderPalette;
