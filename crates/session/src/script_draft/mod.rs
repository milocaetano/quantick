//! Compile, preview and prepare the alarm-only script runtime on loaded closes.
mod contract;
use crate::script_strategies::ScriptReading;
use crate::strategies::TabStrategies;
pub use contract::{Diagnostic, DraftInput, HistoricalClose, Marker, PreparedAlarm};

fn unavailable() -> Vec<Diagnostic> {
    vec![Diagnostic {
        line: None,
        code: "unavailable".into(),
        message: "Script draft preview is unavailable".into(),
    }]
}

pub fn preview<D: Copy>(_input: DraftInput<'_, D>) -> Result<Vec<Marker>, Vec<Diagnostic>> {
    Err(unavailable())
}

pub fn prepare<D: Copy>(_input: DraftInput<'_, D>) -> Result<PreparedAlarm<D>, Vec<Diagnostic>> {
    Err(unavailable())
}

pub fn commit<D: Copy + PartialEq>(
    tab: &mut TabStrategies<D>,
    pane_id: u64,
    alarms: PreparedAlarm<D>,
) -> Vec<ScriptReading<D>> {
    alarms
        .0
        .into_iter()
        .map(|instance| tab.publish_script(pane_id, instance))
        .collect()
}
