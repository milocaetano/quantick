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

use crate::app::AlertsPort;
pub(crate) use quantick_control_schema::notify::*;

use std::time::{Duration, Instant};

use quantick_control::{
    error::{ControlError, codes},
    id::{EventKind, ModuleId},
    limits::{CONTROL_NOTIFICATION_BURST, CONTROL_NOTIFICATION_RATE_PER_MINUTE},
    registry::RegistryError,
    wire::ActorContext,
};

use serde_json::{Value, json};

use crate::metrics;

use super::{
    actions::ActionRegistry,
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    types::known_error,
};

/// The module the notification capabilities belong to.
/// Popup and toast: an interruption the trader can read and dismiss.
/// Sound: off unless the trader says otherwise, because it reaches them even
/// when they are not looking at the window.
/// The effect every notification carries. Separate from `annotate` because
/// nothing here is reversible.
/// Declared by every notification: it takes attention that was somewhere else.
pub(crate) use quantick_control_host::authority::{
    NOTIFY_MODULE_ID, NOTIFY_PERMISSION_ID, NOTIFY_SOUND_PERMISSION_ID,
};

/// Per-client notification budget: stricter than the ordinary request limit
/// because the cost of exceeding it is a trader who cannot work, not a queue
/// that fills.
pub(crate) struct NotificationLimiter {
    available_token_nanos: u128,
    last_refill: Instant,
}

impl NotificationLimiter {
    const ONE_TOKEN_NANOS: u128 = 1_000_000_000;
    /// The budget is stated per minute and the clock ticks in nanoseconds,
    /// so every refill divides by the seconds in a minute.
    const SECONDS_PER_MINUTE: u128 = 60;

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
            / Self::SECONDS_PER_MINUTE;
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
        let nanos = missing.saturating_mul(Self::SECONDS_PER_MINUTE)
            / u128::from(CONTROL_NOTIFICATION_RATE_PER_MINUTE).max(1);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

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

fn raise_popup<P: AlertsPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Popup)
}

fn raise_toast<P: AlertsPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Toast)
}

fn sound_alert<P: AlertsPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Sound)
}

/// One notification path: budget first, then the surface, then the journal.
fn raise<P: AlertsPort + ?Sized>(
    app: &mut P,
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
            app.alerts().show_popup(AgentPopup {
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
            app.alerts().show_toast(displayed_text.clone());
            None
        }
        NotifyChannel::Sound => app.alerts().sound_alert(),
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

/// The notify handlers driven through the alerts family of a fake window,
/// with no application behind it.
#[cfg(test)]
mod port_tests {
    use super::*;
    use crate::app::control_host::tests::fake::FakeWindow;

    #[test]
    fn a_toast_lands_on_the_fake_windows_lane_with_its_author() {
        let mut window = FakeWindow::new();
        let result = raise_toast(
            &mut window,
            &mut ControlAccess::new(),
            &FakeWindow::assistant(),
            &json!({ "message": "hello" }),
        )
        .expect("a toast within budget is raised");
        assert_eq!(result["raised"], true);
        assert_eq!(
            window.toast.message(),
            Some("hello — fake assistant (agent)")
        );
    }

    #[test]
    fn every_refused_sound_is_reported_as_not_raised() {
        let mut window = FakeWindow::with_refusing_speaker("no audio output device");
        let mut access = ControlAccess::new();
        for call in 0..2 {
            let result = sound_alert(
                &mut window,
                &mut access,
                &FakeWindow::assistant(),
                &json!({ "message": "listen" }),
            )
            .expect("a refused sound is an answer, not an error");
            assert_eq!(result["raised"], false, "call {call}");
            assert_eq!(result["unavailable_reason"], "no audio output device");
        }
    }
}
