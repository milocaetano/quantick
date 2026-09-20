//! Panels and the layer each pane shows.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::PENDING_SURFACE;
use super::{Mapping, Source, UiBehaviour};

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
        reach: "toolbar sidebar button, View menu, Ctrl+B",
        keys: &[
            (Source::ToolbarAction, "ToggleDock"),
            (Source::Hotkey, "DOCK_SHORTCUT"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "layer.bubbles.toggle",
        title: "Switch the aggression bubbles on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetBubbles"),
            (Source::LayerToggle, "Bubbles"),
        ],
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
        id: "layer.footprint.toggle",
        title: "Switch the candle footprint on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetFootprint"),
            (Source::LayerToggle, "Footprint"),
        ],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.heatmap.toggle",
        title: "Switch the L2 depth map on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetHeatmap"),
            (Source::LayerToggle, "Heatmap"),
        ],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.live_strip.toggle",
        title: "Switch the live depth strip on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetLiveStrip"),
            (Source::LayerToggle, "LiveStrip"),
        ],
        mapping: LAYER_SWITCH,
    },
];
