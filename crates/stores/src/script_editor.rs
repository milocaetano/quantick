//! Editor decisions are deterministic; its adapter supplies elapsed milliseconds.
use crate::script_library::{ScriptDocument, ScriptEditorAction, ScriptLibraryPort};

pub const DEBOUNCE_MS: u64 = 350;
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;

#[derive(Default, Debug)]
pub struct ScriptEditor {
    pub document: Option<ScriptDocument>,
    pub diagnostics: Option<String>,
    pub dirty: bool,
    pub revision: u64,
    pub compiling: bool,
    pending_since: Option<u64>,
    pending: bool,
}
impl ScriptEditor {
    pub fn dispatch(
        &mut self,
        library: &mut dyn ScriptLibraryPort,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, String> {
        match action {
            ScriptEditorAction::Inspect => (),
            ScriptEditorAction::Open { id } => {
                if self.dirty || self.pending {
                    return Err("save the current source before opening another script".into());
                }
                let document = library.read(&id)?;
                self.reset(Some(document));
            }
            ScriptEditorAction::Copy { id, new_id } => {
                if self.dirty || self.pending {
                    return Err("save the current source before creating a copy".into());
                }
                let document = library.copy(&id, &new_id)?;
                self.reset(Some(document));
            }
            ScriptEditorAction::ReplaceSource { source } => {
                if source.len() > MAX_SOURCE_BYTES {
                    return Err("source exceeds 256 KiB".into());
                }
                let document = self.document.as_mut().ok_or("open a script first")?;
                if document.read_only {
                    return Err("built-in scripts are read-only; create an editable copy".into());
                }
                if document.source != source {
                    document.source = source;
                    self.dirty = true;
                    self.revision += 1;
                    self.pending_since = None;
                    self.pending = true;
                    self.diagnostics = None;
                }
            }
            ScriptEditorAction::Save => {
                let document = self.document.as_ref().ok_or("open a script first")?;
                library.save(document)?;
                if self.dirty {
                    self.pending = true;
                }
                self.dirty = false;
            }
            ScriptEditorAction::Close => {
                if self.pending {
                    return Err("wait for source validation before closing".into());
                }
                if self.dirty {
                    return Err("save the current source before closing".into());
                }
                self.reset(None);
            }
        }
        Ok(self.document.clone())
    }
    fn reset(&mut self, document: Option<ScriptDocument>) {
        self.document = document;
        self.revision += 1;
        self.diagnostics = None;
        self.dirty = false;
        self.compiling = false;
        self.pending_since = None;
        self.pending = false;
    }
    /// At most one outstanding compile; later edits stay pending until it completes.
    pub fn due(&mut self, now_ms: u64) -> Option<(u64, ScriptDocument)> {
        if !self.pending || self.compiling {
            return None;
        }
        let since = self.pending_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(*since) < DEBOUNCE_MS {
            return None;
        }
        self.compiling = true;
        Some((self.revision, self.document.clone()?))
    }
    /// Reject a completion for an older edit or an editor that has been closed.
    pub fn complete(&mut self, revision: u64, error: Option<String>) -> bool {
        self.compiling = false;
        if revision != self.revision {
            return false;
        }
        self.diagnostics = error;
        self.dirty = false;
        self.pending_since = None;
        self.pending = false;
        true
    }
}
