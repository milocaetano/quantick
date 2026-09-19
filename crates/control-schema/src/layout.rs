//! The cockpit tier's canvas capabilities: rearranging the trader's charts.
//!
//! Every one of these calls the same function the menu and the keyboard call.
//! That is the point rather than a tidiness preference: a capability with its
//! own copy of "apply a layout" would drift from the one a click takes, and
//! the drift would be an assistant and a trader disagreeing about what the
//! canvas is currently showing.
//!
//! They live under the `cockpit` effect, which the `annotator` profile does
//! **not** inherit. The annotate tier's consent text tells the trader that
//! nothing granted there can rearrange their window; a capability that arrived
//! under a grant whose own words deny it would be a trust bug with no surface
//! to find it on.

use crate::retry_matrix::Readback;
use quantick_control_host::authority::{
    CAPABILITY_VERSION, COCKPIT_EFFECT_ID, COCKPIT_LAYOUT_PERMISSION_ID, COCKPIT_PERMISSION_ID,
    NO_CONFIRMATION_ID, SNAPSHOT_CAPABILITY_ID, UI_BOUNDED_COST_ID,
};

use std::collections::BTreeSet;

use quantick_control::{
    id::{CapabilityId, CostClassId, ModuleId, PermissionId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
    wire::WireU64,
};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use serde_json::Value;

/// The module every layout capability belongs to.
pub use quantick_control_host::authority::LAYOUT_MODULE_ID;

pub const APPLY_PRESET_CAPABILITY_ID: &str = "layout.preset.apply";

pub const MOVE_PANE_CAPABILITY_ID: &str = "layout.pane.move";

pub const RESIZE_CAPABILITY_ID: &str = "layout.pane.resize";

pub const COLLAPSE_CAPABILITY_ID: &str = "layout.pane.collapse";

pub const EXPAND_CAPABILITY_ID: &str = "layout.pane.expand";

pub const FOCUS_CAPABILITY_ID: &str = "layout.focus.set";

pub const INTERVAL_CAPABILITY_ID: &str = "layout.pane.set_interval";

pub const BAR_SPEC_CAPABILITY_ID: &str = "layout.pane.set_bar_spec";

pub const TAB_SWITCH_CAPABILITY_ID: &str = "layout.tab.switch";

pub const TAB_CREATE_CAPABILITY_ID: &str = "layout.tab.create";

pub const TAB_RENAME_CAPABILITY_ID: &str = "layout.tab.rename";

/// Which tab a call is about. Omitted means the one the trader is looking at —
/// the same default the chrome's own commands take.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
pub struct TabTarget {
    /// The tab's id, as `observe.workspace` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// Apply a named arrangement from the layout registry.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ApplyPresetInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// A preset id from the registry — `describe` lists them.
    pub preset_id: String,
}

/// Move one context chart within the stack.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MovePaneInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address now. `0` is the flow pane and cannot be moved.
    pub from: WireU64,
    /// Where it should sit.
    pub to: WireU64,
}

/// Set the context column's share of the canvas.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ResizeInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The share, 0..1. Held inside the same floor a drag is held to, so a
    /// call cannot reach a width a hand could not.
    pub fraction: f64,
}

/// Focus one pane: the chart the chrome speaks for and commands land on.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FocusInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address. `0` is the flow pane.
    pub pane: WireU64,
}

/// Set one context chart's timeframe.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct IntervalInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address. `0` is the flow pane, which the toolbar governs.
    pub pane: WireU64,
    /// The interval in milliseconds.
    pub interval_ms: i64,
}

/// Set any pane's complete alternative-bar rule.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BarSpecInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address (`0` is the flow pane).
    pub pane: WireU64,
    /// The same stable spelling configuration and workspace files use, such
    /// as `tick:50`, `time:60000`, or `trades:2000`.
    pub spec: String,
}

/// Which layout tab a call is about: by id, by name, or — omitted — the
/// active one. An id and a name that disagree are refused rather than
/// resolved, because a caller that gave both meant both.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct LayoutTabTarget {
    /// The layout's id, as `observe.workspace` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_id: Option<WireU64>,
    /// The layout's name, as the strip shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Put a layout on one pane.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct SwitchLayoutTabInput {
    #[serde(flatten)]
    pub layout: LayoutTabTarget,
    /// Which tab's pane changes layout. Omitted: the active tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// The pane's address (`0` the flow pane, `1..` the context stack).
    /// Omitted: the focused pane — the pane the strip's own click switches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<WireU64>,
}

/// Add a layout tab and switch to it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct CreateLayoutTabInput {
    /// What to call it. Omitted: the first free `Layout N`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Rename a layout tab.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RenameLayoutTabInput {
    #[serde(flatten)]
    pub target: LayoutTabTarget,
    /// The new name.
    pub new_name: String,
}

/// One layout tab, as the strip lists it.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutTabSnapshot {
    pub layout_id: WireU64,
    pub name: String,
    pub active: bool,
    /// How many indicators the layout holds.
    pub indicator_count: WireU64,
}

/// What every layout-tab call answers with: the strip as it now stands.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LayoutTabResult {
    pub active_layout_id: WireU64,
    pub active_layout_name: String,
    pub layouts: Vec<LayoutTabSnapshot>,
    /// Whether the call changed anything. `false` is a real answer:
    /// switching to the layout already showing is a no-op, not a failure.
    pub changed: bool,
}

/// What every layout call answers with: the arrangement as it now stands.
///
/// Echoed back rather than assumed, so a client never has to guess whether a
/// call it made is the reason the canvas looks the way it does.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LayoutResult {
    /// The tab that changed.
    pub tab_id: WireU64,
    /// The preset the canvas now matches.
    pub preset_id: String,
    /// How many panes it draws.
    pub pane_count: WireU64,
    /// The focused pane's address.
    pub focused_pane: WireU64,
    /// The context column's share of the canvas.
    pub fraction: f64,
    /// Whether the context column is collapsed to its rail.
    pub collapsed: bool,
    /// Whether the call changed anything. `false` is a real answer: applying
    /// the layout that is already showing is a no-op, not a failure.
    pub changed: bool,
}

pub fn tab_descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
) -> CapabilityDescriptor {
    let mut descriptor = descriptor(id, title, description, input_schema);
    descriptor.output_schema = generated_schema::<LayoutTabResult>();
    descriptor.stale_input_safety = Some(
        "Switching, creating or renaming a layout removes no work: every layout keeps its indicators and drawings while another is showing. A stale caller can only show the wrong layout, which the result it gets back names."
            .to_owned(),
    );
    descriptor
}

pub fn layout_permissions() -> BTreeSet<PermissionId> {
    [COCKPIT_PERMISSION_ID, COCKPIT_LAYOUT_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect()
}

pub fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(LAYOUT_MODULE_ID).expect("static module ID is valid"),
        input_schema,
        output_schema: generated_schema::<LayoutResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(COCKPIT_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        // Applying the same arrangement twice leaves the same arrangement, so
        // a client may retry a dropped call without wondering what the first
        // one did. The gateway makes that exact: a repeat under the same key
        // replays the first answer rather than acting again, for as long as
        // the connection that made it lasts. A client that reconnects arrives
        // as a new principal and its keys start over --
        // `gateway/idempotency.rs` says why.
        idempotency: IdempotencyPolicy::Optional,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "Rearranging a canvas removes no work: a chart taken off the screen keeps its drawings, its indicators and its bars, and comes back with them. A stale caller can only show the wrong charts, which the result it gets back names."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: layout_permissions(),
        preconditions: Vec::new(),
        confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

pub const RESIZE_PAIR_CAPABILITY_ID: &str = "layout.pane.resize_pair";

/// The scene reports the same splitter's current bounds after an uncertain call.
pub const READBACK: Readback = Readback {
    capability: RESIZE_PAIR_CAPABILITY_ID,
    policy: IdempotencyPolicy::Optional,
    read: SNAPSHOT_CAPABILITY_ID,
    scope: Some(crate::scene::CONTROLS_SCOPE_ID),
    event: None,
    field: "controls[].bounds",
    applied_when: "the addressed context divider's bounds reflect the applied vertical position",
    proven_by: &["every_reachable_optional_row_replays_a_dropped_answer_and_begins_once"],
};
