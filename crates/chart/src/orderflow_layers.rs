//! The layer policy's reading of an order-flow config: the facts the config
//! holds for it, and what the policy says about the two tape switches.
use quantick_layers::{ChartLayer, LayerBlock, LayerFacts, LayerState};
use quantick_orderflow::HeatmapConfig;

/// The layer policy's facts `config` holds, one reading for the layer
/// call, the menu and the settings box; the pane adds its own.
pub fn layer_facts(config: &HeatmapConfig) -> LayerFacts {
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
pub fn native_tape_block(config: &HeatmapConfig) -> Option<LayerBlock> {
    LayerState::blocked(ChartLayer::NativeTape, layer_facts(config))
}
/// Why the best bid/ask lines switch cannot move for `config`.
pub fn bbo_lines_block(config: &HeatmapConfig) -> Option<LayerBlock> {
    LayerState::blocked(ChartLayer::TapeBboLines, layer_facts(config))
}
/// Whether the best bid/ask lines draw for `config`, as the layer call reads back.
pub fn bbo_lines_drawn(config: &HeatmapConfig) -> bool {
    let layer = ChartLayer::TapeBboLines;
    LayerState::effective(layer, config.live_lane.bbo_lines, layer_facts(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_draw_exactly_when_the_layer_reads_back_drawn() {
        let mut config = quantick_orderflow::HeatmapConfig {
            enabled: true,
            ..quantick_orderflow::HeatmapConfig::default()
        };
        assert!(
            bbo_lines_drawn(&config),
            "on by default while the book is captured"
        );
        config.enabled = false;
        assert!(
            !bbo_lines_drawn(&config),
            "no capture, no lines, as the readback says"
        );
        config.enabled = true;
        config.live_lane.bbo_lines = false;
        assert!(!bbo_lines_drawn(&config));
    }
}
