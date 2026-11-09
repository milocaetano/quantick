//! `annotate.drawing_scope.set`: where one drawing shows, decided here so the
//! call, the context bar and the inspector agree. The app only finds the pane
//! and journals.

use quantick_control::error::ControlError;
use quantick_control::wire::WireU64;
pub use quantick_control_schema::drawing_scope::*;
use quantick_drawings::{DrawingId, DrawingScope, DrawingTool, Drawings};
use serde_json::Value;

#[cfg(test)]
#[path = "drawing_scope_tests.rs"]
mod drawing_scope_tests;

/// Set the scope `input` names on the drawing it names in `drawings`, the
/// store of a pane cut at `bars`, as one undo step; a drawing already
/// standing there is left untouched. The next write of the pane stores it:
/// a drawing kept to its chart is pinned to `bars` then.
///
/// # Errors
/// An id the store does not hold, and `all_charts` for a drawing with an
/// anchor that names no market instant.
pub fn set_scope(
    drawings: &mut Drawings<DrawingTool>,
    input: &DrawingScopeInput,
    bars: &str,
) -> Result<DrawingScopeResult, ControlError> {
    let index = drawings
        .index_of(DrawingId(input.drawing_id.get()))
        .ok_or_else(|| input.unknown_drawing())?;
    let drawing = &drawings.items()[index];
    if input.scope == ScopeName::AllCharts && !drawing.shareable() {
        return Err(no_market_time(drawing.tool.id()));
    }
    let changed = crate::analysis::scope_name(drawing) != input.scope.as_str();
    if changed {
        let before = drawing.clone();
        let scope = match input.scope {
            ScopeName::AllCharts => DrawingScope::AllCharts,
            ScopeName::ThisChart => DrawingScope::ThisChart,
        };
        drawings.items_mut()[index].set_scope(scope);
        drawings.record_edit_of(index, before);
    }
    let after = &drawings.items()[index];
    let pinned = !after.all_timeframes() && !after.legacy_unpinned;
    Ok(input.result(pinned.then(|| bars.to_owned()), changed))
}

/// The whole call: `pane` finds the named pane's store and the bars it is cut
/// to, `journal` records the result under its event kind.
///
/// # Errors
/// Malformed input, a pane `pane` refuses, and what [`set_scope`] refuses.
pub fn call<'a, J>(
    input: &Value,
    pane: impl FnOnce(WireU64, WireU64) -> Result<(&'a mut Drawings<DrawingTool>, String), ControlError>,
    journal: impl FnOnce(&'static str, Value) -> J,
) -> Result<Value, ControlError> {
    let invalid = |error: serde_json::Error| ControlError::invalid_request(error.to_string());
    let input: DrawingScopeInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let (drawings, bars) = pane(input.tab_id, input.pane_id)?;
    let result = set_scope(drawings, &input, &bars)?;
    let _ = journal(DRAWING_SCOPE_EVENT_KIND, event_payload(&result));
    serde_json::to_value(result).map_err(invalid)
}
