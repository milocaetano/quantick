//! Visual priority without changing regional membership, quantity or earned area.
use super::{FlowBounds, FlowDisc};
use quantick_orderflow::projection::flow_tape::{
    FlowTapeDot, FlowTapeFrame, LARGE_REGION_REFERENCE_DIVISOR,
};
use rust_decimal::Decimal;

mod placement_index;
use placement_index::PlacedIndex;

// Small leaves bound exact circle checks after spatial bounds reject a branch.
const CIRCLE_INDEX_LEAF_CAPACITY: usize = 8;
// Logical-pixel movement stays near the factual source, preserving local reading.
const MAX_PEAK_DISPLACEMENT_PX: usize = 24;
const PEAK_DISPLACEMENT_STEP_PX: usize = 2;
// Only relocated circles request breathing room; separate source circles stay put.
const RELOCATED_PEAK_GAP_PX: f32 = 1.0;
// Screen-space degrees: above-left first, then nearby upward alternatives.
const PEAK_DISPLACEMENT_ANGLES_DEGREES: [f32; 9] = [
    -135.0, -90.0, -45.0, -180.0, 0.0, -157.5, -112.5, -67.5, -22.5,
];

/// Paint in this order; context retains its factual circle behind local peaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowRegionRole {
    Opening,
    Context,
    Peak,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowRegionVisual {
    pub dot_index: usize,
    /// Earned circle after the uniform FLOW offset, before collision placement.
    pub source_disc: FlowDisc,
    /// Displayed circle; any placement change must preserve its radius.
    pub disc: FlowDisc,
    pub role: FlowRegionRole,
    /// No free candidate existed within the bounded search; the source stays visible.
    pub unresolved_overlap: bool,
}

/// One backend-independent plan for drawing and pointer selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlowPresentation {
    pub regions: Vec<FlowRegionVisual>,
}

impl FlowPresentation {
    pub fn new(
        frame: &FlowTapeFrame,
        history: FlowBounds,
        mut center: impl FnMut(&FlowTapeDot) -> Option<[f32; 2]>,
    ) -> Self {
        if !history.iter().flatten().all(|value| value.is_finite())
            || history[0][0] >= history[1][0]
            || history[0][1] >= history[1][1]
        {
            return Self::default();
        }
        let mut regions: Vec<_> = frame
            .dots
            .iter()
            .enumerate()
            .filter_map(|(dot_index, dot)| {
                let disc = center(dot).and_then(|at| FlowDisc::new(dot, at))?;
                disc.visible(history).then_some(FlowRegionVisual {
                    dot_index,
                    source_disc: disc,
                    disc,
                    role: if dot.opening_oversized {
                        FlowRegionRole::Opening
                    } else {
                        FlowRegionRole::Context
                    },
                    unresolved_overlap: false,
                })
            })
            .collect();
        let ordinary: Vec<_> = (0..regions.len())
            .filter(|&index| regions[index].role != FlowRegionRole::Opening)
            .collect();
        let threshold = frame.large_region_threshold().unwrap_or_else(|| {
            ordinary
                .iter()
                .map(|&index| frame.dots[regions[index].dot_index].mark.quantity)
                .max()
                .unwrap_or_default()
                / Decimal::from(LARGE_REGION_REFERENCE_DIVISOR)
        });
        // Camera spacing never promotes small volume out of the source trajectory.
        for &region_index in &ordinary {
            let region = regions[region_index];
            if frame.dots[region.dot_index].mark.quantity >= threshold {
                regions[region_index].role = FlowRegionRole::Peak;
            }
        }
        place_peaks(&mut regions, frame, history);
        regions.sort_by(|a, b| {
            a.role
                .cmp(&b.role)
                .then_with(|| {
                    frame.dots[a.dot_index]
                        .mark
                        .quantity
                        .cmp(&frame.dots[b.dot_index].mark.quantity)
                })
                .then_with(|| a.dot_index.cmp(&b.dot_index))
        });
        Self { regions }
    }

    /// Exact foreground circles win, then the nearest tiny-circle tolerance.
    pub fn hit(&self, history: FlowBounds, pointer: [f32; 2]) -> Option<usize> {
        let mut nearest: Option<(f32, usize)> = None;
        for region in self.regions.iter().rev() {
            let Some(distance) = region.disc.hit_distance(history, pointer) else {
                continue;
            };
            if distance <= region.disc.radius.powi(2) {
                return Some(region.dot_index);
            }
            if nearest.is_none_or(|(held, _)| distance < held) {
                nearest = Some((distance, region.dot_index));
            }
        }
        nearest.map(|(_, index)| index)
    }
}

#[inline]
fn intersects(a: FlowDisc, b: FlowDisc, gap: f32) -> bool {
    let dx = f64::from(a.center[0]) - f64::from(b.center[0]);
    let dy = f64::from(a.center[1]) - f64::from(b.center[1]);
    let separation = f64::from(a.radius) + f64::from(b.radius) + f64::from(gap);
    dx * dx + dy * dy < separation * separation
}

#[inline]
fn intersects_bounds(disc: FlowDisc, bounds: [[f64; 2]; 2], gap: f32) -> bool {
    if bounds[0][0] > bounds[1][0] {
        return false;
    }
    let x = f64::from(disc.center[0]);
    let y = f64::from(disc.center[1]);
    let dx = x - x.clamp(bounds[0][0], bounds[1][0]);
    let dy = y - y.clamp(bounds[0][1], bounds[1][1]);
    dx * dx + dy * dy < (f64::from(disc.radius) + f64::from(gap)).powi(2)
}

fn place_peaks(regions: &mut [FlowRegionVisual], frame: &FlowTapeFrame, history: FlowBounds) {
    let mut peaks: Vec<_> = (0..regions.len())
        .filter(|&index| regions[index].role == FlowRegionRole::Peak)
        .collect();
    peaks.sort_by(|&a, &b| {
        frame.dots[regions[b].dot_index]
            .mark
            .quantity
            .cmp(&frame.dots[regions[a].dot_index].mark.quantity)
            .then_with(|| regions[a].dot_index.cmp(&regions[b].dot_index))
    });
    let directions = PEAK_DISPLACEMENT_ANGLES_DEGREES.map(|degrees| {
        [
            degrees.to_radians().cos(),
            degrees.to_radians().sin().min(0.0),
        ]
    });
    let mut placed = PlacedIndex::new(regions, &peaks);
    for index in peaks {
        let original = regions[index].source_disc;
        // Cosmetic spacing never moves an already separate source circle.
        let mut selected = (!placed.overlaps(original, regions, 0.0)).then_some(original);
        if selected.is_none() {
            'search: for distance in (PEAK_DISPLACEMENT_STEP_PX..=MAX_PEAK_DISPLACEMENT_PX)
                .step_by(PEAK_DISPLACEMENT_STEP_PX)
            {
                for direction in directions {
                    let candidate = FlowDisc {
                        center: [
                            original.center[0] + direction[0] * distance as f32,
                            original.center[1] + direction[1] * distance as f32,
                        ],
                        ..original
                    };
                    let contained = (0..2).all(|axis| {
                        candidate.center[axis] - candidate.radius >= history[0][axis]
                            && candidate.center[axis] + candidate.radius <= history[1][axis]
                    });
                    if contained && !placed.overlaps(candidate, regions, RELOCATED_PEAK_GAP_PX) {
                        selected = Some(candidate);
                        break 'search;
                    }
                }
            }
        }
        regions[index].unresolved_overlap = selected.is_none();
        regions[index].disc = selected.unwrap_or(original);
        placed.insert(index, regions);
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
