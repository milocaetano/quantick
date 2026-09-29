//! Wire snapshots for semantic pointer state and current UI selections.

use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_control_host::wire::{AvailabilitySnapshot, PaneSideDto};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chart::BarSnapshot;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CursorSnapshot {
    pub active_tab_id: WireU64,
    pub focused_pane_id: WireU64,
    pub focused_pane_side: PaneSideDto,
    pub pointer: Option<PointerSnapshot>,
    pub pointer_availability: AvailabilitySnapshot,
    pub semantic_scene: AvailabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
///
/// `screen_x_px` and `screen_y_px` keep their names for the clients already
/// reading them, but the unit is the window's **logical points** — what the
/// window lays out in, and what `scene.controls` reports a control's rectangle
/// in, so a pointer and a rectangle can be compared without a scale factor
/// neither side knows. On a 200% display the framebuffer holds twice these
/// numbers.
pub struct PointerSnapshot {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub pane_focused: bool,
    pub feed_id: String,
    pub symbol: String,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub screen_x_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub screen_y_px: CanonicalDecimal,
    pub band: String,
    pub axis_value: Option<CanonicalDecimal>,
    pub axis_unit: String,
    pub price: Option<CanonicalDecimal>,
    pub slot: Option<WireU64>,
    pub bar: Option<BarSnapshot>,
    pub flow_cell: Option<FlowCellSnapshot>,
    pub drawing: Option<DrawingHitSnapshot>,
    pub control_id: Option<String>,
    pub control_id_availability: AvailabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlowCellSnapshot {
    pub generation: WireU64,
    pub side: String,
    pub price_bucket: CanonicalDecimal,
    pub price_span: CanonicalDecimal,
    pub quantity: CanonicalDecimal,
    /// The closed-bar slots under the cell. A cell that lies wholly in the
    /// live lane has none: both bounds then equal the lane boundary, and
    /// `live_lane` says where it is.
    pub start_slot: WireU64,
    pub end_slot_exclusive: WireU64,
    pub live_lane: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DrawingHitSnapshot {
    pub owner_pane_id: WireU64,
    pub owner_pane_side: PaneSideDto,
    pub mirrored: bool,
    pub drawing_id: WireU64,
    pub tool_id: String,
    pub label: String,
    pub user_label_present: bool,
    pub handle_index: Option<WireU64>,
    pub selected: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SelectionSnapshot {
    pub active_tab_id: WireU64,
    pub focused_pane_id: WireU64,
    pub focused_pane_side: PaneSideDto,
    pub drawing: Option<DrawingSelectionSnapshot>,
    pub paper_trade_row: Option<PaperTradeSelectionSnapshot>,
    pub event_row: AvailabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DrawingSelectionSnapshot {
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub drawing_id: WireU64,
    pub tool_id: String,
    pub label: String,
    pub user_label_present: bool,
    pub band: String,
    pub scope: String,
    pub locked: bool,
    pub hidden: bool,
    pub foreign_market: bool,
    pub off_series: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperTradeSelectionSnapshot {
    pub row_index: WireU64,
    pub provenance: String,
}
