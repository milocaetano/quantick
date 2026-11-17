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
/// `all_windows`, when named, is set in the same undo step, on the store's
/// layout: a drawing from before layouts were recorded becomes its own.
///
/// # Errors
/// An id the store does not show, `all_charts` for a drawing with an anchor
/// that names no market instant, and `all_windows: true` beside
/// `this_chart`.
pub fn set_scope(
    drawings: &mut Drawings<DrawingTool>,
    input: &DrawingScopeInput,
    bars: &str,
) -> Result<DrawingScopeResult, ControlError> {
    let index = drawings
        .index_of(DrawingId(input.drawing_id.get()))
        .filter(|&index| drawings.in_layout(index))
        .ok_or_else(|| input.unknown_drawing())?;
    let drawing = &drawings.items()[index];
    if input.scope == ScopeName::AllCharts && !drawing.shareable() {
        return Err(no_market_time(drawing.tool.id()));
    }
    if input.scope == ScopeName::ThisChart && input.all_windows == Some(true) {
        return Err(all_windows_needs_all_charts(drawing.tool.id()));
    }
    let scope = match input.scope {
        ScopeName::AllCharts => DrawingScope::AllCharts,
        ScopeName::ThisChart => DrawingScope::ThisChart,
    };
    let before = drawing.clone();
    let here = drawings.layout();
    let after = &mut drawings.items_mut()[index];
    // The pick, not what shows: an undated copy of a mark on every timeframe
    // shows on its chart alone until dated, and a call keeping it there holds.
    if after.scope != scope {
        after.set_scope(scope);
    }
    // What shows moves, or a pick left on a drawing kept to its chart is
    // cleared; an old drawing every chart already shows stays as it is.
    let windows_moved = input
        .all_windows
        .is_some_and(|on| after.in_all_windows() != on || (after.all_windows && !on));
    if let Some(on) = input.all_windows.filter(|_| windows_moved) {
        after.all_windows = on;
        after.layout = after.layout.or(here);
    }
    let changed = *after != before;
    if changed {
        drawings.record_edit_of(index, before);
    }
    let after = &drawings.items()[index];
    let pinned = !after.all_timeframes() && !after.legacy_unpinned;
    let readback = ScopeReadback {
        pinned_bars: pinned.then(|| bars.to_owned()),
        all_windows: after.in_all_windows(),
        layout_id: after.layout.map(|layout| layout.0),
    };
    Ok(input.result(readback, changed))
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
