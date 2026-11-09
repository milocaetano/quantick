//! Source-editor wire contracts dock here when the editor becomes available.
pub use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};

pub const READBACKS: &[crate::readback::Readback] = &[];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    Vec::new()
}
