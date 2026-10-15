//! The retained display frame must match the factual source and current look.
use quantick_orderflow::{TickLook, ViewportBubbleScale, projection::flow_tape::FlowTapeFrame};
use std::sync::{Arc, Weak};

struct PaintedFrame {
    source: Weak<FlowTapeFrame>,
    look: Option<TickLook>,
    frame: Arc<FlowTapeFrame>,
}

/// Last-painted presentation, independent of a UI backend or its cache storage.
#[derive(Default)]
pub struct FlowPaintState {
    last: Option<PaintedFrame>,
}

impl FlowPaintState {
    pub fn clear(&mut self) {
        self.last = None;
    }

    pub fn record(
        &mut self,
        source: &Arc<FlowTapeFrame>,
        frame: Option<&Arc<FlowTapeFrame>>,
        look: Option<TickLook>,
    ) {
        self.last = frame.map(|frame| PaintedFrame {
            source: Arc::downgrade(source),
            look,
            frame: Arc::clone(frame),
        });
    }

    /// Read back the current-camera frame last painted. A changed look or
    /// source cannot expose a stale presentation as the new appearance.
    /// Legacy looks resume their published source without a presentation copy.
    pub fn frame_for(
        &self,
        source: &Arc<FlowTapeFrame>,
        look: Option<TickLook>,
    ) -> Option<Arc<FlowTapeFrame>> {
        if look
            .and_then(|(tick, _)| tick.viewport_scale)
            .and_then(ViewportBubbleScale::sanitized)
            .is_none()
        {
            return Some(Arc::clone(source));
        }
        let paint = self.last.as_ref()?;
        (paint.look == look
            && paint
                .source
                .upgrade()
                .is_some_and(|held| Arc::ptr_eq(&held, source)))
        .then(|| Arc::clone(&paint.frame))
    }
}

#[cfg(test)]
#[path = "paint_state_tests.rs"]
mod tests;
