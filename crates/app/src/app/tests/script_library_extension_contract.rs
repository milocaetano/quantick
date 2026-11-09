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
    document: Option<ScriptDocument>,
}
impl ScriptEditorPort for FakeEditor {
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, String> {
        match action {
            ScriptEditorAction::Open { id } => self.document = Some(self.library.read(&id)?),
            ScriptEditorAction::Copy { id, new_id } => {
                self.document = Some(self.library.copy(&id, &new_id)?);
            }
            ScriptEditorAction::ReplaceSource { source } => {
                self.document.as_mut().ok_or("no document")?.source = source;
            }
            ScriptEditorAction::Save => self
                .library
                .save(self.document.as_ref().ok_or("no document")?)?,
            ScriptEditorAction::Close => self.document = None,
            ScriptEditorAction::Inspect => (),
        }
        Ok(self.document.clone())
    }
}
struct FakeSurface {
    actions: Vec<ScriptEditorAction>,
}
impl Surface for FakeSurface {
    fn id(&self) -> &'static str {
        "fake-script-editor"
    }
    fn draw(&mut self, _ctx: &egui::Context, _env: &SurfaceEnv<'_>) -> SurfaceResponse {
        SurfaceResponse {
            script_editor_actions: std::mem::take(&mut self.actions),
            ..Default::default()
        }
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
        document: None,
    };
    assert_eq!(editor.library.list()[0].name, "starter.pine");
    let mut surface = FakeSurface {
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
    let response = crate::surfaces::script_editor::draw(
        &mut surface,
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
    assert!(library.copy(&id, "copy.pine").is_err());
    assert!(library.save(&original).is_err());
    let mut surface = crate::surfaces::script_editor::ScriptEditorSurface::default();
    assert_eq!(
        crate::surfaces::script_editor::draw(
            &mut surface,
            &egui::Context::default(),
            &SurfaceEnv::quiet(Instant::now()),
        ),
        SurfaceResponse::default()
    );
    assert!(
        surface
            .state
            .dispatch(&mut library, ScriptEditorAction::Open { id },)
            .is_err()
    );
    assert!(surface.state.document.is_none());
    assert!(!dir.join("copy.pine").exists());
}
