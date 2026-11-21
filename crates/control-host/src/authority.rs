//! The observer contract's authority, as data: the profiles, the
//! permissions and their ceilings, the modules, the effect policies and the
//! read capabilities every Quantick instance publishes.
//!
//! The application composes a contract from these tables and binds its own
//! handlers to the reads; nothing here knows what a handler does. Keeping the
//! authority as data is what lets a generator, a test or a second host build
//! the same contract the gateway serves without the window.

use std::collections::BTreeSet;

use quantick_control::{
    handshake::ProtocolLimits,
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, InstanceId, ModuleId,
        PermissionId, ProfileId, RiskFlagId, SnapshotScopeId,
    },
    limits::{
        CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS, CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE,
        CONTROL_MAX_SNAPSHOT_SCOPES,
    },
    registry::{
        Availability, CapabilityDescriptor, DefaultGrant, EffectConstraints, EffectPersistence,
        EffectPolicy, ExpectedCost, IdempotencyPolicy, McpHintFloor, ModuleDescriptor,
        PermissionDescriptor, ProfileDescriptor, RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalogue::SnapshotScopeDescriptor;
use crate::contract::ContractBuilder;

mod families;

pub const OBSERVER_PROFILE_ID: &str = "observer";
/// The tier that reads what the observer may not.
///
/// The observer floor is the reading every assistant starts with: bars,
/// layout, health, the tape. The private reads — the paper account, the
/// trader's own words, redacted logs, an evidence bundle, a picture of the
/// window — are a different decision, and they used to be ticked at the same
/// ceiling as the framing of a chart. `analyst` is where they live now: still
/// read-only, still one tick each, but asked for by a client that says which
/// tier it wants, and named in the connected-clients panel as the authority
/// the connection actually holds.
///
/// It sits below every tier that writes, so an assistant that analyses deeply
/// is not thereby an assistant that can put something on the chart.
pub const ANALYST_PROFILE_ID: &str = "analyst";
/// The tier that may rearrange the trader's window.
///
/// Its own profile rather than a permission inside `annotator`, because the
/// annotate tier's consent text makes a promise it would otherwise break: it
/// tells the trader that nothing they grant there can change their layout.
/// A capability that arrives under a grant whose own words deny it is a trust
/// bug, and the trader has no way to find it.
pub const COCKPIT_PROFILE_ID: &str = "cockpit";
/// Rearranging the window: which charts are shown, where, and how wide.
pub const COCKPIT_PERMISSION_ID: &str = "cockpit";
/// The permission for the canvas layout specifically.
pub const COCKPIT_LAYOUT_PERMISSION_ID: &str = "cockpit.layout";
/// The effect every cockpit capability declares.
pub const COCKPIT_EFFECT_ID: &str = "cockpit";
/// Explicit consent to add, change or remove saved native indicators.
pub const COCKPIT_INDICATORS_PERMISSION_ID: &str = "cockpit.indicators";
/// Native indicator changes, including removal and layout mirrors.
pub const INDICATORS_EFFECT_ID: &str = "cockpit.indicators";
/// Saved indicator settings may be overwritten or removed without undo.
pub const INDICATORS_CHANGED_RISK_FLAG: &str = "saved_indicators_changed";
/// Permission for the one cockpit act that can remove the trader's work.
///
/// Separate from `cockpit.layout` on purpose, and marked sensitive: a grant
/// that lets an assistant rearrange panes must not silently also let it close
/// an open position. The layout tier's own doc comment names that class of
/// trust bug; this is the same rule applied to the tier that destroys.
pub const COCKPIT_RECOVER_PERMISSION_ID: &str = "cockpit.recover";
/// The effect for recovering a feed by rebuilding what it fed.
pub const RECOVER_EFFECT_ID: &str = "cockpit.recover";
/// What a capability under [`RECOVER_EFFECT_ID`] declares it may cost: the
/// chart's timeline, and with it the paper position and every armed strategy.
pub const TIMELINE_REBUILT_RISK_FLAG: &str = "timeline_rebuilt";
/// Permission to move a saved trading session out of the trades journal.
///
/// The second cockpit act that removes the trader's work, so it gets what
/// [`COCKPIT_RECOVER_PERMISSION_ID`] got and for the same reason: its own
/// scope, marked sensitive and off until ticked, under an effect of its own.
/// Neither `cockpit` — "never removes" — nor `cockpit.recover` — whose words
/// are a rebuilt chart — is consent the trader gave to losing a session.
pub const COCKPIT_JOURNAL_DELETE_PERMISSION_ID: &str = "cockpit.journal.delete";
/// Permission for the calls that replace or remove the saved desk. Separate from
/// `cockpit.layout` and sensitive: rearranging charts on screen must not silently
/// also let a connection overwrite what the trader comes back to.
pub const COCKPIT_DESK_PERMISSION_ID: &str = "cockpit.desk";
/// The effect for deleting a journal session.
pub const JOURNAL_DELETE_EFFECT_ID: &str = "cockpit.journal.delete";
/// What a capability under [`JOURNAL_DELETE_EFFECT_ID`] declares it may
/// cost: a session the ledger, the report and every listing stop reading.
pub const SESSION_REMOVED_RISK_FLAG: &str = "journal_session_removed";
/// Separate consent to permanently remove alert definitions or alert history.
pub const COCKPIT_ALERT_DELETE_PERMISSION_ID: &str = "cockpit.alert.delete";
/// Irreversible alert removal, distinct from reversible cockpit changes.
pub const ALERT_DELETE_EFFECT_ID: &str = "cockpit.alert.delete";
/// Alert definitions or recorded alert events are permanently removed.
pub const ALERT_REMOVED_RISK_FLAG: &str = "alert_removed";
/// The ceiling the `trade.*` family sits under.
///
/// It exists because the registry requires every permission to name one, and
/// because naming it is better than the alternatives: a trade cannot borrow
/// the annotate tier (whose own description promises it never affects a
/// position), and a permission with no ceiling at all is not representable.
///
/// **Nothing hands this profile out.** The access panel does not offer it,
/// `default_grant` is `Denied`, and the handshake can only reach a profile
/// the trader has granted — so today the only caller that gets past the
/// gateway to a `trade.*` capability is the in-process operator: a hotkey,
/// a harness hook, a deterministic test. Deciding that some connection may
/// trade is a decision about a real account, and it is not this change's to
/// make. The carve-out is here so that decision has somewhere to land.
pub const TRADER_PROFILE_ID: &str = "trader";
pub const DESCRIBE_CAPABILITY_ID: &str = "control.describe";
pub const SNAPSHOT_CAPABILITY_ID: &str = "snapshot.read";
pub const CHART_WINDOW_CAPABILITY_ID: &str = "chart.window.read";
pub const DIAGNOSTICS_CAPABILITY_ID: &str = "health.diagnostics.read";
pub const SCENE_CAPABILITY_ID: &str = "scene.read";

pub const OBSERVE_PERMISSION_ID: &str = "observe";
pub const OBSERVE_EFFECT_ID: &str = "observe";
pub const NO_CONFIRMATION_ID: &str = "none";
/// The cost class every bounded application-thread capability declares.
pub const UI_BOUNDED_COST_ID: &str = "ui_bounded";
/// The version every capability this contract publishes today carries.
pub const CAPABILITY_VERSION: u32 = 1;

/// The annotate tier: reversible objects and bounded interruptions.
pub const ANNOTATOR_PROFILE_ID: &str = "annotator";
pub const ANNOTATE_PERMISSION_ID: &str = "annotate";
pub const ANNOTATE_ATTENTION_PERMISSION_ID: &str = "annotate.attention";
pub const ANNOTATE_EFFECT_ID: &str = "annotate";
pub const ATTENTION_MODULE_ID: &str = "attention";
pub use quantick_control::annotation::{ANNOTATE_CHART_PERMISSION_ID, ANNOTATE_MODULE_ID};

/// The trade tier: a real account one day, simulated fills today.
pub const TRADE_MODULE_ID: &str = "trade";
pub const TRADE_EFFECT_ID: &str = "trade";
pub const TRADE_PERMISSION_ID: &str = "trade";

/// Strategies armed on a chart: Pine scripts that ring alarm-only.
pub const STRATEGY_MODULE_ID: &str = "strategy";

/// Notifications: a popup, a toast, a sound.
pub const NOTIFY_MODULE_ID: &str = "notify";
/// Permission to raise a popup or a toast the trader has to read.
pub const NOTIFY_PERMISSION_ID: &str = "annotate.notification";
/// Permission to make a sound, which reaches the trader anywhere.
pub const NOTIFY_SOUND_PERMISSION_ID: &str = "annotate.sound";
pub const NOTIFY_EFFECT_ID: &str = "notify";
/// Every notification declares that it interrupts.
pub const USER_INTERRUPT_RISK_FLAG: &str = "user_interrupt";

/// Indicator scripts: code an operator puts on the chart.
pub const SCRIPT_MODULE_ID: &str = "indicator";
pub const SCRIPT_PERMISSION_ID: &str = "annotate.script";

/// The canvas: which charts are on screen, where and how wide.
pub const LAYOUT_MODULE_ID: &str = "layout";
/// The workspace canvas: its layout catalogue, per-chart menu, add button and
/// floating charts.
pub const CANVAS_MODULE_ID: &str = "canvas";

/// Names that once meant something and now split, with what replaced each, and
/// which of those are named ahead of being built (none today).
/// A caller holding an old name is told where to go rather than that nothing
/// by that name ever existed.
pub const RETIRED_CAPABILITIES: &[(&str, &[&str], &[&str])] =
    &[("workspace.save", &["desk.save", "workspace.save_to"], &[])];

/// The refusal for a retired name, naming its successors in the message and in
/// `details.successors`; `None` for any other name.
#[must_use]
pub fn retired_capability(id: &str) -> Option<quantick_control::error::ControlError> {
    use quantick_control::{error::ControlError, error::codes, id::ErrorCode};
    let (_, successors, planned) = RETIRED_CAPABILITIES
        .iter()
        .find(|(name, _, _)| *name == id)?;
    let not_built = if planned.is_empty() {
        String::new()
    } else {
        format!(", of which {} is not built yet", planned.join(", "))
    };
    let mut refusal = ControlError::new(
        ErrorCode::new(codes::CAPABILITY_RETIRED).expect("static error code is valid"),
        format!(
            "{id} is retired; its successors are {}{not_built}",
            successors.join(", ")
        ),
        false,
    );
    refusal.context.details =
        Some(serde_json::json!({ "successors": successors, "planned": planned }));
    Some(refusal)
}

/// The Home tab: what the central canvas shows instead of a market.
pub const HOME_MODULE_ID: &str = "home";
pub const SETTINGS_MODULE_ID: &str = "settings";
pub const DESK_MODULE_ID: &str = "desk";
/// Calls on one open tab.
pub const TAB_MODULE_ID: &str = "tab";
/// Opening a market, and the palette that finds one.
pub const MARKET_MODULE_ID: &str = "market";
/// The venues' lists of markets.
pub const CATALOG_MODULE_ID: &str = "catalog";
/// The widget panes' floating windows: Depth, Big Trades and the rest.
pub const WIDGET_MODULE_ID: &str = "widget";

/// Evidence bundles: one hashed, redacted capture of a running session.
pub const EVIDENCE_MODULE_ID: &str = "evidence";
pub const EVIDENCE_PERMISSION_ID: &str = "observe.evidence";
pub const EVIDENCE_CAPTURE_CAPABILITY_ID: &str = "evidence.capture";
pub const EVIDENCE_READ_CAPABILITY_ID: &str = "evidence.read";

pub use crate::events::{EVENTS_MODULE_ID, EVENTS_PERMISSION_ID};

/// The profiles a grant can hand out, lowest ceiling first. `trader` is
/// registered and never handed out: nothing constructs it as a ceiling.
pub const GRANTABLE_PROFILE_IDS: [&str; 4] = [
    OBSERVER_PROFILE_ID,
    ANALYST_PROFILE_ID,
    ANNOTATOR_PROFILE_ID,
    COCKPIT_PROFILE_ID,
];

pub use families::OBSERVER_SCOPE_IDS;

/// Whether a permission is one of the private reads — the analyst tier.
///
/// Answered from the published scope table rather than from a prefix, because
/// the tier *is* "the reads marked sensitive": every one of them starts with
/// `observe.` exactly like the ordinary reads do, and a scope added tomorrow
/// joins the tier the moment the table marks it, with no second list to
/// remember. It lives here, beside the table, so the window and a headless
/// host answer the question the same way.
#[must_use]
pub fn is_private_read(permission: &PermissionId) -> bool {
    OBSERVER_SCOPE_IDS
        .iter()
        .any(|(id, _, sensitive)| *sensitive && *id == permission.as_str())
}

/// Whether a permission belongs to the trade tier — see the access
/// panel's read-scope filter for why it is excluded from every section.
#[must_use]
pub fn is_trade_permission(permission: &PermissionId) -> bool {
    permission.as_str() == TRADE_PERMISSION_ID || permission.as_str().starts_with("trade.")
}

/// Whether a permission belongs to the annotate tier — the `annotate` floor
/// itself or one of its scopes.
#[must_use]
pub fn is_annotate_permission(permission: &PermissionId) -> bool {
    permission.as_str() == ANNOTATE_PERMISSION_ID || is_annotate_scope(permission)
}

/// Any permission of the cockpit tier — the floor or one of its scopes.
#[must_use]
pub fn is_cockpit_permission(permission: &PermissionId) -> bool {
    permission.as_str() == COCKPIT_PERMISSION_ID || is_cockpit_scope(permission)
}

/// A scope *of* the cockpit tier, as opposed to the floor they stand on.
#[must_use]
pub fn is_cockpit_scope(permission: &PermissionId) -> bool {
    permission.as_str().starts_with(concat!("cockpit", "."))
}

/// A scope *of* the annotate tier — the ones that actually open a capability,
/// as opposed to the `annotate` floor they all stand on.
#[must_use]
pub fn is_annotate_scope(permission: &PermissionId) -> bool {
    permission.as_str().starts_with(concat!("annotate", "."))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotReadInput {
    /// Live native market window, pinned once when this read reaches the UI thread.
    /// Omission selects the focused live window at that admission point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<quantick_control::wire::WireU64>,

    #[schemars(length(min = 1, max = CONTROL_MAX_SNAPSHOT_SCOPES))]
    pub scopes: Vec<SnapshotScopeId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DescribeResult {
    pub instance_id: InstanceId,
    pub application_version: String,
    pub application_commit: String,
    pub protocol_version: u32,
    pub effective_profile: ProfileId,
    pub effective_scopes: BTreeSet<PermissionId>,
    pub effective_limits: ProtocolLimits,
    pub modules: Vec<ModuleDescriptor>,
    pub profiles: Vec<ProfileDescriptor>,
    pub permissions: Vec<PermissionDescriptor>,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub snapshot_scopes: Vec<SnapshotScopeDescriptor>,
}

/// The permissions, sorted by identifier, each with the profile that ceilings it.
#[must_use]
pub fn permissions() -> Vec<PermissionDescriptor> {
    let observer = profile(OBSERVER_PROFILE_ID);
    let analyst = profile(ANALYST_PROFILE_ID);
    let mut permissions = families::tables().permissions;
    permissions.extend(
        OBSERVER_SCOPE_IDS
            .iter()
            .map(|(id, description, sensitive)| PermissionDescriptor {
                id: permission(id),
                label: id.replace('.', " "),
                description: (*description).to_owned(),
                sensitive: *sensitive,
                default_grant: if *sensitive {
                    DefaultGrant::Prompt
                } else {
                    DefaultGrant::Granted
                },
                // The sensitive reads are the analyst tier: the paper
                // account, the trader's own words, redacted logs, evidence
                // bundles and screenshots. Ceilinged there rather than at
                // the floor, so "let this tool read my charts" and "let this
                // tool read my account" are two different grants, asked for
                // by two different profiles. The ordinary reads stay at the
                // floor; the analyst inherits them.
                profile_ceilings: BTreeSet::from([if *sensitive {
                    analyst.clone()
                } else {
                    observer.clone()
                }]),
            }),
    );
    permissions.sort_by(|left, right| left.id.cmp(&right.id));
    permissions
}

/// The profile chain, in registration order.
#[must_use]
pub fn profiles() -> Vec<ProfileDescriptor> {
    let observer = profile(OBSERVER_PROFILE_ID);
    let analyst = profile(ANALYST_PROFILE_ID);
    let annotator = profile(ANNOTATOR_PROFILE_ID);
    let cockpit = profile(COCKPIT_PROFILE_ID);
    let trader = profile(TRADER_PROFILE_ID);
    let profiles = vec![
        ProfileDescriptor {
            id: observer.clone(),
            label: "Observer".to_owned(),
            inherits: BTreeSet::new(),
            permissions: BTreeSet::new(),
        },
        ProfileDescriptor {
            id: analyst.clone(),
            label: "Analyst".to_owned(),
            // Directly above the floor: everything an observer reads, plus
            // the private reads ceilinged here. Nothing it holds writes.
            inherits: BTreeSet::from([observer.clone()]),
            permissions: BTreeSet::new(),
        },
        ProfileDescriptor {
            id: annotator.clone(),
            label: "Annotator".to_owned(),
            // Above the analyst rather than beside it, for the reason the
            // cockpit sits above the annotator: the handshake names a
            // connection's authority by comparing two ceilings, and two
            // tiers that merely overlap are refused outright. A trader who
            // ticks a private read and an annotate scope gets the annotator
            // ceiling, which contains both — inheriting is not granting, so
            // this hands nobody a scope they did not tick.
            inherits: BTreeSet::from([analyst.clone()]),
            permissions: BTreeSet::new(),
        },
        ProfileDescriptor {
            id: trader.clone(),
            label: "Trader".to_owned(),
            // Above the cockpit, so the chain stays a chain and any two
            // profiles remain comparable — the property the handshake
            // depends on. Inheriting is not granting: what a connection
            // may call is its ceiling intersected with what the trader
            // ticked, and nothing ticks this one.
            inherits: BTreeSet::from([cockpit.clone()]),
            permissions: BTreeSet::new(),
        },
        ProfileDescriptor {
            id: cockpit.clone(),
            label: "Cockpit".to_owned(),
            // Inherits the annotator, and through it the observer's reads
            // — rearranging a window you cannot see is not a coherent
            // grant. A *ceiling* is not a grant: what a connection may
            // actually call is the ceiling intersected with the scopes the
            // trader ticked, so nesting cockpit above annotator hands
            // nobody a capability they did not tick. What it does buy is
            // the property the handshake depends on: the profiles are a
            // chain, so any two of them are comparable.
            //
            // Left as a sibling of the annotator, the two ceilings
            // overlapped without nesting, and `handshake::authorize`
            // refuses an incomparable pair outright. A trader who ticked
            // both tiers got the cockpit ceiling on the panel, which drops
            // every `annotate.*` scope on the way out — and a client asking
            // for `--profile annotator` against that grant could not
            // connect at all.
            inherits: BTreeSet::from([annotator.clone()]),
            permissions: BTreeSet::new(),
        },
    ];
    profiles
}

/// The modules every instance publishes; the snapshot modules a projection
/// registry adds are the host's to register beside these.
#[must_use]
pub fn modules() -> Vec<ModuleDescriptor> {
    families::tables().modules
}

/// The effect policies, in registration order.
#[must_use]
pub fn effects() -> Vec<EffectPolicy> {
    families::tables().effects
}

/// The authority composed: profiles, permissions, the fixed modules and the
/// effects, ready for a host to add its snapshot modules and build.
///
/// # Errors
///
/// A table that contradicts itself — a permission with no ceiling, a
/// duplicate identifier — is refused by the registry, never published.
pub fn builder() -> Result<ContractBuilder, RegistryError> {
    let mut registry = ContractBuilder::new(profiles(), permissions())?;
    for descriptor in modules() {
        registry.register_module(descriptor)?;
    }
    for policy in effects() {
        registry.register_effect(policy)?;
    }
    Ok(registry)
}

/// What a fresh connection is granted before the trader ticks anything:
/// `observe` and the scopes that carry no user text.
#[must_use]
pub fn default_grant() -> BTreeSet<PermissionId> {
    std::iter::once(permission(OBSERVE_PERMISSION_ID))
        .chain(
            OBSERVER_SCOPE_IDS
                .iter()
                .filter(|(_, _, sensitive)| !sensitive)
                .map(|(id, _, _)| permission(id)),
        )
        .collect()
}

/// One read capability's identity and reach: everything about it except the
/// input and output shapes, which the host names when it binds a handler.
#[derive(Clone, Copy, Debug)]
pub struct ReadSpec {
    pub id: &'static str,
    pub module: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub permissions: &'static [&'static str],
    /// The pagination mode and that mode's own page ceiling. The ceiling
    /// travels with the mode because it is per capability, not per protocol:
    /// a chart page is bounded by the bars an owned DTO may copy, an evidence
    /// page by the chunks that fit one response.
    pub pagination: Option<(quantick_control::cursor::PaginationConsistency, usize)>,
}

/// `describe`.
pub const DESCRIBE: ReadSpec = ReadSpec {
    id: DESCRIBE_CAPABILITY_ID,
    module: "control",
    title: "Describe observer access",
    description: "Reports this instance, protocol, modules, scopes, profiles, permissions, and registered read capabilities.",
    permissions: &[OBSERVE_PERMISSION_ID],
    pagination: None,
};

/// `snapshot`.
pub const SNAPSHOT: ReadSpec = ReadSpec {
    id: SNAPSHOT_CAPABILITY_ID,
    module: "snapshot",
    title: "Read semantic snapshot",
    description: "Captures the requested registered scopes coherently on the application thread. Optional window_id pins a live market window; omission pins the focused window at UI admission, before any frame wait.",
    permissions: &[OBSERVE_PERMISSION_ID],
    pagination: None,
};

/// `chart.window`.
pub const CHART_WINDOW: ReadSpec = ReadSpec {
    id: CHART_WINDOW_CAPABILITY_ID,
    module: "chart",
    title: "Read chart window",
    description: "Reads a bounded append-only page of chart bars with an optional continuation cursor.",
    permissions: &[OBSERVE_PERMISSION_ID, "observe.market", "observe.chart"],
    pagination: Some((
        quantick_control::cursor::PaginationConsistency::AppendOnly,
        CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
    )),
};

/// `diagnostics`.
pub const DIAGNOSTICS: ReadSpec = ReadSpec {
    id: DIAGNOSTICS_CAPABILITY_ID,
    module: "health",
    title: "Read diagnostics",
    description: "Captures bounded structured application, indicator, and order-flow health.",
    permissions: &[
        OBSERVE_PERMISSION_ID,
        "observe.health",
        "observe.indicators",
        "observe.orderflow",
    ],
    pagination: None,
};

/// `scene`.
pub const SCENE: ReadSpec = ReadSpec {
    id: SCENE_CAPABILITY_ID,
    module: "scene",
    title: "Read semantic scene",
    description: "Names controls on the requested window_id with frame-stable IDs, selection, geometry and coded availability. Omission pins the focused live window at UI admission; a closed pinned target is refused.",
    permissions: &[
        OBSERVE_PERMISSION_ID,
        "observe.attention",
        "observe.workspace",
        "observe.market",
    ],
    pagination: None,
};

/// `events.read`.
pub const EVENTS_READ: ReadSpec = ReadSpec {
    id: crate::events::READ_CAPABILITY_ID,
    module: EVENTS_MODULE_ID,
    title: "Read events",
    description: "Reads a bounded page of the semantic event journal after a cursor or from an explicit start, and says when older events were dropped.",
    permissions: &[OBSERVE_PERMISSION_ID, EVENTS_PERMISSION_ID],
    pagination: None,
};

/// `events.wait`.
pub const EVENTS_WAIT: ReadSpec = ReadSpec {
    id: crate::events::WAIT_CAPABILITY_ID,
    module: EVENTS_MODULE_ID,
    title: "Wait for change",
    description: "Parks off the application thread until the journal moves past the cursor or the timeout elapses, then reads the bounded page that completes the call.",
    permissions: &[OBSERVE_PERMISSION_ID, EVENTS_PERMISSION_ID],
    pagination: None,
};

/// `evidence.capture`.
pub const EVIDENCE_CAPTURE: ReadSpec = ReadSpec {
    id: EVIDENCE_CAPTURE_CAPABILITY_ID,
    module: EVIDENCE_MODULE_ID,
    title: "Capture evidence",
    description: "Freezes named scopes, recent events and redacted configuration into one hashed in-memory bundle. Optional window_id pins a live target at UI admission; screenshot pixels and painted scene retain that target identity even if focus changes.",
    permissions: &[OBSERVE_PERMISSION_ID, EVIDENCE_PERMISSION_ID],
    pagination: None,
};

/// `evidence.read`.
pub const EVIDENCE_READ: ReadSpec = ReadSpec {
    id: EVIDENCE_READ_CAPABILITY_ID,
    module: EVIDENCE_MODULE_ID,
    title: "Read evidence bundle",
    description: "Reads a retained bundle in chunks of its canonical text, rechecking the grant the bundle aggregated on every page.",
    permissions: &[OBSERVE_PERMISSION_ID, EVIDENCE_PERMISSION_ID],
    pagination: Some((
        quantick_control::cursor::PaginationConsistency::RetainedResource,
        CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE,
    )),
};

/// The read capability descriptor for `spec`, with `I` and `O` as its schemas.
#[must_use]
pub fn read_descriptor<I, O>(spec: &ReadSpec) -> CapabilityDescriptor
where
    I: JsonSchema,
    O: JsonSchema,
{
    CapabilityDescriptor {
        id: CapabilityId::new(spec.id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: spec.title.to_owned(),
        description: spec.description.to_owned(),
        module: module(spec.module),
        input_schema: generated_schema::<I>(),
        output_schema: generated_schema::<O>(),
        examples: Vec::new(),
        effect: effect(OBSERVE_EFFECT_ID),
        risk_flags: BTreeSet::new(),
        read_only: true,
        idempotency: IdempotencyPolicy::Forbidden,
        revision_policy: RevisionPolicy::Forbidden,
        stale_input_safety: None,
        dry_run_supported: false,
        persistence: EffectPersistence::None,
        reversible: false,
        destructive: false,
        risk_reducing: false,
        required_permissions: spec.permissions.iter().map(|id| permission(id)).collect(),
        preconditions: Vec::new(),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: spec.pagination.map(|(_, max_items)| max_items),
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: spec.pagination.map(|(mode, _)| mode),
    }
}

/// The effect policy notifications answer to. Registered beside `annotate`
/// and `observe`; nothing else uses it.
#[must_use]
pub fn notify_effect_policy(annotator: &quantick_control::id::ProfileId) -> EffectPolicy {
    EffectPolicy {
        id: EffectId::new(NOTIFY_EFFECT_ID).expect("static effect ID is valid"),
        permission_floor: PermissionId::new(ANNOTATE_PERMISSION_ID)
            .expect("static permission ID is valid"),
        profile_ceilings: BTreeSet::from([annotator.clone()]),
        confirmation_class: ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        },
        // Every capability under this policy must say that it interrupts.
        required_risk_flags: BTreeSet::from([
            RiskFlagId::new(USER_INTERRUPT_RISK_FLAG).expect("static risk flag is valid")
        ]),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            allows_destructive: false,
            durable_requires_reversible: true,
            // A notification is transient and cannot be taken back, so the
            // contract demands the flag that says so.
            irreversible_transient_risk: Some(
                RiskFlagId::new(USER_INTERRUPT_RISK_FLAG).expect("static risk flag is valid"),
            ),
            allows_risk_reducing: false,
        },
    }
}

/// Dock the notification actions.
#[must_use]
pub fn module(id: &str) -> ModuleId {
    ModuleId::new(id).expect("static module ID is valid")
}

#[must_use]
pub fn permission(id: &str) -> PermissionId {
    PermissionId::new(id).expect("static permission ID is valid")
}

#[must_use]
pub fn profile(id: &str) -> ProfileId {
    ProfileId::new(id).expect("static profile ID is valid")
}

#[must_use]
pub fn effect(id: &str) -> EffectId {
    EffectId::new(id).expect("static effect ID is valid")
}

#[must_use]
pub fn confirmation(id: &str) -> ConfirmationClassId {
    ConfirmationClassId::new(id).expect("static confirmation ID is valid")
}
