//! One factual FLOW-to-chart transform, shared by paint, inspection and cache keys.
use crate::viewport::Viewport;
use quantick_orderflow::projection::{PriceWindow, flow_tape::FlowTapeDot};
use rust_decimal::prelude::ToPrimitive as _;

#[derive(Debug, Clone, Copy)]
pub struct FlowExecutionGeometry {
    viewport: Viewport,
    total: usize,
    prefix_len: usize,
    range: (f64, f64),
    prices: PriceWindow,
    history: [f32; 4],
    inverted: bool,
}

impl FlowExecutionGeometry {
    /// `history` is [left, top, right, bottom] in logical pixels, excluding Tape.
    pub fn new(
        viewport: Viewport,
        total: usize,
        prefix_len: usize,
        range: (f64, f64),
        history: [f32; 4],
        inverted: bool,
    ) -> Option<Self> {
        let prices = super::flow_price_window(range)?;
        Some(Self {
            viewport,
            total,
            prefix_len,
            range,
            prices,
            history,
            inverted,
        })
    }

    /// Exact key for every input affecting the transform or its visible clip.
    /// A resolved live/manual anchor is sufficient; its gesture state is not geometry.
    pub fn fingerprint(&self) -> [u64; 11] {
        [
            self.range.0.to_bits(),
            self.range.1.to_bits(),
            u64::from(self.history[0].to_bits()),
            u64::from(self.history[1].to_bits()),
            u64::from(self.history[2].to_bits()),
            u64::from(self.history[3].to_bits()),
            u64::from(self.inverted),
            u64::from(self.viewport.px_per_bar().to_bits()),
            u64::from(self.viewport.right_edge_bar(self.total).to_bits()),
            self.prefix_len as u64,
            self.total as u64,
        ]
    }

    /// Keep the viewport's fractional-bar operation order and unclamped price mapping.
    pub fn point(&self, dot: &FlowTapeDot) -> Option<(f32, f32)> {
        let position = dot.candle_position.to_f32()? + self.prefix_len as f32 - 0.5;
        Some((
            self.viewport
                .x_at_bar_position(position, self.history[2], self.total),
            super::flow_price_y(
                self.prices,
                dot.mark.price,
                self.history[1],
                self.history[3] - self.history[1],
                self.inverted,
            ),
        ))
    }
}

impl PartialEq for FlowExecutionGeometry {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint() == other.fingerprint()
    }
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;
