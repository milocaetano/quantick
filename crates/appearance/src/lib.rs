//! Shared, renderer-independent appearance configuration.
//!
//! A caller resolves TOML at its configuration boundary, then retains the
//! resulting immutable palette. No file access or ambient theme state lives here.

pub mod color;
pub mod contrast;
mod document;
mod error;
pub mod heat;
pub mod orderflow;
pub mod palette;
mod state;
mod typography;

pub use color::PremultipliedRgba;
pub use document::{ColorValue, PremultipliedValue, ResolvedTheme, ThemeDocument};
pub use error::ThemeError;
pub use palette::Token;
pub use state::{Appearance, ApplyOutcome, Inheritance};
pub use typography::{AxisMetrics, FontFamily, FontOverride, FontRole, FontSpec, Typography};

pub const DARK: &str = include_str!("../themes/dark.toml");
pub const LIGHT: &str = include_str!("../themes/light.toml");

#[cfg(test)]
mod tests;

mod render_palette;
pub use render_palette::RenderPalette;
