//! The widget panes (Depth, Big Trades, Liquidations, OI + Funding, Spot vs
//! Perp CVD) over the control plane: open one in its floating window, close
//! it, and read what it paints. Presentation only: nothing a chart holds
//! changes, and nothing is saved.
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control_host::authority::{ReadSpec, WIDGET_MODULE_ID, read_descriptor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};

pub const WIDGET_OPEN_CAPABILITY_ID: &str = "widget.open";
pub const WIDGET_CLOSE_CAPABILITY_ID: &str = "widget.close";
pub const WIDGET_READ_CAPABILITY_ID: &str = "widget.read";
pub const WIDGET_CHANGED_EVENT_KIND: &str = "widget.changed";

/// The most text items a read returns; a pane paints far fewer.
pub const MAX_READ_TEXTS: usize = 512;

/// Which pane.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WidgetInput {
    /// `depth`, `big_trades`, `liquidations`, `open_interest` or
    /// `spot_perp_cvd`.
    pub id: String,
}

/// A pane after an open or a close.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WidgetState {
    pub id: String,
    pub title: String,
    /// Whether its window is on screen now.
    pub open: bool,
    /// Whether the active tab's feed provides what the pane shows; when
    /// false the pane says so instead of drawing data.
    pub provided: bool,
}

/// A header chip and whether it is the one in force.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WidgetChip {
    pub text: String,
    pub on: bool,
}

/// What the pane paints for the active tab, built from the same scene the
/// window draws.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WidgetRead {
    #[serde(flatten)]
    pub state: WidgetState,
    /// The active tab's market, as the header names it.
    pub market: String,
    pub chips: Vec<WidgetChip>,
    /// Data rows laid out: ladder rows, prints, liquidations or points.
    pub rows: u64,
    /// What the pane says instead of data, when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty: Option<String>,
    /// Every text item in paint order, at most [`MAX_READ_TEXTS`].
    pub texts: Vec<String>,
}

pub fn open_descriptor() -> CapabilityDescriptor {
    crate::layout::transient_descriptor::<WidgetInput, WidgetState>(
        WIDGET_OPEN_CAPABILITY_ID,
        WIDGET_MODULE_ID,
        "Open a widget pane",
        "Opens one widget pane in its floating window over the chart, reading the active tab: depth (the DOM ladder), big_trades, liquidations, open_interest or spot_perp_cvd. A pane whose data the feed does not provide opens and says so; provided reports which. Read what it paints with widget.read.",
        "Opening an open pane is the same state, so a stale or repeated call can only show it.",
    )
}

pub fn close_descriptor() -> CapabilityDescriptor {
    crate::layout::transient_descriptor::<WidgetInput, WidgetState>(
        WIDGET_CLOSE_CAPABILITY_ID,
        WIDGET_MODULE_ID,
        "Close a widget pane",
        "Closes one widget pane's floating window, as its close button does.",
        "Closing a closed pane is the same state, so a stale or repeated call can only hide it.",
    )
}

pub fn read_widget_descriptor() -> CapabilityDescriptor {
    read_descriptor::<WidgetInput, WidgetRead>(&ReadSpec {
        id: WIDGET_READ_CAPABILITY_ID,
        module: WIDGET_MODULE_ID,
        title: "Read a widget pane",
        description: "Returns what one widget pane paints for the active tab, open or not: its chips, the rows it lays out, the empty-state sentence when it has no data, and every text item in paint order (at most 512). Built from the same scene the window draws, so the read and the screen agree.",
        permissions: &["observe", "observe.orderflow"],
        pagination: None,
    })
}

pub const READBACKS: &[Readback] = &[
    journal(
        WIDGET_OPEN_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WIDGET_CHANGED_EVENT_KIND,
        "payload.open",
        "an event after the pre-call cursor names the pane with open true",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WIDGET_CLOSE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WIDGET_CHANGED_EVENT_KIND,
        "payload.open",
        "an event after the pre-call cursor names the pane with open false",
        &[EVERY_OPTIONAL_TEST],
    ),
];

/// Published schemas of the widget panes.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        ("widget-input-v1.schema.json", schema::<WidgetInput>()),
        ("widget-state-v1.schema.json", schema::<WidgetState>()),
        ("widget-read-v1.schema.json", schema::<WidgetRead>()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_id_is_accepted() {
        let ok = serde_json::json!({"id": "depth"});
        assert!(serde_json::from_value::<WidgetInput>(ok).is_ok());
        let extra = serde_json::json!({"id": "depth", "tab_id": "1"});
        assert!(serde_json::from_value::<WidgetInput>(extra).is_err());
    }
}
