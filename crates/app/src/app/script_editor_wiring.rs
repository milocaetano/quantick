//! Bind editor requests and its frame tick to app-owned panes and library.
use super::{arrangement_host::ArrangementHost, indicator_manager::IndicatorState};
use crate::surfaces::Surfaces;
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};
use std::time::Instant;

pub(super) fn apply(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    _tabs: &mut ArrangementHost,
    action: ScriptEditorAction,
) -> Result<Option<ScriptDocument>, String> {
    surfaces
        .script_editor
        .state
        .dispatch(&mut indicators.script_library, action)
}

/// The editor's frame phase also owns eventual debounce/worker scheduling.
/// An idle skeleton has no actions and does no I/O or compilation.
pub(super) fn frame(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    tabs: &mut ArrangementHost,
    actions: Vec<ScriptEditorAction>,
    now: Instant,
) {
    for action in actions {
        if let Err(error) = apply(surfaces, indicators, tabs, action) {
            surfaces.toast.note(error, now);
        }
    }
}
