//! Earned FLOW circles and pointer selection, independent of a drawing backend.
use quantick_orderflow::projection::flow_tape::{FlowTapeDot, FlowTapeFrame};
use rust_decimal::Decimal;

use super::constants::{FLOW_EXECUTION_OFFSET, FLOW_POINTER_RADIUS_PX};

/// Minimum and maximum corners in logical pixels.
pub type FlowBounds = [[f32; 2]; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowDisc {
    pub center: [f32; 2],
    pub radius: f32,
}

impl FlowDisc {
    pub fn new(dot: &FlowTapeDot, center: [f32; 2]) -> Option<Self> {
        (center.iter().all(|coordinate| coordinate.is_finite())
            && dot.radius.is_finite()
            && dot.radius > 0.0
            && dot.mark.quantity > Decimal::ZERO)
            .then_some(Self {
                center: [
                    center[0] + FLOW_EXECUTION_OFFSET[0],
                    center[1] + FLOW_EXECUTION_OFFSET[1],
                ],
                radius: dot.radius,
            })
    }

    pub fn visible(self, history: FlowBounds) -> bool {
        if history[1][0] < history[0][0] || history[1][1] < history[0][1] {
            return false;
        }
        let distance = |axis| {
            if history[0][axis] > self.center[axis] {
                history[0][axis] - self.center[axis]
            } else if self.center[axis] > history[1][axis] {
                self.center[axis] - history[1][axis]
            } else {
                0.0
            }
        };
        let (dx, dy) = (distance(0), distance(1));
        dx * dx + dy * dy < self.radius.powi(2)
    }

    pub fn hit_distance(self, history: FlowBounds, pointer: [f32; 2]) -> Option<f32> {
        let dx = self.center[0] - pointer[0];
        let dy = self.center[1] - pointer[1];
        let distance = dx * dx + dy * dy;
        let inside = history[0][0] <= pointer[0]
            && pointer[0] <= history[1][0]
            && history[0][1] <= pointer[1]
            && pointer[1] <= history[1][1];
        (inside
            && self.visible(history)
            && distance <= self.radius.max(FLOW_POINTER_RADIUS_PX).powi(2))
        .then_some(distance)
    }
}

/// Select the top painted circle first, then the nearest tiny-circle tolerance.
pub fn hit_flow_region(
    frame: &FlowTapeFrame,
    history: FlowBounds,
    pointer: [f32; 2],
    center: impl FnMut(&FlowTapeDot) -> Option<[f32; 2]>,
) -> Option<&FlowTapeDot> {
    let presentation = super::FlowPresentation::new(frame, history, center);
    presentation
        .hit(history, pointer)
        .map(|index| &frame.dots[index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_visibility_and_pointer_tolerance_preserve_the_circle_and_clip() {
        let history = [[0.0, 0.0], [100.0, 100.0]];
        let disc = FlowDisc {
            center: [-10.0, 50.0],
            radius: 12.0,
        };
        assert!(disc.visible(history));
        assert_eq!(disc.hit_distance(history, [1.0, 50.0]), Some(121.0));
        assert_eq!(disc.hit_distance(history, disc.center), None);
        assert!(
            !FlowDisc {
                center: [-10.0, -10.0],
                radius: 12.0
            }
            .visible(history)
        );
        assert!(!disc.visible([[100.0, 0.0], [0.0, 100.0]]));
        let tiny = FlowDisc {
            center: [50.0, 50.0],
            radius: 0.1,
        };
        assert_eq!(tiny.hit_distance(history, [53.0, 50.0]), Some(9.0));
        assert_eq!(tiny.radius, 0.1);
    }
}
