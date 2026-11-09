//! Source editor state, separate from script storage and calculation.
use crate::script_library::{ScriptDocument, ScriptEditorAction, ScriptLibraryPort};

#[derive(Default, Debug)]
pub struct ScriptEditor {
    pub document: Option<ScriptDocument>,
}
impl ScriptEditor {
    /// The skeleton accepts no editor action. No source or storage is changed.
    pub fn dispatch(
        &mut self,
        _library: &mut dyn ScriptLibraryPort,
        _action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, String> {
        Err("script editing is not available in this build".into())
    }
}
