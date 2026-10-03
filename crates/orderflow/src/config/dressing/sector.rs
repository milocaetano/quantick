//! Circle-sector geometry shared by flat regions and shaded native Tape.
use super::{SPHERE_CORE_RADIUS, sphere_segments};

/// World-coordinate vertices keep the renderer's floating-point operation order.
/// Vertex roles select core, body and edge colours without owning a UI colour type.
#[derive(Clone, Copy)]
pub struct SectorGeometry {
    center: [f32; 2],
    radius: f32,
    start: f32,
    sweep: f32,
    light_offset: f32,
    flat: bool,
    segments: usize,
}

impl SectorGeometry {
    #[inline]
    pub fn new(
        center: [f32; 2],
        radius: f32,
        start: f32,
        sweep: f32,
        light_offset: f32,
        equal_colours: bool,
    ) -> Option<Self> {
        if !radius.is_finite()
            || radius <= 0.0
            || !center.iter().all(|coordinate| coordinate.is_finite())
            || !sweep.is_finite()
        {
            return None;
        }
        let sweep = sweep.clamp(0.0, std::f32::consts::TAU);
        if sweep <= 0.0 {
            return None;
        }
        let segments = if light_offset == 0.0 {
            // Shared quarter-circle divisions preserve half and quarter areas.
            let full = sphere_segments(radius).next_multiple_of(4);
            ((full as f32 * (sweep / std::f32::consts::TAU)).round() as usize).max(2)
        } else {
            let full = sphere_segments(radius);
            (((full as f32) * (sweep / std::f32::consts::TAU)).ceil() as usize).max(2)
        };
        Some(Self {
            center,
            radius,
            start,
            sweep,
            light_offset,
            flat: light_offset == 0.0 && equal_colours,
            segments,
        })
    }

    /// Visit position and colour role (core 0, body 1, edge 2), without buffers.
    #[inline]
    pub fn for_each_vertex(self, mut emit: impl FnMut([f32; 2], usize)) {
        let offset = -self.radius * self.light_offset;
        let core_center = self
            .center
            .map(|coordinate| coordinate + offset * (1.0 - SPHERE_CORE_RADIUS));
        emit(self.center.map(|coordinate| coordinate + offset), 0);
        let mut ring = |center: [f32; 2], radius: f32, role| {
            for index in 0..=self.segments {
                let angle = self.start + self.sweep * (index as f32 / self.segments as f32);
                emit(
                    [
                        center[0] + angle.cos() * radius,
                        center[1] + angle.sin() * radius,
                    ],
                    role,
                );
            }
        };
        if !self.flat {
            ring(core_center, self.radius * SPHERE_CORE_RADIUS, 1);
        }
        ring(self.center, self.radius, 2);
    }

    /// Visit triangle indices relative to the first [`Self::for_each_vertex`] call.
    #[inline]
    pub fn for_each_triangle(self, mut emit: impl FnMut([u32; 3])) {
        let count = self.segments as u32;
        let core = 1;
        let rim = core + count + 1;
        for index in 0..count {
            let next = index + 1;
            emit([0, core + index, core + next]);
            if !self.flat {
                emit([core + index, rim + index, rim + next]);
                emit([core + index, rim + next, core + next]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_quarter_has_only_the_centre_and_earned_outer_arc() {
        let sector = SectorGeometry::new(
            [30.0, 40.0],
            12.0,
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::FRAC_PI_2,
            0.0,
            true,
        )
        .unwrap();
        let mut vertices = Vec::new();
        sector.for_each_vertex(|point, role| vertices.push((point, role)));
        assert_eq!(vertices.len(), 8);
        assert_eq!(vertices[0], ([30.0, 40.0], 0));
        assert_eq!(vertices[1], ([30.0, 28.0], 2));
        assert_eq!(vertices[7], ([42.0, 40.0], 2));
        let mut triangles = Vec::new();
        sector.for_each_triangle(|indices| triangles.push(indices));
        assert_eq!(triangles.len(), 6);
        assert_eq!(triangles[0], [0, 1, 2]);
        assert_eq!(triangles[5], [0, 6, 7]);
        for (point, role) in &vertices[1..] {
            assert_eq!(*role, 2);
            assert!(((point[0] - 30.0).hypot(point[1] - 40.0) - 12.0).abs() < 0.00001);
        }
    }

    #[test]
    fn shaded_native_disc_retains_both_rings_and_the_shared_light_offset() {
        let sector = SectorGeometry::new(
            [30.0, 40.0],
            10.0,
            0.0,
            std::f32::consts::TAU,
            super::super::SPHERE_LIGHT_OFFSET,
            false,
        )
        .unwrap();
        let mut vertices = Vec::new();
        sector.for_each_vertex(|point, role| vertices.push((point, role)));
        assert_eq!(vertices.len(), 43);
        assert_eq!(vertices[0], ([26.5, 36.5], 0));
        assert!(vertices[1..22].iter().all(|(_, role)| *role == 1));
        assert!(vertices[22..].iter().all(|(_, role)| *role == 2));
        let mut triangles = Vec::new();
        sector.for_each_triangle(|indices| triangles.push(indices));
        assert_eq!(triangles.len(), 60);
        assert_eq!(&triangles[..3], &[[0, 1, 2], [1, 22, 23], [1, 23, 2]]);
        assert_eq!(&triangles[57..], &[[0, 20, 21], [20, 41, 42], [20, 42, 21]]);
        assert!(
            triangles
                .iter()
                .flatten()
                .all(|index| (*index as usize) < vertices.len())
        );
    }

    #[test]
    fn degenerate_inputs_never_produce_geometry() {
        for (center, radius, sweep) in [
            ([f32::NAN, 0.0], 12.0, 1.0),
            ([0.0, f32::INFINITY], 12.0, 1.0),
            ([0.0, 0.0], 0.0, 1.0),
            ([0.0, 0.0], -1.0, 1.0),
            ([0.0, 0.0], f32::INFINITY, 1.0),
            ([0.0, 0.0], 12.0, 0.0),
            ([0.0, 0.0], 12.0, f32::NAN),
        ] {
            assert!(SectorGeometry::new(center, radius, 0.0, sweep, 0.35, false).is_none());
        }
    }
}
