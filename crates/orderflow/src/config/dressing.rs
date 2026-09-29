//! Scalar bubble dressing shared by painters and headless style consumers.

use super::{BubbleStyle, INV_PHI, theme::finite_unit};

/// Interior alpha of a hollow bubble, as a fraction of the configured fill
/// alpha: enough tint to keep the disc's area readable, light enough that the
/// ring is what the eye catches.
pub const HOLLOW_FILL_ALPHA: f32 = 0.22;

/// Ring thickness of a hollow bubble as a fraction of its radius, and the
/// pixel range it is held to — thin enough to stay a ring on a full-size
/// sweep, thick enough to survive at dot size.
const HOLLOW_RING_SCALE: f32 = 0.42;

/// See [`HOLLOW_RING_SCALE`].
const HOLLOW_MIN_RING_PX: f32 = 1.2;

/// See [`HOLLOW_RING_SCALE`].
const HOLLOW_MAX_RING_PX: f32 = 3.0;

/// Ring thickness of a hollow bubble of this radius.
pub fn hollow_ring_width(radius: f32) -> f32 {
    (radius * HOLLOW_RING_SCALE).clamp(HOLLOW_MIN_RING_PX, HOLLOW_MAX_RING_PX)
}

/// Width of the dark separator hair drawn just outside a bubble's rim, as a
/// fraction of the radius, and the pixel range it is held to.
///
/// The heat ramp passes through greens the buy side almost matches, so without
/// a dark hair between them "aggression" and "liquidity" melt into one layer
/// wherever a bubble sits on warm heat — the hair is what keeps them two.
///
/// Proportional rather than fixed: at a flat 1.2px the hair was over half the
/// radius of a routine 2px print and under a tenth of a full sweep's, so small
/// prints wore a heavy black collar and large ones a thread. One ratio makes
/// every bubble the same drawing at a different size.
const SEPARATOR_RING_SCALE: f32 = 0.14;

/// See [`SEPARATOR_RING_SCALE`].
const SEPARATOR_MIN_RING_PX: f32 = 0.5;

/// See [`SEPARATOR_RING_SCALE`].
const SEPARATOR_MAX_RING_PX: f32 = 1.5;

/// Alpha of that hair: translucent black, so it darkens whatever heat is
/// behind it instead of assuming one canvas colour.
pub const SEPARATOR_RING_ALPHA: u8 = 170;

/// Separator-hair width for a bubble of this radius.
pub fn separator_ring_width(radius: f32) -> f32 {
    (radius * SEPARATOR_RING_SCALE).clamp(SEPARATOR_MIN_RING_PX, SEPARATOR_MAX_RING_PX)
}

/// Pixels added beyond `front_length_scale × radius`, so the consumption mark
/// on even the smallest bubble is long enough to read as a mark.
pub const FRONT_END_PADDING_PX: f32 = 6.0;

/// How far the halo opens up at full print size, as a fraction of
/// `halo_strength`: a sweep reads heavier than a routine print of the same
/// colour, without needing a second colour for it.
const HALO_SIZE_BOOST: f32 = 0.5;

/// Rim alpha relative to the fill. A hair below opaque keeps the rim reading
/// as the bubble's edge rather than as a separate ring on a dark canvas.
pub const RIM_ALPHA: f32 = 0.96;

/// Impact-ring alpha every consuming print gets, before the matched share.
pub const IMPACT_RING_BASE_ALPHA: f32 = 0.75;

/// Share of the impact ring's alpha that tracks how much of the print actually
/// matched resting liquidity, so a full sweep rings brighter than a nibble.
const IMPACT_RING_MATCH_ALPHA: f32 = 0.25;

/// Matched-fraction floor for the consumption marks: a barely matched print
/// still ate something, so it still leaves a visible mark.
const MIN_MATCH_STRENGTH: f32 = 0.25;

/// Fraction of the radius the sphere's lit core is offset toward the upper
/// left. One fixed light direction keeps every bubble shaded identically, so
/// the eye reads the gradient as volume instead of as data.
pub const SPHERE_LIGHT_OFFSET: f32 = 0.35;

/// Radius of the sphere's full-brightness core ring, as a fraction of the
/// bubble radius. Vertex colours interpolate highlight → side colour inside
/// it and side colour → darkened rim outside it; that gradient is the whole
/// shading model.
pub const SPHERE_CORE_RADIUS: f32 = 0.62;

/// Ring segments per pixel of radius on a sphere-shaded bubble, bounded by
/// [`SPHERE_MIN_SEGMENTS`] and [`SPHERE_MAX_SEGMENTS`]: a small dressed
/// bubble stays cheap, a full-size sweep stays round.
const SPHERE_SEGMENTS_PER_RADIUS_PX: f32 = 2.0;

/// See [`SPHERE_SEGMENTS_PER_RADIUS_PX`].
const SPHERE_MIN_SEGMENTS: usize = 12;

/// See [`SPHERE_SEGMENTS_PER_RADIUS_PX`].
const SPHERE_MAX_SEGMENTS: usize = 32;

/// Tessellation of a sphere-shaded bubble of this radius.
pub fn sphere_segments(radius: f32) -> usize {
    ((radius * SPHERE_SEGMENTS_PER_RADIUS_PX) as usize)
        .clamp(SPHERE_MIN_SEGMENTS, SPHERE_MAX_SEGMENTS)
}

/// Half-length, in pixels, of the vertical consumption front on a bubble of
/// this radius.
pub fn front_half_length(radius: f32, bubbles: &BubbleStyle) -> f32 {
    radius * bubbles.front_length_scale + FRONT_END_PADDING_PX
}

/// Halo alpha for a print of this normalized size.
pub fn halo_alpha(size: f32, bubbles: &BubbleStyle) -> f32 {
    (bubbles.halo_strength * (1.0 + HALO_SIZE_BOOST * finite_unit(size))).min(1.0)
}

/// Impact-ring alpha for a print that matched this fraction of resting
/// liquidity.
pub fn impact_ring_alpha(matched_fraction: f32) -> f32 {
    IMPACT_RING_BASE_ALPHA
        + finite_unit(matched_fraction).max(MIN_MATCH_STRENGTH) * IMPACT_RING_MATCH_ALPHA
}

/// Half-height of the trail behind a bubble of this radius, and the pixel
/// range it is held to.
///
/// `1/φ` of the radius, so the trail stays strictly *smaller* than the bubble
/// it belongs to. It used to borrow the consumption front's half-length, which
/// carries a fixed 6px addition — on a routine 2px print that made the trail a
/// 17px-tall bar behind a 4px disc, and a chart whose signal is horizontal
/// bands does not need decorative horizontal bands eight times the ink of the
/// mark they decorate.
pub fn trail_half_height(radius: f32) -> f32 {
    (radius * INV_PHI).clamp(TRAIL_MIN_HALF_HEIGHT_PX, TRAIL_MAX_HALF_HEIGHT_PX)
}

/// See [`trail_half_height`].
const TRAIL_MIN_HALF_HEIGHT_PX: f32 = 1.5;

/// See [`trail_half_height`].
const TRAIL_MAX_HALF_HEIGHT_PX: f32 = 9.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_marks_scale_with_size_and_matched_share() {
        let bubbles = BubbleStyle::default();
        // The front grows with the radius, and never collapses to nothing on
        // the smallest bubble.
        assert!(front_half_length(10.0, &bubbles) > front_half_length(2.0, &bubbles));
        assert!(front_half_length(0.0, &bubbles) >= FRONT_END_PADDING_PX);
        // A sweep haloes brighter than a routine print, and alpha stays legal.
        assert!(halo_alpha(1.0, &bubbles) > halo_alpha(0.0, &bubbles));
        assert!(halo_alpha(1.0, &bubbles) <= 1.0);
        assert_eq!(
            halo_alpha(0.0, &bubbles),
            bubbles.halo_strength,
            "an unsized print gets the plain halo"
        );
        assert!(
            halo_alpha(f32::NAN, &bubbles).is_finite(),
            "a non-finite size must not poison the alpha"
        );
        // The ring brightens with the share of the print that matched, from a
        // floor that keeps a nibble visible.
        assert!(impact_ring_alpha(1.0) > impact_ring_alpha(0.0));
        assert!(impact_ring_alpha(0.0) >= IMPACT_RING_BASE_ALPHA);
        assert!(impact_ring_alpha(1.0) <= 1.0);
    }
}
