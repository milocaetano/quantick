//! Source editor dispatch over the indicator owner, with no app dependency.
use crate::indicator_host::{IndicatorControlHost, IndicatorRefusal, ScriptEditorPort};
use crate::{dock::ActionDock, tabs::JournalAccess};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::script_editor::{
    EVENT_KIND, EditorInput, EditorOperation, EditorResult, MAX_PAGE, descriptor,
};
use serde_json::Value;

/// Dock the editor call; a replay never records or runs it.
pub fn register<H: IndicatorControlHost + ?Sized, A: JournalAccess>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), edit)
}

fn edit<H: IndicatorControlHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: EditorInput = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = run(&mut *host.indicators(), input)?;
    access.record_event(NewEvent::by(
        "indicator",
        EVENT_KIND,
        actor,
        serde_json::json!({"script_editor": {
            "open": result.document.is_some(), "revision": result.revision,
            "dirty": result.dirty, "compiling": result.compiling,
        }}),
    ));
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}

/// Run one editor request and read the editor back.
///
/// # Errors
///
/// The page is out of range, or the owner refuses the operation.
pub fn run(
    host: &mut (impl ScriptEditorPort + ?Sized),
    input: EditorInput,
) -> Result<EditorResult, IndicatorRefusal> {
    if input.limit == 0 || input.limit > MAX_PAGE {
        return Err(IndicatorRefusal::Rejected(
            "limit must be in 1..=64".to_owned(),
        ));
    }
    match input.operation {
        EditorOperation::SetNumericInput { id, name, value } => {
            host.set_script_numeric_input(&id, &name, value)?;
        }
        EditorOperation::AddToChart { id } => host.add_library_script(&id)?,
        operation => {
            if let Some(action) = operation.action() {
                host.script_editor(action)?;
            }
        }
    }
    Ok(host.script_library_state(input.offset, input.limit))
}

#[cfg(test)]
#[path = "script_editor_tests.rs"]
mod tests;
