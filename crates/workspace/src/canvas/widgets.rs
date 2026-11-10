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
    /// The pane it opens on the canvas; `None` for a widget surface the
    /// window opens by `id` in a window of its own over the chart.
    pub kind: Option<PaneKind>,
}

const fn surface(id: &'static str, title: &'static str, line: &'static str) -> WidgetSpec {
    WidgetSpec {
        id,
        title,
        line,
        kind: None,
    }
}

/// Every widget the add menu lists, in menu order: the two charts, then the
/// widget surfaces under the ids the window's surface list declares.
pub static WIDGETS: &[WidgetSpec] = &[
    WidgetSpec {
        id: "timeframe-chart",
        title: "Timeframe chart",
        line: "Candles on a time interval",
        kind: Some(PaneKind::Time),
    },
    WidgetSpec {
        id: "order-flow",
        title: "Order flow",
        line: "Heatmap, footprint and tape",
        kind: Some(PaneKind::Flow),
    },
    surface("depth", "Depth", "The book's resting size by price"),
    surface(
        "big_trades",
        "Big trades",
        "The largest prints as they land",
    ),
    surface("liquidations", "Liquidations", "Forced closes by side"),
    surface(
        "open_interest",
        "OI + Funding",
        "Open interest and the funding rate",
    ),
    surface(
        "spot_perp_cvd",
        "Spot vs Perp CVD",
        "Cumulative delta, spot against perp",
    ),
];

/// The widget `id` names, if the registry has one.
#[must_use]
pub fn widget(id: &str) -> Option<&'static WidgetSpec> {
    WIDGETS.iter().find(|widget| widget.id == id.trim())
}
