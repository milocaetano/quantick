//! Wire-to-canvas adapter for the addressed vertical splitter operation.

use eframe::egui;
use quantick_control::{registry::IdempotencyPolicy, wire::CanonicalDecimal};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::super::{
    retry_matrix::Readback, types::canonical_f32, workspace::SPLIT_FRACTION_DECIMAL_PLACES,
};
use super::*;
use crate::tab::context_resize::ResizeContextPair;

pub(crate) const RESIZE_PAIR_CAPABILITY_ID: &str = "layout.pane.resize_pair";

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct ResizePairInput {
    #[serde(flatten)]
    target: TabTarget,
    /// Stable pane IDs from workspace.summary, in upper/lower order.
    upper_pane_id: WireU64,
    lower_pane_id: WireU64,
    /// Divider position in the whole context column, top=0 and bottom=1.
    /// At most six decimal places. The applied position obeys both neighbor floors.
    fraction: CanonicalDecimal,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ResizePairResult {
    tab_id: WireU64,
    upper_pane_id: WireU64,
    lower_pane_id: WireU64,
    /// Applied divider position, including any floor clamp.
    fraction: CanonicalDecimal,
    changed: bool,
}

pub(super) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    let mut descriptor = super::descriptor(
        RESIZE_PAIR_CAPABILITY_ID,
        "Resize adjacent context charts",
        "Moves the divider between the named upper and lower context panes. Uses the same neighbor floors as a pointer drag, keeps other boundaries and column width unchanged, and returns the applied position. The current stack must have been drawn.",
        generated_schema::<ResizePairInput>(),
    );
    descriptor.output_schema = generated_schema::<ResizePairResult>();
    descriptor.stale_input_safety = Some("Stable pane IDs must still name adjacent visible charts; otherwise the request is refused before any height changes.".to_owned());
    registry.register(descriptor, resize)
}

fn resize(
    app: &mut ControlWindow,
    _access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizePairInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let fraction: Decimal = input
        .fraction
        .as_str()
        .parse()
        .map_err(|error| ControlError::invalid_request(format!("fraction: {error}")))?;
    if fraction.scale() > SPLIT_FRACTION_DECIMAL_PLACES
        || fraction < Decimal::ZERO
        || fraction > Decimal::ONE
    {
        return Err(ControlError::invalid_request(
            "fraction must be between 0 and 1 with at most six decimal places",
        ));
    }
    let fraction = fraction
        .to_f32()
        .ok_or_else(|| ControlError::invalid_request("fraction is out of range"))?;
    let index = super::tab_index(app, input.target)?;
    let tab_id = app.tab_reads().tabs().id_at(index);
    let tab = app
        .control_actions()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let (column, _) = tab.context_stack_geometry().ok_or_else(|| {
        ControlError::invalid_request(
            "the current context stack must be visible and drawn before resizing",
        )
    })?;
    let result = tab
        .resize_context_pair(
            tab_id,
            ResizeContextPair {
                upper_pane_id: input.upper_pane_id.get(),
                lower_pane_id: input.lower_pane_id.get(),
                wanted_y: egui::lerp(column.y_range(), fraction),
            },
            actor.actor_kind,
        )
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let result = ResizePairResult {
        tab_id: WireU64::new(tab_id),
        upper_pane_id: input.upper_pane_id,
        lower_pane_id: input.lower_pane_id,
        fraction: canonical_f32(result.fraction, SPLIT_FRACTION_DECIMAL_PLACES)
            .expect("a clamped canvas fraction is a finite canonical decimal"),
        changed: result.changed,
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// The scene reports the same splitter's current bounds after an uncertain call.
pub(crate) const READBACK: Readback = Readback {
    capability: RESIZE_PAIR_CAPABILITY_ID,
    policy: IdempotencyPolicy::Optional,
    read: super::super::contract::SNAPSHOT_CAPABILITY_ID,
    scope: Some(super::super::scene::CONTROLS_SCOPE_ID),
    event: None,
    field: "controls[].bounds",
    applied_when: "the addressed context divider's bounds reflect the applied vertical position",
    proven_by: &["every_reachable_optional_row_replays_a_dropped_answer_and_begins_once"],
};
