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

use quantick_control_host::authority::{
    ANNOTATE_PERMISSION_ID, CAPABILITY_VERSION, NO_CONFIRMATION_ID, NOTIFY_EFFECT_ID,
    NOTIFY_MODULE_ID, UI_BOUNDED_COST_ID, USER_INTERRUPT_RISK_FLAG,
};

use std::collections::BTreeSet;

use quantick_control::{
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, ModuleId, PermissionId,
        RiskFlagId,
    },
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
};

use schemars::JsonSchema;

use crate::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};
use quantick_control::registry::IdempotencyPolicy::Forbidden;
use serde::{Deserialize, Serialize};

// The module the notification capabilities belong to.
// Popup and toast: an interruption the trader can read and dismiss.
// Sound: off unless the trader says otherwise, because it reaches them even
// when they are not looking at the window.
// The effect every notification carries. Separate from `annotate` because
// nothing here is reversible.
// Declared by every notification: it takes attention that was somewhere else.

/// Declared by the one that also makes noise.
pub const AUDIBLE_OUTPUT_RISK_FLAG: &str = "audible_output";

pub const POPUP_CAPABILITY_ID: &str = "notify.popup";

pub const TOAST_CAPABILITY_ID: &str = "notify.toast";

pub const SOUND_CAPABILITY_ID: &str = "notify.sound";

pub const NOTIFICATION_EVENT_KIND: &str = "notify.raised";

/// The longest notification text. A popup is a sentence the trader reads
/// mid-session, not a report; the report goes in a snapshot.
pub const NOTIFICATION_TEXT_MAX_BYTES: usize = 240;

/// The longest popup title.
pub const NOTIFICATION_TITLE_MAX_BYTES: usize = 80;

/// What a notification says. The actor is not part of it: the interface
/// stamps who asked, so a client cannot sign a popup as the platform.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotifyInput {
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
pub struct NotifyResult {
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
pub enum NotifyChannel {
    Popup,
    Toast,
    Sound,
}

impl NotifyChannel {
    /// The channel's wire name.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Popup => "popup",
            Self::Toast => "toast",
            Self::Sound => "sound",
        }
    }
}

/// A popup waiting to be read, owned by the application and drawn by it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPopup {
    pub title: String,
    pub message: String,
    /// Who asked for it, shown in the window's own chrome.
    pub author: String,
}

pub fn notify_descriptor(
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

pub const NOTIFY_TEST: &str = "an_interrupted_notification_is_resolved_by_its_readback";
/// How a client reconciles an interrupted notification.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[
    journal(
        "notify.popup",
        Forbidden,
        NOTIFICATION_EVENT_KIND,
        "payload.message",
        "an event after the pre-call cursor carries the call's own `message`; send one unique to the call",
        &[NOTIFY_TEST, EVERY_FORBIDDEN_TEST],
    ),
    journal(
        "notify.sound",
        Forbidden,
        NOTIFICATION_EVENT_KIND,
        "payload.message",
        "an event after the pre-call cursor carries the call's own `message`; send one unique to the call",
        &[NOTIFY_TEST, EVERY_FORBIDDEN_TEST],
    ),
    journal(
        "notify.toast",
        Forbidden,
        NOTIFICATION_EVENT_KIND,
        "payload.message",
        "an event after the pre-call cursor carries the call's own `message`; send one unique to the call",
        &[NOTIFY_TEST, EVERY_FORBIDDEN_TEST],
    ),
];
