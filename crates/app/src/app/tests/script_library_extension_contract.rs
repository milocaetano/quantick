//! C0: alternate surface, control host and library share the docked contracts.
use std::collections::BTreeMap;
use std::time::Instant;

use eframe::egui;
use quantick_control::{error::ControlError, wire::ActorContext};
use quantick_control_handlers::script_editor::{ScriptEditorPort, dispatch};
use quantick_control_host::actions::ActionRegistry;
use quantick_stores::script_library::{
    ScriptDocument, ScriptEditorAction, ScriptEntry, ScriptLibrary, ScriptLibraryPort, ScriptOrigin,
};
use serde_json::Value;

use crate::surfaces::{Surface, SurfaceEnv, SurfaceResponse};
use quantick_ui_model::script_editor::{Effect, Intent, Model, State};

struct FakeLibrary {
    entries: Vec<ScriptEntry>,
    documents: BTreeMap<String, ScriptDocument>,
}
impl FakeLibrary {
    fn new() -> Self {
        let doc = ScriptDocument {
            id: "starter.pine".into(),
            title: "Starter".into(),
            source: "plot(close)".into(),
            read_only: true,
        };
        Self {
            entries: vec![ScriptEntry {
                name: doc.id.clone(),
                origin: ScriptOrigin::Embedded("plot(close)"),
            }],
            documents: BTreeMap::from([(doc.id.clone(), doc)]),
        }
    }
}
impl ScriptLibraryPort for FakeLibrary {
    fn list(&self) -> &[ScriptEntry] {
        &self.entries
    }
    fn read(&self, id: &str) -> Result<ScriptDocument, String> {
        self.documents
            .get(id)
            .cloned()
            .ok_or_else(|| "unknown script".into())
    }
    fn copy(&mut self, id: &str, new_id: &str) -> Result<ScriptDocument, String> {
        let mut doc = self.read(id)?;
        doc.id = new_id.into();
        doc.read_only = false;
        self.documents.insert(doc.id.clone(), doc.clone());
        Ok(doc)
    }
    fn save(&mut self, doc: &ScriptDocument) -> Result<(), String> {
        if doc.read_only {
            return Err("read-only script".into());
        }
        self.documents.insert(doc.id.clone(), doc.clone());
        Ok(())
    }
}
struct FakeEditor {
    library: FakeLibrary,
    state: State,
}
impl ScriptEditorPort for FakeEditor {
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, String> {
        use quantick_ui_model::Model as _;
        let Effect::Dispatch { mut editor, action } =
            Model::update(&mut self.state, Intent::Action(action))
                .pop()
                .unwrap()
        else {
            panic!("dispatch")
        };
        let result = editor.dispatch(&mut self.library, action);
        let effects = Model::update(&mut self.state, Intent::ActionSettled { editor, result });
        let Effect::ActionResult(result) = effects.into_iter().last().unwrap() else {
            panic!("result")
        };
        result
    }
}
struct FakeView {
    actions: Vec<ScriptEditorAction>,
}
impl crate::surfaces::View<Model> for FakeView {
    fn id(&self) -> &'static str {
        "fake-script-editor"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        Some(Intent::Requests {
            actions: std::mem::take(&mut self.actions),
            add_to_chart: None,
            copy_name: String::new(),
        })
    }
}
fn control_dispatch(
    editor: &mut FakeEditor,
    _access: &mut (),
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let action = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = dispatch(editor, action).map_err(ControlError::invalid_request)?;
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}
fn actor() -> ActorContext {
    use quantick_control::id::{ConnectionId, PrincipalId, RequestId};
    ActorContext {
        actor_kind: quantick_control::wire::ActorKind::Agent,
        principal_id: PrincipalId::from_bytes(
            [1; quantick_control::limits::CONTROL_RUNTIME_ID_BYTES],
        ),
        client_name: "extension contract".into(),
        connection_id: ConnectionId::from_bytes(
            [2; quantick_control::limits::CONTROL_RUNTIME_ID_BYTES],
        ),
        request_id: RequestId::new("extension-contract").unwrap(),
        reason: None,
        requested_at_unix_ms: 0,
    }
}

#[test]
fn script_library_extension_contract() {
    let mut registry: ActionRegistry<FakeEditor, ()> = ActionRegistry::new();
    let mut descriptor = quantick_control_schema::indicator_picker::descriptor();
    descriptor.id = quantick_control::id::CapabilityId::new("indicator.fake_editor").unwrap();
    descriptor.input_schema = quantick_control::schema::generated_schema::<Value>();
    descriptor.output_schema = quantick_control::schema::generated_schema::<Value>();
    registry.register(descriptor, control_dispatch).unwrap();
    let registered = registry.lookup("indicator.fake_editor", 1).unwrap();
    let mut editor = FakeEditor {
        library: FakeLibrary::new(),
        state: State::default(),
    };
    assert_eq!(editor.library.list()[0].name, "starter.pine");
    let view = FakeView {
        actions: vec![
            ScriptEditorAction::Open {
                id: "starter.pine".into(),
            },
            ScriptEditorAction::Copy {
                id: "starter.pine".into(),
                new_id: "copy.pine".into(),
            },
            ScriptEditorAction::ReplaceSource {
                source: "plot(high)".into(),
            },
            ScriptEditorAction::Save,
        ],
    };
    let mut surface =
        crate::surfaces::Adapter::<Model, _>::new(view, crate::surfaces::script_editor::effects);
    let response = surface.draw(
        &egui::Context::default(),
        &SurfaceEnv::quiet(Instant::now()),
    );
    for action in response.script_editor_actions {
        (registered.handler)(
            &mut editor,
            &mut (),
            &actor(),
            &serde_json::to_value(action).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        editor.library.read("copy.pine").unwrap().source,
        "plot(high)"
    );
    assert_eq!(
        editor.library.read("starter.pine").unwrap().source,
        "plot(close)"
    );
    assert!(editor.library.read("starter.pine").unwrap().read_only);
    use quantick_ui_model::Model as _;
    let document = editor.state.editor().document.clone().unwrap();
    let revision = editor.state.editor().revision;
    Model::update(
        &mut editor.state,
        Intent::Completed {
            revision,
            document,
            error: None,
            inputs: vec![],
        },
    );
    dispatch(&mut editor, ScriptEditorAction::Close).unwrap();
    assert!(
        dispatch(&mut editor, ScriptEditorAction::Inspect)
            .unwrap()
            .is_none()
    );
}

#[test]
fn script_library_extension_contract_preserves_existing_behavior() {
    let dir = crate::scratch::ScratchDir::new("script-contract");
    let mut library = ScriptLibrary::scan_dir(&dir);
    let id = library.entries()[0].name.clone();
    let original = ScriptLibraryPort::read(&library, &id).unwrap();
    assert_eq!(Some(Ok(original.source.clone())), library.read(0));
    assert!(original.read_only);
    assert!(ScriptLibraryPort::read(&library, "missing.pine").is_err());
    assert!(!library.copy(&id, "copy.pine").unwrap().read_only);
    assert!(library.save(&original).is_err());
    let mut surfaces = crate::surfaces::Surfaces::default();
    assert!(
        surfaces
            .script_editor
            .supports_pass(quantick_ui_model::Slot::Overlay)
    );
    assert_eq!(
        surfaces.script_editor.draw(
            &egui::Context::default(),
            &SurfaceEnv::quiet(Instant::now())
        ),
        SurfaceResponse::default()
    );
    let response = surfaces
        .script_editor
        .update(Intent::Action(ScriptEditorAction::Open { id }));
    let quantick_ui_model::HostEffect::ScriptEditor(mut effects) =
        response.model_effects.into_iter().next().unwrap()
    else {
        panic!("catalog effect")
    };
    let Effect::Dispatch { mut editor, action } = effects.pop().unwrap() else {
        panic!("dispatch")
    };
    let result = editor.dispatch(&mut library, action);
    surfaces
        .script_editor
        .update(Intent::ActionSettled { editor, result });
    assert_eq!(
        surfaces.script_editor.model_state.editor().document,
        Some(original)
    );
    assert!(dir.join("copy.pine").exists());
}
