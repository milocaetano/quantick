//! Portable geometry and colour weights for bubble consumption crowns.

use super::theme::finite_unit;
use super::{GOLDEN_ANGLE, INV_PHI, INV_PHI_2, INV_PHI_3};
use quantick_engine::Side;

/// Gap between the rim and the consumption crown, as a fraction of the radius,
/// and the pixel range it is held to. `1/φ³` — the innermost step of the
/// nested `1/φ³` gap + `1/φ²` stroke that lands the whole crown apparatus at
/// about `r/φ²` beyond the rim on a full-size print.
const CROWN_GAP_SCALE: f32 = INV_PHI_3;

/// See [`CROWN_GAP_SCALE`].
const CROWN_MIN_GAP_PX: f32 = 1.4;

/// See [`CROWN_GAP_SCALE`].
const CROWN_MAX_GAP_PX: f32 = 3.0;

/// Stroke width of the crown as a fraction of the radius, and the pixel range
/// it is held to. `1/φ²`.
const CROWN_WIDTH_SCALE: f32 = INV_PHI_2;

/// See [`CROWN_WIDTH_SCALE`].
const CROWN_MIN_WIDTH_PX: f32 = 1.0;

/// See [`CROWN_WIDTH_SCALE`].
const CROWN_MAX_WIDTH_PX: f32 = 2.4;

/// Arc length, in pixels, below which an arc has stopped reading as an arc.
/// Under it the crown collapses to a pip at the pole — a print small enough to
/// be a speck still gets to say it ate something, for about four pixels of ink.
pub const CROWN_MIN_ARC_PX: f32 = 4.0;

/// Radius of that pip, in pixels.
pub const CROWN_PIP_RADIUS_PX: f32 = 1.2;

/// How far the crown's colour is pushed from its side colour toward white.
///
/// `1/φ`. Consumption is the same event, hotter — deriving the crown from the
/// side keeps a third hue off the canvas, and means N stacked crowns saturate
/// toward their own green or red instead of toward glare or toward mud.
pub const CROWN_WHITE_MIX: f32 = INV_PHI;

/// Crown alpha before the matched share.
const CROWN_BASE_ALPHA: f32 = 0.62;

/// Share of the crown's alpha that tracks the matched fraction. Secondary to
/// arc length, which is the channel actually carrying the reading.
const CROWN_MATCH_ALPHA: f32 = 0.38;

/// Extra width of the dark stroke laid under the crown, so the arc survives
/// over a bright heat band without assuming one canvas colour. A hairline each
/// side, never a mass.
pub const CROWN_BACKING_PX: f32 = 1.0;

/// See [`CROWN_BACKING_PX`].
pub const CROWN_BACKING_ALPHA: u8 = 110;

/// The pole a crown is centred on.
///
/// Buy aggression lifts the ask, so its crown sits above the print; sell
/// aggression hits the bid and wears it below. Screen y grows downward. This
/// is the same fact [`crate::side_offset_y`] encodes, deliberately restated: on a
/// dense tape the two reinforce each other rather than compete.
pub const fn crown_center_angle(side: Side) -> f32 {
    match side {
        Side::Buy => -std::f32::consts::FRAC_PI_2,
        Side::Sell => std::f32::consts::FRAC_PI_2,
    }
}

/// The crown's geometry for a bubble of this radius and matched share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrownGeometry {
    /// Radius of the arc itself — outside the rim, never on it.
    pub arc_radius: f32,
    /// Stroke width.
    pub width: f32,
    /// Angular length, in radians. Never exceeds the [`GOLDEN_ANGLE`], so the
    /// crown cannot close into a second circle around the bubble.
    pub sweep: f32,
}

impl CrownGeometry {
    /// Length of the arc in pixels — what decides whether it can be drawn as
    /// an arc at all.
    pub fn arc_length(self) -> f32 {
        self.arc_radius * self.sweep
    }
}

/// Crown geometry for a print of this radius that matched this fraction of
/// resting liquidity.
pub fn crown_geometry(radius: f32, matched: f32) -> CrownGeometry {
    let gap = (radius * CROWN_GAP_SCALE).clamp(CROWN_MIN_GAP_PX, CROWN_MAX_GAP_PX);
    CrownGeometry {
        arc_radius: radius + gap,
        width: (radius * CROWN_WIDTH_SCALE).clamp(CROWN_MIN_WIDTH_PX, CROWN_MAX_WIDTH_PX),
        // A `1/φ²` floor plus a `1/φ` span: a nibble still shows a mark, a
        // full sweep reaches the golden angle and no further.
        sweep: GOLDEN_ANGLE * (INV_PHI_2 + INV_PHI * finite_unit(matched)),
    }
}

/// The crown's colour for a print that matched this fraction.
pub fn crown_alpha(matched: f32) -> f32 {
    CROWN_BASE_ALPHA + CROWN_MATCH_ALPHA * finite_unit(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crown_never_touches_the_disc_and_never_closes_a_circle() {
        // The two properties the mark exists for. The first is why it replaced
        // the vertical front: a bubble's area is its quantity, so nothing may
        // be drawn over it. The second is why it is an arc and not a ring: a
        // closed circle concentric with the disc makes the disc's own edge
        // ambiguous, which is what the impact ring did.
        for radius in [1.0_f32, 2.2, 3.5, 5.7, 9.3, 15.0, 22.0, 48.0] {
            for matched in [0.0_f32, 0.1, 0.5, 0.99, 1.0] {
                let geometry = crown_geometry(radius, matched);
                assert!(
                    geometry.arc_radius - geometry.width / 2.0 > radius,
                    "at r={radius} m={matched} the crown's inner edge \
                         {} is not clear of the rim",
                    geometry.arc_radius - geometry.width / 2.0
                );
                assert!(
                    geometry.sweep <= GOLDEN_ANGLE + 1e-5,
                    "at r={radius} m={matched} the sweep {} exceeds the golden angle",
                    geometry.sweep
                );
                assert!(
                    geometry.sweep >= GOLDEN_ANGLE * INV_PHI_2 - 1e-5,
                    "a print that ate anything still shows a mark"
                );
            }
        }
    }

    #[test]
    fn the_crown_grows_with_the_matched_share() {
        // Arc length is the channel a trader reads ordinally without a
        // reference beside it, so it has to be monotone in what it encodes.
        let radius = 12.0;
        let mut previous = f32::NEG_INFINITY;
        for matched in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let length = crown_geometry(radius, matched).arc_length();
            assert!(
                length > previous,
                "matched={matched} must draw a longer arc than the share below it"
            );
            previous = length;
        }
        // A full sweep reaches the golden angle exactly, and a nibble 1/φ² of it.
        assert!((crown_geometry(radius, 1.0).sweep - GOLDEN_ANGLE).abs() < 1e-5);
        assert!((crown_geometry(radius, 0.0).sweep - GOLDEN_ANGLE * INV_PHI_2).abs() < 1e-5);
    }
}
