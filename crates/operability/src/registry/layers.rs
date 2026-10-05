//! Panels and the layer each pane shows.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{Mapping, PENDING_SURFACE, Source, UiBehaviour};

/// A chart layer switch: the toolbar button and the pane's layer menu both set
/// one pane's layer, which `layers.visibility.set` does by stable pane ID.
const LAYER_SWITCH: Mapping = capability!("layers.visibility.set");

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "dock.tab.open",
        title: "Open a panel — L2, bubbles, session, trading or trades",
        reach: "View menu, the dock's own strip, a layer button's right-click",
        keys: &[
            (Source::ToolbarAction, "OpenDockTab"),
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
        id: "layer.toolbar.set",
        title: "Set a chart layer from its toolbar control",
        reach: "toolbar LAYERS group",
        keys: &[(Source::ToolbarAction, "SetLayer")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.bubbles.toggle",
        title: "Switch the aggression bubbles on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu, Ctrl+B",
        keys: &[(Source::LayerToggle, "Bubbles")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.footprint.settings.open",
        title: "Open the footprint's settings window",
        reach: "right-click the toolbar's footprint button",
        keys: &[(Source::ToolbarAction, "OpenFootprintSettings")],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "layer.candle_aggression.toggle",
        title: "Show discreet aggression dots over tick candles",
        reach: "pane right-click layer menu, off until switched on",
        keys: &[(
            Source::Authored,
            "the per-pane candle aggression layer is opt-in and has no toolbar duplicate",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.footprint.toggle",
        title: "Switch the candle footprint on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu, Ctrl+F",
        keys: &[(Source::LayerToggle, "Footprint")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.heatmap.toggle",
        title: "Switch the L2 depth map on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[(Source::LayerToggle, "Heatmap")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.live_strip.toggle",
        title: "Switch the live depth strip on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[(Source::LayerToggle, "LiveStrip")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.tape_only.toggle",
        title: "Give the tape its own canvas without candles",
        reach: "pane right-click layer menu; Bubbles settings, Tape only (hide candles)",
        keys: &[(
            Source::Authored,
            "the per-pane order-flow menu and settings checkbox are not toolbar LayerToggle entries",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.native_tape.toggle",
        title: "Draw the tape at execution time and price beside the candles",
        reach: "pane right-click layer menu; Bubbles settings, Native tape (execution time and price)",
        keys: &[(
            Source::Authored,
            "the per-pane order-flow menu and settings checkbox are not toolbar LayerToggle entries",
        )],
        mapping: LAYER_SWITCH,
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
];
