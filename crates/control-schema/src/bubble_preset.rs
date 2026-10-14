//! Named bubble presets, shared with the Bubbles settings picker.
use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_control_host::authority::{ReadSpec, read_descriptor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const BUBBLE_PRESET_CAPABILITY_ID: &str = "orderflow.bubbles.preset.apply";
pub const BUBBLE_PRESETS_LIST_CAPABILITY_ID: &str = "orderflow.bubbles.presets.list";
pub const MAX_PRESETS: usize = quantick_stores::bubble_look::PRESET_PAGE_MAX;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Exact name returned by orderflow.bubbles.presets.list.
    pub preset: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApplyResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub changed: bool,
    pub preset: String,
    pub native_tape_requested: bool,
    pub native_tape: bool,
    pub stable_size: bool,
    pub depth_time_averaging: bool,
    pub tick_bubbles: Option<crate::tick_bubbles::TickBubblesSnapshot>,
    pub asset: Option<crate::orderflow::BubbleAssetSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub offset: WireU64,
    #[schemars(range(min = 1, max = MAX_PRESETS))]
    pub limit: usize,
    /// Required on continuation pages; restart at offset zero after a reload.
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub preset: String,
    #[schemars(length(max = MAX_PRESETS))]
    pub names: Vec<String>,
    pub total: WireU64,
    pub revision: String,
    pub next_offset: Option<WireU64>,
}

pub fn page(
    input: &ListInput,
    look: &quantick_stores::bubble_look::BubbleLook,
) -> Result<ListResult, String> {
    let offset = usize::try_from(input.offset.get()).map_err(|_| "Preset offset is too large")?;
    let revision = quantick_control::canonical::canonical_digest(&serde_json::json!(
        look.available_presets()
            .map(|preset| &preset.name)
            .collect::<Vec<_>>()
    ))
    .map_err(|error| error.to_string())?;
    if (offset != 0 && input.revision.is_none())
        || input
            .revision
            .as_ref()
            .is_some_and(|value| value != &revision)
    {
        return Err(
            "Preset catalog changed or continuation revision is missing; restart at offset zero"
                .to_owned(),
        );
    }
    let names = look.preset_page(offset, input.limit)?;
    let total = look.available_presets().count();
    let next = offset + names.len();
    Ok(ListResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        preset: look.look_name().to_owned(),
        names,
        revision,
        total: WireU64::new(total as u64),
        next_offset: (next < total).then(|| WireU64::new(next as u64)),
    })
}

pub fn applied(
    input: &ApplyInput,
    changed: bool,
    snapshot: crate::orderflow::BubblesStateSnapshot,
    config: &quantick_orderflow::HeatmapConfig,
) -> ApplyResult {
    ApplyResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        changed,
        preset: snapshot.preset,
        native_tape_requested: config.live_lane.native_tape,
        native_tape: snapshot.native_tape,
        stable_size: snapshot.stable_size,
        depth_time_averaging: snapshot.depth_time_averaging,
        tick_bubbles: snapshot.tick_bubbles,
        asset: snapshot.asset,
    }
}

pub fn snapshot(
    config: &quantick_orderflow::HeatmapConfig,
    floored: rust_decimal::Decimal,
    scale: Option<&quantick_orderflow::DotScale>,
    bursts: &[i64],
    look: &quantick_stores::bubble_look::BubbleLook,
) -> crate::orderflow::BubblesStateSnapshot {
    let mut snapshot = crate::orderflow::BubblesStateSnapshot::from_config(
        config,
        floored,
        scale,
        bursts,
        look.asset()
            .map(|binding| crate::orderflow::BubbleAssetSnapshot::of(binding, look.look_name())),
        look.tick_presets(),
    );
    snapshot.preset = look.look_name().to_owned();
    snapshot
}

pub fn apply_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<ApplyInput, ApplyResult>(
        BUBBLE_PRESET_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Apply bubble preset",
        "Applies or reapplies an exact named preset through the Bubbles picker owner. Discover names with orderflow.bubbles.presets.list. Whole looks apply their tape navigation and appearance; the B3 WIN tick spheres preset changes only the WIN tick-chart calibration and preserves the existing tape and look name. Layer visibility is preserved. Changes follow the asset save switch, including session-only changes when off. Read selection and effective policies in orderflow.bubbles; tick-only selection remains independent.",
        "Stable tab/pane IDs and exact preset names are checked before mutation. Same-name drift is corrected; identical selection and configuration are a no-op. Keyed retries replay the first answer.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor
}

pub fn list_descriptor() -> CapabilityDescriptor {
    read_descriptor::<ListInput, ListResult>(&ReadSpec {
        id: BUBBLE_PRESETS_LIST_CAPABILITY_ID,
        module: crate::orderflow::MODULE_ID,
        title: "List whole bubble presets",
        description: "Lists 1..64 exact whole-look names in picker order for a pane, with selected name, total and next_offset. Offset starts at zero. Reloading presets may change the order; restart pagination after a reload. This read does not apply or save anything.",
        permissions: &["observe", "observe.orderflow"],
        pagination: Some((
            quantick_control::cursor::PaginationConsistency::RevisionLocked,
            MAX_PRESETS,
        )),
    })
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    BUBBLE_PRESET_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.preset",
    "the addressed pane reports the selected whole look and effective tape policy after application",
    &[
        "whole_presets_reapply_drift_and_are_authorized_retry_safe",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];

/// Published schemas for the bubble configuration family, generated canonically.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "orderflow-bubble-preset-input-v1.schema.json",
            schema::<crate::bubble_preset::ApplyInput>(),
        ),
        (
            "orderflow-bubble-preset-result-v1.schema.json",
            schema::<crate::bubble_preset::ApplyResult>(),
        ),
        (
            "orderflow-bubble-presets-list-input-v1.schema.json",
            schema::<crate::bubble_preset::ListInput>(),
        ),
        (
            "orderflow-bubble-presets-list-result-v1.schema.json",
            schema::<crate::bubble_preset::ListResult>(),
        ),
        (
            "orderflow-tick-bubbles-input-v1.schema.json",
            schema::<crate::tick_bubbles::TickBubblesInput>(),
        ),
        (
            "orderflow-tick-bubbles-result-v1.schema.json",
            schema::<crate::tick_bubbles::TickBubblesResult>(),
        ),
        (
            "orderflow-bubbles-save-changes-input-v1.schema.json",
            schema::<crate::bubble_save::SaveChangesInput>(),
        ),
        (
            "orderflow-bubbles-save-changes-result-v1.schema.json",
            schema::<crate::bubble_save::SaveChangesResult>(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_stores::{
        bubble_look::BubbleLook,
        bubble_presets::{PresetSource, embedded},
    };

    #[test]
    fn whole_preset_pages_cover_custom_names_and_refuse_stale_or_invalid_continuations() {
        let mut presets = embedded();
        let template = presets.presets[0].clone();
        for index in 0..150 {
            let mut preset = template.clone();
            preset.name = format!("custom {index:03}");
            presets.presets.push(preset);
        }
        let expected: Vec<_> = presets
            .presets
            .iter()
            .filter(|preset| !preset.tick_only)
            .map(|preset| preset.name.clone())
            .collect();
        let mut look = BubbleLook::open(
            (presets, PresetSource::Embedded, None),
            &mut quantick_orderflow::HeatmapConfig::default(),
        );
        let mut input = ListInput {
            tab_id: WireU64::new(1),
            pane_id: WireU64::new(2),
            offset: WireU64::new(0),
            limit: 64,
            revision: None,
        };
        let mut names = Vec::new();
        loop {
            let result = page(&input, &look).unwrap();
            assert!(result.names.len() <= 64);
            assert_eq!(result.total, WireU64::new(expected.len() as u64));
            names.extend(result.names);
            let Some(next) = result.next_offset else {
                break;
            };
            input.offset = next;
            input.revision = Some(result.revision);
        }
        assert_eq!(names, expected);
        assert!(
            !names.iter().any(|name| name == "B3 WIN tick spheres"),
            "unbound panes cannot apply chart-only asset calibrations"
        );
        input.offset = WireU64::new(1);
        input.revision = None;
        assert!(page(&input, &look).is_err());
        input.offset = WireU64::new(u64::MAX);
        assert!(page(&input, &look).is_err());
        input.offset = WireU64::new(0);
        input.limit = 0;
        assert!(page(&input, &look).is_err());
        input.limit = 65;
        assert!(page(&input, &look).is_err());
        input.limit = 1;
        let result = page(&input, &look).unwrap();
        input.offset = result.next_offset.unwrap();
        input.revision = Some(result.revision);
        look.reload(
            (embedded(), PresetSource::Embedded, None),
            &mut quantick_orderflow::HeatmapConfig::default(),
        );
        assert!(page(&input, &look).is_err());
    }
}
