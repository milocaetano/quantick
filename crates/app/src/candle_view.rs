//! Egui adapter for candle appearance.
//!
//! Direction, opacity and input sanitization stay in [`crate::style`], while
//! pixel geometry stays in [`crate::chart`]. This module only turns those pure
//! descriptions into egui shapes and widgets.

// The bar itself in `candles`, the appearance window in `style_window`.
// The leaves are private: a surface that draws one candle no longer reads
// the settings panel to find it.
mod candles;
mod style_window;

pub use candles::{BarSlot, draw_laid_candle, is_bullish};
use candles::{color32, paint_pixels};
pub use style_window::draw_style_contents;

#[cfg(test)]
mod tests;
