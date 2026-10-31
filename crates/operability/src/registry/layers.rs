//! Panels and the layer each pane shows.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{Mapping, PENDING_SURFACE, Source, UiBehaviour};

/// A chart layer switch: the header glyph and the pane's layer menu both set
/// one pane's layer, which `layers.visibility.set` does by stable pane ID.
const LAYER_SWITCH: Mapping = capability!("layers.visibility.set");

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "dock.tab.open",
        title: "Open a panel — L2, bubbles, session, trading or trades",
        reach: "View menu, the dock's own strip, a header glyph's right-click",
        keys: &[
            (Source::DockTab, "l2"),
            (Source::DockTab, "bubbles"),
            (Source::DockTab, "session"),
            (Source::DockTab, "trading"),
            (Source::DockTab, "trades"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "dock.toggle",
        title: "Show or hide the panels dock",
        reach: "toolbar sidebar button, View menu, Ctrl+Shift+B",
        keys: &[
            (Source::ToolbarAction, "ToggleDock"),
            (Source::Hotkey, "DOCK_SHORTCUT"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "layer.bubbles.toggle",
        title: "Switch the aggression bubbles on or off",
        reach: "pane header glyph, pane right-click layer menu, Ctrl+B",
        keys: &[(Source::LayerToggle, "Bubbles")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.footprint.settings.open",
        title: "Open the footprint's settings window",
        reach: "right-click the pane header's footprint glyph; the pane's footprint menu",
        keys: &[(
            Source::Authored,
            "a pane header glyph's right click and the pane menu's entry, not entries in the toolbar's action enum",
        )],
        mapping: capability!("chart.settings.set"),
    },
    UiBehaviour {
        id: "layer.candle_aggression.toggle",
        title: "Show discreet aggression dots over tick candles",
        reach: "pane right-click layer menu, off until switched on",
        keys: &[(
            Source::Authored,
            "the per-pane candle aggression layer is opt-in and has no pane header glyph",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.footprint.toggle",
        title: "Switch the candle footprint on or off",
        reach: "pane header glyph, pane right-click layer menu, Ctrl+F",
        keys: &[(Source::LayerToggle, "Footprint")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.heatmap.toggle",
        title: "Switch the L2 depth map on or off",
        reach: "pane header glyph, pane right-click layer menu",
        keys: &[(Source::LayerToggle, "Heatmap")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.live_strip.toggle",
        title: "Switch the live depth strip on or off",
        reach: "pane header glyph, pane right-click layer menu",
        keys: &[(Source::LayerToggle, "LiveStrip")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.tape_only.toggle",
        title: "Give the tape its own canvas without candles",
        reach: "pane right-click layer menu; Bubbles settings, Tape only (hide candles)",
        keys: &[(
            Source::Authored,
            "the per-pane order-flow menu and settings checkbox are not pane header LayerToggle entries",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.native_tape.toggle",
        title: "Draw the tape at execution time and price beside the candles",
        reach: "pane right-click layer menu; Bubbles settings, Native tape (execution time and price)",
        keys: &[(
            Source::Authored,
            "the per-pane order-flow menu and settings checkbox are not pane header LayerToggle entries",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "orderflow.tape.view.set",
        title: "Show the candles and tape, the candles alone or the tape alone",
        reach: "pane header's view button; Ctrl+Shift+T on a focused flow pane; let go of the tape lane's divider at the left or right edge of the pane",
        keys: &[(Source::Hotkey, "TAPE_VIEW_SHORTCUT")],
        mapping: capability!("orderflow.tape.view.set", "orderflow.tape.fold.set"),
    },
    UiBehaviour {
        id: "orderflow.tape.opening_scale.set",
        title: "Choose whether the first recorded burst sets Tape or FLOW region size references",
        reach: "Bubbles settings for Tape; tick FLOW chart layers, Bubbles, Ignore first recorded burst in regional scale for the independent pane preference",
        keys: &[(
            Source::Authored,
            "opening-scale preferences are checkboxes inside the volume-dot settings and the tick FLOW Bubbles layer menu",
        )],
        mapping: capability!("orderflow.tape.opening_scale.set"),
    },
    UiBehaviour {
        id: "orderflow.bubbles.save_changes.set",
        title: "Choose whether bubble changes are saved for the asset on screen",
        reach: "Bubbles settings, Save changes for this asset",
        keys: &[(
            Source::Authored,
            "the per-asset save switch is a checkbox under the bubble preset picker",
        )],
        mapping: capability!("orderflow.bubbles.save_changes.set"),
    },
    UiBehaviour {
        id: "orderflow.bubbles.size.set",
        title: "Adjust Tape and tick-chart bubble size independently",
        reach: "drag a sample bubble on the flow pane's header; double-click it for 1x",
        keys: &[(
            Source::Authored,
            "independent sample bubbles appear while their bubble display is active",
        )],
        mapping: capability!("orderflow.bubbles.size.set"),
    },
    UiBehaviour {
        id: "orderflow.bubbles.tick.set",
        title: "Select tick-chart spheres for a config-mode asset independently of the tape",
        reach: "Bubbles settings, Tick chart spheres preset picker",
        keys: &[(
            Source::Authored,
            "chart-only preset picker under the whole-look presets",
        )],
        mapping: capability!("orderflow.bubbles.tick.set"),
    },
    UiBehaviour {
        id: "orderflow.bubbles.preset.apply",
        title: "Apply or reapply a named whole bubble look",
        reach: "Bubbles settings, whole-look preset picker",
        keys: &[(
            Source::Authored,
            "whole-look preset selection permits same-name reapplication",
        )],
        mapping: capability!("orderflow.bubbles.preset.apply"),
    },
];
