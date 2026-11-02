//! Market-tab presentation and native-window inspection. Runtime ownership stays in workspace.
use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const WINDOWS_SCOPE_ID: &str = "workspace.windows";
pub const REORDER_CAPABILITY_ID: &str = "layout.market_tab.reorder";
pub const DETACH_CAPABILITY_ID: &str = "layout.market_tab.detach";
pub const ATTACH_CAPABILITY_ID: &str = "layout.market_tab.attach";
pub const SELECT_CAPABILITY_ID: &str = "layout.market_tab.select";
pub const BEGIN_DRAG_CAPABILITY_ID: &str = "layout.market_tab.drag.begin";
pub const CANCEL_DRAG_CAPABILITY_ID: &str = "layout.market_tab.drag.cancel";
pub const CLOSE_WINDOW_CAPABILITY_ID: &str = "layout.window.close";
pub const PLACE_WINDOW_CAPABILITY_ID: &str = "layout.window.placement.set";
pub const MAX_WINDOWS: usize = 256;
pub const MAX_TABS_PER_WINDOW: usize = 1024;
pub const MAX_DESKTOP_COORDINATE: i32 = 1_000_000;
pub const MAX_WINDOW_SIZE: u32 = 32_768;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabInput {
    pub tab_id: WireU64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReorderInput {
    pub tab_id: WireU64,
    pub position: WireU64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachInput {
    pub tab_id: WireU64,
    pub window_id: WireU64,
    pub position: WireU64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectInput {
    pub tab_id: WireU64,
    pub window_id: WireU64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowTarget {
    /// Omitted means the focused live market window, never a hidden root shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<WireU64>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelInput {}

/// Physical desktop pixels, including negative coordinates on secondary monitors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhysicalPlacement {
    #[schemars(range(min = -1_000_000, max = 1_000_000))]
    pub x_px: i32,
    #[schemars(range(min = -1_000_000, max = 1_000_000))]
    pub y_px: i32,
    #[schemars(range(min = 1, max = 32768))]
    pub width_px: u32,
    #[schemars(range(min = 1, max = 32768))]
    pub height_px: u32,
}
impl PhysicalPlacement {
    pub fn validate(self) -> Result<Self, quantick_control::error::ControlError> {
        if self.x_px.abs_diff(0) > MAX_DESKTOP_COORDINATE as u32
            || self.y_px.abs_diff(0) > MAX_DESKTOP_COORDINATE as u32
            || !(1..=MAX_WINDOW_SIZE).contains(&self.width_px)
            || !(1..=MAX_WINDOW_SIZE).contains(&self.height_px)
        {
            return Err(quantick_control::error::ControlError::invalid_request(
                "physical placement exceeds the desktop coordinate or size bounds",
            ));
        }
        Ok(self)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlacementInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<WireU64>,
    pub placement: PhysicalPlacement,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PlacementResult {
    pub window_id: WireU64,
    /// Accepted for a native frame. The OS may clamp size or placement; inspect workspace.windows.
    pub requested: PhysicalPlacement,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DragSnapshot {
    pub pending: bool,
    /// True after cancellation until another drag begins or completes.
    pub cancelled: bool,
    pub tab_id: Option<WireU64>,
    pub source_window_id: Option<WireU64>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeObservation {
    /// Geometry is derived from the OS-fed native viewport, never from an accepted request.
    pub source: String,
    pub outer: Option<PhysicalPlacement>,
    pub inner: Option<PhysicalPlacement>,
    pub pixels_per_point: Option<CanonicalDecimal>,
    pub focused: Option<bool>,
    pub minimized: Option<bool>,
    pub maximized: Option<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketWindowSnapshot {
    pub window_id: WireU64,
    #[schemars(length(max = 1024))]
    pub ordered_tab_ids: Vec<WireU64>,
    pub selected_tab_id: WireU64,
    pub omitted_tabs: WireU64,
    /// None until this native viewport has supplied an observation.
    pub observed: Option<NativeObservation>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WindowsSnapshot {
    /// Windows uses an exposed same-process native target. Other platforms preserve
    /// same-window reorder/detach and cancel ambiguous cross-window pointer drops.
    pub native_drop_targeting: String,
    pub focused_window_id: Option<WireU64>,
    pub presented_window_id: Option<WireU64>,
    #[schemars(length(max = 256))]
    pub windows: Vec<MarketWindowSnapshot>,
    pub omitted_windows: WireU64,
    pub drag: DragSnapshot,
}
impl Default for WindowsSnapshot {
    fn default() -> Self {
        Self {
            native_drop_targeting: "unavailable".to_owned(),
            focused_window_id: None,
            presented_window_id: None,
            windows: Vec::new(),
            omitted_windows: WireU64::new(0),
            drag: DragSnapshot::default(),
        }
    }
}
impl WindowsSnapshot {
    pub fn tab_location(&self, tab: WireU64) -> (Option<WireU64>, Option<WireU64>, bool) {
        self.windows
            .iter()
            .find_map(|window| {
                window
                    .ordered_tab_ids
                    .iter()
                    .position(|id| *id == tab)
                    .map(|index| {
                        (
                            Some(window.window_id),
                            Some(WireU64::new(index as u64)),
                            window.selected_tab_id == tab,
                        )
                    })
            })
            .unwrap_or((None, None, false))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TabOperationResult {
    pub changed: bool,
    pub tab_id: Option<WireU64>,
    pub source_window_id: Option<WireU64>,
    pub window_id: Option<WireU64>,
    pub position: Option<WireU64>,
    pub drag: DragSnapshot,
}

/// Validated handler request; workspace alone decides ownership and insertion policy.
pub enum TabOperation {
    Reorder(ReorderInput),
    Detach(TabInput),
    Attach(AttachInput),
    Select(SelectInput),
    BeginDrag(TabInput),
    CancelDrag,
    Close(WindowTarget),
}

pub fn descriptor<I: JsonSchema>(id: &str, title: &str, description: &str) -> CapabilityDescriptor {
    let mut value = crate::layout::transient_descriptor::<I, TabOperationResult>(
        id,
        crate::layout::LAYOUT_MODULE_ID,
        title,
        description,
        "A required idempotency key replays the original structured result without repeating the operation. Invalid or stale targets change nothing. Inspect workspace.windows for the current presentation; native effects are asynchronous.",
    );
    value.idempotency = IdempotencyPolicy::Required;
    value.expected_cost.max_response_bytes = Some(2048);
    value
}
pub fn placement_descriptor() -> CapabilityDescriptor {
    let mut value = crate::layout::transient_descriptor::<PlacementInput, PlacementResult>(
        PLACE_WINDOW_CAPABILITY_ID,
        crate::layout::LAYOUT_MODULE_ID,
        "Place a native market window",
        "Requests outer origin and inner size in physical desktop pixels for the named window, or the focused window when omitted. Coordinates may be negative. The OS can clamp the requested rectangle; workspace.windows reports the actual native viewport separately. No market, feed or worker is recreated.",
        "A required key replays the accepted placement without issuing it again.",
    );
    value.idempotency = IdempotencyPolicy::Required;
    value.expected_cost.max_response_bytes = Some(1024);
    value
}

pub const READBACKS: &[crate::readback::Readback] = &[
    crate::readback::snapshot(
        REORDER_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "windows[].ordered_tab_ids",
        "the source window lists the tab at the requested insertion index",
        PROOF,
    ),
    crate::readback::snapshot(
        DETACH_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "windows[].ordered_tab_ids",
        "the result's new window owns the same tab identity",
        PROOF,
    ),
    crate::readback::snapshot(
        ATTACH_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "windows[].ordered_tab_ids",
        "the destination lists the same tab at the requested index",
        PROOF,
    ),
    crate::readback::snapshot(
        SELECT_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "windows[].selected_tab_id",
        "only the named window selects the requested tab",
        PROOF,
    ),
    crate::readback::snapshot(
        BEGIN_DRAG_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "drag",
        "pending is true with the source and tab, and ownership and order are unchanged",
        PROOF,
    ),
    crate::readback::snapshot(
        CANCEL_DRAG_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "drag",
        "pending is false and cancelled is true after an active gesture was cancelled, without changing ownership or order",
        PROOF,
    ),
    crate::readback::snapshot(
        CLOSE_WINDOW_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "windows",
        "the closed window is absent and all of its tabs survive in the result's survivor; final-window exit is refused",
        PROOF,
    ),
    crate::readback::snapshot(
        PLACE_WINDOW_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        WINDOWS_SCOPE_ID,
        "windows[].observed",
        "native viewport observations report actual placement after OS handling, independently of request acceptance",
        PROOF,
    ),
];
const PROOF: &[&str] =
    &["market_window_operations_validate_authority_preserve_identity_and_replay_safely"];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "workspace-windows-v1.schema.json",
            schema::<WindowsSnapshot>(),
        ),
        ("market-tab-input-v1.schema.json", schema::<TabInput>()),
        (
            "market-tab-reorder-input-v1.schema.json",
            schema::<ReorderInput>(),
        ),
        (
            "market-tab-attach-input-v1.schema.json",
            schema::<AttachInput>(),
        ),
        (
            "market-tab-select-input-v1.schema.json",
            schema::<SelectInput>(),
        ),
        (
            "market-tab-operation-result-v1.schema.json",
            schema::<TabOperationResult>(),
        ),
        (
            "native-window-target-v1.schema.json",
            schema::<WindowTarget>(),
        ),
        (
            "native-window-placement-input-v1.schema.json",
            schema::<PlacementInput>(),
        ),
        (
            "native-window-placement-result-v1.schema.json",
            schema::<PlacementResult>(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_placement_and_tab_inputs_are_strict_and_bounded() {
        let placement = PhysicalPlacement {
            x_px: -1920,
            y_px: 104,
            width_px: 900,
            height_px: 600,
        };
        assert!(placement.validate().is_ok());
        assert!(
            PhysicalPlacement {
                x_px: i32::MIN,
                ..placement
            }
            .validate()
            .is_err()
        );
        assert!(
            PhysicalPlacement {
                width_px: 0,
                ..placement
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_value::<TabInput>(serde_json::json!({"tab_id":"1","extra":true}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<AttachInput>(
                serde_json::json!({"tab_id":"1","window_id":"0","position":"18446744073709551616"})
            )
            .is_err()
        );
    }
}
