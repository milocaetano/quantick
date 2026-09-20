//! Layer discovery and visibility actions over the same catalog used by pane menus.

use quantick_control::wire::WireU64;

use schemars::JsonSchema;

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::registry::IdempotencyPolicy::Optional;
use serde::{Deserialize, Serialize};

pub const SCOPE_ID: &str = "layers.visibility";

pub const SET_VISIBILITY_CAPABILITY_ID: &str = "layers.visibility.set";

pub const EVENT_KIND: &str = "layers.visibility.set";

pub const MODULE_ID: &str = "layers";

/// A bounded snapshot; omitted panes are explicit and can still be targeted by ID.
pub const MAX_SNAPSHOT_PANES: usize = 64;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVisibilityInput {
    pub tab_id: WireU64,
    /// Stable identity from chart.summary, never a position that a pane move can change.
    pub pane_id: WireU64,
    #[schemars(length(min = 1, max = quantick_layers::MAX_LAYER_ID_BYTES))]
    pub layer_id: String,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LayerSnapshot {
    pub id: String,
    pub label: String,
    pub scope: String,
    pub persistence: String,
    pub requested: bool,
    /// Visibility eligible under current layer policy; no pixel geometry is claimed.
    pub effective: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PaneLayersSnapshot {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    #[schemars(length(max = quantick_layers::MAX_LAYERS))]
    pub layers: Vec<LayerSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct LayersSnapshot {
    #[schemars(length(max = MAX_SNAPSHOT_PANES))]
    pub panes: Vec<PaneLayersSnapshot>,
    pub omitted_panes: WireU64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct VisibilityResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub layer: LayerSnapshot,
    pub changed: bool,
}

/// How a client reconciles an interrupted layer switch.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[journal(
    "layers.visibility.set",
    Optional,
    EVENT_KIND,
    "payload.result",
    "an event after the pre-call cursor matches connection_id and request_id, stable tab/pane IDs, layer ID and requested boolean; emitted even for a no-op, including panes omitted from the bounded snapshot; a retention gap leaves the outcome unknown",
    &[EVERY_OPTIONAL_TEST],
)];
