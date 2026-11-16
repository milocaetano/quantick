//! The layer policy's vocabulary over the order-flow view: which owner
//! doors each [`OrderflowSwitch`] reads and writes through, and the facts
//! the config holds for the policy.
use super::OrderflowView as V;
pub(crate) use quantick_chart::orderflow_layers::{
    bbo_lines_block, bbo_lines_drawn, layer_facts, native_tape_block,
};
use quantick_layers::OrderflowSwitch;
use quantick_orderflow::HeatmapConfig;
/// How one switch reads its owner and writes through it: `(read, write)`.
struct Switch(fn(&V) -> bool, fn(&mut V, bool));
/// Indexed by [`OrderflowSwitch`], in its declaration order.
const SWITCHES: [Switch; 12] = [
    Switch(V::lane_enabled, V::set_lane_enabled),
    Switch(V::lane_depth_switched_on, V::set_lane_depth_visible),
    Switch(V::lane_bubbles_enabled, V::set_lane_bubbles_enabled),
    Switch(V::depth_switched_on, V::set_depth_visible),
    Switch(V::bubbles_enabled, V::set_bubbles_enabled),
    Switch(V::lane_marks_visible, V::set_lane_marks_visible),
    Switch(V::legend_visible, V::set_legend_visible),
    Switch(V::gaps_visible, V::set_gaps_visible),
    // A preset switch: one config field, through the one door every change uses.
    Switch(
        |view| view.cached_config().volume_dots.enabled,
        |view, on| commit(view, |config| config.volume_dots.enabled = on),
    ),
    Switch(
        |view| view.cached_config().live_lane.tape_only,
        |view, on| commit(view, |config| _ = config.live_lane.set_tape_only(on)),
    ),
    // The request, like tape only: whether the pane builds the native tape
    // is `HeatmapConfig::native_tape`, and the layer policy says why not.
    Switch(
        |view| view.cached_config().live_lane.native_tape,
        |view, on| commit(view, |config| config.live_lane.native_tape = on),
    ),
    Switch(
        |view| view.cached_config().live_lane.bbo_lines,
        |view, on| commit(view, |config| config.live_lane.bbo_lines = on),
    ),
];
fn commit(view: &mut V, change: impl FnOnce(&mut HeatmapConfig)) {
    view.edit_config(change);
}
/// Whether `switch` stands on for `view`.
pub(crate) fn layer_switch(view: &V, switch: OrderflowSwitch) -> bool {
    (SWITCHES[switch as usize].0)(view)
}
/// Move `switch` on `view` through the door its toolbar or menu uses.
pub(crate) fn set_layer_switch(view: &mut V, switch: OrderflowSwitch, visible: bool) {
    (SWITCHES[switch as usize].1)(view, visible);
}
