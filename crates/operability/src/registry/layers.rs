//! Panels and the layer each pane shows.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{Mapping, PENDING_SURFACE, Source, UiBehaviour};

/// A chart layer switch: the capsule or header glyph and the pane's layer menu both set
/// one pane's layer, which `layers.visibility.set` does by stable pane ID.
const LAYER_SWITCH: Mapping = capability!("layers.visibility.set");

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "dock.tab.open",
        title: "Open a panel — session, trading or trades",
        reach: "window menu, Panels, the dock's own strip",
        keys: &[
            (Source::DockTab, "session"),
            (Source::DockTab, "trading"),
            (Source::DockTab, "trades"),
            (Source::MenuEntry, "L2"),
            (Source::MenuEntry, "Bubbles"),
            (Source::MenuEntry, "Session"),
            (Source::MenuEntry, "Paper trading"),
            (Source::MenuEntry, "Trades"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "dock.toggle",
        title: "Show or hide the panels dock",
        reach: "toolbar sidebar button, window menu, Ctrl+Shift+B",
        keys: &[
            (Source::ToolbarAction, "ToggleDock"),
            (Source::Hotkey, "DOCK_SHORTCUT"),
            (Source::MenuEntry, "Show panels"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "layer.bubbles.toggle",
        title: "Switch the aggression bubbles on or off",
        reach: "trail capsule glyph, pane right-click layer menu, Ctrl+B",
        keys: &[(Source::LayerToggle, "Bubbles")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.footprint.settings.open",
        title: "Open the footprint's settings window",
        reach: "right-click the capsule's footprint glyph; the pane's footprint menu",
        keys: &[(
            Source::Authored,
            "a capsule glyph's right click and the pane menu's entry, not entries in the toolbar's action enum",
        )],
        mapping: capability!("chart.settings.set"),
    },
    UiBehaviour {
        id: "layer.candle_aggression.toggle",
        title: "Show discreet aggression dots over tick candles",
        reach: "pane right-click layer menu, off until switched on",
        keys: &[(
            Source::Authored,
            "the per-pane candle aggression layer is opt-in and has no glyph on a capsule or header",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.footprint.toggle",
        title: "Switch the candle footprint on or off",
        reach: "pane capsule glyph, pane right-click layer menu, Ctrl+F",
        keys: &[(Source::LayerToggle, "Footprint")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.heatmap.toggle",
        title: "Switch the L2 depth map on or off",
        reach: "capsule glyph, pane right-click layer menu",
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
        id: "layer.tape_bubbles.toggle",
        title: "Switch the bubbles on the tape on or off",
        reach: "tape capsule glyph, pane right-click layer menu",
        keys: &[(Source::LayerToggle, "TapeBubbles")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.tape_heatmap.toggle",
        title: "Switch the book on the tape on or off",
        reach: "tape capsule glyph, pane right-click layer menu",
        keys: &[(Source::LayerToggle, "TapeHeatmap")],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "layer.tape_only.toggle",
        title: "Give the tape its own canvas without candles",
        reach: "pane right-click layer menu; Bubbles settings, Tape only (hide trail)",
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
        id: "layer.tape_bbo_lines.toggle",
        title: "Draw the best bid and best ask as bold stepped lines on the tape",
        reach: "pane right-click layer menu; Bubbles settings, Best bid/ask lines",
        keys: &[(
            Source::Authored,
            "the per-pane order-flow menu and settings checkbox are not pane header LayerToggle entries",
        )],
        mapping: LAYER_SWITCH,
    },
    UiBehaviour {
        id: "orderflow.tape.fold.set",
        title: "Close the candles or the tape to a rail, or bring it back",
        reach: "drag the tape lane's divider to the left or right edge of the pane; click or drag the rail either single view keeps",
        keys: &[(
            Source::Authored,
            "the tape lane's divider and rail the pane handles directly",
        )],
        mapping: capability!("orderflow.tape.fold.set"),
    },
    UiBehaviour {
        id: "orderflow.tape.market_fold.set",
        title: "Fold the tape while the market is closed",
        reach: "the checkbox in the tape section of the Bubbles settings",
        keys: &[(
            Source::Authored,
            "the per-asset checkbox in the Bubbles settings is not a pane header control",
        )],
        mapping: capability!("orderflow.tape.market_fold.set"),
    },
    UiBehaviour {
        id: "orderflow.settings.open",
        title: "Open the order-flow panel, on the L2 or the Bubbles section if one is named",
        reach: "the flow pane's gear and its right-click menu; window menu, Panels; a capsule glyph's right-click; Ctrl+Shift+O",
        keys: &[(Source::Hotkey, "ORDERFLOW_SETTINGS_SHORTCUT")],
        mapping: capability!("orderflow.settings.open"),
    },
    UiBehaviour {
        id: "orderflow.settings.close",
        title: "Close the order-flow panel",
        reach: "the panel's ×; the gear's left click; Ctrl+Shift+O",
        keys: &[(
            Source::Authored,
            "the panel's close button is drawn by the panel, not a pane header entry",
        )],
        mapping: capability!("orderflow.settings.close"),
    },
    UiBehaviour {
        id: "orderflow.settings.section.set",
        title: "Fold or open a section of the order-flow panel, for the asset on screen",
        reach: "a click on the section's header in the panel",
        keys: &[(
            Source::Authored,
            "the panel's collapsible headers are drawn by the panel, not a pane header entry",
        )],
        mapping: capability!("orderflow.settings.section.set"),
    },
    UiBehaviour {
        id: "orderflow.tape.view.set",
        title: "Show the trail and tape, the trail alone or the tape alone",
        reach: "pane header's Trail and Tape switches; Ctrl+Shift+R and Ctrl+Shift+T on a focused flow pane",
        keys: &[
            (Source::Hotkey, "TAPE_VIEW_SHORTCUT"),
            (Source::Hotkey, "TRAIL_VIEW_SHORTCUT"),
        ],
        mapping: capability!("orderflow.tape.view.set"),
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
    UiBehaviour {
        id: "orderflow.bubbles.reset",
        title: "Reset the bubble look to the asset's factory preset",
        reach: "Bubbles settings, reset bubble visuals",
        keys: &[(
            Source::Authored,
            "the reset button at the foot of the Bubbles settings",
        )],
        mapping: capability!("orderflow.bubbles.reset"),
    },
];
