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

use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{CapabilityId, CostClassId, ModuleId, PermissionId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::QuantickApp;
use quantick_feed::stall::Recovery;

use super::{
    actions::{ActionRegistry, CAPABILITY_VERSION, NO_CONFIRMATION_ID, UI_BOUNDED_COST_ID},
    contract::{
        COCKPIT_EFFECT_ID, COCKPIT_PERMISSION_ID, COCKPIT_RECOVER_PERMISSION_ID, RECOVER_EFFECT_ID,
        TIMELINE_REBUILT_RISK_FLAG,
    },
    gateway::ControlAccess,
};

/// The module both recovery capabilities belong to.
pub(crate) const RECOVERY_MODULE_ID: &str = "feed";

const RECONNECT_CAPABILITY_ID: &str = "feed.reconnect";
const RELOAD_CAPABILITY_ID: &str = "feed.reload";

/// The capability a recovery control calls.
///
/// `control::scene` names it beside the button, so an operator reading the
/// screen can invoke exactly what a click invokes. One mapping, and it lives
/// beside the registrations it names — a second copy in the projection would
/// be a string that goes stale the day either ID changes.
pub(crate) const fn capability_id(recovery: Recovery) -> &'static str {
    match recovery {
        Recovery::Reconnect => RECONNECT_CAPABILITY_ID,
        Recovery::Reload => RELOAD_CAPABILITY_ID,
    }
}

/// Which tab to recover. Omitted means the one the trader is looking at — the
/// same default every other cockpit call takes.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
pub(crate) struct RecoveryInput {
    /// The tab's id, as `observe.feed.status` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// What a recovery call did.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RecoveryResult {
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

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        descriptor(
            RECONNECT_CAPABILITY_ID,
            "Reconnect a stalled feed",
            "Respawns the transport and keeps everything the chart has built: bars, drawings, indicators, armed strategies and any open paper position. The window the new session replays is dropped rather than counted twice, and a silence long enough to leave a hole in the tape is marked on the chart. The same call the Reconnect button in the chart's offline corner makes.",
            false,
            generated_schema::<RecoveryInput>(),
        ),
        reconnect,
    )?;
    registry.register(
        descriptor(
            RELOAD_CAPABILITY_ID,
            "Reload a chart from a new feed session",
            "Throws the timeline away and rebuilds it: refetches history, closes any open paper position (journaled, with its reason) and disarms every strategy. For a terminal that froze while its socket stayed open, where reconnecting fixes nothing. The same call the Reload button in the chart's offline corner makes.",
            true,
            generated_schema::<RecoveryInput>(),
        ),
        reload,
    )?;
    Ok(())
}

fn reconnect(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    recover(app, input, true)
}

fn reload(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    recover(app, input, false)
}

/// The shared body. `keep_timeline` picks which of the tab's two methods runs;
/// nothing else differs, so the two capabilities can never drift apart in
/// anything but the act they name.
fn recover(
    app: &mut QuantickApp,
    input: &Value,
    keep_timeline: bool,
) -> Result<Value, ControlError> {
    let input: RecoveryInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let (tab, config) = app
        .control_tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // Asked of the tab rather than inferred from one of its fields: a
    // recorded session owns the chart while it plays, and a tab whose feed id
    // has left the feed table has nothing to spawn either. Reported rather
    // than refused — "there was nothing to recover" is a true and useful
    // answer, and an error would read as "the call is broken" — but reported
    // from what actually happened, never from what was asked for.
    let respawned = if keep_timeline {
        tab.reconnect_feed(config)
    } else {
        tab.reload_feed(config)
    };
    let result = RecoveryResult {
        tab_id: WireU64::new(tab.id),
        symbol: tab.symbol.clone(),
        respawned,
        timeline_kept: keep_timeline || !respawned,
    };
    serde_json::to_value(result).map_err(|error| {
        ControlError::invalid_request(format!("the recovery result could not be encoded: {error}"))
    })
}

/// Which tab a call named, or the one the trader is looking at.
fn tab_index(app: &QuantickApp, tab_id: Option<WireU64>) -> Result<usize, ControlError> {
    let Some(id) = tab_id else {
        return Ok(app.control_active_tab_index());
    };
    app.control_tabs()
        .iter()
        .position(|tab| tab.id == id.get())
        .ok_or_else(|| ControlError::invalid_request(format!("no open tab has id {}", id.get())))
}

fn descriptor(
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
        // claim that the second call did nothing.
        idempotency: IdempotencyPolicy::Optional,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(if destructive {
            "A stale caller can only rebuild a chart that had already come back, which costs it the history refetch and any open paper position — journaled, with its reason. The call names the tab and the market it acted on and says whether it really respawned, so a caller that guessed wrong can see it did.".to_owned()
        } else {
            "A stale caller can only reconnect a transport that had already reconnected, which costs it the overlap window it would otherwise have dropped. The call names the tab and the market it acted on, so a caller that guessed wrong can see it did.".to_owned()
        }),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        // A rebuilt timeline cannot be put back. The position it closed is
        // journaled with its reason, so nothing is lost silently — but nothing
        // reopens it either, and a client reading this field before calling is
        // exactly who needs to know that.
        reversible: !destructive,
        // Deliberately `false` even for the rebuild, and the reason is a
        // contract one rather than a claim about the act. `registry.rs` forces
        // a `destructive` capability to `RevisionPolicy::Required`, while this
        // host's `ObserverContract::prepare` refuses any envelope carrying
        // `expected_revisions` at all — so a capability marked destructive here
        // is one no conforming client can invoke, and the staleness guarantee
        // the flag advertises is one this host cannot honour. Claiming it would
        // be the worse lie. What is enforced carries the truth instead: the
        // `timeline_rebuilt` risk flag, `reversible: false`, a
        // `cockpit.recover` permission of its own that is marked sensitive and
        // off until the trader ticks it, and a description naming what it
        // closes. Lifting this needs the revision check the reference host in
        // `control::fake` already models and this one has never implemented.
        destructive: false,
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
