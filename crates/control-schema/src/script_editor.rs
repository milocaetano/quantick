//! Bounded editor/library control contract; stable ids are separate from titles.
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
pub use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CAPABILITY_ID: &str = "indicator.library.edit";
pub const EVENT_KIND: &str = "indicator.library.changed";
pub const MAX_PAGE: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorOperation {
    List,
    Inspect,
    Open { id: String },
    Copy { id: String, new_id: String },
    AddToChart { id: String },
    ReplaceSource { source: String },
    Save,
    Close,
}
impl EditorOperation {
    pub fn action(self) -> Option<ScriptEditorAction> {
        Some(match self {
            Self::List | Self::AddToChart { .. } => return None,
            Self::Inspect => ScriptEditorAction::Inspect,
            Self::Open { id } => ScriptEditorAction::Open { id },
            Self::Copy { id, new_id } => ScriptEditorAction::Copy { id, new_id },
            Self::ReplaceSource { source } => ScriptEditorAction::ReplaceSource { source },
            Self::Save => ScriptEditorAction::Save,
            Self::Close => ScriptEditorAction::Close,
        })
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorInput {
    pub operation: EditorOperation,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    MAX_PAGE
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct LibraryEntry {
    pub id: String,
    pub title: String,
    pub read_only: bool,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub source: String,
    pub read_only: bool,
}
impl From<ScriptDocument> for Document {
    fn from(document: ScriptDocument) -> Self {
        Self {
            id: document.id,
            title: document.title,
            source: document.source,
            read_only: document.read_only,
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct EditorResult {
    pub document: Option<Document>,
    pub dirty: bool,
    pub compiling: bool,
    pub revision: String,
    pub diagnostics: Option<String>,
    pub entries: Vec<LibraryEntry>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<EditorInput, EditorResult>(
        CAPABILITY_ID,
        "indicator",
        "Read, copy and edit Pine indicator source",
        "List the script library (1..64 entries per page), open/read source by stable filename id, create a distinct persisted editable copy, replace the open copy's source, save, add a library identity to the focused chart, inspect editor state or close. Built-ins are read-only. Source is limited to 256 KiB. Edits save and validate after 350 ms idle; valid edits reload existing attached instances, errors retain last-valid chart output. Inspect reports source, revision, dirty/compiling status and diagnostics. Copy names use letters, digits, underscores or hyphens followed by .pine. Existing names are refused, never overwritten.",
        "Open and replace set explicit state. Copy is exclusive: retry with the same idempotency key to recover the original result. Add-to-chart uses the existing library attachment path and is a no-op if that identity is already on the focused pane. Inspect/list reconcile current state. Editing never attaches a duplicate chart indicator.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.required_permissions.insert(
        quantick_control::id::PermissionId::new("observe.user_text").expect("static permission"),
    );
    descriptor
}
pub const READBACKS: &[Readback] = &[journal(
    CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    EVENT_KIND,
    "payload.script_editor",
    "the event reports the resulting editor revision; inspect reconciles source and diagnostics",
    &[EVERY_OPTIONAL_TEST],
)];
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "indicator-library-editor-input-v1.schema.json",
            quantick_control::schema::generated_schema::<EditorInput>(),
        ),
        (
            "indicator-library-editor-result-v1.schema.json",
            quantick_control::schema::generated_schema::<EditorResult>(),
        ),
    ]
}

/// Pure paged projection shared by app and alternate editor hosts.
pub fn capture(
    state: &quantick_stores::script_editor::ScriptEditor,
    entries: &[quantick_stores::script_library::ScriptEntry],
    offset: usize,
    limit: usize,
) -> EditorResult {
    let next = offset.saturating_add(limit);
    EditorResult {
        document: state.document.clone().map(Into::into),
        dirty: state.dirty,
        compiling: state.compiling,
        revision: state.revision.to_string(),
        diagnostics: state.diagnostics.clone(),
        entries: entries
            .iter()
            .skip(offset)
            .take(limit.min(MAX_PAGE))
            .map(|entry| LibraryEntry {
                id: entry.name.clone(),
                title: quantick_stores::script_library::script_title(&entry.name),
                read_only: matches!(
                    entry.origin,
                    quantick_stores::script_library::ScriptOrigin::Embedded(_)
                ),
            })
            .collect(),
        total: entries.len(),
        next_offset: (next < entries.len()).then_some(next),
    }
}
