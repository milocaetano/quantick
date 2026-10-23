//! Bounded, retry-safe control of the indicator library dialog.
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_stores::{
    indicator_picker::{Category, Choice, Picker},
    script_library::ScriptEntry,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const PICKER_CAPABILITY_ID: &str = "indicator.picker.set";
pub const EVENT_KIND: &str = "indicator.picker.changed";
pub const MAX_QUERY_BYTES: usize = 256;
pub const MAX_PAGE: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PickerAction {
    Inspect,
    Open,
    Close,
    Search { query: String, category: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PickerInput {
    pub operation: PickerAction,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    MAX_PAGE
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PickerEntry {
    pub id: String,
    pub name: String,
    pub category: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PickerResult {
    pub open: bool,
    pub tab_id: Option<WireU64>,
    pub pane_id: Option<WireU64>,
    pub query: String,
    pub category: String,
    pub categories: Vec<String>,
    pub entries: Vec<PickerEntry>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

/// Validate the whole request before editing the shared modal state.
pub fn apply(
    state: &mut Picker,
    scripts: &[ScriptEntry],
    target: (u64, u64),
    input: PickerInput,
) -> Result<PickerResult, String> {
    if input.limit == 0 || input.limit > MAX_PAGE {
        return Err("limit must be in 1..=64".into());
    }
    let search = match &input.operation {
        PickerAction::Search { query, category } => {
            if query.len() > MAX_QUERY_BYTES {
                return Err("query exceeds 256 bytes".into());
            }
            Some((
                query.clone(),
                Category::parse(category).ok_or("unknown indicator category")?,
            ))
        }
        _ => None,
    };
    state.retain_target(target);
    match input.operation {
        PickerAction::Open => state.open(target),
        PickerAction::Close => state.close(),
        PickerAction::Search { .. } => {
            let (query, category) = search.expect("validated search");
            state.query = query;
            state.category = category;
        }
        PickerAction::Inspect => (),
    }
    let entries = state.entries(scripts);
    let total = entries.len();
    let next = input.offset.saturating_add(input.limit);
    Ok(PickerResult {
        open: state.target.is_some(),
        tab_id: state.target.map(|t| WireU64::new(t.0)),
        pane_id: state.target.map(|t| WireU64::new(t.1)),
        query: state.query.clone(),
        category: state.category.id().into(),
        categories: Category::ALL.iter().map(|c| c.id().into()).collect(),
        entries: entries
            .into_iter()
            .skip(input.offset)
            .take(input.limit)
            .map(|e| PickerEntry {
                id: match e.choice {
                    Choice::Native(id) => id.into(),
                    Choice::Script(index) => format!("script.{index}"),
                },
                name: e.name.chars().take(512).collect(),
                category: e.category.id().into(),
            })
            .collect(),
        total,
        next_offset: (next < total).then_some(next),
    })
}
pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<PickerInput, PickerResult>(
        PICKER_CAPABILITY_ID,
        "indicator",
        "Inspect and search the indicator picker",
        "Opens or closes the centered indicator picker on the focused pane, searches its built-in and script library by case-insensitive name, or inspects its current state. Search does not attach an indicator. Categories: all, native, scripts, active. Active slots and their errors are read through analysis.indicators. Results are paginated (1..64); script IDs are library indices for this process. Changing focus dismisses the sitting. All operations return the state and matching library entries.",
        "Open, close and search set explicit state and are safe to repeat. Inspect returns state without opening the dialog. No indicator is attached or removed.",
    );
    descriptor.required_permissions.insert(
        quantick_control::id::PermissionId::new("observe.user_text").expect("static permission"),
    );
    descriptor
}
pub const READBACKS: &[Readback] = &[journal(
    PICKER_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    EVENT_KIND,
    "payload.indicator_picker",
    "the event reports the picker state after the call",
    &[EVERY_OPTIONAL_TEST],
)];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_search_is_atomic_and_pages_are_bounded() {
        let mut state = Picker::default();
        state.open((9, 0));
        let before = state.clone();
        let request = |action, limit| PickerInput {
            operation: action,
            offset: 0,
            limit,
        };
        assert!(
            apply(
                &mut state,
                &[],
                (9, 0),
                request(
                    PickerAction::Search {
                        query: "a".repeat(257),
                        category: "native".into(),
                    },
                    64
                )
            )
            .is_err()
        );
        assert_eq!(state, before);
        assert!(apply(&mut state, &[], (9, 0), request(PickerAction::Close, 0)).is_err());
        assert_eq!(state, before);
        let page = apply(&mut state, &[], (9, 0), request(PickerAction::Inspect, 1)).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.next_offset, Some(1));
        let result = apply(
            &mut state,
            &[],
            (9, 0),
            request(
                PickerAction::Search {
                    query: "cVd".into(),
                    category: "native".into(),
                },
                64,
            ),
        )
        .unwrap();
        assert_eq!(result.entries[0].id, "native.cvd");
    }
}
