//! Earned FLOW circles, independent of a drawing backend.
use quantick_orderflow::projection::flow_tape::FlowTapeDot;
use rust_decimal::Decimal;

use super::constants::FLOW_EXECUTION_OFFSET;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_visibility_preserves_the_circle_and_clip() {
        let history = [[0.0, 0.0], [100.0, 100.0]];
        let disc = FlowDisc {
            center: [-10.0, 50.0],
            radius: 12.0,
        };
        assert!(disc.visible(history));
        assert!(
            !FlowDisc {
                center: [-10.0, -10.0],
                radius: 12.0
            }
            .visible(history)
        );
        assert!(!disc.visible([[100.0, 0.0], [0.0, 100.0]]));
    }
}
