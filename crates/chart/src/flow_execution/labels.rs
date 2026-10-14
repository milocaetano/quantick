//! Exact regional quantities eligible for labels; the painter checks font fit.
use super::FlowPresentation;
use quantick_orderflow::{TickBubbleStyle, projection::flow_tape::FlowTapeFrame};

pub struct FlowQuantityLabel {
    pub center: [f32; 2],
    pub radius: f32,
    pub text: String,
}

impl FlowPresentation {
    #[must_use]
    pub fn quantity_labels(
        &self,
        frame: &FlowTapeFrame,
        tick: Option<TickBubbleStyle>,
    ) -> Vec<FlowQuantityLabel> {
        let Some(style) = tick.map(TickBubbleStyle::bubbles) else {
            return Vec::new();
        };
        self.regions
            .iter()
            .filter(|region| region.disc.radius >= style.label_min_radius)
            .map(|region| FlowQuantityLabel {
                center: region.disc.center,
                radius: region.disc.radius,
                text: tick
                    .expect("a tick style was selected")
                    .quantity_label(frame.dots[region.dot_index].mark.quantity),
            })
            .collect()
    }
}
