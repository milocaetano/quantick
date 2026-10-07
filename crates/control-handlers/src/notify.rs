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
//! The budget itself is the access's ([`NotifyAccess`]): it is per
//! connection, and its clock is the window's. What is decided here is the
//! order — budget first, then the surface, then the journal — and the words.
//!
//! Rate class: a human or an agent asking for attention. Never per trade,
//! never per frame.

use std::time::Duration;

use quantick_control::{
    error::{ControlError, codes},
    id::{EventKind, ModuleId},
    limits::{CONTROL_NOTIFICATION_BURST, CONTROL_NOTIFICATION_RATE_PER_MINUTE},
    registry::RegistryError,
    wire::ActorContext,
};
use quantick_control_host::{
    actions::ActionRegistry,
    admission::known_error,
    authority::{NOTIFY_MODULE_ID, NOTIFY_PERMISSION_ID, NOTIFY_SOUND_PERMISSION_ID},
    journal::{EventActor, NewEvent},
    wire::actor_kind_name,
};
use quantick_control_schema::notify::{
    AgentPopup, NOTIFICATION_EVENT_KIND, NotifyChannel, NotifyInput, NotifyResult,
    POPUP_CAPABILITY_ID, SOUND_CAPABILITY_ID, TOAST_CAPABILITY_ID, notify_descriptor,
};
use serde_json::{Value, json};

/// The three lanes the assistant answers on: its popup, the window's
/// acknowledgement toast, and the attention sound.
pub trait AttentionPort {
    /// Open the assistant's popup; a second replaces the first.
    fn show_popup(&mut self, popup: AgentPopup);
    /// Post one line to the window's acknowledgement lane.
    fn show_toast(&mut self, message: String);
    /// Ask for the platform's attention sound; the reason it could not be
    /// made, if it could not.
    fn sound_alert(&mut self) -> Option<String>;
}

/// What a notification needs of the control access it runs in.
pub trait NotifyAccess {
    /// Whether this actor may interrupt the trader once more, or how long
    /// until it may.
    fn allow_notification(&mut self, actor: &ActorContext) -> Result<(), Duration>;
    /// Journal one event, stamped by the access's own clock.
    fn record_event(&mut self, event: NewEvent);
}

/// Dock the three notification capabilities.
pub fn register<H, A>(registry: &mut ActionRegistry<H, A>) -> Result<(), RegistryError>
where
    H: AttentionPort + ?Sized,
    A: NotifyAccess,
{
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

fn raise_popup<H: AttentionPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Popup)
}

fn raise_toast<H: AttentionPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Toast)
}

fn sound_alert<H: AttentionPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    raise(app, access, actor, input, NotifyChannel::Sound)
}

/// One notification path: budget first, then the surface, then the journal.
fn raise<H: AttentionPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
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
        actor_kind_name(actor.actor_kind)
    );
    let displayed_text = format!("{} — {author}", input.message);
    let unavailable_reason = match channel {
        NotifyChannel::Popup => {
            app.show_popup(AgentPopup {
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
            app.show_toast(displayed_text.clone());
            None
        }
        NotifyChannel::Sound => app.sound_alert(),
    };

    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    access.record_event(NewEvent {
        module_id: ModuleId::new(NOTIFY_MODULE_ID).expect("static module ID is valid"),
        kind: EventKind::new(NOTIFICATION_EVENT_KIND).expect("static event kind is valid"),
        actor: Some(event_actor),
        payload: json!({
            "channel": channel.id(),
            "message": input.message,
            "delivered": unavailable_reason.is_none(),
        }),
    });

    serde_json::to_value(NotifyResult {
        channel: channel.id().to_owned(),
        raised: unavailable_reason.is_none(),
        displayed_text,
        unavailable_reason,
    })
    .map_err(|error| ControlError::invalid_request(format!("notification result: {error}")))
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod notify_tests;
