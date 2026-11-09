//! Source editor dispatch over an alternate host, with no app dependency.
use crate::dock::ActionDock;
use quantick_control::registry::RegistryError;
use quantick_control_schema::script_editor::{ScriptDocument, ScriptEditorAction};

pub trait ScriptEditorPort {
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, String>;
}

/// Docking point only: no unfinished capability is advertised by the skeleton.
pub fn register<H: ScriptEditorPort + ?Sized, A>(
    _registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    Ok(())
}

pub fn dispatch<P: ScriptEditorPort + ?Sized>(
    host: &mut P,
    action: ScriptEditorAction,
) -> Result<Option<ScriptDocument>, String> {
    host.script_editor(action)
}
