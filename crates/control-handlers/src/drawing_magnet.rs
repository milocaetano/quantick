//! The OHLC magnet's two calls: the rail's (`annotate.magnet.set`) and one
//! drawing's own (`annotate.drawing_magnet.set`), decided here beside
//! `drawing_scope`. The app only finds the switch or the pane and journals.

use quantick_control::error::ControlError;
use quantick_control::wire::WireU64;
pub use quantick_control_schema::drawing_magnet::*;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use serde_json::Value;

fn invalid(error: serde_json::Error) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// The rail's call: `set` flips the rail magnet and answers whether it moved,
/// `journal` records the result under its event kind.
///
/// # Errors
/// Malformed input.
pub fn set_rail<J>(
    input: &Value,
    set: impl FnOnce(bool) -> bool,
    journal: impl FnOnce(&'static str, Value) -> J,
) -> Result<Value, ControlError> {
    let input: DrawingMagnetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let result = DrawingMagnetResult {
        enabled: input.enabled,
        changed: set(input.enabled),
    };
    let _ = journal(
        DRAWING_MAGNET_EVENT_KIND,
        serde_json::json!({ "drawing_magnet": result }),
    );
    serde_json::to_value(result).map_err(invalid)
}

/// One drawing's call: `pane` finds the named pane's store, `journal` records
/// the result under its event kind. The switch is one undo step.
///
/// # Errors
/// Malformed input, a pane `pane` refuses, an id the store does not hold, and
/// on for a tool that snaps by its own rule.
pub fn set_object<'a, J>(
    input: &Value,
    pane: impl FnOnce(WireU64, WireU64) -> Result<&'a mut Drawings<DrawingTool>, ControlError>,
    journal: impl FnOnce(&'static str, Value) -> J,
) -> Result<Value, ControlError> {
    let input: ObjectMagnetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let drawings = pane(input.tab_id, input.pane_id)?;
    let changed = drawings.set_own_magnet(DrawingId(input.drawing_id.get()), input.enabled);
    let result = input.result(changed.map_err(|tool| input.refusal(tool))?);
    let _ = journal(
        OBJECT_MAGNET_EVENT_KIND,
        input.event_payload(result.changed),
    );
    serde_json::to_value(result).map_err(invalid)
}
