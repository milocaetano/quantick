//! Exact regional quantities eligible for labels, and where the ones that
//! fit their sphere stand; the painter only measures and draws the text.
use super::{FlowBounds, FlowPresentation};
use quantick_orderflow::{TickBubbleStyle, projection::flow_tape::FlowTapeFrame};

/// The clear margin around a sphere's quantity label that no other label
/// may enter.
const LABEL_CLEARANCE_PX: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct FlowQuantityLabel {
    pub center: [f32; 2],
    pub radius: f32,
    pub text: String,
}

impl FlowPresentation {
    /// Every sphere big enough to carry its quantity, largest first.
    #[must_use]
    pub fn quantity_labels(
        &self,
        frame: &FlowTapeFrame,
        tick: Option<TickBubbleStyle>,
    ) -> Vec<FlowQuantityLabel> {
        let Some(style) = tick.map(TickBubbleStyle::bubbles) else {
            return Vec::new();
        };
        let mut labels: Vec<_> = self
            .regions
            .iter()
            .filter(|region| region.disc.radius >= style.label_min_radius)
            .map(|region| FlowQuantityLabel {
                center: region.disc.center,
                radius: region.disc.radius,
                text: quantick_orderflow::config::labels::format_quantity(
                    frame.dots[region.dot_index].mark.quantity,
                ),
            })
            .collect();
        labels.sort_by(|a, b| b.radius.total_cmp(&a.radius));
        labels
    }
}

/// The labels, in the order given (largest sphere first), that fit their
/// sphere — a text `measure` sizes no wider and no taller than `fit` times
/// the radius — inside `history` and clear of every one placed before, each
/// with the top-left corner its text starts at.
#[must_use]
pub fn place_quantity_labels(
    labels: Vec<FlowQuantityLabel>,
    history: FlowBounds,
    fit: [f32; 2],
    mut measure: impl FnMut(&FlowQuantityLabel) -> [f32; 2],
) -> Vec<([f32; 2], FlowQuantityLabel)> {
    let inside = |[x, y]: [f32; 2]| {
        history[0][0] <= x && x <= history[1][0] && history[0][1] <= y && y <= history[1][1]
    };
    let mut occupied = Vec::<FlowBounds>::new();
    let place = |label: FlowQuantityLabel| {
        let size = measure(&label);
        let fits = size[0] <= label.radius * fit[0] && size[1] <= label.radius * fit[1];
        let at = [0, 1].map(|axis| label.center[axis] - size[axis] / 2.0);
        let bounds = [
            at.map(|start| start - LABEL_CLEARANCE_PX),
            [0, 1].map(|axis| at[axis] + size[axis] + LABEL_CLEARANCE_PX),
        ];
        let free = fits
            && inside(bounds[0])
            && inside(bounds[1])
            && !occupied.iter().any(|other| {
                (0..2).all(|axis| {
                    other[0][axis] <= bounds[1][axis] && bounds[0][axis] <= other[1][axis]
                })
            });
        free.then(|| {
            occupied.push(bounds);
            (at, label)
        })
    };
    labels.into_iter().filter_map(place).collect()
}
