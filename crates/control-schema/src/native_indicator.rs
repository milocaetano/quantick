//! Native indicator operations over the same layout entries as the settings UI.
mod decimal;
use quantick_control::{
    id::{EffectId, PermissionId, RiskFlagId},
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_control_host::authority::{
    COCKPIT_INDICATORS_PERMISSION_ID, COCKPIT_PERMISSION_ID, INDICATORS_CHANGED_RISK_FLAG,
    INDICATORS_EFFECT_ID, ReadSpec, read_descriptor,
};
use quantick_indicators::{InputSpec, InputValue, Rgba8, SourceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const EDIT_CAPABILITY_ID: &str = "indicator.native.edit";
pub const LIST_CAPABILITY_ID: &str = "indicator.native.list";
pub const EVENT: &str = "indicator.native.changed";
pub const MAX_PAGE: usize = 64;
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Int(i64),
    Float(
        #[serde(with = "decimal::float")]
        #[schemars(with = "quantick_control::wire::CanonicalDecimal")]
        f64,
    ),
    Bool(bool),
    Color([u8; 4]),
    Str(String),
    Source(String),
}
impl Value {
    pub fn from_input(value: &InputValue) -> Self {
        match value {
            InputValue::Int(v) => Self::Int(*v),
            InputValue::Float(v) => Self::Float(*v),
            InputValue::Bool(v) => Self::Bool(*v),
            InputValue::Color(v) => Self::Color([v.r, v.g, v.b, v.a]),
            InputValue::Str(v) => Self::Str(v.clone()),
            InputValue::Source(v) => Self::Source(v.as_str().into()),
        }
    }
    pub fn validate(&self, spec: &InputSpec) -> Result<InputValue, String> {
        let value = match (self, spec) {
            (
                Self::Int(v),
                InputSpec::Int {
                    min, max, options, ..
                },
            ) if min.is_none_or(|m| *v >= m)
                && max.is_none_or(|m| *v <= m)
                && (options.is_empty() || options.contains(v)) =>
            {
                InputValue::Int(*v)
            }
            (
                Self::Float(v),
                InputSpec::Float {
                    min, max, options, ..
                },
            ) if v.is_finite()
                && min.is_none_or(|m| *v >= m)
                && max.is_none_or(|m| *v <= m)
                && (options.is_empty() || options.contains(v)) =>
            {
                InputValue::Float(*v)
            }
            (Self::Bool(v), InputSpec::Bool { .. }) => InputValue::Bool(*v),
            (Self::Color(v), InputSpec::Color { .. }) => {
                InputValue::Color(Rgba8::new(v[0], v[1], v[2], v[3]))
            }
            (Self::Str(v), InputSpec::Str { options, .. })
                if v.len() <= 256 && (options.is_empty() || options.contains(v)) =>
            {
                InputValue::Str(v.clone())
            }
            (Self::Source(v), InputSpec::Source { .. }) => InputValue::Source(
                SourceId::ALL
                    .into_iter()
                    .find(|s| s.as_str() == v)
                    .ok_or("unknown source")?,
            ),
            _ => return Err(format!("invalid type or value for input {}", spec.name())),
        };
        Ok(value)
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlotStyle {
    pub visible: Option<bool>,
    pub color: Option<[u8; 4]>,
    #[serde(default, with = "decimal::width")]
    #[schemars(with = "Option<quantick_control::wire::CanonicalDecimal>")]
    pub width: Option<f32>,
}
impl PlotStyle {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .width
            .is_some_and(|w| !w.is_finite() || !(0.5..=8.0).contains(&w))
        {
            return Err("plot width must be in 0.5..=8".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Add {
        native_id: String,
    },
    SetInputs {
        slot_id: WireU64,
        values: BTreeMap<String, Value>,
    },
    SetPlotStyle {
        slot_id: WireU64,
        plot: usize,
        style: PlotStyle,
    },
    SetHidden {
        slot_id: WireU64,
        hidden: bool,
    },
    Remove {
        slot_id: WireU64,
    },
}
impl Operation {
    pub fn slot(&self) -> Option<u64> {
        match self {
            Self::Add { .. } => None,
            Self::SetInputs { slot_id, .. }
            | Self::SetPlotStyle { slot_id, .. }
            | Self::SetHidden { slot_id, .. }
            | Self::Remove { slot_id } => Some(slot_id.get()),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub target: Target,
    pub operation: Operation,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListInput {
    pub target: Target,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page_size")]
    pub limit: usize,
}
fn page_size() -> usize {
    MAX_PAGE
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Entry {
    pub slot_id: WireU64,
    pub native_id: String,
    pub hidden: bool,
    pub inputs: BTreeMap<String, Value>,
    pub plot_styles: Vec<PlotStyle>,
    pub loaded: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct CatalogEntry {
    pub native_id: String,
    pub title: String,
    pub source_note: Option<String>,
    pub defaults: BTreeMap<String, Value>,
    pub plot_count: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListResult {
    pub target: Target,
    pub catalog: Vec<CatalogEntry>,
    pub entries: Vec<Entry>,
    pub total: usize,
    pub next_offset: Option<usize>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct EditResult {
    pub target: Target,
    pub slot_id: WireU64,
    pub removed: bool,
}
/// Validate a whole patch before sending any worker command or persistence edit.
pub fn inputs(
    native_id: &str,
    saved: &[InputValue],
    patch: &BTreeMap<String, Value>,
) -> Result<Vec<InputValue>, String> {
    let specs = quantick_indicators::native::native(native_id)
        .ok_or("unknown native indicator")?
        .default_inputs();
    if patch
        .keys()
        .any(|key| !specs.iter().any(|s| s.name() == key))
    {
        return Err("unknown native input name".into());
    }
    specs
        .iter()
        .enumerate()
        .map(|(i, s)| match patch.get(s.name()) {
            Some(v) => v.validate(s),
            None => Ok(saved.get(i).cloned().unwrap_or_else(|| s.default_value())),
        })
        .collect()
}
pub fn catalog() -> Vec<CatalogEntry> {
    quantick_indicators::native::NATIVES
        .iter()
        .map(|n| {
            let instance = n.build();
            let d = instance.descriptor();
            CatalogEntry {
                native_id: n.id.into(),
                title: n.menu_label.into(),
                source_note: n.source_note().map(str::to_owned),
                defaults: d
                    .inputs
                    .iter()
                    .map(|s| (s.name().into(), Value::from_input(&s.default_value())))
                    .collect(),
                plot_count: d.plots.len(),
            }
        })
        .collect()
}
pub fn edit_descriptor() -> CapabilityDescriptor {
    let mut d = crate::layout::transient_descriptor::<EditInput, EditResult>(
        EDIT_CAPABILITY_ID,
        "indicator",
        "Edit a native indicator",
        "Add, configure inputs or plot style, hide or remove native indicators on an exact tab and pane. Add creates a new instance. Slot IDs come from indicator.native.list. Input patches use declared names and typed values. Float values and plot widths use canonical decimal strings (for example \"2.5\"). Plot style replaces one plot override; null fields restore defaults, color is RGBA (alpha is opacity). Requires explicit cockpit.indicators consent, including changes to user-created indicators; removal and overwritten settings have no undo. Uses the same layout persistence and mirroring as UI edits. Close settings before control edits. Read committed settings with indicator.native.list.",
        "Exact pane and slot identities are validated. A required idempotency key replays the original answer within the connection; a different key permits another instance.",
    );
    d.effect = EffectId::new(INDICATORS_EFFECT_ID).expect("static effect ID is valid");
    d.required_permissions = [COCKPIT_PERMISSION_ID, COCKPIT_INDICATORS_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect();
    d.risk_flags =
        [RiskFlagId::new(INDICATORS_CHANGED_RISK_FLAG).expect("static risk flag is valid")].into();
    // No undo restores overwritten settings or removed user-owned entries.
    // As for recovery, destructive=true requires an unsupported revision guard.
    d.reversible = false;
    d.persistence = EffectPersistence::Durable;
    d.idempotency = IdempotencyPolicy::Required;
    d.expected_cost.max_response_bytes = Some(4096);
    d
}
pub fn list_descriptor() -> CapabilityDescriptor {
    read_descriptor::<ListInput, ListResult>(&ReadSpec {
        id: LIST_CAPABILITY_ID,
        module: "indicator",
        title: "List native indicators and committed settings",
        description: "Lists the native catalog and a bounded page (1..64) of attached native slots on an exact pane, including committed inputs, hidden state, plot overrides and worker readiness. Does not open settings or modify the chart.",
        permissions: &["observe", "observe.indicators"],
        pagination: None,
    })
}
pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::journal(
    EDIT_CAPABILITY_ID,
    IdempotencyPolicy::Required,
    EVENT,
    "payload.native_indicator.slot_id",
    "the event names the created or edited slot; indicator.native.list reads committed settings",
    &["native_control_edits_share_layout_persistence"],
)];
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as s;
    vec![
        (
            "indicator-native-edit-input-v1.schema.json",
            s::<EditInput>(),
        ),
        (
            "indicator-native-edit-result-v1.schema.json",
            s::<EditResult>(),
        ),
        (
            "indicator-native-list-input-v1.schema.json",
            s::<ListInput>(),
        ),
        (
            "indicator-native-list-result-v1.schema.json",
            s::<ListResult>(),
        ),
    ]
}

/// The shared owner hands over saved documents, never preview inputs.
pub fn capture(
    input: ListInput,
    saved: Vec<(u64, quantick_stores::indicator_state::SavedIndicator, bool)>,
) -> Result<ListResult, String> {
    use quantick_stores::indicator_state::{SavedInputExt, SavedKind};
    if input.limit == 0 || input.limit > MAX_PAGE {
        return Err("limit must be in 1..=64".into());
    }
    let entries: Vec<_> = saved
        .into_iter()
        .filter_map(|(slot, entry, loaded)| {
            let SavedKind::Native { id } = entry.kind else {
                return None;
            };
            let native = quantick_indicators::native::native(&id)?;
            Some(Entry {
                slot_id: WireU64::new(slot),
                native_id: id,
                hidden: entry.hidden,
                inputs: native
                    .default_inputs()
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        (
                            s.name().into(),
                            Value::from_input(
                                &entry
                                    .inputs
                                    .get(i)
                                    .and_then(|v| v.to_value())
                                    .unwrap_or_else(|| s.default_value()),
                            ),
                        )
                    })
                    .collect(),
                plot_styles: entry
                    .plot_styles
                    .iter()
                    .map(|s| PlotStyle {
                        visible: s.visible,
                        color: s.color,
                        width: s.width.map(|w| w as f32),
                    })
                    .collect(),
                loaded,
            })
        })
        .collect();
    let total = entries.len();
    let next = input.offset.saturating_add(input.limit);
    Ok(ListResult {
        target: input.target,
        catalog: catalog(),
        entries: entries
            .into_iter()
            .skip(input.offset)
            .take(input.limit)
            .collect(),
        total,
        next_offset: (next < total).then_some(next),
    })
}
/// A fully validated effect, dispatched through the same UI persistence path.
pub enum Prepared {
    Add(String),
    Inputs(Vec<InputValue>),
    Style(usize, PlotStyle),
    Hidden(bool),
    Remove,
}
pub fn prepare(
    operation: Operation,
    entry: Option<&Entry>,
    settings_open: bool,
) -> Result<Prepared, String> {
    if settings_open {
        return Err("close indicator settings before control edits".into());
    }
    if let Operation::Add { native_id } = operation {
        quantick_indicators::native::native(&native_id).ok_or("unknown native indicator")?;
        return Ok(Prepared::Add(native_id));
    }
    let entry = entry.ok_or("unknown native slot")?;
    Ok(match operation {
        Operation::SetInputs { values, .. } => {
            let saved = inputs(&entry.native_id, &[], &entry.inputs)?;
            Prepared::Inputs(inputs(&entry.native_id, &saved, &values)?)
        }
        Operation::SetPlotStyle { plot, style, .. } => {
            style.validate()?;
            let native = quantick_indicators::native::native(&entry.native_id)
                .ok_or("unknown native indicator")?;
            if plot >= native.build().descriptor().plots.len() {
                return Err("unknown plot index".into());
            }
            if !entry.loaded {
                return Err("wait for indicator to load".into());
            }
            Prepared::Style(plot, style)
        }
        Operation::SetHidden { hidden, .. } => {
            if !entry.loaded {
                return Err("wait for indicator to load".into());
            }
            Prepared::Hidden(hidden)
        }
        Operation::Remove { .. } => Prepared::Remove,
        Operation::Add { .. } => unreachable!("add was prepared above"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn volume_catalog_exposes_source_provenance_and_validates_average_inputs() {
        let entry = catalog()
            .into_iter()
            .find(|entry| entry.native_id == "native.volume")
            .unwrap();
        assert_eq!(
            entry.source_note.as_deref(),
            Some(quantick_indicators::native::VOLUME_SOURCE_NOTE)
        );
        let values = inputs(
            "native.volume",
            &[],
            &BTreeMap::from([
                ("show_average".into(), Value::Bool(true)),
                ("length".into(), Value::Int(3)),
            ]),
        )
        .unwrap();
        assert_eq!(values, vec![InputValue::Bool(true), InputValue::Int(3)]);
        for length in [0, 501] {
            assert!(
                inputs(
                    "native.volume",
                    &values,
                    &BTreeMap::from([("length".into(), Value::Int(length))])
                )
                .is_err()
            );
        }
    }
    #[test]
    fn native_input_patches_validate_every_field_before_dispatch() {
        let good = BTreeMap::from([
            ("len".into(), Value::Int(17)),
            ("source".into(), Value::Source("volume".into())),
        ]);
        let values = inputs("native.ema", &[], &good).unwrap();
        assert_eq!(
            values,
            vec![InputValue::Int(17), InputValue::Source(SourceId::Volume)]
        );
        for patch in [
            BTreeMap::from([("len".into(), Value::Int(0))]),
            BTreeMap::from([("len".into(), Value::Bool(true))]),
            BTreeMap::from([("source".into(), Value::Source("missing".into()))]),
            BTreeMap::from([("missing".into(), Value::Int(1))]),
        ] {
            assert!(inputs("native.ema", &values, &patch).is_err());
        }
        assert!(inputs("native.cvd", &[], &good).is_err());
        assert!(inputs("missing", &[], &BTreeMap::new()).is_err());
        for width in [f32::NAN, f32::INFINITY, 0.0, 8.1] {
            assert!(
                PlotStyle {
                    width: Some(width),
                    ..PlotStyle::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            PlotStyle {
                width: Some(0.5),
                ..PlotStyle::default()
            }
            .validate()
            .is_ok()
        );
    }
    #[test]
    fn new_native_contracts_are_valid_and_require_retry_keys() {
        assert_eq!(edit_descriptor().idempotency, IdempotencyPolicy::Required);
        assert!(list_descriptor().read_only);
        assert!(serde_json::from_value::<EditInput>(serde_json::json!({"target":{"tab_id":"0","pane_id":"0"},"operation":{"action":"remove","slot_id":"0","extra":true}})).is_err());
    }
}
