//! What the canvas's add button offers. A later widget is one entry here.

use super::presets::PaneKind;

/// One thing the add menu can put on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetSpec {
    /// The stable wire name the control plane adds it by.
    pub id: &'static str,
    /// The menu row's title.
    pub title: &'static str,
    /// The one line under the title.
    pub line: &'static str,
    /// The pane it opens.
    pub kind: PaneKind,
}

/// Every widget the add menu lists, in menu order.
pub static WIDGETS: &[WidgetSpec] = &[
    WidgetSpec {
        id: "timeframe-chart",
        title: "Timeframe chart",
        line: "Candles on a time interval",
        kind: PaneKind::Time,
    },
    WidgetSpec {
        id: "order-flow",
        title: "Order flow",
        line: "Heatmap, footprint and tape",
        kind: PaneKind::Flow,
    },
];

/// The widget `id` names, if the registry has one.
#[must_use]
pub fn widget(id: &str) -> Option<&'static WidgetSpec> {
    WIDGETS.iter().find(|widget| widget.id == id.trim())
}
