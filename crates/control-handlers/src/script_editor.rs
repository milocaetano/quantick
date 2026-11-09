//! Source editor dispatch over an alternate host, with no app dependency.
use crate::{dock::ActionDock, tabs::JournalAccess};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::script_editor::{
    EVENT_KIND, EditorInput, EditorResult, MAX_PAGE, ScriptDocument, ScriptEditorAction, descriptor,
};
use serde_json::Value;

pub trait ScriptEditorPort {
    fn set_script_numeric_input(
        &mut self,
        _id: &str,
        _name: &str,
        _value: quantick_control_schema::script_editor::NumericValue,
    ) -> Result<(), String> {
        Err("numeric script inputs are not available in this build".into())
    }
    fn add_library_script(&mut self, _id: &str) -> Result<(), String> {
        Err("library attachment is not available in this build".into())
    }
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, String>;
    fn script_library_state(&self, _offset: usize, _limit: usize) -> EditorResult {
        EditorResult::default()
    }
}

pub fn register<H: ScriptEditorPort + ?Sized, A: JournalAccess>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), edit)
}
fn edit<H: ScriptEditorPort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: EditorInput = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    if input.limit == 0 || input.limit > MAX_PAGE {
        return Err(ControlError::invalid_request("limit must be in 1..=64"));
    }
    match input.operation {
        quantick_control_schema::script_editor::EditorOperation::SetNumericInput {
            id,
            name,
            value,
        } => {
            host.set_script_numeric_input(&id, &name, value)
                .map_err(ControlError::invalid_request)?;
        }
        quantick_control_schema::script_editor::EditorOperation::AddToChart { id } => {
            host.add_library_script(&id)
                .map_err(ControlError::invalid_request)?;
        }
        operation => {
            if let Some(action) = operation.action() {
                dispatch(host, action).map_err(ControlError::invalid_request)?;
            }
        }
    }
    let result = host.script_library_state(input.offset, input.limit);
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
pub fn dispatch<P: ScriptEditorPort + ?Sized>(
    host: &mut P,
    action: ScriptEditorAction,
) -> Result<Option<ScriptDocument>, String> {
    host.script_editor(action)
}
