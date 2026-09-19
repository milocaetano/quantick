//! Published annotation vocabulary and capability contracts, independent of a host.
//! Admission/execution stays with the host; schemas and policy descriptions have one owner.

use crate::{
    id::{CapabilityId, CostClassId, ModuleId, PermissionId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const ANNOTATE_EFFECT_ID: &str = "annotate";
const ANNOTATE_PERMISSION_ID: &str = "annotate";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaneSideDto {
    Flow,
    Time,
}

/// The module every annotation capability belongs to.
pub const ANNOTATE_MODULE_ID: &str = "annotate";
/// The scope that lets an operator add objects to the chart.
pub const ANNOTATE_CHART_PERMISSION_ID: &str = "annotate.chart";

pub const LABEL_CAPABILITY_ID: &str = "annotate.label.create";
pub const ARROW_CAPABILITY_ID: &str = "annotate.arrow.create";
pub const ZONE_CAPABILITY_ID: &str = "annotate.zone.create";
pub const PROFILE_CAPABILITY_ID: &str = "annotate.fixed_range_profile.create";
pub const PROFILE_CAPABILITY_VERSION: u32 = 2;
pub const FIB_RETRACEMENT_CAPABILITY_ID: &str = "annotate.fib_retracement.create";
pub const FIB_PROJECTION_CAPABILITY_ID: &str = "annotate.fib_projection.create";
pub const FIB_CAPABILITY_VERSION: u32 = CAPABILITY_VERSION;
pub const REMOVE_CAPABILITY_ID: &str = "annotate.remove";

pub const ANNOTATION_CREATED_EVENT_KIND: &str = "annotate.object.created";
pub const ANNOTATION_REMOVED_EVENT_KIND: &str = "annotate.object.removed";

pub const CAPABILITY_VERSION: u32 = 1;
pub const NO_CONFIRMATION_ID: &str = "none";
pub const UI_BOUNDED_COST_ID: &str = "ui_bounded";

/// The registry ids of the drawing tools an annotation reaches for. They are
/// looked up by id in `DRAWING_TOOLS`, exactly as the rail does, so a rename
/// in the tool registry is a compile-time-visible lookup failure here rather
/// than a second list of tools.
pub const LABEL_TOOL_ID: &str = "text";
pub const ARROW_TOOL_ID: &str = "arrow";
pub const ZONE_TOOL_ID: &str = "rectangle";

/// The longest label an annotation may carry. A note is a sentence on a
/// chart, not a document; the bound is what keeps one call from covering the
/// tape.
pub const ANNOTATION_TEXT_MAX_BYTES: usize = 280;
/// The longest trader-facing name an annotation may carry, matching what the
/// object manager's rename accepts.
pub const ANNOTATION_NAME_MAX_BYTES: usize = 120;
/// Decimal places an anchor price is reported back with. Prices on the wire
/// are exact text (types.rs); eight places is past every venue's tick.
pub const ANNOTATION_PRICE_DECIMALS: u32 = 8;

/// Where an annotation goes. Omitting both halves means the chart the trader
/// is looking at — the same default the toolbar's own placement uses.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationTarget {
    /// The tab, by the id every snapshot reports. Omitted: the active tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// The pane within that tab. Omitted: the pane drawings go to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_side: Option<PaneSideDto>,
    /// Which context chart `pane_side = "time"` means, top to bottom from
    /// `0`. Omitted: the top one. Ignored for the flow pane, which has no
    /// stack to pick from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_slot: Option<WireU64>,
}

/// One anchor, in the coordinates the cursor and the chart window report:
/// market time and price. Screen pixels are deliberately not accepted — they
/// mean nothing a frame later, and an agent that read a bar knows its time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationAnchor {
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub time_unix_ms: i64,
    pub price: CanonicalDecimal,
}

/// What a chart annotation takes. The anchor count is the tool's (one for a
/// label, two for the ranged tools) and is checked against the registry rather
/// than restated here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<AnnotationTarget>,
    #[schemars(length(min = 1, max = 2))]
    pub anchors: Vec<AnnotationAnchor>,
    /// The words a label carries. Ignored by the tools that have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = ANNOTATION_TEXT_MAX_BYTES))]
    pub text: Option<String>,
    /// The name the object manager and the inspector show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = ANNOTATION_NAME_MAX_BYTES))]
    pub name: Option<String>,
}

/// One coordinate from a chart gesture. Market time is retained when a bar
/// exists; `bar_position` keeps future chart space expressible without
/// inventing a timestamp.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartAnnotationAnchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub time_unix_ms: Option<i64>,
    pub bar_position: CanonicalDecimal,
    pub price: CanonicalDecimal,
}

/// Ranged drawings share this future-aware input; the selected tool still
/// owns whether exactly two or three anchors complete it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartAnnotationInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<AnnotationTarget>,
    #[schemars(length(min = 2, max = 3))]
    pub anchors: Vec<ChartAnnotationAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = ANNOTATION_NAME_MAX_BYTES))]
    pub name: Option<String>,
    /// Optional exact series identity. Absent: the original timestamp/future
    /// behavior. Present: bar positions and supplied times must agree with
    /// this pane's current series and active layout before anything is placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_reference: Option<ChartReference>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartReference {
    pub pane_id: WireU64,
    pub series_revision: WireU64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_id: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ChartResolvedAnchor {
    Market(ResolvedAnchor),
    Future {
        bar_position: CanonicalDecimal,
        price: CanonicalDecimal,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChartAnnotationResult {
    pub annotation_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub tool_id: String,
    pub anchors: Vec<ChartResolvedAnchor>,
    pub author: AnnotationAuthor,
    pub label: String,
}

/// What an annotation returns: the object's stable id, where it actually
/// landed, and the authorship the trader sees on it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnnotationResult {
    pub annotation_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub tool_id: String,
    /// Where each anchor landed after resolution: the slot it fell on and the
    /// market time of that slot, which is not always the time asked for.
    pub anchors: Vec<ResolvedAnchor>,
    pub author: AnnotationAuthor,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedAnchor {
    pub slot: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub time_unix_ms: i64,
    pub price: CanonicalDecimal,
}

/// Who placed an object, as the interface shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnnotationAuthor {
    /// The actor kind on the wire: `agent`, `automation`, `human_ui`.
    pub actor_kind: String,
    /// The client's own name from its handshake.
    pub client_name: String,
}

/// What a removal takes: the annotation's id, and nothing else. There is no
/// "remove all" here on purpose — an operator that can sweep the chart is a
/// cockpit capability, and the trader's own sweep lives in the object manager.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveInput {
    pub annotation_id: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemoveResult {
    pub annotation_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub removed: bool,
}

fn annotate_permissions(scope: &str) -> BTreeSet<PermissionId> {
    [ANNOTATE_PERMISSION_ID, scope]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect()
}

pub fn annotation_descriptor(id: &str, title: &str, description: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<AnnotationInput>(),
        output_schema: generated_schema::<AnnotationResult>(),
        examples: Vec::new(),
        effect: crate::id::EffectId::new(ANNOTATE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        // An annotation adds an object and overwrites none, so a stale caller
        // can only place the wrong thing — visible, attributed, and removed
        // in one action.
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "An annotation adds one object and edits nothing; a stale caller places an object that is visibly attributed and removed in one action."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: annotate_permissions(ANNOTATE_CHART_PERMISSION_ID),
        preconditions: Vec::new(),
        confirmation_class: crate::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(crate::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

pub fn fib_descriptor(id: &str, title: &str, description: &str) -> CapabilityDescriptor {
    chart_descriptor(id, FIB_CAPABILITY_VERSION, title, description)
}

pub fn chart_descriptor(
    id: &str,
    version: u32,
    title: &str,
    description: &str,
) -> CapabilityDescriptor {
    let mut descriptor = annotation_descriptor(id, title, description);
    descriptor.version = version;
    descriptor.input_schema = generated_schema::<ChartAnnotationInput>();
    descriptor.output_schema = generated_schema::<ChartAnnotationResult>();
    descriptor
}

pub fn remove_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(REMOVE_CAPABILITY_ID).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: "Remove an annotation".to_owned(),
        description: "Removes one object placed by an operator other than the trader. An object the trader drew by hand is never removable through this tier.".to_owned(),
        module: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<RemoveInput>(),
        output_schema: generated_schema::<RemoveResult>(),
        examples: Vec::new(),
        effect: crate::id::EffectId::new(ANNOTATE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "Removing an already-removed annotation reports that it was not there; no other object can be reached."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: annotate_permissions(ANNOTATE_CHART_PERMISSION_ID),
        preconditions: Vec::new(),
        confirmation_class: crate::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(crate::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}
use crate::error::ControlError;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};

pub fn parse_price(price: &CanonicalDecimal) -> Result<f64, ControlError> {
    price
        .as_str()
        .parse::<rust_decimal::Decimal>()
        .ok()
        .and_then(|value| value.to_f64())
        .filter(|value| value.is_finite() && rust_decimal::Decimal::from_f64(*value).is_some())
        .ok_or_else(|| ControlError::invalid_request("an anchor price is not a finite decimal"))
}

impl ChartAnnotationAnchor {
    /// Legacy timestamp requests did not consume the fallback position.
    /// Exact references and future-only anchors do consume and validate it.
    pub fn validation_bar(&self, exact: bool) -> Result<Option<f32>, ControlError> {
        if exact || self.time_unix_ms.is_none() {
            parse_bar_position(&self.bar_position).map(Some)
        } else {
            Ok(None)
        }
    }
}

pub fn parse_bar_position(position: &CanonicalDecimal) -> Result<f32, ControlError> {
    position
        .as_str()
        .parse::<rust_decimal::Decimal>()
        .ok()
        .and_then(|value| value.to_f32())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| {
            ControlError::invalid_request("an anchor bar position is not a non-negative number")
        })
}

/// Preserve chart coordinates through the wire instead of rounding across a
/// slot boundary. Refuse a value the decimal transport cannot round-trip.
pub fn canonical_bar_position(value: f32) -> Option<CanonicalDecimal> {
    let decimal = CanonicalDecimal::new(if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    })
    .ok()?;
    parse_bar_position(&decimal)
        .is_ok_and(|restored| restored == value)
        .then_some(decimal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_positions_round_trip_without_quantizing_slot_boundaries() {
        for value in [0.0, -0.0, 0.5004, 0.50000006, 7.5004, 100_000.51] {
            let encoded = canonical_bar_position(value).unwrap();
            assert_eq!(parse_bar_position(&encoded).unwrap(), value);
        }
        for value in [f32::NAN, f32::INFINITY, -1.0, f32::MAX, f32::MIN_POSITIVE] {
            assert!(canonical_bar_position(value).is_none(), "{value}");
        }
    }

    #[test]
    fn annotation_decimal_rules_preserve_finite_prices_and_nonnegative_positions() {
        let decimal = |value: &str| CanonicalDecimal::new(value).unwrap();
        assert_eq!(parse_price(&decimal("-12.25")).unwrap(), -12.25);
        assert_eq!(parse_bar_position(&decimal("12.5")).unwrap(), 12.5);
        assert!(parse_bar_position(&decimal("-1")).is_err());
        assert!(parse_price(&decimal("99999999999999999999999999999999999999")).is_err());
        assert!(
            parse_price(&decimal("79228162514264337593543950335")).is_err(),
            "the f64 round-trip must remain a representable wire decimal"
        );
    }

    #[test]
    fn legacy_input_stays_timestamp_only_and_exact_reference_is_optional() {
        let legacy = serde_json::json!({"anchors": [{"time_unix_ms": 1000, "price": "12"}]});
        assert!(serde_json::from_value::<AnnotationInput>(legacy).is_ok());
        let projected = serde_json::json!({"anchors": [{"bar_position": "12.5", "price": "12"}]});
        let input: ChartAnnotationInput = serde_json::from_value(projected).unwrap();
        assert!(input.chart_reference.is_none());
        assert!(input.anchors[0].time_unix_ms.is_none());
        assert!(
            serde_json::to_value(input)
                .unwrap()
                .get("chart_reference")
                .is_none()
        );
    }
}
