//! The cockpit tier's recovery capabilities: getting a stalled feed back.
//!
//! Both call the same `Tab` method the button in the chart's offline corner
//! calls, through [`FeedRecoveryPort`]. That is the point rather than a
//! tidiness preference — a capability with its own copy of "start the feed
//! over" would drift from the one a click takes, and the drift would be an
//! assistant and a trader disagreeing about what just happened to the
//! timeline they are both looking at.
//!
//! The pair is deliberately two capabilities rather than one with a flag,
//! because they differ in what a client has to read before calling: `reload`
//! is irreversible, carries the `timeline_rebuilt` risk flag, and needs a
//! permission of its own that is off until the trader ticks it. A single call
//! whose cost depended on its input could not answer that question in its
//! descriptor, and the answer is the whole reason the trader is offered a
//! choice at all.

use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};
use quantick_control_host::actions::ActionRegistry;
use quantick_control_schema::recovery::{
    RECONNECT_CAPABILITY_ID, RELOAD_CAPABILITY_ID, RecoveryInput, RecoveryResult, descriptor,
};
use serde_json::Value;

use crate::tabs::{TabDirectory, tab_closed, tab_index};

/// What the window did when asked to recover a tab's feed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveredFeed {
    /// The market the tab shows.
    pub symbol: String,
    /// Whether a feed session was actually spawned: a recorded session owns
    /// the chart while it plays, and a feed id that left the feed table has
    /// nothing to spawn.
    pub respawned: bool,
}

/// The window's own recovery acts, the ones its offline-corner buttons call.
pub trait FeedRecoveryPort: TabDirectory {
    /// Reconnect (`keep_timeline`) or reload the feed of the tab at `index`;
    /// `None` when that tab is gone.
    fn recover_feed(&mut self, index: usize, keep_timeline: bool) -> Option<RecoveredFeed>;
}

/// Dock both recovery capabilities.
pub fn register<H, A>(registry: &mut ActionRegistry<H, A>) -> Result<(), RegistryError>
where
    H: FeedRecoveryPort + ?Sized,
{
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

fn reconnect<H: FeedRecoveryPort + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    recover(app, input, true)
}

fn reload<H: FeedRecoveryPort + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    recover(app, input, false)
}

/// The shared body. `keep_timeline` picks which of the tab's two methods runs;
/// nothing else differs, so the two capabilities can never drift apart in
/// anything but the act they name.
fn recover<H: FeedRecoveryPort + ?Sized>(
    app: &mut H,
    input: &Value,
    keep_timeline: bool,
) -> Result<Value, ControlError> {
    let input: RecoveryInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.tab_id_at(index);
    // Asked of the tab rather than inferred from one of its fields, and
    // reported rather than refused — "there was nothing to recover" is a true
    // and useful answer, and an error would read as "the call is broken" —
    // but reported from what actually happened, never from what was asked
    // for.
    let recovered = app
        .recover_feed(index, keep_timeline)
        .ok_or_else(tab_closed)?;
    let result = RecoveryResult {
        tab_id: WireU64::new(tab_id),
        symbol: recovered.symbol,
        respawned: recovered.respawned,
        timeline_kept: keep_timeline || !recovered.respawned,
    };
    serde_json::to_value(result).map_err(|error| {
        ControlError::invalid_request(format!("the recovery result could not be encoded: {error}"))
    })
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod recovery_tests;
