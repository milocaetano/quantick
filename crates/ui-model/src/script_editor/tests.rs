use super::*;
use crate::Model as _;
use quantick_stores::script_library::{ScriptEntry, ScriptLibraryPort};

#[derive(Default)]
struct Library {
    saved: Vec<ScriptDocument>,
    fail: bool,
}
impl ScriptLibraryPort for Library {
    fn list(&self) -> &[ScriptEntry] {
        &[]
    }
    fn read(&self, id: &str) -> Result<ScriptDocument, String> {
        Ok(ScriptDocument {
            id: id.into(),
            title: id.into(),
            source: "plot(close)".into(),
            read_only: id == "builtin",
        })
    }
    fn copy(&mut self, _: &str, id: &str) -> Result<ScriptDocument, String> {
        self.read(id)
    }
    fn save(&mut self, document: &ScriptDocument) -> Result<(), String> {
        if self.fail {
            return Err("save refused".into());
        }
        self.saved.push(document.clone());
        Ok(())
    }
}
fn action(state: &mut State, library: &mut Library, action: ScriptEditorAction) -> Vec<Effect> {
    let mut effects = Model::update(state, Intent::Action(action));
    assert!(state.editor.is_none());
    let Effect::Dispatch { mut editor, action } = effects.pop().unwrap() else {
        panic!("dispatch")
    };
    let result = editor.dispatch(library, action);
    let effects = Model::update(state, Intent::ActionSettled { editor, result });
    assert!(
        state.editor.is_some(),
        "both success and refusal restore ownership"
    );
    effects
}
fn open(state: &mut State, library: &mut Library) {
    assert!(matches!(
        action(
            state,
            library,
            ScriptEditorAction::Open { id: "copy".into() }
        )
        .as_slice(),
        [Effect::ActionResult(Ok(Some(_)))]
    ));
}
fn edit(state: &mut State, library: &mut Library) {
    action(
        state,
        library,
        ScriptEditorAction::ReplaceSource {
            source: "plot(close * 4)".into(),
        },
    );
}
fn due(state: &mut State, at: u64) -> (u64, ScriptDocument, bool) {
    assert!(Model::update(state, Intent::Tick(at)).is_empty());
    assert!(Model::update(state, Intent::Tick(at + 349)).is_empty());
    match Model::update(state, Intent::Tick(at + 350)).pop().unwrap() {
        Effect::Save { revision, document } => (revision, document, true),
        Effect::Validate { revision, document } => (revision, document, false),
        other => panic!("unexpected {other:?}"),
    }
}
fn spec(name: &str) -> InputSpec {
    InputSpec::Int {
        name: name.into(),
        title: name.into(),
        default: 9,
        min: None,
        max: None,
        step: None,
        options: vec![],
    }
}
fn complete(
    state: &mut State,
    revision: u64,
    document: ScriptDocument,
    error: Option<&str>,
) -> Vec<Effect> {
    Model::update(
        state,
        Intent::Completed {
            revision,
            document,
            error: error.map(str::to_owned),
            inputs: vec![spec("length")],
        },
    )
}

/// C17: orchestration previously asserted through QuantickApp now uses intents.
#[test]
fn migration_preserves_state_and_effects() {
    let (mut state, mut library) = (State::default(), Library::default());
    open(&mut state, &mut library);
    assert!(!state.editor().inputs_ready(&[spec("length")]));
    let (revision, document, save) = due(&mut state, 0);
    assert!(!save);
    assert!(
        complete(&mut state, revision, document, None).is_empty(),
        "read validation does not publish"
    );
    assert!(state.editor().inputs_ready(&[spec("length")]));
    edit(&mut state, &mut library);
    assert!(!state.editor().inputs_ready(&[spec("length")]));
    let (revision, document, save) = due(&mut state, 1000);
    assert!(save);
    let effects = Model::update(
        &mut state,
        Intent::Saved {
            revision,
            document: document.clone(),
            result: Ok(()),
        },
    );
    assert_eq!(
        effects,
        vec![
            Effect::ObserveSaved(document.clone()),
            Effect::Validate {
                revision,
                document: document.clone()
            }
        ]
    );
    assert!(
        Model::update(&mut state, Intent::Tick(9000)).is_empty(),
        "one outstanding compile"
    );
    assert_eq!(
        complete(&mut state, revision, document.clone(), None),
        vec![Effect::Publish(document)]
    );
    assert!(state.editor().inputs_ready(&[spec("length")]));
    assert!(!state.editor().inputs_ready(&[spec("other")]));
}

#[test]
fn stale_completion_and_disconnect_preserve_newer_edit() {
    for failure in [None, Some("invalid"), Some("disconnect")] {
        let (mut state, mut library) = (State::default(), Library::default());
        open(&mut state, &mut library);
        let (revision, document, _) = due(&mut state, 0);
        open(&mut state, &mut library);
        assert!(!state.editor().compiling, "reopen resets domain flags");
        assert!(
            Model::update(&mut state, Intent::Tick(1000)).is_empty(),
            "old receiver still occupies validation"
        );
        edit(&mut state, &mut library);
        let effects = if failure == Some("disconnect") {
            Model::update(
                &mut state,
                Intent::Failed {
                    revision,
                    error: "source validation worker stopped".into(),
                },
            )
        } else {
            complete(&mut state, revision, document, failure)
        };
        assert!(effects.is_empty());
        let editor = state.editor();
        assert!(editor.dirty && editor.source_edit_pending() && editor.validation_pending());
        assert!(editor.diagnostics.is_none());
        assert_eq!(editor.document.as_ref().unwrap().source, "plot(close * 4)");
        assert!(matches!(
            action(&mut state, &mut library, ScriptEditorAction::Close).as_slice(),
            [Effect::ActionResult(Err(_))]
        ));
        let (revision, document, save) = due(&mut state, 2000);
        assert!(save);
        assert_eq!(
            complete(&mut state, revision, document.clone(), None),
            vec![Effect::Publish(document)]
        );
    }
}

#[test]
fn manual_save_and_failure_recovery_preserve_publication_boundary() {
    let (mut state, mut library) = (State::default(), Library::default());
    open(&mut state, &mut library);
    edit(&mut state, &mut library);
    let (revision, document, _) = due(&mut state, 0);
    assert!(
        Model::update(
            &mut state,
            Intent::Saved {
                revision,
                document,
                result: Err("disk unavailable".into())
            }
        )
        .is_empty()
    );
    assert!(state.editor().dirty);
    assert_eq!(
        state.editor().diagnostics.as_deref(),
        Some("disk unavailable")
    );
    library.fail = true;
    assert!(matches!(
        action(&mut state, &mut library, ScriptEditorAction::Save).as_slice(),
        [Effect::ActionResult(Err(_))]
    ));
    assert!(state.editor().dirty);
    library.fail = false;
    assert!(matches!(
        action(&mut state, &mut library, ScriptEditorAction::Save).as_slice(),
        [Effect::ObserveSaved(_), Effect::ActionResult(Ok(_))]
    ));
    assert_eq!(library.saved.len(), 1);
    assert!(state.editor().source_edit_pending() && state.editor().validation_pending());
    assert!(!state.editor().dirty);
    let (revision, document, save) = due(&mut state, 1000);
    assert!(!save);
    assert!(complete(&mut state, revision, document, Some("PINE_UNKNOWN_NAME")).is_empty());
    assert!(!state.editor().inputs_ready(&[spec("length")]));
    action(
        &mut state,
        &mut library,
        ScriptEditorAction::ReplaceSource {
            source: "plot(high)".into(),
        },
    );
    let (revision, document, _) = due(&mut state, 2000);
    assert_eq!(
        complete(&mut state, revision, document.clone(), None),
        vec![Effect::Publish(document)]
    );
}

#[test]
fn reopened_validation_and_clean_save_cannot_publish_old_read_snapshot() {
    let (mut state, mut library) = (State::default(), Library::default());
    open(&mut state, &mut library);
    let (old, document, _) = due(&mut state, 0);
    assert!(complete(&mut state, old, document.clone(), None).is_empty());
    open(&mut state, &mut library);
    assert!(!state.editor().inputs_ready(&[spec("length")]));
    assert!(complete(&mut state, old, document, None).is_empty());
    assert!(!state.editor().inputs_ready(&[spec("length")]));
    assert!(matches!(
        action(&mut state, &mut library, ScriptEditorAction::Save).as_slice(),
        [Effect::ActionResult(Ok(_))]
    ));
    assert!(library.saved.is_empty());
    let (revision, document, _) = due(&mut state, 1000);
    assert!(complete(&mut state, revision, document, None).is_empty());
    assert!(state.editor().inputs_ready(&[spec("length")]));
}

#[test]
fn current_disconnect_and_start_failure_are_errors_without_publication() {
    for error in [
        "source validation worker stopped",
        "cannot validate source: unavailable",
    ] {
        let (mut state, mut library) = (State::default(), Library::default());
        open(&mut state, &mut library);
        let (revision, _, _) = due(&mut state, 0);
        assert!(
            Model::update(
                &mut state,
                Intent::Failed {
                    revision,
                    error: error.into()
                }
            )
            .is_empty()
        );
        assert_eq!(state.editor().diagnostics.as_deref(), Some(error));
        assert!(!state.editor().validation_pending());
    }
}
