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
    /// Every sphere big enough to carry its quantity, largest first; under
    /// peak spheres, only the largest masses
    /// ([`FlowTapeFrame::peak_label_floor`]).
    #[must_use]
    pub fn quantity_labels(
        &self,
        frame: &FlowTapeFrame,
        tick: Option<TickBubbleStyle>,
    ) -> Vec<FlowQuantityLabel> {
        let Some(style) = tick.map(TickBubbleStyle::bubbles) else {
            return Vec::new();
        };
        let floor = frame.peak_label_floor();
        let mut labels: Vec<_> = self
            .regions
            .iter()
            .filter(|region| floor.is_some() || region.disc.radius >= style.label_min_radius)
            .filter(|region| {
                floor.is_none_or(|floor| frame.dots[region.dot_index].mark.quantity >= floor)
            })
            .map(|region| FlowQuantityLabel {
                center: region.disc.center,
                radius: region.disc.radius,
                text: if floor.is_some() {
                    quantick_orderflow::config::labels::format_quantity_compact(
                        frame.dots[region.dot_index].mark.quantity,
                    )
                } else {
                    quantick_orderflow::config::labels::format_quantity(
                        frame.dots[region.dot_index].mark.quantity,
                    )
                },
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
    measure: impl FnMut(&FlowQuantityLabel) -> [f32; 2],
) -> Vec<([f32; 2], FlowQuantityLabel)> {
    place_quantity_labels_until_miss(labels, history, fit, measure, false)
}

/// [`place_quantity_labels`], and when `stop_at_miss`, the first sphere too
/// small for its text ends the placement, so no larger sphere goes
/// unlabelled for want of room beside a smaller labelled one. A label off
/// screen, or one that would cover a label already placed, is skipped alone:
/// the one exception to that order. Peak spheres place their labels so.
#[must_use]
pub fn place_quantity_labels_until_miss(
    labels: Vec<FlowQuantityLabel>,
    history: FlowBounds,
    fit: [f32; 2],
    mut measure: impl FnMut(&FlowQuantityLabel) -> [f32; 2],
    stop_at_miss: bool,
) -> Vec<([f32; 2], FlowQuantityLabel)> {
    let mut stopped = false;
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
        let on_screen = inside(bounds[0]) && inside(bounds[1]);
        let free = !stopped
            && fits
            && on_screen
            && !occupied.iter().any(|other| {
                (0..2).all(|axis| {
                    other[0][axis] <= bounds[1][axis] && bounds[0][axis] <= other[1][axis]
                })
            });
        stopped |= stop_at_miss && !fits;
        free.then(|| {
            occupied.push(bounds);
            (at, label)
        })
    };
    labels.into_iter().filter_map(place).collect()
}
