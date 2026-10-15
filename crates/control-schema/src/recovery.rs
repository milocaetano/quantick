//! The cockpit tier's recovery capabilities: getting a stalled feed back.
//!
//! Both call the same `Tab` method the button in the chart's offline corner
//! calls. That
//! is the point rather than a tidiness preference — a capability with its own
//! copy of "start the feed over" would drift from the one a click takes, and
//! the drift would be an assistant and a trader disagreeing about what just
//! happened to the timeline they are both looking at.
//!
//! The pair is deliberately two capabilities rather than one with a flag,
//! because they differ in what a client has to read before calling: `reload`
//! is irreversible, carries the `timeline_rebuilt` risk flag, and needs a
//! permission of its own that is off until the trader ticks it. A single call
//! whose cost depended on its input could not answer that question in its
//! descriptor, and the answer is the whole reason the trader is offered a
//! choice at all.

use quantick_control_host::authority::{
    CAPABILITY_VERSION, COCKPIT_EFFECT_ID, COCKPIT_PERMISSION_ID, COCKPIT_RECOVER_PERMISSION_ID,
    NO_CONFIRMATION_ID, RECOVER_EFFECT_ID, TIMELINE_REBUILT_RISK_FLAG, UI_BOUNDED_COST_ID,
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

/// The module both recovery capabilities belong to.
pub const RECOVERY_MODULE_ID: &str = "feed";

pub const RECONNECT_CAPABILITY_ID: &str = "feed.reconnect";

pub const RELOAD_CAPABILITY_ID: &str = "feed.reload";

/// The journal event a reload that respawned writes: who threw the tab's
/// timeline away. Its payload is the call's result under `feed_reload`.
pub const RELOAD_EVENT_KIND: &str = "feed.timeline.rebuilt";

/// Which tab to recover. Omitted means the one the trader is looking at — the
/// same default every other cockpit call takes.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
pub struct RecoveryInput {
    /// The tab's id, as `observe.feed.status` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// What a recovery call did.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryResult {
    /// The tab the call acted on.
    pub tab_id: WireU64,
    /// The market it is showing.
    pub symbol: String,
    /// Whether the feed was actually respawned. `false` is a real answer, not
    /// a failure: a tab playing a recorded session has no transport to
    /// recover, and says so here rather than pretending to have acted.
    pub respawned: bool,
    /// Whether the chart's timeline survived the call.
    pub timeline_kept: bool,
}

pub fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    destructive: bool,
    input_schema: Value,
) -> CapabilityDescriptor {
    // The two acts differ in exactly one thing a client cares about before
    // calling, and every field below follows from it. Reconnect keeps the
    // trader's work and sits under the plain cockpit effect, whose words are
    // "nothing here removes the trader's work"; reload does remove it, so it
    // sits under the one effect in this contract that permits that, behind a
    // permission of its own that the trader has to tick.
    let (effect, permissions, risk_flags) = if destructive {
        (
            RECOVER_EFFECT_ID,
            vec![COCKPIT_PERMISSION_ID, COCKPIT_RECOVER_PERMISSION_ID],
            BTreeSet::from([
                RiskFlagId::new(TIMELINE_REBUILT_RISK_FLAG).expect("static risk flag is valid")
            ]),
        )
    } else {
        (
            COCKPIT_EFFECT_ID,
            vec![COCKPIT_PERMISSION_ID],
            BTreeSet::<RiskFlagId>::new(),
        )
    };
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(RECOVERY_MODULE_ID).expect("static module ID is valid"),
        input_schema,
        output_schema: generated_schema::<RecoveryResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(effect).expect("static effect ID is valid"),
        risk_flags,
        read_only: false,
        // Recovering twice leaves one running session either way, so a client
        // may retry a dropped call. It is not free — a reload rebuilds the
        // chart both times — which is why this is `Optional` rather than a
        // claim that the second call did nothing. Under a key the gateway
        // does make the second call cost nothing, within one connection;
        // `gateway/idempotency.rs` states where that stops.
        idempotency: IdempotencyPolicy::Optional,
        // The registry couples `destructive` to `Required`. The strict tier
        // every connection uses checks no revision and so asks for none
        // (`control_host::admission`): a client calls a rebuild without one,
        // and what guards a stale caller is what the call answers, the tab
        // and the market it acted on and whether it really respawned.
        revision_policy: if destructive {
            RevisionPolicy::Required
        } else {
            RevisionPolicy::OptionalForAdditive
        },
        stale_input_safety: (!destructive).then(|| {
            "A stale caller can only reconnect a transport that had already reconnected, which costs it the overlap window it would otherwise have dropped. The call names the tab and the market it acted on, so a caller that guessed wrong can see it did.".to_owned()
        }),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        // A rebuilt timeline cannot be put back. The position it closed is
        // journaled with its reason, so nothing is lost silently — but nothing
        // reopens it either, and a client reading this field before calling is
        // exactly who needs to know that.
        reversible: !destructive,
        // A rebuild closes the paper position and disarms every strategy, and
        // the flag says so to every client that reads it before calling, along
        // with the `timeline_rebuilt` risk flag, `reversible: false` and a
        // `cockpit.recover` permission of its own that is off until the
        // trader ticks it. Who rebuilt is journaled by the handler.
        destructive,
        risk_reducing: false,
        required_permissions: permissions
            .into_iter()
            .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
            .collect(),
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
