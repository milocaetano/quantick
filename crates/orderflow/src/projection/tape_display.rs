//! Native Tape presentation after factual groups have been selected and placed.

use rust_decimal::Decimal;

use super::{AggressionPrimitive, normalized_area_size};
use crate::config::BubbleStyle;

/// One common scale for the final visible native Tape groups. It contains no
/// merger inputs or remembered camera state, so live, retained and replay frames
/// use the same rule, including while a factual rebuild runs elsewhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeBubbleDisplay {
    /// Largest positive final visible Tape group, or one for an empty frame.
    pub full_quantity: Decimal,
    /// Effective largest drawn radius: available envelope times `radius_gain`.
    pub max_radius: f32,
    /// The common camera factor before the available-space restriction.
    pub radius_gain: f32,
}

impl TapeBubbleDisplay {
    /// Resolve only when explicitly configured. `visible_marks` must contain
    /// the final groups whose centers are inside the current Tape viewport;
    /// candle marks are excluded here. Densities come from the current camera,
    /// never the geometry on which a cached projection was originally built.
    #[must_use]
    pub fn resolve<'a>(
        bubbles: &BubbleStyle,
        visible_marks: impl IntoIterator<Item = &'a AggressionPrimitive>,
        px_per_ms: f64,
        px_per_price: f64,
        available_radius: f32,
    ) -> Option<Self> {
        let camera = bubbles.viewport_scale?.sanitized()?;
        let full_quantity = visible_marks
            .into_iter()
            .filter(|mark| mark.live && mark.quantity > Decimal::ZERO)
            .map(|mark| mark.quantity)
            .max()
            .unwrap_or(Decimal::ONE);
        let radius_gain = camera.radius_gain(px_per_ms, px_per_price);
        let max_radius = bubbles.max_radius.min(available_radius).max(0.0) * radius_gain;
        Some(Self {
            full_quantity,
            max_radius,
            radius_gain,
        })
    }

    /// Normalized quantity size and actual drawn radius. A common envelope
    /// preserves every positive quantity ratio; there is no per-dot radius floor.
    #[must_use]
    pub fn draw(self, quantity: Decimal) -> (f32, f32) {
        let size = normalized_area_size(quantity, self.full_quantity);
        (size, self.max_radius * size)
    }
}
