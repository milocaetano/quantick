//! Getting the trader's attention: a popup, a toast, a sound.
//!
//! These are the only capabilities in the tier that cannot be undone. A
//! drawing can be removed and the chart is as it was; a sound has already been
//! heard, and a popup has already taken the eye of someone reading a tape.
//! That is why they carry their own effect policy (`notify`) with the
//! `user_interrupt` risk flag the contract then *requires* of every capability
//! under it, why sound is off by default and needs its own scope, and why they
//! have a rate and burst limit of their own, stricter than an ordinary call's.
//!
//! Rate class: a human or an agent asking for attention. Never per trade,
//! never per frame.

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use quantick_control::{
    error::{ControlError, codes},
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, EventKind, ModuleId,
        PermissionId, RiskFlagId,
    },
    limits::{CONTROL_NOTIFICATION_BURST, CONTROL_NOTIFICATION_RATE_PER_MINUTE},
    registry::{
        Availability, CapabilityDescriptor, EffectConstraints, EffectPersistence, EffectPolicy,
        ExpectedCost, IdempotencyPolicy, McpHintFloor, RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::ActorContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{app::QuantickApp, metrics};

use super::{
    actions::{ANNOTATE_PERMISSION_ID, ActionRegistry},
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    types::known_error,
};

/// The module the notification capabilities belong to.
pub(crate) const NOTIFY_MODULE_ID: &str = "notify";
/// Popup and toast: an interruption the trader can read and dismiss.
pub(crate) const NOTIFY_PERMISSION_ID: &str = "annotate.notification";
/// Sound: off unless the trader says otherwise, because it reaches them even
/// when they are not looking at the window.
pub(crate) const NOTIFY_SOUND_PERMISSION_ID: &str = "annotate.sound";
/// The effect every notification carries. Separate from `annotate` because
/// nothing here is reversible.
pub(crate) const NOTIFY_EFFECT_ID: &str = "notify";
/// Declared by every notification: it takes attention that was somewhere else.
pub(crate) const USER_INTERRUPT_RISK_FLAG: &str = "user_interrupt";
/// Declared by the one that also makes noise.
pub(crate) const AUDIBLE_OUTPUT_RISK_FLAG: &str = "audible_output";

pub(crate) const POPUP_CAPABILITY_ID: &str = "notify.popup";
pub(crate) const TOAST_CAPABILITY_ID: &str = "notify.toast";
pub(crate) const SOUND_CAPABILITY_ID: &str = "notify.sound";

pub(crate) const NOTIFICATION_EVENT_KIND: &str = "notify.raised";

const CAPABILITY_VERSION: u32 = 1;
const NO_CONFIRMATION_ID: &str = "none";
const UI_BOUNDED_COST_ID: &str = "ui_bounded";

/// The longest notification text. A popup is a sentence the trader reads
/// mid-session, not a report; the report goes in a snapshot.
pub(crate) const NOTIFICATION_TEXT_MAX_BYTES: usize = 240;
/// The longest popup title.
pub(crate) const NOTIFICATION_TITLE_MAX_BYTES: usize = 80;

/// What a notification says. The actor is not part of it: the interface
/// stamps who asked, so a client cannot sign a popup as the platform.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NotifyInput {
    #[schemars(length(min = 1, max = NOTIFICATION_TEXT_MAX_BYTES))]
    pub message: String,
    /// A popup's heading. Ignored by the toast and the sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = NOTIFICATION_TITLE_MAX_BYTES))]
    pub title: Option<String>,
}

/// What a notification returns: that it was raised, and what the trader will
/// see attributed to whom.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct NotifyResult {
    pub channel: String,
    pub raised: bool,
    /// What the interface shows, including the attribution it added.
    pub displayed_text: String,
    /// Present when the channel cannot reach the trader in this build; the
    /// call still says so rather than pretending it was heard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

/// The trader-visible surface a notification arrives on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NotifyChannel {
    Popup,
    Toast,
    Sound,
}

impl NotifyChannel {
    fn id(self) -> &'static str {
        match self {
            Self::Popup => "popup",
            Self::Toast => "toast",
            Self::Sound => "sound",
        }
    }
}

/// A popup waiting to be read, owned by the application and drawn by it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentPopup {
    pub title: String,
    pub message: String,
    /// Who asked for it, shown in the window's own chrome.
    pub author: String,
}

/// Per-client notification budget: stricter than the ordinary request limit
/// because the cost of exceeding it is a trader who cannot work, not a queue
/// that fills.
pub(crate) struct NotificationLimiter {
    available_token_nanos: u128,
    last_refill: Instant,
}

impl NotificationLimiter {
    const ONE_TOKEN_NANOS: u128 = 1_000_000_000;
    const NANOS_PER_MINUTE: u128 = 60;

    pub fn new() -> Self {
        Self {
            available_token_nanos: u128::from(CONTROL_NOTIFICATION_BURST) * Self::ONE_TOKEN_NANOS,
            last_refill: Instant::now(),
        }
    }

    /// Whether one more notification fits, refilling by elapsed time first.
    pub fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        let capacity = u128::from(CONTROL_NOTIFICATION_BURST) * Self::ONE_TOKEN_NANOS;
        let refill = elapsed
            .as_nanos()
            .saturating_mul(u128::from(CONTROL_NOTIFICATION_RATE_PER_MINUTE))
            / Self::NANOS_PER_MINUTE;
        self.available_token_nanos = self
            .available_token_nanos
            .saturating_add(refill)
            .min(capacity);
        if self.available_token_nanos < Self::ONE_TOKEN_NANOS {
            return false;
        }
        self.available_token_nanos -= Self::ONE_TOKEN_NANOS;
        true
    }

    /// How long until one more notification would be allowed.
    pub fn retry_after(&self) -> Duration {
        if self.available_token_nanos >= Self::ONE_TOKEN_NANOS {
            return Duration::ZERO;
        }
        let missing = Self::ONE_TOKEN_NANOS - self.available_token_nanos;
        let nanos = missing.saturating_mul(Self::NANOS_PER_MINUTE)
            / u128::from(CONTROL_NOTIFICATION_RATE_PER_MINUTE).max(1);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

/// The effect policy notifications answer to. Registered beside `annotate`
/// and `observe`; nothing else uses it.
pub(crate) fn effect_policy(annotator: &quantick_control::id::ProfileId) -> EffectPolicy {
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
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        notify_descriptor(
            POPUP_CAPABILITY_ID,
            "Raise a popup",
            "Opens a small window over the chart carrying one message, attributed to whoever asked for it, dismissed by the trader.",
            NOTIFY_PERMISSION_ID,
            false,
        ),
        raise_popup,
    )?;
    registry.register(
        notify_descriptor(
            TOAST_CAPABILITY_ID,
            "Raise a toast",
            "Posts one line to the window's acknowledgement lane, attributed to whoever asked for it.",
            NOTIFY_PERMISSION_ID,
            false,
        ),
        raise_toast,
    )?;
    registry.register(
        notify_descriptor(
            SOUND_CAPABILITY_ID,
            "Sound an alert",
            "Asks the platform to make an audible alert. Needs its own scope, which is off by default, and reports honestly when this build has no audio backend.",
            NOTIFY_SOUND_PERMISSION_ID,
            true,
        ),
        sound_alert,
    )?;
    Ok(())
}

fn notify_descriptor(
    id: &str,
    title: &str,
    description: &str,
    scope: &str,
    audible: bool,
) -> CapabilityDescriptor {
    let mut risk_flags = BTreeSet::from([
        RiskFlagId::new(USER_INTERRUPT_RISK_FLAG).expect("static risk flag is valid")
    ]);
    if audible {
        risk_flags
            .insert(RiskFlagId::new(AUDIBLE_OUTPUT_RISK_FLAG).expect("static risk flag is valid"));
    }
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(NOTIFY_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<NotifyInput>(),
        output_schema: generated_schema::<NotifyResult>(),
        examples: Vec::new(),
        effect: EffectId::new(NOTIFY_EFFECT_ID).expect("static effect ID is valid"),
        risk_flags,
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "A notification changes no state a later call depends on; a stale caller interrupts once and is attributed."
                .to_owned(),
        ),
        dry_run_supported: false,
        // Transient and irreversible: it has already been seen or heard.
        persistence: EffectPersistence::Transient,
        reversible: false,
        destructive: false,
        risk_reducing: false,
        required_permissions: [ANNOTATE_PERMISSION_ID, scope]
            .into_iter()
            .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
            .collect(),
        preconditions: Vec::new(),
        confirmation_class: ConfirmationClassId::new(NO_CONFIRMATION_ID)
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

fn raise_popup(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Popup)
}

fn raise_toast(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Toast)
}

fn sound_alert(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Sound)
}

/// One notification path: budget first, then the surface, then the journal.
fn raise(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
    channel: NotifyChannel,
) -> Result<Value, ControlError> {
    let input: NotifyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    // The budget is per actor, checked before anything is shown: a client
    // that floods is refused at the door rather than after the tenth popup.
    if let Err(retry_after) = access.allow_notification(actor) {
        let mut error = known_error(
            codes::BACKPRESSURE,
            "this client's notification budget is spent",
            true,
        );
        error.context.next_steps = vec![format!(
            "Notifications are limited to {CONTROL_NOTIFICATION_RATE_PER_MINUTE} per minute with a burst of {CONTROL_NOTIFICATION_BURST}; retry in about {} second(s).",
            retry_after.as_secs().max(1)
        )];
        return Err(error);
    }
    // Attribution is the interface's, never the caller's: the trader always
    // reads who asked, in the same words on every channel.
    let author = format!(
        "{} ({})",
        actor.client_name,
        super::types::actor_kind_name(actor.actor_kind)
    );
    let displayed_text = format!("{} — {author}", input.message);
    let unavailable_reason = match channel {
        NotifyChannel::Popup => {
            app.show_agent_popup(AgentPopup {
                title: input
                    .title
                    .clone()
                    .unwrap_or_else(|| "Message from an assistant".to_owned()),
                message: input.message.clone(),
                author: author.clone(),
            });
            None
        }
        NotifyChannel::Toast => {
            app.show_agent_toast(displayed_text.clone());
            None
        }
        NotifyChannel::Sound => app.sound_agent_alert(),
    };

    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(NOTIFY_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(NOTIFICATION_EVENT_KIND).expect("static event kind is valid"),
            actor: Some(event_actor),
            payload: json!({
                "channel": channel.id(),
                "message": input.message,
                "delivered": unavailable_reason.is_none(),
            }),
        },
        metrics::wall_clock_ms(),
    );

    serde_json::to_value(NotifyResult {
        channel: channel.id().to_owned(),
        raised: unavailable_reason.is_none(),
        displayed_text,
        unavailable_reason,
    })
    .map_err(|error| ControlError::invalid_request(format!("notification result: {error}")))
}
