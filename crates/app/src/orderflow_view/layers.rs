//! The layer policy's vocabulary over the order-flow view: which owner
//! doors each [`OrderflowSwitch`] reads and writes through, and the facts
//! the config holds for the policy.
use super::OrderflowView as V;
use quantick_layers::{ChartLayer, LayerBlock, LayerFacts, LayerState, OrderflowSwitch};
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
    Switch(V::status_badge_visible, V::set_status_badge_visible),
    Switch(V::gaps_visible, V::set_gaps_visible),
    // A preset switch: one config field, through the one door every change uses.
    Switch(
        |view| view.cached_config().volume_dots.enabled,
        |view, on| commit(view, |config| config.volume_dots.enabled = on),
    ),
    Switch(
        |view| view.cached_config().live_lane.tape_only,
        |view, on| commit(view, |config| config.live_lane.tape_only = on),
    ),
    // The request, like tape only: whether the pane builds the native tape
    // is `HeatmapConfig::native_tape`, and the layer policy says why not.
    Switch(
        |view| view.cached_config().live_lane.native_tape,
        |view, on| commit(view, |config| config.live_lane.native_tape = on),
    ),
];
fn commit(view: &mut V, change: impl FnOnce(&mut HeatmapConfig)) {
    view.edit_config(|config| {
        change(config);
        true
    });
}
/// Whether `switch` stands on for `view`.
pub(crate) fn layer_switch(view: &V, switch: OrderflowSwitch) -> bool {
    (SWITCHES[switch as usize].0)(view)
}
/// Move `switch` on `view` through the door its toolbar or menu uses.
pub(crate) fn set_layer_switch(view: &mut V, switch: OrderflowSwitch, visible: bool) {
    (SWITCHES[switch as usize].1)(view, visible);
}
/// The layer policy's facts `config` holds, one reading for the layer
/// call, the menu and the settings box; the pane adds its own.
pub(crate) fn layer_facts(config: &HeatmapConfig) -> LayerFacts {
    LayerFacts {
        flow_pane: true,
        tape_on: config.lane_enabled(),
        tape_only: config.tape_only(),
        native_tape: config.native_tape(),
        volume_dots: config.volume_dots.enabled,
        capture_enabled: config.enabled,
        depth_visible: config.depth_visible(),
        ..LayerFacts::default()
    }
}
/// Why the native tape switch cannot move for `config`, as the layer call says.
pub(crate) fn native_tape_block(config: &HeatmapConfig) -> Option<LayerBlock> {
    LayerState::blocked(ChartLayer::NativeTape, layer_facts(config))
}
