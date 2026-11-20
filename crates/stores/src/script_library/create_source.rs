use super::{ScriptDocument, ScriptLibrary};

impl ScriptLibrary {
    /// Create a new file-backed strategy without replacing an existing identity.
    pub fn create_source(&mut self, _id: &str, _source: &str) -> Result<ScriptDocument, String> {
        Err("Creating a strategy from source is unavailable".into())
    }
}
