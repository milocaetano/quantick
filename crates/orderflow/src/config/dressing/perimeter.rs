//! Annular coordinates contained by the factual outer volume radius.

const WIDTH_PX: f32 = 0.75;
const RADIUS_FRACTION: f32 = 0.25;
const DASH_PHASES: usize = 24;

/// Local annular quads, with twelve visible dashes when requested.
/// A centred stroke would inflate tiny discs; every returned vertex stays inside.
pub fn inner_perimeter_quads(
    radius: f32,
    angle: f64,
    sweep: f64,
    dashed: bool,
) -> impl Iterator<Item = [[f32; 2]; 4]> {
    let sweep = if radius.is_finite()
        && radius > 0.0
        && angle.is_finite()
        && sweep.is_finite()
        && sweep > 0.0
    {
        sweep.min(std::f64::consts::TAU)
    } else {
        0.0
    };
    let full = if dashed {
        super::sphere_segments(radius).next_multiple_of(DASH_PHASES)
    } else {
        super::sphere_segments(radius)
    };
    let steps = (sweep / std::f64::consts::TAU * full as f64).ceil() as usize;
    let inner = radius - WIDTH_PX.min(radius * RADIUS_FRACTION);
    (0..steps).filter_map(move |step| {
        let start = angle + sweep * step as f64 / steps as f64;
        let end = angle + sweep * (step + 1) as f64 / steps as f64;
        let phase =
            ((start + end) * 0.5 + f64::from(std::f32::consts::FRAC_PI_2)) / std::f64::consts::TAU;
        if dashed && (phase * DASH_PHASES as f64).floor() as usize % 2 == 1 {
            return None;
        }
        Some(
            [(start, inner), (start, radius), (end, radius), (end, inner)]
                .map(|(angle, radius)| [angle.cos() as f32 * radius, angle.sin() as f32 * radius]),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_and_giant_perimeters_preserve_the_earned_outer_radius() {
        for radius in [0.01, 0.2, 1.0, 3.0, 12.0, 120.0, 1200.0] {
            let plain: Vec<_> =
                inner_perimeter_quads(radius, 0.0, std::f64::consts::TAU, false).collect();
            let dashed: Vec<_> =
                inner_perimeter_quads(radius, 0.0, std::f64::consts::TAU, true).collect();
            for quads in [&plain, &dashed] {
                assert!(!quads.is_empty());
                for [x, y] in quads.iter().flatten() {
                    let distance = x.hypot(*y);
                    assert!(distance <= radius * 1.000001);
                    assert!(distance >= radius * 0.749999);
                }
            }
            assert_ne!(plain, dashed);
        }
    }

    #[test]
    fn invalid_geometry_has_no_vertices() {
        for radius in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                inner_perimeter_quads(radius, 0.0, 1.0, true)
                    .next()
                    .is_none()
            );
        }
        for sweep in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                inner_perimeter_quads(12.0, 0.0, sweep, true)
                    .next()
                    .is_none()
            );
        }
    }
}
