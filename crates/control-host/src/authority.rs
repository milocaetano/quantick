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

pub const SAFE_DEFAULT_SCOPE_IDS: &[&str] = &[
    "observe.system",
    "observe.workspace",
    "observe.market",
    "observe.chart",
    "observe.indicators",
    "observe.drawings",
    "observe.orderflow",
    "observe.replay",
    "observe.health",
    "observe.attention",
    "observe.events",
];

pub const OBSERVER_SCOPE_IDS: &[(&str, &str, bool)] = &[
    (
        "observe.system",
        "Application build and runtime identity",
        false,
    ),
    ("observe.workspace", "Open tabs, layout, and focus", false),
    (
        "observe.market",
        "Feed, symbol, and visible market data",
        false,
    ),
    ("observe.chart", "Chart framing, viewport, and bars", false),
    (
        "observe.indicators",
        "Indicator state and diagnostics",
        false,
    ),
    ("observe.drawings", "Drawing state and references", false),
    (
        "observe.orderflow",
        "Order-flow and local depth state",
        false,
    ),
    ("observe.replay", "Replay state", false),
    (
        "observe.paper",
        "Paper positions, orders, and performance",
        true,
    ),
    (
        "observe.health",
        "Bounded structured health diagnostics",
        false,
    ),
    (
        "observe.attention",
        "Semantic cursor and current selection",
        false,
    ),
    ("observe.events", "Bounded semantic event stream", false),
    (
        "observe.evidence",
        "Correlated in-memory evidence bundles",
        true,
    ),
    (
        "observe.user_text",
        "User-authored labels, notes, and scripts",
        true,
    ),
    (
        "observe.diagnostic_logs",
        "Redacted diagnostic log records",
        true,
    ),
    (
        "observe.screenshot",
        "Explicit raster evidence capture",
        true,
    ),
];

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotReadInput {
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
    let annotator = profile(ANNOTATOR_PROFILE_ID);
    let cockpit = profile(COCKPIT_PROFILE_ID);
    let trader = profile(TRADER_PROFILE_ID);
    let mut permissions = vec![
        PermissionDescriptor {
            id: permission(OBSERVE_PERMISSION_ID),
            label: "Observe".to_owned(),
            description: "Invoke read-only observer capabilities.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Granted,
            profile_ceilings: BTreeSet::from([observer.clone()]),
        },
        // The annotate tier's floor. Every scope below it is off by
        // default and reaches a client only when the trader ticks it in
        // the access panel, which is also what raises the connection's
        // ceiling to the `annotator` profile (contract §7.1).
        PermissionDescriptor {
            id: permission(ANNOTATE_PERMISSION_ID),
            label: "Annotate".to_owned(),
            description: "Add reversible state or bounded notifications; never remove existing work or affect a position.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([annotator.clone()]),
        },
        // The trade tier, ceilinged at the `trader` profile — which
        // nothing hands out. A permission with no ceiling at all is not
        // representable (`register_permission` refuses it), so the gate
        // is not the ceiling: it is that `configured_profile` never
        // returns `trader` and the access panel never offers the scope.
        // Say that here rather than something tidier, because the next
        // person hardening this tier will read this comment and go
        // looking for the gate it names. `annotate` promises it never
        // affects a position, so a trade cannot borrow it, and deciding
        // which profile *may* trade is a decision about a real account
        // rather than a detail of the change that carved this out.
        PermissionDescriptor {
            id: permission(TRADE_PERMISSION_ID),
            label: "Trade".to_owned(),
            description: "Place, bracket and cancel orders on the charted symbol. Fills are simulated today; the permission exists so that the day they are not, nothing has to be re-decided in a hurry.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Denied,
            profile_ceilings: BTreeSet::from([trader.clone()]),
        },
        PermissionDescriptor {
            id: permission(COCKPIT_PERMISSION_ID),
            label: "Rearrange the window".to_owned(),
            description: "Change which charts are on screen, where they sit and how wide they are. Never places or removes an object, and never touches a position.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
        },
        PermissionDescriptor {
            id: permission(COCKPIT_LAYOUT_PERMISSION_ID),
            label: "Change the chart layout".to_owned(),
            description: "Apply a layout preset, move a chart within the stack, resize a column, collapse it to its rail or expand it again, and move focus between charts.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
        },
        PermissionDescriptor {
            id: permission(COCKPIT_RECOVER_PERMISSION_ID),
            label: "Rebuild a stalled chart".to_owned(),
            description: "Throw a stalled feed's timeline away and rebuild it, which closes any open paper position (journaled, with its reason) and disarms every strategy.".to_owned(),
            // Marked, and off until ticked: this is the one cockpit act
            // that ends something the trader started. Reconnecting — which
            // keeps the timeline, the position and the strategies — needs
            // none of this and stays under plain `cockpit`.
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
        },
        PermissionDescriptor {
            id: permission(ANNOTATE_ATTENTION_PERMISSION_ID),
            label: "Create marks".to_owned(),
            description: "Append marks carrying the resolved cursor target to the event journal.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([annotator.clone()]),
        },
        PermissionDescriptor {
            id: permission(ANNOTATE_CHART_PERMISSION_ID),
            label: "Answer on the chart".to_owned(),
            description: "Place labels, arrows and zones, attributed and removable in one action.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([annotator.clone()]),
        },
        PermissionDescriptor {
            id: permission(NOTIFY_PERMISSION_ID),
            label: "Interrupt with a message".to_owned(),
            description: "Raise a popup or a toast the trader has to read and dismiss.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([annotator.clone()]),
        },
        PermissionDescriptor {
            id: permission(NOTIFY_SOUND_PERMISSION_ID),
            label: "Make a sound".to_owned(),
            description: "Play the platform's alert sound, which reaches the trader even when they are not looking at the window.".to_owned(),
            // Off by default and marked: a sound cannot be taken back and
            // arrives whether or not anyone is at the screen.
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([annotator.clone()]),
        },
        PermissionDescriptor {
            id: permission(SCRIPT_PERMISSION_ID),
            label: "Attach an indicator script".to_owned(),
            description: "Compile a Quantick Pine script and attach the indicator it produces to a pane, with a detach that restores the pane exactly.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([annotator.clone()]),
        },
    ];
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
    vec![
        ModuleDescriptor {
            id: module("control"),
            title: "Control".to_owned(),
            description: "Running-instance contract and authority metadata.".to_owned(),
        },
        ModuleDescriptor {
            id: module("snapshot"),
            title: "Snapshot".to_owned(),
            description: "Coherent multi-module semantic captures.".to_owned(),
        },
        ModuleDescriptor {
            id: module(TRADE_MODULE_ID),
            title: "Trade".to_owned(),
            description:
                "Placing, bracketing and cancelling orders on the charted symbol. Fills are simulated."
                    .to_owned(),
        },
        ModuleDescriptor {
            id: module(EVENTS_MODULE_ID),
            title: "Events".to_owned(),
            description: "The bounded semantic event journal and its cursor.".to_owned(),
        },
        ModuleDescriptor {
            id: module(EVIDENCE_MODULE_ID),
            title: "Evidence".to_owned(),
            description:
                "Coherent in-memory investigation bundles, read back as a paginated resource."
                    .to_owned(),
        },
        ModuleDescriptor {
            id: module(ANNOTATE_MODULE_ID),
            title: "Annotate".to_owned(),
            description: "Objects an operator places on the chart, attributed and removable."
                .to_owned(),
        },
        ModuleDescriptor {
            id: module(LAYOUT_MODULE_ID),
            title: "Layout".to_owned(),
            description: "The canvas: which charts are on screen, where they sit, and how wide."
                .to_owned(),
        },
        ModuleDescriptor {
            id: module(NOTIFY_MODULE_ID),
            title: "Notify".to_owned(),
            description: "Interruptions: a popup, a toast, a sound.".to_owned(),
        },
        ModuleDescriptor {
            id: module(SCRIPT_MODULE_ID),
            title: "Indicators".to_owned(),
            description: "Indicator slots, and the Quantick Pine scripts attached to them."
                .to_owned(),
        },
        ModuleDescriptor {
            id: module(ATTENTION_MODULE_ID),
            title: "Attention".to_owned(),
            description: "Human marks: what the user pointed at, as a durable referent.".to_owned(),
        },
    ]
}

/// The effect policies, in registration order.
#[must_use]
pub fn effects() -> Vec<EffectPolicy> {
    let observer = profile(OBSERVER_PROFILE_ID);
    let analyst = profile(ANALYST_PROFILE_ID);
    let annotator = profile(ANNOTATOR_PROFILE_ID);
    let cockpit = profile(COCKPIT_PROFILE_ID);
    let trader = profile(TRADER_PROFILE_ID);
    vec![
        EffectPolicy {
            id: effect(ANNOTATE_EFFECT_ID),
            permission_floor: permission(ANNOTATE_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([annotator.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                destructive: false,
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                allows_destructive: false,
                durable_requires_reversible: true,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        },
        EffectPolicy {
            id: effect(COCKPIT_EFFECT_ID),
            permission_floor: permission(COCKPIT_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                destructive: false,
                // Applying the same layout twice leaves the same layout, which
                // is what lets a client retry a dropped call without wondering
                // what it did the first time.
                idempotent: true,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                // Nothing here removes the trader's work. A layout that hides
                // a pane keeps its drawings and its indicators, which is why
                // rearranging is not destructive even when it takes a chart
                // off the screen.
                allows_destructive: false,
                durable_requires_reversible: true,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        },
        EffectPolicy {
            id: effect(RECOVER_EFFECT_ID),
            permission_floor: permission(COCKPIT_RECOVER_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                // Follows the capabilities under it, which cannot claim
                // `destructive` while this host refuses the expected-revision
                // check the registry couples to it — see the note on the
                // descriptor in `super::recovery`. The irreversibility is
                // declared through the required risk flag below and through
                // each capability's `reversible: false`.
                destructive: false,
                // Rebuilding twice rebuilds twice. Each call really does throw
                // a timeline away, so a client must not be told a retry is
                // free.
                idempotent: false,
                open_world: false,
            },
            // Every capability here says, in its own descriptor, that it can
            // cost the trader their timeline.
            required_risk_flags: BTreeSet::from([
                RiskFlagId::new(TIMELINE_REBUILT_RISK_FLAG).expect("static risk flag is valid")
            ]),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                // The one effect in this contract that may. It exists because
                // the honest alternative was worse: a capability that destroys
                // while declaring it does not, so that it could sit under the
                // `cockpit` effect whose own words are "nothing here removes
                // the trader's work".
                allows_destructive: true,
                // A rebuilt chart is durable and cannot be put back. Saying so
                // here is what lets the descriptor say `reversible: false`
                // instead of claiming a reversal it cannot perform.
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        },
        EffectPolicy {
            id: effect(TRADE_EFFECT_ID),
            permission_floor: permission(TRADE_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([trader.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            // Cancelling is risk-reducing and crosses no extra gate: taking
            // an order off the book is the direction a trader must never be
            // slowed down in — the same reason flatten is instant.
            risk_reducing_confirmation_class: Some(confirmation(NO_CONFIRMATION_ID)),
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                destructive: false,
                // Placing the same order twice places two orders; there is
                // no key that could make a retry safe.
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                allows_destructive: false,
                durable_requires_reversible: true,
                irreversible_transient_risk: None,
                allows_risk_reducing: true,
            },
        },
        notify_effect_policy(&annotator),
        EffectPolicy {
            id: effect(OBSERVE_EFFECT_ID),
            permission_floor: permission(OBSERVE_PERMISSION_ID),
            // Both read-only tiers: a read whose scopes are all ordinary is
            // reachable from the floor, and one that names a private scope
            // only from the analyst. The registry checks that *some* listed
            // ceiling grants every permission a capability declares, so
            // leaving the analyst out would fail `evidence.capture` at
            // startup rather than at the gate.
            profile_ceilings: BTreeSet::from([observer, analyst]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: true,
                destructive: false,
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(true),
                allows_destructive: false,
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        },
    ]
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
        .chain(SAFE_DEFAULT_SCOPE_IDS.iter().map(|id| permission(id)))
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
    description: "Captures the requested registered scopes coherently on the application thread.",
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
    description: "Names every control on screen with a frame-stable ID, its owner, whether it is selected, and the coded reason when it cannot be operated.",
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
    description: "Freezes the named scopes, the events around them and the effective configuration into one hashed, redacted in-memory bundle, and answers with its manifest.",
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
