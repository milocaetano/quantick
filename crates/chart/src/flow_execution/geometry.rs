//! One factual FLOW-to-chart transform, shared by paint and cache keys.
use crate::viewport::Viewport;
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowTapeDot, FlowTapeFrame},
};
use rust_decimal::{Decimal, prelude::ToPrimitive as _};

#[derive(Debug, Clone, Copy)]
pub struct FlowExecutionGeometry {
    viewport: Viewport,
    total: usize,
    prefix_len: usize,
    range: (f64, f64),
    prices: PriceWindow,
    history: [f32; 4],
    inverted: bool,
    display_scale: f32,
}

impl FlowExecutionGeometry {
    /// Manual gain belongs to presentation, never the worker's merging camera.
    #[must_use]
    pub fn with_display_scale(mut self, scale: f32) -> Self {
        self.display_scale = quantick_orderflow::sane_bubble_display_scale(scale);
        self
    }

    /// Build the paint plan and its matching optional current-camera frame.
    /// The backend uses the same dot indices for sectors, labels and readback.
    pub fn presentation(
        &self,
        frame: &FlowTapeFrame,
        tick: Option<quantick_orderflow::TickBubbleStyle>,
        clip: super::FlowBounds,
    ) -> (
        super::FlowPresentation,
        Option<std::sync::Arc<FlowTapeFrame>>,
    ) {
        let displayed = self
            .viewport_frame(frame, tick, clip)
            .map(std::sync::Arc::new);
        let legacy_gain = tick.is_none() && self.display_scale != 1.0;
        let mut plan = super::FlowPresentation::styled(
            if legacy_gain {
                frame
            } else {
                displayed.as_deref().unwrap_or(frame)
            },
            clip,
            tick,
            |dot| self.point(dot).map(|(x, y)| [x, y]),
        );
        if legacy_gain {
            // Legacy peak placement keeps its original collision inputs.
            for region in &mut plan.regions {
                region.source_disc.radius *= self.display_scale;
                region.disc.radius *= self.display_scale;
            }
        }
        (plan, displayed)
    }

    /// Current-camera presentation, independent of the worker's held grouping
    /// scale. Identity gains keep legacy appearances on their published frame.
    pub fn viewport_frame(
        &self,
        frame: &FlowTapeFrame,
        tick: Option<quantick_orderflow::TickBubbleStyle>,
        clip: super::FlowBounds,
    ) -> Option<FlowTapeFrame> {
        let Some((tick, calibration)) = tick.and_then(|tick| {
            tick.viewport_scale?
                .sanitized()
                .map(|calibration| (tick, calibration))
        }) else {
            if self.display_scale == 1.0 {
                return None;
            }
            let mut displayed = frame.clone();
            self.scale_radii(&mut displayed);
            return Some(displayed);
        };
        let gain = calibration.radius_gain(
            f64::from(self.viewport.px_per_bar()),
            f64::from(self.history[3] - self.history[1]) / (self.range.1 - self.range.0),
        );
        let mut off_axis = 0;
        let mut offscreen = 0;
        let mut displayed = frame.viewport_relative(tick.bubbles().max_radius * gain, |dot| {
            let Some((x, y)) = self.point(dot) else {
                off_axis += dot.members.len();
                return false;
            };
            if y < clip[0][1] || y > clip[1][1] {
                off_axis += dot.members.len();
                false
            } else if x < clip[0][0] || x > clip[1][0] {
                offscreen += dot.members.len();
                false
            } else {
                true
            }
        });
        // The retained publication cannot recover source already excluded by
        // its worker. Add current-camera exclusions to those inherited counts.
        displayed.off_axis_executions += off_axis;
        displayed.offscreen_executions += offscreen;
        let position = |x| {
            Decimal::from_f32_retain(
                self.viewport.bar_at_x(x, self.history[2], self.total) - self.prefix_len as f32
                    + 0.5,
            )
            .unwrap_or_default()
        };
        displayed.view.clip_left = position(clip[0][0]);
        displayed.view.clip_right = position(clip[1][0]);
        displayed.view.prices = self.prices;
        self.scale_radii(&mut displayed);
        Some(displayed)
    }

    fn scale_radii(&self, frame: &mut FlowTapeFrame) {
        if self.display_scale != 1.0 {
            frame.view.radius_limit *= self.display_scale;
            for dot in &mut frame.dots {
                dot.radius *= self.display_scale;
            }
        }
    }

    /// `history` is [left, top, right, bottom] in logical pixels, excluding Tape.
    pub fn new(
        viewport: Viewport,
        total: usize,
        prefix_len: usize,
        range: (f64, f64),
        history: [f32; 4],
        inverted: bool,
    ) -> Option<Self> {
        let prices = PriceWindow::from_f64_range(range)?;
        Some(Self {
            viewport,
            total,
            prefix_len,
            range,
            prices,
            history,
            inverted,
            display_scale: 1.0,
        })
    }

    /// Exact key for every input affecting the transform or its visible clip.
    /// A resolved live/manual anchor is sufficient; its gesture state is not geometry.
    pub fn fingerprint(&self) -> [u64; 12] {
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
            u64::from(self.display_scale.to_bits()),
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
