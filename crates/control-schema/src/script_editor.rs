//! Bounded editor/library control contract; stable ids are separate from titles.
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
pub use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCRIPT_EDITOR_CAPABILITY_ID: &str = "indicator.library.edit";
pub const EVENT_KIND: &str = "indicator.library.changed";
pub const MAX_PAGE: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorOperation {
    List,
    Inspect,
    Open {
        id: String,
    },
    Copy {
        id: String,
        new_id: String,
    },
    AddToChart {
        id: String,
    },
    SetNumericInput {
        id: String,
        name: String,
        value: NumericValue,
    },
    ReplaceSource {
        source: String,
    },
    Save,
    Close,
}
impl EditorOperation {
    pub fn action(self) -> Option<ScriptEditorAction> {
        Some(match self {
            Self::List | Self::AddToChart { .. } | Self::SetNumericInput { .. } => return None,
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
    /// Committed numeric inputs for the open identity on the focused pane.
    pub numeric_inputs: Vec<NumericInput>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NumericValue {
    Int(i64),
    Float(quantick_control::wire::CanonicalDecimal),
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct NumericInput {
    pub name: String,
    pub value: NumericValue,
}

impl NumericValue {
    /// Validate exactly the numeric spec addressed by its stable input name.
    pub fn validate(
        &self,
        spec: &quantick_indicators::InputSpec,
    ) -> Result<quantick_indicators::InputValue, String> {
        use quantick_indicators::{InputSpec, InputValue};
        let input = match self {
            Self::Int(v) => InputValue::Int(*v),
            Self::Float(v) => InputValue::Float(
                v.as_str()
                    .parse::<f64>()
                    .map_err(|_| "invalid numeric decimal")?,
            ),
        };
        let valid = match (&input, spec) {
            (
                InputValue::Int(v),
                InputSpec::Int {
                    min, max, options, ..
                },
            ) => {
                min.is_none_or(|lo| *v >= lo)
                    && max.is_none_or(|hi| *v <= hi)
                    && (options.is_empty() || options.contains(v))
            }
            (
                InputValue::Float(v),
                InputSpec::Float {
                    min, max, options, ..
                },
            ) => {
                v.is_finite()
                    && min.is_none_or(|lo| *v >= lo)
                    && max.is_none_or(|hi| *v <= hi)
                    && (options.is_empty() || options.contains(v))
            }
            _ => return Err("numeric input type does not match its declaration".into()),
        };
        if !valid {
            return Err("numeric input is outside its declared range or options".into());
        }
        Ok(input)
    }
    pub fn from_input(value: &quantick_indicators::InputValue) -> Option<Self> {
        match value {
            quantick_indicators::InputValue::Int(v) => Some(Self::Int(*v)),
            quantick_indicators::InputValue::Float(v) => Some(Self::Float(
                quantick_control::wire::CanonicalDecimal::new(if *v == 0.0 {
                    "0".into()
                } else {
                    v.to_string()
                })
                .ok()?,
            )),
            _ => None,
        }
    }
}

pub fn update_numeric_input(
    specs: &[quantick_indicators::InputSpec],
    saved: &[quantick_indicators::InputValue],
    name: &str,
    value: &NumericValue,
) -> Result<Vec<quantick_indicators::InputValue>, String> {
    let mut matches = specs
        .iter()
        .enumerate()
        .filter(|(_, spec)| spec.name() == name);
    let (index, _) = matches.next().ok_or("unknown script input name")?;
    if matches.next().is_some() {
        return Err("script input name is ambiguous".into());
    }
    let input = value.validate(&specs[index])?;
    let mut values = quantick_indicators::bind_by_position(
        specs,
        &saved.iter().cloned().map(Some).collect::<Vec<_>>(),
    )
    .values;
    values[index] = input;
    Ok(values)
}

pub fn capture_numeric_inputs(
    specs: &[quantick_indicators::InputSpec],
    saved: &[quantick_indicators::InputValue],
) -> Vec<NumericInput> {
    let values = quantick_indicators::bind_by_position(
        specs,
        &saved.iter().cloned().map(Some).collect::<Vec<_>>(),
    )
    .values;
    specs
        .iter()
        .zip(values)
        .filter_map(|(spec, value)| {
            Some(NumericInput {
                name: spec.name().into(),
                value: NumericValue::from_input(&value)?,
            })
        })
        .take(MAX_PAGE)
        .collect()
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<EditorInput, EditorResult>(
        SCRIPT_EDITOR_CAPABILITY_ID,
        "indicator",
        "Read, copy and edit Pine indicator source",
        "List the script library (1..64 entries per page), open/read source by stable filename id, create a distinct persisted editable copy, replace the open copy's source, save, add a library identity to the focused chart, set a named numeric input on the open identity attached to the focused pane, inspect editor state or close. Numeric values use explicit int/float types (float values are canonical decimal strings) and must satisfy the current declaration bounds/options. Open/copy validates asynchronously without rewriting source. Close any settings dialog and wait for current source validation and chart declaration publication before a numeric edit; numeric_inputs is empty until ready. Built-in source is read-only. Source is limited to 256 KiB. Edits save and validate after 350 ms idle; valid edits reload existing attached instances, errors retain last-valid chart output. Inspect reports source, revision, dirty/compiling status, diagnostics and up to 64 committed numeric inputs for the open focused attachment. Saved explicit input overrides survive source edits; untouched defaults follow source live and after restart. Copy names use letters, digits, underscores or hyphens followed by .pine. Existing names are refused, never overwritten.",
        "Open and replace set explicit state. Copy is exclusive: retry with the same idempotency key to recover the original result. Add-to-chart uses the existing library attachment path and is a no-op if that identity is already on the focused pane. Set-numeric-input commits through the existing settings Apply path; repeated explicit values preserve the same durable settings and inspect reconciles them. Inspect/list reconcile current state. Editing never attaches a duplicate chart indicator.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.required_permissions.insert(
        quantick_control::id::PermissionId::new("observe.user_text").expect("static permission"),
    );
    descriptor
}
pub const READBACKS: &[Readback] = &[journal(
    SCRIPT_EDITOR_CAPABILITY_ID,
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
        numeric_inputs: Vec::new(),
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::*;
    use quantick_indicators::{InputSpec, InputValue};
    fn float(value: &str) -> NumericValue {
        NumericValue::Float(quantick_control::wire::CanonicalDecimal::new(value).unwrap())
    }
    #[test]
    fn numeric_update_checks_name_type_ranges_options_and_finite_values() {
        let specs = vec![
            InputSpec::Int {
                name: "length".into(),
                title: "Length".into(),
                default: 9,
                min: Some(1),
                max: Some(30),
                step: None,
                options: vec![4, 9, 17],
            },
            InputSpec::Float {
                name: "factor".into(),
                title: "Factor".into(),
                default: 1.0,
                min: Some(0.0),
                max: Some(3.0),
                step: None,
                options: vec![],
            },
        ];
        let saved = vec![InputValue::Int(17), InputValue::Float(2.0)];
        let changed =
            update_numeric_input(&specs, &saved, "length", &NumericValue::Int(4)).unwrap();
        assert_eq!(changed, vec![InputValue::Int(4), InputValue::Float(2.0)]);
        assert_eq!(
            update_numeric_input(&specs, &changed, "length", &NumericValue::Int(4)).unwrap(),
            changed
        );
        for (name, value) in [
            ("missing", NumericValue::Int(4)),
            ("length", float("4")),
            ("length", NumericValue::Int(0)),
            ("length", NumericValue::Int(31)),
            ("length", NumericValue::Int(5)),
            ("factor", float(&"9".repeat(400))),
            ("factor", float(&"9".repeat(401))),
            ("factor", float("-1")),
            ("factor", float("4")),
        ] {
            assert!(update_numeric_input(&specs, &saved, name, &value).is_err());
        }
        assert_eq!(
            capture_numeric_inputs(&specs, &changed)[0].value,
            NumericValue::Int(4)
        );
        for invalid in ["NaN", "inf", "-inf", "1e0", "1.0"] {
            assert!(quantick_control::wire::CanonicalDecimal::new(invalid).is_err());
        }
        assert_eq!(
            NumericValue::from_input(&InputValue::Float(-0.0)),
            Some(float("0"))
        );
        assert!(
            quantick_control::canonical::validate_control_value(
                &serde_json::to_value(capture_numeric_inputs(&specs, &changed)).unwrap()
            )
            .is_ok()
        );
        let duplicate = vec![specs[0].clone(), specs[0].clone()];
        assert!(update_numeric_input(&duplicate, &[], "length", &NumericValue::Int(4)).is_err());
        let floats = [InputSpec::Float {
            name: "option".into(),
            title: "Option".into(),
            default: 1.0,
            min: None,
            max: None,
            step: None,
            options: vec![1.0, 2.0],
        }];
        assert!(update_numeric_input(&floats, &[], "option", &float("1.5")).is_err());
        assert!(update_numeric_input(&floats, &[], "option", &float("2")).is_ok());
    }
}
