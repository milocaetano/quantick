//! Editor decisions are deterministic; its adapter supplies elapsed milliseconds.
use crate::script_library::{
    MAX_SOURCE_BYTES, ScriptDocument, ScriptEditorAction, ScriptLibraryPort,
};

pub const DEBOUNCE_MS: u64 = 350;

#[derive(Default, Debug)]
pub struct ScriptEditor {
    pub document: Option<ScriptDocument>,
    pub diagnostics: Option<String>,
    pub dirty: bool,
    pub revision: u64,
    pub compiling: bool,
    validated_inputs: Option<Vec<quantick_indicators::InputSpec>>,
    pending_since: Option<u64>,
    pending: bool,
    pending_edit: bool,
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
                if self.dirty || (self.pending && self.pending_edit) {
                    return Err("save the current source before opening another script".into());
                }
                let document = library.read(&id)?;
                self.reset(Some(document));
            }
            ScriptEditorAction::Copy { id, new_id } => {
                if self.dirty || (self.pending && self.pending_edit) {
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
                    self.pending_edit = true;
                    self.diagnostics = None;
                    self.validated_inputs = None;
                }
            }
            ScriptEditorAction::Save => {
                let document = self.document.as_ref().ok_or("open a script first")?;
                if document.read_only {
                    return Err("built-in scripts are read-only; create an editable copy".into());
                }
                if self.dirty {
                    library.save(document)?;
                    self.pending = true;
                    self.pending_edit = true;
                }
                self.dirty = false;
            }
            ScriptEditorAction::Close => {
                if self.pending && self.pending_edit {
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
        self.pending = self.document.is_some();
        self.pending_edit = false;
        self.validated_inputs = None;
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
    pub fn complete(
        &mut self,
        revision: u64,
        error: Option<String>,
        inputs: Vec<quantick_indicators::InputSpec>,
    ) -> bool {
        self.compiling = false;
        if revision != self.revision {
            return false;
        }
        self.validated_inputs = error.is_none().then_some(inputs);
        self.diagnostics = error;
        self.dirty = false;
        self.pending_since = None;
        self.pending = false;
        self.pending_edit = false;
        true
    }
    /// Includes the idle debounce, even when opening source on an idle chart.
    pub fn validation_pending(&self) -> bool {
        self.pending || self.compiling
    }
    /// Only an edited source completion may publish a replacement to the chart.
    pub fn source_edit_pending(&self) -> bool {
        self.pending_edit
    }
    /// A saved-but-unvalidated edit and a lagging worker declaration cannot
    /// authorize a numeric edit. Equal declarations preserve FIFO reload/apply.
    pub fn inputs_ready(&self, current: &[quantick_indicators::InputSpec]) -> bool {
        !self.dirty
            && !self.validation_pending()
            && self.diagnostics.is_none()
            && self
                .validated_inputs
                .as_ref()
                .is_some_and(|validated| validated == current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_indicators::InputSpec;
    fn spec(name: &str) -> InputSpec {
        InputSpec::Int {
            name: name.into(),
            title: name.into(),
            default: 9,
            min: Some(1),
            max: Some(30),
            step: None,
            options: vec![],
        }
    }
    fn document() -> ScriptDocument {
        ScriptDocument {
            id: "copy.pine".into(),
            title: "Copy".into(),
            source: "plot(close)".into(),
            read_only: false,
        }
    }
    #[test]
    fn readiness_requires_current_validation_and_stale_completion_cannot_replace_it() {
        let mut state = ScriptEditor::default();
        state.reset(Some(document()));
        let old = state.revision;
        assert!(
            state.validation_pending(),
            "an idle source open still needs validation frames"
        );
        assert!(!state.inputs_ready(&[spec("old")]));
        assert!(state.complete(old, None, vec![spec("old")]));
        assert!(state.inputs_ready(&[spec("old")]));
        assert!(!state.validation_pending());
        state.reset(Some(document()));
        assert!(!state.inputs_ready(&[spec("old")]));
        assert!(!state.complete(old, None, vec![spec("old")]));
        assert!(!state.inputs_ready(&[spec("old")]));
        assert!(state.complete(state.revision, None, vec![spec("new")]));
        assert!(!state.complete(old, None, vec![spec("old")]));
        assert!(state.inputs_ready(&[spec("new")]));
        assert!(!state.inputs_ready(&[spec("old")]));
    }
    struct SavePort {
        fail: bool,
        saves: usize,
    }
    impl ScriptLibraryPort for SavePort {
        fn list(&self) -> &[crate::script_library::ScriptEntry] {
            &[]
        }
        fn read(&self, _: &str) -> Result<ScriptDocument, String> {
            Ok(document())
        }
        fn copy(&mut self, _: &str, _: &str) -> Result<ScriptDocument, String> {
            Err("not used".into())
        }
        fn save(&mut self, _: &ScriptDocument) -> Result<(), String> {
            self.saves += 1;
            if self.fail {
                Err("disk full".into())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn successful_manual_save_after_idle_save_failure_remains_publishable() {
        let mut library = SavePort {
            fail: true,
            saves: 0,
        };
        let mut state = ScriptEditor::default();
        state.reset(Some(document()));
        state
            .dispatch(
                &mut library,
                ScriptEditorAction::ReplaceSource {
                    source: "plot(close * 2)".into(),
                },
            )
            .unwrap();
        assert!(state.due(0).is_none());
        let (revision, document) = state.due(350).unwrap();
        let error = library.save(&document).unwrap_err();
        // The adapter restores dirty after reporting its failed idle write.
        state.complete(revision, Some(error), Vec::new());
        state.dirty = true;
        assert!(!state.source_edit_pending());
        library.fail = false;
        state
            .dispatch(&mut library, ScriptEditorAction::Save)
            .unwrap();
        assert!(!state.dirty);
        assert!(state.source_edit_pending());
        assert!(state.validation_pending());
        assert!(state.due(1000).is_none());
        let (revision, _) = state.due(1350).unwrap();
        assert!(
            state.source_edit_pending(),
            "the current completion must reload the saved source"
        );
        assert!(state.complete(revision, None, vec![spec("length")]));
        assert!(state.inputs_ready(&[spec("length")]));
        state
            .dispatch(&mut library, ScriptEditorAction::Save)
            .unwrap();
        assert_eq!(
            library.saves, 2,
            "a clean Save never writes an old read snapshot"
        );
    }
}
