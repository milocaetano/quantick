//! Source validation orchestration; stores remains the document/revision authority.
use quantick_indicators::InputSpec;
use quantick_stores::{
    script_editor::ScriptEditor,
    script_library::{ScriptDocument, ScriptEditorAction},
};

#[derive(Debug)]
pub struct State {
    editor: Option<Box<ScriptEditor>>,
    dirty_save: Option<bool>,
    validation: Option<u64>,
    pub copy_name: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            editor: Some(Box::default()),
            dirty_save: None,
            validation: None,
            copy_name: String::new(),
        }
    }
}
impl State {
    /// The host settles synchronous dispatch before any observation or next intent.
    pub fn editor(&self) -> &ScriptEditor {
        self.editor
            .as_deref()
            .expect("script action must settle before observation")
    }
}
#[derive(Debug)]
pub enum Intent {
    Requests {
        actions: Vec<ScriptEditorAction>,
        add_to_chart: Option<usize>,
        copy_name: String,
    },
    Action(ScriptEditorAction),
    ActionSettled {
        editor: Box<ScriptEditor>,
        result: Result<Option<ScriptDocument>, String>,
    },
    Tick(u64),
    Saved {
        revision: u64,
        document: ScriptDocument,
        result: Result<(), String>,
    },
    Completed {
        revision: u64,
        document: ScriptDocument,
        error: Option<String>,
        inputs: Vec<InputSpec>,
    },
    Failed {
        revision: u64,
        error: String,
    },
}
#[derive(Debug, PartialEq)]
pub enum Effect {
    Requests {
        actions: Vec<ScriptEditorAction>,
        add_to_chart: Option<usize>,
    },
    Dispatch {
        editor: Box<ScriptEditor>,
        action: ScriptEditorAction,
    },
    ActionResult(Result<Option<ScriptDocument>, String>),
    Save {
        revision: u64,
        document: ScriptDocument,
    },
    Validate {
        revision: u64,
        document: ScriptDocument,
    },
    ObserveSaved(ScriptDocument),
    Publish(ScriptDocument),
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Vec<Effect>;
    fn update(state: &mut State, intent: Intent) -> Vec<Effect> {
        match intent {
            Intent::ActionSettled { editor, result } => {
                assert!(state.editor.is_none(), "no action awaiting settlement");
                state.editor = Some(editor);
                let saved = state.dirty_save.take().expect("action context");
                let mut effects = Vec::new();
                if saved && let Ok(Some(document)) = &result {
                    effects.push(Effect::ObserveSaved(document.clone()));
                }
                effects.push(Effect::ActionResult(result));
                effects
            }
            intent => {
                let editor = state
                    .editor
                    .as_mut()
                    .expect("settle action before next intent");
                match intent {
                    Intent::Requests {
                        actions,
                        add_to_chart,
                        copy_name,
                    } => {
                        state.copy_name = copy_name;
                        vec![Effect::Requests {
                            actions,
                            add_to_chart,
                        }]
                    }
                    Intent::Action(action) => {
                        state.dirty_save =
                            Some(matches!(action, ScriptEditorAction::Save) && editor.dirty);
                        vec![Effect::Dispatch {
                            editor: state.editor.take().expect("settled editor"),
                            action,
                        }]
                    }
                    Intent::Tick(now) => {
                        if state.validation.is_some() {
                            return vec![];
                        }
                        let Some((revision, document)) = editor.due(now) else {
                            return vec![];
                        };
                        state.validation = Some(revision);
                        vec![if editor.dirty {
                            Effect::Save { revision, document }
                        } else {
                            Effect::Validate { revision, document }
                        }]
                    }
                    Intent::Saved {
                        revision,
                        document,
                        result,
                    } => match result {
                        Ok(()) => vec![
                            Effect::ObserveSaved(document.clone()),
                            Effect::Validate { revision, document },
                        ],
                        Err(error) => {
                            state.validation = None;
                            editor.complete(revision, Some(error), vec![]);
                            editor.dirty = true;
                            vec![]
                        }
                    },
                    Intent::Completed {
                        revision,
                        document,
                        error,
                        inputs,
                    } => {
                        state.validation = None;
                        let publish = error.is_none() && editor.source_edit_pending();
                        if editor.complete(revision, error, inputs) && publish {
                            vec![Effect::Publish(document)]
                        } else {
                            vec![]
                        }
                    }
                    Intent::Failed { revision, error } => {
                        state.validation = None;
                        editor.complete(revision, Some(error), vec![]);
                        vec![]
                    }
                    Intent::ActionSettled { .. } => unreachable!(),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "script_editor/tests.rs"]
mod tests;
