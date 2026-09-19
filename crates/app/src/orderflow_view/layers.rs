//! Layer operations dock directly on the order-flow owner.
use super::OrderflowView;
use quantick_layers::OrderflowSwitch;
struct Switch {
    read: fn(&OrderflowView) -> bool,
    write: fn(&mut OrderflowView, bool),
}
const SWITCHES: [Switch; 9] = [
    Switch {
        read: OrderflowView::lane_enabled,
        write: OrderflowView::set_lane_enabled,
    },
    Switch {
        read: OrderflowView::lane_depth_switched_on,
        write: OrderflowView::set_lane_depth_visible,
    },
    Switch {
        read: OrderflowView::lane_bubbles_enabled,
        write: OrderflowView::set_lane_bubbles_enabled,
    },
    Switch {
        read: OrderflowView::depth_switched_on,
        write: OrderflowView::set_depth_visible,
    },
    Switch {
        read: OrderflowView::bubbles_enabled,
        write: OrderflowView::set_bubbles_enabled,
    },
    Switch {
        read: OrderflowView::lane_marks_visible,
        write: OrderflowView::set_lane_marks_visible,
    },
    Switch {
        read: OrderflowView::legend_visible,
        write: OrderflowView::set_legend_visible,
    },
    Switch {
        read: OrderflowView::status_badge_visible,
        write: OrderflowView::set_status_badge_visible,
    },
    Switch {
        read: OrderflowView::gaps_visible,
        write: OrderflowView::set_gaps_visible,
    },
];
impl OrderflowView {
    pub(crate) fn layer_switch(&self, switch: OrderflowSwitch) -> bool {
        (SWITCHES[switch as usize].read)(self)
    }
    pub(crate) fn set_layer_switch(&mut self, switch: OrderflowSwitch, visible: bool) {
        (SWITCHES[switch as usize].write)(self, visible);
    }
}
